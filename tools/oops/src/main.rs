mod arm_scene;
mod measurement;
mod playback;

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod terminal;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod terminal_keys;

use std::ffi::OsString;
use std::process::ExitCode;

const HELP: &str = "oops 0.0.1\n\
\n\
Play one 30-second procedural arm rotation on a 24 Hz timeline.\n\
Pilot target: a foreground Xfce/VTE-compatible Linux terminal,\n\
with UTF-8, true color, and at least 92 columns by 45 rows.\n\
Run from the normal shell screen. Press q or Ctrl-C to exit.\n\
\n\
Usage: oops [--check | --measure | --help | --version]\n\
\n\
  --check    Check terminal preconditions and display geometry.\n\
  --measure  Play and print process-side timing after restoring the shell.\n\
  --help     Print this help.\n\
  --version  Print the version.\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Play,
    Check,
    Measure,
    Help,
    Version,
}

fn mode(mut args: impl Iterator<Item = OsString>) -> Result<Mode, ()> {
    let parsed = match args.next() {
        None => Mode::Play,
        Some(arg) if arg == "--check" => Mode::Check,
        Some(arg) if arg == "--measure" => Mode::Measure,
        Some(arg) if arg == "--help" || arg == "-h" => Mode::Help,
        Some(arg) if arg == "--version" || arg == "-V" => Mode::Version,
        Some(_) => return Err(()),
    };
    args.next().is_none().then_some(parsed).ok_or(())
}

fn main() -> ExitCode {
    match mode(std::env::args_os().skip(1)) {
        Ok(Mode::Help) => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Ok(Mode::Version) => {
            println!("oops {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Mode::Check) => run(true, false),
        Ok(Mode::Measure) => run(false, true),
        Ok(Mode::Play) => run(false, false),
        Err(()) => {
            eprintln!("oops: expected no arguments, --check, --measure, --help, or --version");
            ExitCode::from(2)
        }
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
fn run(check_only: bool, measure: bool) -> ExitCode {
    terminal::run(check_only, measure).into()
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
fn run(_check_only: bool, _measure: bool) -> ExitCode {
    eprintln!("oops: terminal playback is only implemented on GNU/Linux x86-64");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::{Mode, mode};
    use std::ffi::OsString;

    #[test]
    fn limited_plain_argument_surface() {
        let args = |args: &[&str]| mode(args.iter().map(OsString::from));
        assert_eq!(args(&[]), Ok(Mode::Play));
        assert_eq!(args(&["--check"]), Ok(Mode::Check));
        assert_eq!(args(&["--help"]), Ok(Mode::Help));
        assert_eq!(args(&["-V"]), Ok(Mode::Version));
        assert_eq!(args(&["--help", "--check"]), Err(()));
        assert_eq!(args(&["--duration=10"]), Err(()));
    }
}
