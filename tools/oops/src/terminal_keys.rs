//! Minimal keyboard mode for Linux x86-64 + glibc; no external crates.
use std::ffi::{c_int, c_uint};
use std::io;
use std::os::fd::RawFd;

const TCSANOW: c_int = 0;
const ICANON: c_uint = 0o2;
const ECHO: c_uint = 0o10;
const ECHONL: c_uint = 0o100;
const VTIME: usize = 5;
const VMIN: usize = 6;

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Termios {
    iflag: c_uint,
    oflag: c_uint,
    cflag: c_uint,
    lflag: c_uint,
    line: u8,
    control: [u8; 32],
    input_speed: c_uint,
    output_speed: c_uint,
}

unsafe extern "C" {
    fn tcgetattr(fd: c_int, state: *mut Termios) -> c_int;
    fn tcsetattr(fd: c_int, when: c_int, state: *const Termios) -> c_int;
}

impl Termios {
    pub fn read(fd: RawFd) -> io::Result<Self> {
        let mut value = Self::default();
        // SAFETY: fd is live and the buffer matches this target's glibc layout.
        if unsafe { tcgetattr(fd, &mut value) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(value)
    }

    pub fn apply_playback_keys(mut self, fd: RawFd) -> io::Result<()> {
        // Keep ISIG and signal bindings exactly as they were. Do not flush
        // queues, change output processing, or enable terminal private modes.
        self.lflag &= !(ICANON | ECHO | ECHONL);
        self.control[VTIME] = 0;
        self.control[VMIN] = 0;
        self.apply(fd)
    }

    pub fn apply(&self, fd: RawFd) -> io::Result<()> {
        loop {
            // SAFETY: fd is live and the termios struct is initialized.
            if unsafe { tcsetattr(fd, TCSANOW, self) } != -1 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}

const _: () = assert!(std::mem::size_of::<Termios>() == 60);
