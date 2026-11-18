use std::io::{self, Write};
use std::time::Duration;

pub const FPS: u64 = 24;
pub const RUN_TIME: Duration = Duration::from_secs(30);
pub const TICKS: u64 = RUN_TIME.as_secs() * FPS;

const NS_PER_SEC: u64 = 1_000_000_000;
pub const BACKPRESSURE_POLL: Duration = Duration::from_millis(4);

pub fn tick(elapsed: Duration) -> Option<u64> {
    (elapsed < RUN_TIME).then(|| (elapsed.as_nanos() as u64) * FPS / NS_PER_SEC)
}

pub fn start_of_tick(tick: u64) -> Duration {
    debug_assert!(tick <= TICKS);
    Duration::from_nanos((tick * NS_PER_SEC).div_ceil(FPS))
}

/// Only one packet is ever in flight. Check both cancellation and wall-clock
/// budget before every write or retry. Interrupted writes are replayed only
/// when the OS reports that zero bytes were written.
pub fn write_bounded(
    sink: impl Write,
    remaining: &[u8],
    check: impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    write_bounded_with_wait(sink, remaining, check, || {
        std::thread::sleep(BACKPRESSURE_POLL);
        Ok(())
    })
}

/// The output wait must be bounded so the next iteration can recheck
/// cancellation, viewport and wall-clock budget before writing again.
pub fn write_bounded_with_wait(
    mut sink: impl Write,
    mut remaining: &[u8],
    mut check: impl FnMut() -> io::Result<()>,
    mut wait: impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    while !remaining.is_empty() {
        check()?;
        match sink.write(remaining) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(count) => remaining = &remaining[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait()?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn exact_epoch_boundaries_through_all_720_ticks() {
        assert_eq!(TICKS, 720);
        assert_eq!(start_of_tick(TICKS), RUN_TIME);
        for t in 0..TICKS {
            assert_eq!(tick(start_of_tick(t)), Some(t));
            assert_eq!(
                tick(start_of_tick(t + 1) - Duration::from_nanos(1)),
                Some(t)
            );
        }
        assert_eq!(tick(RUN_TIME), None);
        assert_eq!(tick(RUN_TIME + Duration::from_secs(1)), None);
    }

    #[derive(Default)]
    struct Script {
        steps: VecDeque<io::Result<usize>>,
        written: Vec<u8>,
    }

    impl Write for Script {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let n = self.steps.pop_front().unwrap_or(Ok(bytes.len()))?;
            self.written.extend_from_slice(&bytes[..n]);
            Ok(n)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn short_writes_and_retries_do_not_repeat_prefix() {
        let mut out = Script {
            steps: [
                Ok(2),
                Err(io::ErrorKind::Interrupted.into()),
                Err(io::ErrorKind::WouldBlock.into()),
                Ok(1),
            ]
            .into(),
            ..Script::default()
        };
        let mut checks = 0;
        write_bounded(&mut out, b"abcdefg", || {
            checks += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(out.written, b"abcdefg");
        assert_eq!(checks, 5);
    }

    #[test]
    fn waits_for_readiness_only_when_output_would_block() {
        let mut out = Script {
            steps: [
                Ok(1),
                Err(io::ErrorKind::Interrupted.into()),
                Err(io::ErrorKind::WouldBlock.into()),
                Ok(2),
            ]
            .into(),
            ..Script::default()
        };
        let mut waits = 0;
        let mut checks = 0;
        write_bounded_with_wait(
            &mut out,
            b"abc",
            || {
                checks += 1;
                Ok(())
            },
            || {
                waits += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(out.written, b"abc");
        assert_eq!((waits, checks), (1, 4));
    }

    #[test]
    fn cancellation_does_not_send_unsubmitted_suffix() {
        let mut out = Script {
            steps: [Ok(2), Err(io::ErrorKind::WouldBlock.into())].into(),
            ..Script::default()
        };
        let mut checks = 0;
        let error = write_bounded(&mut out, b"abcdefg", || {
            checks += 1;
            if checks == 3 {
                return Err(io::ErrorKind::TimedOut.into());
            }
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(out.written, b"ab");
    }

    #[test]
    fn write_zero_and_io_errors_are_terminal_for_the_packet() {
        for (step, kind) in [
            (Ok(0), io::ErrorKind::WriteZero),
            (
                Err(io::ErrorKind::BrokenPipe.into()),
                io::ErrorKind::BrokenPipe,
            ),
        ] {
            let mut out = Script {
                steps: [step].into(),
                ..Script::default()
            };
            let error = write_bounded(&mut out, b"abc", || Ok(())).unwrap_err();
            assert_eq!(error.kind(), kind);
            assert!(out.written.is_empty());
        }
    }
}
