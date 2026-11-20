#![cfg_attr(
    all(target_os = "windows", feature = "desktop-window"),
    windows_subsystem = "windows"
)]

#[cfg(any(test, goodhand_embedded_playback, goodhand_release_artwork))]
mod archive_profile;
#[cfg(any(test, goodhand_embedded_playback))]
mod archived_scene;
mod arm_scene;
#[cfg(any(test, goodhand_desktop_artwork))]
mod black_grade;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod cursor_report;
mod dots;
mod measurement;
mod playback;
mod projection;
#[cfg(goodhand_desktop_artwork)]
mod reference_frame;
mod settings;
#[cfg(goodhand_desktop_artwork)]
mod window;

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod terminal;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod terminal_keys;

use playback::Repeat;
use settings::{Key, Overrides};
#[cfg(any(
    goodhand_desktop_artwork,
    all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")
))]
use settings::{Settings, read_config};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "oops 0.0.1

Open an independent Goodhand window (desktop build).
The window loops in 45 seconds; press q or Esc to close.

Usage: oops [--window | --island] [--config PATH] [--once] [TURN OPTIONS]
       oops --terminal [--alternate-buffer] [--config PATH] [--once] [OPTIONS]
       oops --check | --measure [terminal options]
       oops --help | --version

  --window, --island    Explicit aliases for the native window.
  --terminal            Draw in the current shell (GNU/Linux x86-64).
  --alternate-buffer    Use a separate terminal buffer with --terminal,
                        --check or --measure; never opens a window.
  --check               Check terminal preconditions and display geometry.
  --measure             Play and print process timing after restoring the shell.
  --once                Stop after one turn instead of looping.
  --config PATH         Read settings from this explicit file.
  --duration SECONDS    12–60 whole seconds (window 45, terminal 30 by default).
  --direction DIR       reverse (default) or forward; same opening pose.
  --zoom PERCENT        Terminal: 50–200; 100 fits the full turn.
  --cell-aspect ASPECT  Terminal: auto or cell width/height from 0.30 to 0.85.
  --offset-x PERCENT    Terminal: -50 to 50; positive moves right.
  --offset-y PERCENT    Terminal: -50 to 50; positive moves down.
  --brightness PERCENT  Terminal: 0–200; 100 adds no gray gain.
  --dots               Terminal: use single-cell bullets instead of Braille.
  --help, -h            Print this help.
  --version, -V         Print the version.

Command-line options override the corresponding config values.
";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Play,
    Check,
    Measure,
    Terminal,
    Window,
    Help,
    Version,
}

#[derive(Debug, Default, PartialEq)]
struct Options {
    mode: Mode,
    repeat: Repeat,
    alternate_buffer: bool,
    config: Option<PathBuf>,
    overrides: Overrides,
}

fn value(args: &mut impl Iterator<Item = OsString>, option: &str) -> Result<OsString, String> {
    args.next()
        .filter(|value| !value.is_empty() && !value.as_encoded_bytes().starts_with(b"--"))
        .ok_or_else(|| format!("{option}: expected a value"))
}

fn parse_args(mut args: impl Iterator<Item = OsString>) -> Result<Options, String> {
    let mut options = Options::default();
    while let Some(arg) = args.next() {
        let mode = match arg.to_str() {
            Some("--check") => Mode::Check,
            Some("--measure") => Mode::Measure,
            Some("--window" | "--island") => Mode::Window,
            Some("--terminal") => Mode::Terminal,
            Some("--alternate-buffer") => {
                if options.alternate_buffer {
                    return Err("--alternate-buffer: specified more than once".into());
                }
                options.alternate_buffer = true;
                continue;
            }
            Some("--help" | "-h") => Mode::Help,
            Some("--version" | "-V") => Mode::Version,
            Some("--once") => {
                if options.repeat == Repeat::Once {
                    return Err("--once: specified more than once".into());
                }
                options.repeat = Repeat::Once;
                continue;
            }
            Some("--dots") => {
                options
                    .overrides
                    .set(Key::Renderer, "dots")
                    .map_err(|reason| format!("--dots: {reason}"))?;
                continue;
            }
            Some("--config") => {
                if options.config.is_some() {
                    return Err("--config: specified more than once".into());
                }
                options.config = Some(value(&mut args, "--config")?.into());
                continue;
            }
            option => {
                let Some((option, key)) =
                    option.and_then(|name| Key::from_flag(name).map(|key| (name, key)))
                else {
                    return Err(format!("unknown option {arg:?}; see --help"));
                };
                let input = value(&mut args, option)?;
                let input = input
                    .to_str()
                    .ok_or_else(|| format!("{option}: expected UTF-8 text"))?;
                options
                    .overrides
                    .set(key, input)
                    .map_err(|reason| format!("{option}: {reason}"))?;
                continue;
            }
        };
        if options.mode != Mode::Play {
            return Err(
                "choose only one of --terminal, --check, --measure, --window/--island, --help, or --version"
                    .into(),
            );
        }
        options.mode = mode;
    }
    if matches!(options.mode, Mode::Help | Mode::Version)
        && (options.repeat == Repeat::Once
            || options.config.is_some()
            || options.overrides != Overrides::default())
    {
        return Err("--help and --version do not accept playback options".into());
    }
    let terminal = matches!(options.mode, Mode::Terminal | Mode::Check | Mode::Measure);
    if !terminal && (options.alternate_buffer || options.overrides.has_terminal_display_flags()) {
        return Err("terminal display flags require --terminal, --check, or --measure".into());
    }
    Ok(options)
}

#[cfg(any(
    goodhand_desktop_artwork,
    all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")
))]
impl Options {
    fn settings(self) -> Result<Settings, String> {
        let file = self.config.as_deref().map(read_config).transpose()?;
        let mut defaults = Settings::default();
        if matches!(self.mode, Mode::Play | Mode::Window) {
            defaults.cycle.duration = playback::CycleDuration::WINDOW_DEFAULT;
        }
        let defaults = file.unwrap_or_default().apply(defaults);
        Ok(self.overrides.apply(defaults))
    }
}

fn main() -> ExitCode {
    let options = match parse_args(std::env::args_os().skip(1)) {
        Ok(options) => options,
        Err(reason) => return error_exit(&reason),
    };
    match options.mode {
        Mode::Help => {
            print!("{HELP}");
            #[cfg(goodhand_release_artwork)]
            println!(
                "Desktop: reviewed W5 native artwork. Terminal preview: {}.",
                arm_scene::CONTENT_LABEL
            );
            #[cfg(not(goodhand_release_artwork))]
            println!("This build plays {}.", arm_scene::CONTENT_LABEL);
            ExitCode::SUCCESS
        }
        Mode::Play | Mode::Window => run_window(options),
        Mode::Version => {
            println!("oops {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => run(options),
    }
}

#[cfg(goodhand_desktop_artwork)]
fn run_window(options: Options) -> ExitCode {
    let repeat = options.repeat;
    let config = match options.settings() {
        Ok(config) => config,
        Err(message) => return error_exit(&message),
    };
    match window::run(repeat, config.window_appearance, config.cycle) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => error_exit(&error.to_string()),
    }
}

#[cfg(not(goodhand_desktop_artwork))]
fn run_window(_options: Options) -> ExitCode {
    error_exit(
        if cfg!(all(
            target_os = "linux",
            target_arch = "x86_64",
            target_env = "gnu"
        )) {
            "native window is unavailable in this build; use oops --terminal for text playback"
        } else {
            "native window is unavailable in this build; use a supported desktop package"
        },
    )
}

fn error_exit(reason: &str) -> ExitCode {
    eprintln!("oops: {reason}");
    ExitCode::from(2)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
fn run(options: Options) -> ExitCode {
    let Options {
        mode,
        repeat,
        alternate_buffer,
        ..
    } = options;
    match options.settings() {
        Ok(settings) => terminal::run(
            mode == Mode::Check,
            mode == Mode::Measure,
            settings,
            repeat,
            alternate_buffer,
        )
        .into(),
        Err(reason) => error_exit(&reason),
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
fn run(_options: Options) -> ExitCode {
    error_exit("terminal playback is only implemented on GNU/Linux x86-64")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Result<Options, String> {
        parse_args(values.iter().map(OsString::from))
    }

    #[cfg(any(
        goodhand_desktop_artwork,
        all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")
    ))]
    #[test]
    fn window_duration_defaults_to_45_but_terminal_and_overrides_hold() {
        assert_eq!(
            args(&[])
                .unwrap()
                .settings()
                .unwrap()
                .cycle
                .duration
                .seconds(),
            45
        );
        assert_eq!(
            args(&["--terminal"])
                .unwrap()
                .settings()
                .unwrap()
                .cycle
                .duration
                .seconds(),
            30
        );
        assert_eq!(
            args(&["--duration", "30"])
                .unwrap()
                .settings()
                .unwrap()
                .cycle
                .duration
                .seconds(),
            30
        );
    }

    #[test]
    fn modes_and_display_flags_accept_either_order() {
        assert_eq!(args(&[]).unwrap().mode, Mode::Play);
        assert_eq!(args(&[]).unwrap().repeat, Repeat::Loop);
        assert_eq!(args(&["--measure", "--once"]).unwrap().repeat, Repeat::Once);
        assert_eq!(args(&["--check"]).unwrap().mode, Mode::Check);
        assert_eq!(args(&["--island"]).unwrap().mode, Mode::Window);
        assert_eq!(args(&["--window"]).unwrap().mode, Mode::Window);
        assert!(args(&["--duration", "38", "--island"]).is_ok());
        assert!(args(&["--terminal", "--dots"]).is_ok());
        let terminal = args(&["--alternate-buffer", "--terminal"]).unwrap();
        assert_eq!(terminal.mode, Mode::Terminal);
        assert!(terminal.alternate_buffer);
        assert_eq!(args(&["--help"]).unwrap().mode, Mode::Help);
        assert_eq!(args(&["-V"]).unwrap().mode, Mode::Version);
        assert_eq!(
            args(&["--check", "--zoom", "90", "--cell-aspect", "auto"]),
            args(&["--cell-aspect", "auto", "--zoom", "90", "--check"])
        );
    }

    #[test]
    fn ambiguous_or_invalid_flags_fail() {
        for args in [
            &["--help", "--check"][..],
            &["--help", "--once"],
            &["--once", "--once"],
            &["--island", "--island"],
            &["--terminal", "--island"],
            &["--alternate-buffer"],
            &["--island", "--alternate-buffer"],
            &["--terminal", "--alternate-buffer", "--alternate-buffer"],
            &["--island", "--zoom", "80"],
            &["--dots", "--dots"],
            &["--dots", "braille"],
            &["--braille"],
            &["--help", "--dots"],
            &["--duration=10"],
            &["--duration", "11"],
            &["--duration", "61"],
            &["--duration", "12.0"],
            &["--direction", "left"],
            &["--offset-x", "-51"],
            &["--offset-y", "51"],
            &["--brightness", "201"],
            &["--brightness", "100", "--brightness", "150"],
            &["--version", "--config", "oops.conf"],
            &["--config", "one", "--config", "two"],
            &["--config", "--zoom", "125"],
            &["--zoom", "120", "--zoom", "100"],
            &["--zoom", "0"],
            &["--zoom"],
            &["--cell-aspect", "NaN"],
            &["--cell-aspect", "auto", "--cell-aspect", "0.4"],
        ] {
            assert!(self::args(args).is_err(), "{args:?}");
        }
    }
}
