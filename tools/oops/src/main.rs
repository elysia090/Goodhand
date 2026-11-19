#[cfg(any(test, goodhand_embedded_playback))]
mod archive_profile;
#[cfg(any(test, goodhand_embedded_playback))]
mod archived_scene;
mod arm_scene;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod cursor_report;
mod measurement;
mod playback;
mod projection;
mod settings;

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod terminal;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod terminal_keys;

use playback::Repeat;
use settings::{Key, Overrides};
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
use settings::{Settings, read_config};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "oops 0.0.1\n\
\n\
Loop an arm rotation on a 24 Hz timeline (30 seconds per turn by default).\n\
Pilot target: a foreground Xfce/VTE-compatible Linux terminal,\n\
with UTF-8, true color, and at least 80 columns by 24 rows.\n\
Draw inline below the shell command by default. Press q or Ctrl-C to exit.\n\
\n\
Usage: oops [--check | --measure] [--config PATH] [OPTIONS]\n\
       oops --help | --version\n\
\n\
  --check               Check terminal preconditions and display geometry.\n\
  --measure             Play and print process timing after restoring the shell.\n\
  --once                Stop after one turn instead of looping.\n\
  --island          Use Island mode in the same terminal window.\n\
  --config PATH         Read optional display settings from this file only.\n\
  --zoom PERCENT        50–200; 100 fits the model frame, above 100 may crop.\n\
  --cell-aspect ASPECT  auto (default) or cell width/height from 0.30 to 0.85.\n\
  --duration SECONDS    12–60 whole seconds (default 30).\n\
  --direction DIRECTION reverse (default) or forward, same opening pose.\n\
  --offset-x PERCENT    -50 to 50 of drawing width; positive moves right.\n\
  --offset-y PERCENT    -50 to 50 of drawing height; positive moves down.\n\
  --brightness PERCENT  0–200; 100 leaves original gray unchanged.\n\
  --help, -h            Print this help.\n\
  --version, -V         Print the version.\n\
\n\
Command-line options override the corresponding config values.\n";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Play,
    Check,
    Measure,
    Help,
    Version,
}

#[derive(Debug, Default, PartialEq)]
struct Options {
    mode: Mode,
    repeat: Repeat,
    island: bool,
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
            Some("--help" | "-h") => Mode::Help,
            Some("--version" | "-V") => Mode::Version,
            Some("--once") => {
                if options.repeat == Repeat::Once {
                    return Err("--once: specified more than once".into());
                }
                options.repeat = Repeat::Once;
                continue;
            }
            Some("--island") => {
                if std::mem::replace(&mut options.island, true) {
                    return Err("--island: specified more than once".into());
                }
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
            return Err("choose only one of --check, --measure, --help, or --version".into());
        }
        options.mode = mode;
    }
    if matches!(options.mode, Mode::Help | Mode::Version)
        && (options.island
            || options.repeat == Repeat::Once
            || options.config.is_some()
            || options.overrides != Overrides::default())
    {
        return Err("--help and --version must be used on their own".into());
    }
    Ok(options)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
impl Options {
    fn settings(self) -> Result<Settings, String> {
        let file = self.config.as_deref().map(read_config).transpose()?;
        let defaults = file.unwrap_or_default().apply(Settings::default());
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
            println!("This build plays {}.", arm_scene::CONTENT_LABEL);
            ExitCode::SUCCESS
        }
        Mode::Version => {
            println!("oops {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => run(options),
    }
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
        island,
        ..
    } = options;
    match options.settings() {
        Ok(settings) => terminal::run(
            mode == Mode::Check,
            mode == Mode::Measure,
            settings,
            repeat,
            island,
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

    #[test]
    fn modes_and_display_flags_accept_either_order() {
        assert_eq!(args(&[]).unwrap().mode, Mode::Play);
        assert_eq!(args(&[]).unwrap().repeat, Repeat::Loop);
        assert_eq!(args(&["--measure", "--once"]).unwrap().repeat, Repeat::Once);
        assert_eq!(args(&["--check"]).unwrap().mode, Mode::Check);
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
