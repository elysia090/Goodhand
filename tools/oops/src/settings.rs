//! Startup framing settings. Nothing is looked up unless --config names a file.
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub zoom: Zoom,
    pub cell_aspect: CellAspect,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            zoom: Zoom::FIT,
            cell_aspect: CellAspect::Auto,
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Overrides {
    zoom: Option<Zoom>,
    cell_aspect: Option<CellAspect>,
}

impl Overrides {
    pub fn apply(self, defaults: Settings) -> Settings {
        Settings {
            zoom: self.zoom.unwrap_or(defaults.zoom),
            cell_aspect: self.cell_aspect.unwrap_or(defaults.cell_aspect),
        }
    }

    pub fn set_zoom(&mut self, value: &str) -> Result<(), &'static str> {
        set(&mut self.zoom, value)
    }

    pub fn set_cell_aspect(&mut self, value: &str) -> Result<(), &'static str> {
        set(&mut self.cell_aspect, value)
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
        let result = match key {
            "zoom" => settings.set_zoom(value),
            "cell_aspect" => settings.set_cell_aspect(value),
            _ => return Err(format!("line {location}: unknown key {key:?}")),
        };
        result.map_err(|message| format!("line {location}: {key}: {message}"))?;
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
