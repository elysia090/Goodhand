//! Linux x86-64 terminal boundary. Fullscreen ownership is an explicit pilot
//! precondition: launch in the main screen of a normal foreground VTE shell.
use crate::arm_scene::{Display, Layout, MIN_HEIGHT, MIN_WIDTH, Painter, Size};
use crate::measurement::{Measurement, Timings};
use crate::playback::{self, RUN_TIME};
use crate::terminal_keys::Termios;
use std::cell::Cell;
use std::env;
use std::ffi::{c_int, c_ulong, c_void};
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

// Linux x86-64 numeric ABI; intentionally not compiled on other OS/arch.
const TIOCGWINSZ: c_ulong = 0x5413;
const O_NOCTTY: c_int = 0o400;
const O_NONBLOCK: c_int = 0o4000;
const SIGHUP: c_int = 1;
const SIGINT: c_int = 2;
const SIGQUIT: c_int = 3;
const SIGTERM: c_int = 15;
const SIGTSTP: c_int = 20;

const ENTER: &[u8] = b"\x1b[?1049h\x1b[0m\x1b[48;2;0;0;0m\x1b[2J";
const LEAVE: &[u8] = b"\x18\x1b[0m\x1b[?1049l";
const TICK_POLL: Duration = Duration::from_millis(8);
const CONTROL_BUDGET: Duration = Duration::from_millis(250);
const FRAME_BUDGET: Duration = Duration::from_millis(250);

static REQUESTED_SIGNAL: AtomicI32 = AtomicI32::new(0);

#[repr(C)]
#[derive(Default)]
struct WinSize {
    rows: u16,
    cols: u16,
    x_pixel: u16,
    y_pixel: u16,
}

unsafe extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn getpgrp() -> c_int;
    fn tcgetpgrp(fd: c_int) -> c_int;
    fn signal(signum: c_int, handler: extern "C" fn(c_int)) -> *mut c_void;
}

extern "C" fn notify_signal(signum: c_int) {
    // Native lock-free memory operations on supported Linux x86-64; no I/O,
    // allocation, or terminal mode changes are performed in signal context.
    let _ = REQUESTED_SIGNAL.compare_exchange(0, signum, Ordering::Relaxed, Ordering::Relaxed);
}

fn install_signals() -> io::Result<()> {
    for signum in [SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGTSTP] {
        // SAFETY: valid signal numbers for this target and an extern "C"
        // function with process lifetime. SIG_ERR has all address bits set.
        let prior = unsafe { signal(signum, notify_signal) };
        if prior.addr() == usize::MAX {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn requested_signal() -> Option<u8> {
    let signum = REQUESTED_SIGNAL.load(Ordering::Relaxed);
    (signum > 0).then_some(signum as u8)
}

struct Terminal(File);

impl Terminal {
    fn open() -> Result<Self, String> {
        if !io::stdout().is_terminal() {
            return Err("stdout is not a terminal; run oops directly from a shell".into());
        }
        if !io::stdin().is_terminal() {
            return Err("stdin is not a terminal; run oops without redirection".into());
        }
        let same_device = || {
            let input = fs::metadata("/proc/self/fd/0")?;
            let output = fs::metadata("/proc/self/fd/1")?;
            Ok::<_, io::Error>(
                (input.dev(), input.ino(), input.rdev())
                    == (output.dev(), output.ino(), output.rdev()),
            )
        };
        if !same_device().map_err(|error| format!("cannot verify terminal device: {error}"))? {
            return Err("stdin and stdout must use the same terminal".into());
        }
        // A fresh Linux file description on the same tty means nonblocking
        // writes do not alter the terminal flags inherited by the shell.
        // No /dev/tty fallback: redirected stdout must never draw over the UI.
        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(O_NOCTTY | O_NONBLOCK)
            .open("/proc/self/fd/1")
            .map_err(|error| format!("cannot reopen stdout's tty: {error}"))?;
        let terminal = Self(tty);
        terminal
            .foreground()
            .map_err(|error| format!("requires the normal shell's foreground terminal: {error}"))?;
        Ok(terminal)
    }

    fn fd(&self) -> c_int {
        self.0.as_raw_fd()
    }

    fn foreground(&self) -> io::Result<()> {
        // SAFETY: Linux libc calls on the live owned terminal fd.
        let terminal_group = unsafe { tcgetpgrp(self.fd()) };
        if terminal_group == -1 {
            return Err(io::Error::last_os_error());
        }
        if terminal_group != unsafe { getpgrp() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "not the foreground process group",
            ));
        }
        Ok(())
    }

    fn poll_quit_key(&self) -> io::Result<bool> {
        let mut bytes = [0u8; 64];
        match (&self.0).read(&mut bytes) {
            Ok(count) => Ok(bytes[..count].contains(&b'q')),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    fn display(&self) -> io::Result<Display> {
        let mut size = WinSize::default();
        // SAFETY: TIOCGWINSZ accepts a mutable pointer to this Linux winsize
        // layout, whose storage lives until the ioctl returns.
        let status = unsafe { ioctl(self.fd(), TIOCGWINSZ, &mut size as *mut WinSize) };
        if status == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(Display {
            size: Size {
                cols: size.cols,
                rows: size.rows,
            },
            pixels: (size.x_pixel, size.y_pixel),
        })
    }
}

fn verify_profile() -> Result<(), &'static str> {
    let has = |name| env::var_os(name).is_some_and(|value| !value.is_empty());
    if has("NO_COLOR") {
        return Err("NO_COLOR is set, but playback requires true color");
    }
    if ["TMUX", "STY", "SSH_TTY"].into_iter().any(has) {
        return Err("nested terminal and SSH sessions are outside this pilot profile");
    }
    if env::var("TERM").as_deref() != Ok("xterm-256color")
        || env::var("VTE_VERSION")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .is_none_or(|version| version < 5200)
    {
        return Err("requires the expected Xfce/VTE terminal profile");
    }
    if !matches!(env::var("COLORTERM").as_deref(), Ok("truecolor" | "24bit")) {
        return Err("COLORTERM does not advertise 24-bit true color");
    }
    let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .find(|name| has(name))
        .and_then(|name| env::var(name).ok())
        .is_some_and(|name| {
            let name = name.to_ascii_lowercase();
            let encoding = name.split('@').next().unwrap_or(&name);
            encoding.ends_with(".utf-8") || encoding.ends_with(".utf8")
        });
    if !utf8 {
        return Err("requires a UTF-8 locale");
    }
    Ok(())
}

fn size_error(size: Size) -> String {
    format!(
        "terminal is {}×{}; need at least {MIN_WIDTH}×{MIN_HEIGHT} (columns × rows)",
        size.cols, size.rows,
    )
}

fn preflight() -> Result<(Terminal, Display), String> {
    let terminal = Terminal::open()?;
    let display = terminal
        .display()
        .map_err(|error| format!("cannot read terminal size: {error}"))?;
    if !display.size.fits() {
        return Err(size_error(display.size));
    }
    verify_profile()?;
    Ok((terminal, display))
}

struct Screen {
    terminal: Terminal,
    entered: bool,
    original_keys: Option<Termios>,
    quit_pressed: Cell<bool>,
}

struct TrackEntry<'a> {
    output: &'a File,
    entered: &'a mut bool,
}

impl Write for TrackEntry<'_> {
    fn write(&mut self, packet: &[u8]) -> io::Result<usize> {
        let count = self.output.write(packet)?;
        *self.entered |= count > 0;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

impl Screen {
    fn enter(&mut self) -> io::Result<()> {
        let deadline = Instant::now() + CONTROL_BUDGET;
        let writer = TrackEntry {
            output: &self.terminal.0,
            entered: &mut self.entered,
        };
        playback::write_bounded(writer, ENTER, || {
            check_live(&self.terminal, deadline)?;
            let size = self.terminal.display()?.size;
            if size.fits() {
                Ok(())
            } else {
                Err(io::Error::other(size_error(size)))
            }
        })
    }

    fn enable_keys(&mut self) -> io::Result<()> {
        self.terminal.foreground()?;
        let fd = self.terminal.fd();
        let original = Termios::read(fd)?;
        self.original_keys = Some(original);
        original.apply_playback_keys(fd)
    }

    fn stopping(&self) -> io::Result<bool> {
        if requested_signal().is_some() || self.quit_pressed.get() {
            return Ok(true);
        }
        self.terminal.foreground()?;
        if self.terminal.poll_quit_key()? {
            self.quit_pressed.set(true);
        }
        Ok(self.quit_pressed.get())
    }

    fn frame(&self, packet: &[u8], viewport: Display, deadline: Instant) -> io::Result<()> {
        playback::write_bounded(&self.terminal.0, packet, || {
            if self.stopping()? {
                return Err(io::Error::other("playback stopped"));
            }
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            let actual = self.terminal.display()?;
            if actual == viewport {
                Ok(())
            } else if !actual.size.fits() {
                Err(io::Error::other(size_error(actual.size)))
            } else {
                Err(io::Error::other(
                    "terminal resized while a frame was in flight",
                ))
            }
        })
    }

    fn restore(&mut self) -> io::Result<()> {
        // TCSANOW does not wait for the output queue; restore even if the
        // alternate-screen cleanup cannot reach a disconnected/wedged reader.
        let keys = self.original_keys.take().map_or(Ok(()), |original| {
            self.terminal.foreground()?;
            original.apply(self.terminal.fd())
        });
        let screen = self.leave_screen();
        keys.and(screen)
    }

    fn leave_screen(&mut self) -> io::Result<()> {
        if !std::mem::take(&mut self.entered) {
            return Ok(());
        }
        let deadline = Instant::now() + CONTROL_BUDGET;
        // CAN aborts a potentially partial CSI when the one in-flight packet
        // was interrupted. Then reset draws and leave only our alternate screen.
        playback::write_bounded(&self.terminal.0, LEAVE, || {
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            self.terminal.foreground()
        })
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        // Same bounded cleanup if unwinding; normal exits call restore explicitly
        // so stderr can warn when the tty could no longer accept the sequence.
        let _ = self.restore();
    }
}

fn check_live(tty: &Terminal, deadline: Instant) -> io::Result<()> {
    if requested_signal().is_some() {
        return Err(io::Error::other("signal requested shutdown"));
    }
    if Instant::now() >= deadline {
        return Err(io::ErrorKind::TimedOut.into());
    }
    tty.foreground()
}

fn play(screen: &Screen, stats: &mut Measurement) -> io::Result<()> {
    let start = Instant::now();
    let end = start + RUN_TIME;
    let mut packet = Vec::with_capacity(64_000);
    let mut painter = Painter::default();
    let mut attempted = None;
    loop {
        if screen.stopping()? {
            return Ok(());
        }
        let Some(tick) = playback::tick(start.elapsed()) else {
            return Ok(());
        };
        let observed = screen.terminal.display()?;
        if !observed.size.fits() {
            return Err(io::Error::other(size_error(observed.size)));
        }
        let next = start + playback::start_of_tick(tick + 1);
        if attempted != Some(tick) {
            attempted = Some(tick);
            let begin_paint = Instant::now();
            let (layout, trace) = painter.paint(tick, observed, &mut packet)?;
            let encode = begin_paint.elapsed().saturating_sub(trace);
            stats.traces += 1;
            // Skip obsolete work, never play a delayed queue.
            if playback::tick(start.elapsed()) != Some(tick) {
                stats.stale += 1;
                continue;
            }
            let begin_write = Instant::now();
            match screen.frame(&packet, observed, end.min(begin_write + FRAME_BUDGET)) {
                Ok(()) => {
                    let write = begin_write.elapsed();
                    stats.bytes += packet.len();
                    stats.layout = Some(layout);
                    stats.late_write += usize::from(Instant::now() >= next);
                    stats.record(Timings {
                        trace,
                        encode,
                        write,
                    });
                    painter.submitted(layout);
                }
                Err(error) if error.kind() == io::ErrorKind::TimedOut && Instant::now() >= end => {
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        }
        std::thread::sleep(
            next.saturating_duration_since(Instant::now())
                .min(TICK_POLL),
        );
    }
}

// After an in-flight terminal failure, even stderr may be the same wedged
// tty. Runtime diagnostics must not undo the bounded shutdown. Reopen only
// stream-like descriptors; reopening a regular log would reset its offset.
fn report_runtime(message: &str) {
    let Ok(meta) = fs::metadata("/proc/self/fd/2") else {
        return;
    };
    if meta.is_file() {
        let _ = io::stderr().lock().write_all(message.as_bytes());
        return;
    }
    if !(meta.file_type().is_char_device() || meta.file_type().is_fifo()) {
        return;
    }
    let Ok(output) = OpenOptions::new()
        .write(true)
        .custom_flags(O_NOCTTY | O_NONBLOCK)
        .open("/proc/self/fd/2")
    else {
        return;
    };
    let deadline = Instant::now() + Duration::from_millis(50);
    let _ = playback::write_bounded(output, message.as_bytes(), || {
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(())
    });
}

pub fn run(check_only: bool, measure: bool) -> u8 {
    let (terminal, display) = match preflight() {
        Ok(result) => result,
        Err(reason) => {
            eprintln!("oops: {reason}");
            return 2;
        }
    };
    if check_only {
        let layout = Layout::fit(display);
        println!(
            "oops: {} columns by {} rows; model grid {} by {}, cell aspect {:.4} ({})",
            display.size.cols,
            display.size.rows,
            layout.cols,
            layout.rows,
            layout.aspect,
            if layout.measured_pitch {
                "tty pixels"
            } else {
                "assumed"
            }
        );
        return 0;
    }
    if let Err(error) = install_signals() {
        eprintln!("oops: cannot install signal handlers: {error}");
        return 2;
    }
    let mut screen = Screen {
        terminal,
        entered: false,
        original_keys: None,
        quit_pressed: Cell::new(false),
    };
    let mut stats = Measurement::new(measure);
    let playback = screen
        .enter()
        .and_then(|()| screen.enable_keys())
        .and_then(|()| play(&screen, &mut stats));
    let cleanup = screen.restore();
    if let Some(report) = stats.report() {
        report_runtime(&report);
    }
    if let Err(error) = &cleanup {
        report_runtime(&format!(
            "oops: terminal restoration was incomplete: {error}\n"
        ));
    }
    if let Some(signum) = requested_signal() {
        return 128 + signum;
    }
    if screen.quit_pressed.get() {
        return u8::from(cleanup.is_err());
    }
    if let Err(error) = playback {
        report_runtime(&format!("oops: {error}\n"));
        return 1;
    }
    if cleanup.is_err() { 1 } else { 0 }
}
