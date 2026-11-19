//! Linux x86-64 terminal boundary for inline and Island playback in a normal foreground VTE shell.
use crate::arm_scene::{
    CONTENT_LABEL, Display, InlineRows, Layout, MIN_HEIGHT, MIN_WIDTH, Painter, PitchSource, Size,
    clear_rows, cup, reservation,
};
use crate::cursor_report::{Cursor, Decoder};
use crate::measurement::{Measurement, Timings};
use crate::playback::{self, Repeat, Timeline};
use crate::settings::Settings;
use crate::terminal_keys::Termios;
use std::cell::Cell;
use std::env;
use std::ffi::{c_int, c_short, c_ulong, c_void};
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

// Linux x86-64 numeric ABI; intentionally not compiled on other OS/arch.
const TIOCGWINSZ: c_ulong = 0x5413;
pub(crate) const O_NOCTTY: c_int = 0o400;
pub(crate) const O_NONBLOCK: c_int = 0o4000;
const POLLOUT: c_short = 0x004;
const POLL_FAILURE: c_short = 0x008 | 0x010 | 0x020; // ERR, HUP, NVAL
const SIGHUP: c_int = 1;
const SIGINT: c_int = 2;
const SIGQUIT: c_int = 3;
const SIGTERM: c_int = 15;
const SIGTSTP: c_int = 20;

const QUERY_CURSOR: &[u8] = b"\x1b[6n";
const SAVE_CURSOR: &[u8] = b"\x1b7";
const RESTORE_CURSOR: &[u8] = b"\x1b8";
// Xterm private-mode save/restore, supported by the VTE pilot.
const SAVE_CURSOR_VISIBILITY: &[u8] = b"\x1b[?25s";
const HIDE_CURSOR: &[u8] = b"\x1b[?25l";
const RESTORE_CURSOR_VISIBILITY: &[u8] = b"\x18\x1b[?25r";
const RESET: &[u8] = b"\x18\x1b[0m";
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

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: c_short,
    revents: c_short,
}

unsafe extern "C" {
    fn poll(fds: *mut PollFd, nfds: c_ulong, timeout_ms: c_int) -> c_int;
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

    fn wait_writable(&self) -> io::Result<bool> {
        let mut entry = PollFd {
            fd: self.fd(),
            events: POLLOUT,
            revents: 0,
        };
        // SAFETY: the owned terminal fd is live and the Linux pollfd
        // remains writable for the syscall. Bound the wait so q, resize and
        // signals are checked promptly even if output never drains.
        let status = unsafe {
            poll(
                &mut entry,
                1,
                playback::BACKPRESSURE_POLL.as_millis() as c_int,
            )
        };
        if status >= 0 {
            if entry.revents & POLL_FAILURE != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "terminal output is unavailable",
                ));
            }
            return Ok(entry.revents & POLLOUT != 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            Ok(false)
        } else {
            Err(error)
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
    cursor_visibility_saved: bool,
    inline: Cell<Option<InlineState>>,
    quit_pressed: Cell<bool>,
}

#[derive(Clone, Copy)]
struct InlineState {
    display: Display,
    rows: InlineRows,
    /// False while the output cursor may not be on the reserved lower line.
    anchored: bool,
    /// When all owned rows are still known to be hard breaks, this bounds
    /// the last column touched on each row of the committed drawing.
    hard_right: Option<u16>,
    /// Complete ownership is established; after a successful full redraw
    /// (which re-hardens these rows) a width-only fast path can resume.
    whole_area_visible: bool,
    /// A viewport transition was seen during a probe; even a return to the
    /// original size cannot then reuse the previous absolute erase range.
    recheck: bool,
}

impl InlineState {
    fn can_follow_width(self, display: Display) -> bool {
        self.anchored
            && !self.recheck
            && self.display.size.rows == display.size.rows
            && self
                .hard_right
                .is_some_and(|right| right <= display.size.cols)
    }
}

struct DrawingSurface {
    display: Display,
    inline: Option<InlineRows>,
    redraw: bool,
}

impl DrawingSurface {
    fn inline(display: Display, rows: InlineRows, redraw: bool) -> Self {
        Self {
            display,
            inline: Some(rows),
            redraw,
        }
    }
}

enum FrameStop {
    EndOfRun,
    Backpressure,
    ViewportChanged,
    Error(io::Error),
}

enum FrameResult {
    Submitted,
    EndOfRun,
    Dropped {
        wait_for_output: bool,
        partial: bool,
    },
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
    fn enter_island(&mut self) -> io::Result<()> {
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

    fn control(&self, bytes: &[u8], deadline: Instant) -> io::Result<()> {
        playback::write_bounded(&self.terminal.0, bytes, || {
            check_live(&self.terminal, deadline)
        })
    }

    fn cursor_reports<const N: usize>(&self, request: &[u8]) -> io::Result<[Cursor; N]> {
        let deadline = Instant::now() + CONTROL_BUDGET;
        self.control(request, deadline)?;
        let mut decoder = Decoder::default();
        let mut bytes = [0u8; 64];
        let mut cursors = [Cursor { row: 0, col: 0 }; N];
        let mut received = 0;
        loop {
            if Instant::now() >= deadline {
                return Err(io::Error::other(
                    "terminal did not report its cursor position",
                ));
            }
            check_live(&self.terminal, deadline)?;
            match (&self.terminal.0).read(&mut bytes) {
                Ok(count) if bytes[..count].contains(&b'q') => {
                    self.quit_pressed.set(true);
                    return Err(io::Error::other("playback stopped"));
                }
                Ok(count) => {
                    for cursor in bytes[..count].iter().filter_map(|&byte| decoder.feed(byte)) {
                        cursors[received] = cursor;
                        received += 1;
                        if received == N {
                            return Ok(cursors);
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error),
            }
            std::thread::sleep(playback::BACKPRESSURE_POLL);
        }
    }

    fn cursor(&self) -> io::Result<Cursor> {
        let [cursor] = self.cursor_reports(QUERY_CURSOR)?;
        Ok(cursor)
    }

    fn settle(&self, mut display: Display) -> io::Result<Display> {
        // Dragging an edge can deliver a series of ioctl sizes. Do not move
        // the live lower marker while the emulator is still reflowing it.
        const SETTLE: Duration = Duration::from_millis(120);
        let mut unchanged_since = Instant::now();
        let deadline = unchanged_since + CONTROL_BUDGET;
        loop {
            if self.stopping()? {
                return Err(io::Error::other("playback stopped"));
            }
            let actual = self.terminal.display()?;
            if actual != display {
                display = actual;
                unchanged_since = Instant::now();
            }
            if unchanged_since.elapsed() >= SETTLE {
                return Ok(display);
            }
            if Instant::now() >= deadline {
                // Return to the clock and key checks while the drag continues.
                return Err(io::ErrorKind::WouldBlock.into());
            }
            std::thread::sleep(TICK_POLL);
        }
    }

    fn enter_inline(&self, display: Display) -> io::Result<()> {
        let before = self.cursor()?;
        if before.col != 1 || !(2..=display.size.rows).contains(&before.row) {
            return Err(io::Error::other(
                "run oops on a fresh line in a normal shell",
            ));
        }
        let reserved = reservation(display.size);
        // EL must use the default background to trim, not fill, rows.
        let mut allocation = Vec::from(b"\x1b[0m");
        allocation.extend_from_slice(&b"\r\n".repeat(usize::from(reserved)));
        self.control(&allocation, Instant::now() + CONTROL_BUDGET)?;
        let below = self.cursor()?;
        let expected =
            (u32::from(before.row) + u32::from(reserved)).min(u32::from(display.size.rows));
        if below.col != 1
            || u32::from(below.row) != expected
            || display != self.terminal.display()?
        {
            return Err(io::Error::other(
                "terminal geometry changed while reserving inline rows",
            ));
        }
        let rows = InlineRows {
            top: below.row - reserved,
            below: below.row,
        };
        self.inline.set(Some(InlineState {
            display,
            rows,
            anchored: true,
            hard_right: Some(0),
            whole_area_visible: true,
            recheck: false,
        }));
        // DEC save/restore has one slot in VTE. We borrow it only in a
        // foreground shell, after the allocation's scrolling has finished.
        let mut packet = Vec::new();
        cup(&mut packet, rows.top, 1);
        packet.extend_from_slice(SAVE_CURSOR);
        cup(&mut packet, rows.below, 1);
        self.control(&packet, Instant::now() + CONTROL_BUDGET)
    }

    fn drawing_surface(&self, display: Display) -> io::Result<DrawingSurface> {
        let Some(state) = self.inline.get() else {
            return Ok(DrawingSurface {
                display,
                inline: None,
                redraw: false,
            });
        };
        if !state.recheck && state.display.size == display.size {
            // Pixel pitch may change without relocating either marker.
            self.inline.set(Some(InlineState { display, ..state }));
            return Ok(DrawingSurface::inline(display, state.rows, false));
        }
        if !state.anchored {
            return Err(io::Error::other(
                "resized during a partial inline draw; stopped to preserve shell output",
            ));
        }
        if state.can_follow_width(display) {
            if let Some(surface) = self.follow_width(state, display)? {
                return Ok(surface);
            }
        }
        let display = self.settle(display)?;
        if !display.size.fits() {
            return Err(io::Error::other(size_error(display.size)));
        }
        if !state.recheck && state.display == display {
            return Ok(DrawingSurface::inline(display, state.rows, false));
        }
        // Both cursor reports are requested in one packet, so normally
        // they describe the same reflow snapshot. Do not erase if a new
        // geometry nevertheless invalidates either marker.
        self.mark_anchor(false);
        let request = [
            b"\r".as_slice(),
            QUERY_CURSOR,
            RESTORE_CURSOR,
            b"\r",
            QUERY_CURSOR,
        ]
        .concat();
        let [below, top] = self.cursor_reports(&request)?;
        let actual = self.terminal.display()?;
        if top.col != 1
            || below.col != 1
            || top.row >= below.row
            || below.row > display.size.rows
            || display.size != actual.size
        {
            return Err(io::Error::other(format!(
                "cannot locate reserved inline rows after resize (top {top:?}, below {below:?}, expected {display:?}, read {actual:?})"
            )));
        }
        let rows = InlineRows {
            top: top.row,
            below: below.row,
        };
        let mut packet = Vec::new();
        cup(&mut packet, rows.below, 1);
        self.control(&packet, Instant::now() + CONTROL_BUDGET)?;
        self.inline.set(Some(InlineState {
            display: actual,
            rows,
            anchored: true,
            // The visible area is mapped by both reports. If the upper
            // marker might be clipped at row 1, do not infer its true span.
            hard_right: None,
            whole_area_visible: top.row > 1,
            recheck: false,
        }));
        Ok(DrawingSurface::inline(actual, rows, true))
    }

    fn follow_width(
        &self,
        state: InlineState,
        display: Display,
    ) -> io::Result<Option<DrawingSurface>> {
        // No cursor save/restore or erasure here. A same-height width change
        // cannot split our hard rows if the complete prior drawing still fits.
        // Earlier shell output may reflow, so measure the live lower row.
        let below = self.cursor()?;
        let actual = self.terminal.display()?;
        if display.size != actual.size {
            if !state.can_follow_width(actual) {
                self.inline.set(Some(InlineState {
                    recheck: true,
                    hard_right: None,
                    whole_area_visible: false,
                    ..state
                }));
            }
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let count = state.rows.below - state.rows.top;
        if below.col != 1 || below.row <= count || below.row > actual.size.rows {
            // The upper marker may have moved off-screen. Use the full
            // measured pair rather than guessing whether top was clipped.
            return Ok(None);
        }
        let rows = InlineRows {
            top: below.row - count,
            below: below.row,
        };
        self.inline.set(Some(InlineState {
            display: actual,
            rows,
            ..state
        }));
        Ok(Some(DrawingSurface::inline(actual, rows, true)))
    }

    fn note_submitted(&self, right: u16) {
        if let Some(state) = self.inline.get() {
            self.inline.set(Some(InlineState {
                hard_right: state.whole_area_visible.then_some(right),
                ..state
            }));
        }
    }

    fn mark_anchor(&self, anchored: bool) {
        if let Some(state) = self.inline.get() {
            self.inline.set(Some(InlineState { anchored, ..state }));
        }
    }

    fn enable_keys(&mut self) -> io::Result<()> {
        self.terminal.foreground()?;
        let fd = self.terminal.fd();
        let original = Termios::read(fd)?;
        self.original_keys = Some(original);
        original.apply_playback_keys(fd)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        let deadline = Instant::now() + CONTROL_BUDGET;
        self.control(SAVE_CURSOR_VISIBILITY, deadline)?;
        // Only restore a fully sent save. If the hide is interrupted, CAN
        // aborts the unfinished sequence and the saved value is still valid.
        self.cursor_visibility_saved = true;
        self.control(HIDE_CURSOR, deadline)
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

    fn frame(
        &self,
        packet: &[u8],
        viewport: Display,
        end: Option<Instant>,
        frame_deadline: Instant,
    ) -> io::Result<FrameResult> {
        use playback::PacketError;
        let result = playback::write_bounded_with_wait(
            &self.terminal.0,
            packet,
            || {
                if self.stopping().map_err(FrameStop::Error)? {
                    return Err(FrameStop::Error(io::Error::other("playback stopped")));
                }
                let now = Instant::now();
                if end.is_some_and(|deadline| now >= deadline) {
                    return Err(FrameStop::EndOfRun);
                }
                if now >= frame_deadline {
                    return Err(FrameStop::Backpressure);
                }
                let actual = self.terminal.display().map_err(FrameStop::Error)?;
                if actual == viewport {
                    Ok(())
                } else if !actual.size.fits() {
                    Err(FrameStop::Error(io::Error::other(size_error(actual.size))))
                } else {
                    Err(FrameStop::ViewportChanged)
                }
            },
            || self.terminal.wait_writable().map(|_| ()),
        );
        match result {
            Ok(()) => {
                self.mark_anchor(true);
                Ok(FrameResult::Submitted)
            }
            Err(PacketError::Stopped { cause, written }) => {
                if written != 0 {
                    self.mark_anchor(false);
                }
                match cause {
                    FrameStop::Backpressure | FrameStop::ViewportChanged => {
                        Ok(FrameResult::Dropped {
                            wait_for_output: matches!(cause, FrameStop::Backpressure),
                            partial: written != 0,
                        })
                    }
                    FrameStop::Error(error) => Err(error),
                    FrameStop::EndOfRun => Ok(FrameResult::EndOfRun),
                }
            }
            Err(PacketError::Io(error)) => {
                self.mark_anchor(false);
                Err(error)
            }
        }
    }

    fn restore(&mut self) -> io::Result<()> {
        // TCSANOW does not wait for the output queue; restore even if the
        // alternate-screen cleanup cannot reach a disconnected/wedged reader.
        let keys = self.original_keys.take().map_or(Ok(()), |original| {
            self.terminal.foreground()?;
            original.apply(self.terminal.fd())
        });
        let screen = self.leave_inline().and_then(|()| self.leave_island());
        // The cursor is independent of row ownership, even after a resize
        // interrupted cleanup. Never force a formerly hidden cursor visible.
        let cursor = if std::mem::take(&mut self.cursor_visibility_saved) {
            self.cleanup_write(RESTORE_CURSOR_VISIBILITY, None)
        } else {
            Ok(())
        };
        keys.and(screen).and(cursor)
    }

    fn cleanup_write(&self, packet: &[u8], size: Option<Size>) -> io::Result<()> {
        let deadline = Instant::now() + CONTROL_BUDGET;
        playback::write_bounded(&self.terminal.0, packet, || {
            if Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            self.terminal.foreground()?;
            if let Some(expected) = size {
                if self.terminal.display()?.size != expected {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
            }
            Ok(())
        })
    }

    fn leave_inline(&self) -> io::Result<()> {
        let Some(state) = self.inline.take() else {
            return Ok(());
        };
        let display = self.terminal.display()?;
        let mut packet = Vec::from(RESET);
        if !state.recheck && display.size == state.display.size {
            clear_rows(&mut packet, state.rows);
            cup(&mut packet, state.rows.top, 1);
            match self.cleanup_write(&packet, Some(display.size)) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    // A resize interrupted cleanup. Do not resume the old
                    // suffix, even if part of an erase sequence was sent.
                    packet.truncate(RESET.len());
                }
                result => return result,
            }
        } else if state.anchored {
            // The cursor itself has reflowed with the lower marker. Its
            // position is safe, but the old absolute erase range is not.
            packet.push(b'\r');
            return self.cleanup_write(&packet, None);
        }
        // Lost ownership of absolute rows during an interrupted draw or
        // cleanup. Append a fresh prompt line without erasing those rows.
        cup(&mut packet, self.terminal.display()?.size.rows.max(1), 1);
        packet.extend_from_slice(b"\r\n");
        self.cleanup_write(&packet, None)
    }

    fn leave_island(&mut self) -> io::Result<()> {
        if !std::mem::take(&mut self.entered) {
            return Ok(());
        }
        // CAN aborts a potentially partial CSI; leave only our alternate screen.
        self.cleanup_write(LEAVE, None)
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

fn play(
    screen: &Screen,
    settings: Settings,
    repeat: Repeat,
    stats: &mut Measurement,
    painter: &mut Painter,
) -> io::Result<()> {
    let start = Instant::now();
    let timeline = Timeline {
        cycle: settings.cycle,
        repeat,
    };
    let end = (repeat == Repeat::Once).then(|| start + timeline.cycle.length());
    let mut packet = Vec::with_capacity(64_000);
    let mut attempted = None;
    let mut awaiting_output = false;
    loop {
        if screen.stopping()? {
            return Ok(());
        }
        let now = Instant::now();
        let Some(slot) = timeline.sample(now.duration_since(start)) else {
            return Ok(());
        };
        let next = now + slot.until_next_tick;
        let join = now + slot.until_join;
        let observed = screen.terminal.display()?;
        if !observed.size.fits() {
            return Err(io::Error::other(size_error(observed.size)));
        }
        if awaiting_output {
            // A congested tty should not provoke fresh renders or more
            // writes until it can accept output again. Recompute the tick
            // and viewport after readiness, never resume the old prefix.
            awaiting_output = !screen.terminal.wait_writable()?;
            continue;
        }
        if attempted != Some(slot.phase) {
            attempted = Some(slot.phase);
            let begin_paint = Instant::now();
            let DrawingSurface {
                display: observed,
                inline,
                redraw,
            } = match screen.drawing_surface(observed) {
                Ok(surface) => surface,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                Err(error) => return Err(error),
            };
            let layout = Layout::for_surface(observed, settings, painter.frame_size(), inline);
            if redraw {
                painter.interrupted_submission();
            }
            let Some(trace) = painter.paint(slot.phase.tick, layout, &mut packet)? else {
                stats.record_hold(slot.phase);
                continue;
            };
            let encode = begin_paint.elapsed().saturating_sub(trace);
            stats.record_trace();
            // Compare both turn and tick so even a pause of a whole turn
            // cannot submit or suppress a packet at the same local tick.
            if timeline
                .sample(start.elapsed())
                .map(|current| current.phase)
                != Some(slot.phase)
            {
                stats.record_stale();
                continue;
            }
            let begin_write = Instant::now();
            // A partial packet that reaches a loop join is handled just
            // like a stalled submission: wait for readiness and resync.
            let deadline = (begin_write + FRAME_BUDGET).min(join);
            match screen.frame(&packet, observed, end, deadline)? {
                FrameResult::Submitted => {
                    let write = begin_write.elapsed();
                    stats.record_frame(
                        slot.phase,
                        layout,
                        packet.len(),
                        Instant::now() >= next,
                        Timings {
                            trace,
                            encode,
                            write,
                        },
                    );
                    let right = painter.submitted(layout);
                    screen.note_submitted(right);
                }
                FrameResult::EndOfRun => return Ok(()),
                FrameResult::Dropped {
                    wait_for_output,
                    partial,
                } => {
                    stats.record_drop(partial);
                    if partial {
                        painter.interrupted_submission();
                    }
                    awaiting_output = wait_for_output;
                    continue;
                }
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

pub fn run(
    check_only: bool,
    measure: bool,
    settings: Settings,
    repeat: Repeat,
    island: bool,
) -> u8 {
    let (terminal, display) = match preflight() {
        Ok(result) => result,
        Err(reason) => {
            eprintln!("oops: {reason}");
            return 2;
        }
    };
    let mut painter = match Painter::new() {
        Ok(painter) => painter,
        Err(error) => {
            eprintln!("oops: cannot initialize playback: {error}");
            return 2;
        }
    };
    if check_only {
        let layout = if island {
            Layout::for_surface(display, settings, painter.frame_size(), None)
        } else {
            // Preflight remains read-only: this is the planned inline
            // drawing size, before any terminal reflow or allocation.
            let rows = InlineRows {
                top: 1,
                below: 1 + reservation(display.size),
            };
            Layout::for_surface(display, settings, painter.frame_size(), Some(rows))
        };
        println!(
            "oops: {} columns by {} rows; model grid {} by {}, cell aspect {:.4} ({}), zoom {}%; content: {CONTENT_LABEL}; {} playback={repeat} surface={} frame={}x{}",
            display.size.cols,
            display.size.rows,
            layout.cols,
            layout.rows,
            layout.aspect,
            match layout.pitch {
                PitchSource::Measured => "tty pixels",
                other => other.label(),
            },
            settings.zoom.percent(),
            settings.playback_status(),
            if island { "island" } else { "inline" },
            painter.frame_size().0,
            painter.frame_size().1,
        );
        #[cfg(goodhand_w5_profile)]
        println!(
            "oops: W5 shading {} at this drawing layout",
            if layout.w5_active() {
                "active"
            } else {
                "inactive (using unmodified review tones)"
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
        cursor_visibility_saved: false,
        inline: Cell::new(None),
        quit_pressed: Cell::new(false),
    };
    let mut stats = Measurement::new(measure, settings, repeat);
    let playback = screen
        .enable_keys()
        .and_then(|()| {
            if island {
                screen.enter_island()
            } else {
                screen.enter_inline(display)
            }
        })
        .and_then(|()| screen.hide_cursor())
        .and_then(|()| play(&screen, settings, repeat, &mut stats, &mut painter));
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
