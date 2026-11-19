//! Startup playback and framing settings. Nothing is looked up unless --config names a file.
use crate::playback::{Cycle, CycleDuration, Direction};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;

const MAX_CONFIG_BYTES: usize = 8 * 1024;
pub const MIN_ASPECT: f64 = 0.30;
pub const MAX_ASPECT: f64 = 0.85;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zoom(u16);

impl Zoom {
    const FIT: Self = Self(100);

    pub fn percent(self) -> u16 {
        self.0
    }

    pub fn factor(self) -> f64 {
        f64::from(self.0) / f64::from(Self::FIT.0)
    }
}

impl FromStr for Zoom {
    type Err = &'static str;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        input
            .parse::<u16>()
            .ok()
            .filter(|value| (50..=200).contains(value))
            .map(Self)
            .ok_or("expected an integer percentage from 50 to 200")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CellAspect {
    Auto,
    Fixed(f64),
}

impl FromStr for CellAspect {
    type Err = &'static str;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if input == "auto" {
            return Ok(Self::Auto);
        }
        input
            .parse::<f64>()
            .ok()
            .filter(|value| (MIN_ASPECT..=MAX_ASPECT).contains(value))
            .map(Self::Fixed)
            .ok_or("expected auto or a width/height ratio from 0.30 to 0.85")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Offset(i8);

impl Offset {
    pub fn percent(self) -> i8 {
        self.0
    }

    pub fn fraction(self) -> f64 {
        f64::from(self.0) / 100.0
    }
}

impl FromStr for Offset {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<i8>()
            .ok()
            .filter(|value| (-50..=50).contains(value))
            .map(Self)
            .ok_or("expected an integer percentage from -50 to 50")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Brightness(u8);

impl Default for Brightness {
    fn default() -> Self {
        Self(100)
    }
}

impl Brightness {
    pub fn percent(self) -> u8 {
        self.0
    }

    pub fn apply(self, value: u8) -> u8 {
        // Maximum intermediate: 255 * 200 + 50, which fits u16.
        let adjusted = (u16::from(value) * u16::from(self.0) + 50) / 100;
        adjusted.min(u16::from(u8::MAX)) as u8
    }
}

impl FromStr for Brightness {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<u8>()
            .ok()
            .filter(|value| *value <= 200)
            .map(Self)
            .ok_or("expected an integer percentage from 0 to 200")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub zoom: Zoom,
    pub cell_aspect: CellAspect,
    pub cycle: Cycle,
    pub offset: [Offset; 2],
    pub brightness: Brightness,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            zoom: Zoom::FIT,
            cell_aspect: CellAspect::Auto,
            cycle: Cycle::default(),
            offset: [Offset::default(); 2],
            brightness: Brightness::default(),
        }
    }
}

impl Settings {
    pub fn playback_status(self) -> String {
        format!(
            "duration={}s direction={} offset={}%,{}% brightness={}%",
            self.cycle.duration.seconds(),
            self.cycle.direction,
            self.offset[0].percent(),
            self.offset[1].percent(),
            self.brightness.percent(),
        )
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Overrides {
    zoom: Option<Zoom>,
    cell_aspect: Option<CellAspect>,
    duration: Option<CycleDuration>,
    direction: Option<Direction>,
    offset_x: Option<Offset>,
    offset_y: Option<Offset>,
    brightness: Option<Brightness>,
}

impl Overrides {
    pub fn apply(self, defaults: Settings) -> Settings {
        Settings {
            zoom: self.zoom.unwrap_or(defaults.zoom),
            cell_aspect: self.cell_aspect.unwrap_or(defaults.cell_aspect),
            cycle: Cycle {
                duration: self.duration.unwrap_or(defaults.cycle.duration),
                direction: self.direction.unwrap_or(defaults.cycle.direction),
            },
            offset: [
                self.offset_x.unwrap_or(defaults.offset[0]),
                self.offset_y.unwrap_or(defaults.offset[1]),
            ],
            brightness: self.brightness.unwrap_or(defaults.brightness),
        }
    }

    pub fn set(&mut self, key: Key, value: &str) -> Result<(), &'static str> {
        match key {
            Key::Zoom => set(&mut self.zoom, value),
            Key::CellAspect => set(&mut self.cell_aspect, value),
            Key::Duration => set(&mut self.duration, value),
            Key::Direction => set(&mut self.direction, value),
            Key::OffsetX => set(&mut self.offset_x, value),
            Key::OffsetY => set(&mut self.offset_y, value),
            Key::Brightness => set(&mut self.brightness, value),
        }
    }
}

#[derive(Clone, Copy)]
pub enum Key {
    Zoom,
    CellAspect,
    Duration,
    Direction,
    OffsetX,
    OffsetY,
    Brightness,
}

impl Key {
    const NAMES: [(Self, &'static str, &'static str); 7] = [
        (Self::Zoom, "--zoom", "zoom"),
        (Self::CellAspect, "--cell-aspect", "cell_aspect"),
        (Self::Duration, "--duration", "duration"),
        (Self::Direction, "--direction", "direction"),
        (Self::OffsetX, "--offset-x", "offset_x"),
        (Self::OffsetY, "--offset-y", "offset_y"),
        (Self::Brightness, "--brightness", "brightness"),
    ];

    pub fn from_flag(flag: &str) -> Option<Self> {
        Self::NAMES
            .iter()
            .find_map(|&(key, name, _)| (flag == name).then_some(key))
    }

    fn from_config(name: &str) -> Option<Self> {
        Self::NAMES
            .iter()
            .find_map(|&(key, _, entry)| (name == entry).then_some(key))
    }
}

fn set<T: FromStr<Err = &'static str>>(
    slot: &mut Option<T>,
    value: &str,
) -> Result<(), &'static str> {
    if slot.is_some() {
        return Err("specified more than once");
    }
    *slot = Some(value.parse()?);
    Ok(())
}

fn parse_config(source: &str) -> Result<Overrides, String> {
    let mut settings = Overrides::default();
    for (line, raw) in source.lines().enumerate() {
        let entry = raw.split_once('#').map_or(raw, |(entry, _)| entry).trim();
        if entry.is_empty() {
            continue;
        }
        let location = line + 1;
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| format!("line {location}: expected key = value"))?;
        let key = key.trim();
        let value = value.trim();
        let setting =
            Key::from_config(key).ok_or_else(|| format!("line {location}: unknown key {key:?}"))?;
        settings
            .set(setting, value)
            .map_err(|message| format!("line {location}: {key}: {message}"))?;
    }
    Ok(settings)
}

pub fn read_config(path: &Path) -> Result<Overrides, String> {
    read_config_inner(path).map_err(|error| format!("config {path:?}: {error}"))
}

fn read_config_inner(path: &Path) -> Result<Overrides, String> {
    let mut options = File::options();
    options.read(true);
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    {
        use crate::terminal::{O_NOCTTY, O_NONBLOCK};
        use std::os::unix::fs::OpenOptionsExt as _;
        // Opening a FIFO must not block before the descriptor can be checked.
        options.custom_flags(O_NOCTTY | O_NONBLOCK);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    // Check the file we actually opened, not an earlier path lookup.
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("expected a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err(format!("file exceeds {MAX_CONFIG_BYTES} bytes"));
    }
    let source = std::str::from_utf8(&bytes).map_err(|_| "file must be UTF-8")?;
    parse_config(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_accepts_comments_whitespace_and_crlf() {
        let config =
            parse_config("# framing\r\n zoom = 125 # deliberate crop\r\n\r\ncell_aspect=0.467")
                .unwrap()
                .apply(Settings::default());
        assert_eq!(config.zoom, Zoom(125));
        assert_eq!(config.zoom.factor(), 1.25);
        assert_eq!(config.cell_aspect, CellAspect::Fixed(0.467));
        assert_eq!(
            parse_config("").unwrap().apply(Settings::default()),
            Settings::default()
        );
    }

    #[test]
    fn invalid_config_is_rejected_with_location() {
        for (source, contains) in [
            ("# comment\nzoom 125", "line 2: expected key = value"),
            ("# comment\n[display]", "line 2: expected key = value"),
            (" zoom = 125\nzz = 150", "line 2: unknown key \"zz\""),
            (
                "zoom=125\nzoom=125",
                "line 2: zoom: specified more than once",
            ),
            (
                "cell_aspect=auto\ncell_aspect=0.4",
                "line 2: cell_aspect: specified more than once",
            ),
            ("zoom=100=200", "line 1: zoom: expected"),
            ("cell_aspect = ", "line 1: cell_aspect: expected"),
        ] {
            assert!(
                parse_config(source).unwrap_err().contains(contains),
                "{source}"
            );
        }
    }

    #[test]
    fn playback_overrides_are_independent_and_bounded() {
        let file =
            parse_config("duration=12\ndirection=forward\noffset_x=-50\noffset_y=50\nbrightness=0")
                .unwrap()
                .apply(Settings::default());
        let cli = parse_config("duration=60\noffset_y=-35\nbrightness=200")
            .unwrap()
            .apply(file);
        assert_eq!(file.cycle.duration.seconds(), 12);
        assert_eq!(file.brightness.percent(), 0);
        assert_eq!(cli.cycle.duration.seconds(), 60);
        assert_eq!(cli.cycle.direction, Direction::Forward);
        assert_eq!(cli.offset.map(Offset::percent), [-50, -35]);
        assert_eq!(cli.brightness.percent(), 200);
        for source in [
            "duration=11",
            "duration=61",
            "duration=12.5",
            "direction=clockwise",
            "offset_x=51",
            "offset_y=-51",
            "brightness=-1",
            "brightness=201",
            "direction=reverse\ndirection=forward",
        ] {
            assert!(parse_config(source).is_err(), "{source}");
        }
    }

    #[test]
    fn gray_gain_is_monotonic_zero_preserving_and_rounded() {
        for percent in 0..=200 {
            let gain = Brightness(percent);
            assert_eq!(gain.apply(0), 0);
            assert!((0..u8::MAX).all(|gray| gain.apply(gray) <= gain.apply(gray + 1)));
        }
        for gray in 0..=u8::MAX {
            assert_eq!(Brightness::default().apply(gray), gray);
            assert_eq!(Brightness(0).apply(gray), 0);
        }
        assert_eq!(Brightness(50).apply(79), 40);
        assert_eq!(Brightness(200).apply(79), 158);
        assert_eq!(Brightness(200).apply(129), 255);
    }

    #[test]
    fn input_range_rejects_nonfinite_fractional_and_out_of_range_values() {
        for value in ["NaN", "inf", "-inf", "0", "49", "201", "1.5", "-1", "65536"] {
            assert!(value.parse::<Zoom>().is_err(), "{value}");
        }
        assert_eq!("50".parse(), Ok(Zoom(50)));
        assert_eq!("200".parse(), Ok(Zoom(200)));
        for value in [
            "NaN", "inf", "-inf", "0", "-0.5", "0.299", "0.851", "1", "AUTO",
        ] {
            assert!(value.parse::<CellAspect>().is_err(), "{value}");
        }
        assert_eq!("0.30".parse(), Ok(CellAspect::Fixed(0.30)));
        assert_eq!("0.85".parse(), Ok(CellAspect::Fixed(0.85)));
        assert_eq!("auto".parse(), Ok(CellAspect::Auto));
    }
}
