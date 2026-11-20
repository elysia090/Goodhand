use std::fmt;
use std::io::{self, Write};
use std::str::FromStr;
use std::time::Duration;

pub const FPS: u64 = 24;
const NS_PER_SEC: u64 = 1_000_000_000;
pub const BACKPRESSURE_POLL: Duration = Duration::from_millis(4);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CycleDuration(u8);

impl CycleDuration {
    pub const WINDOW_DEFAULT: Self = Self(45);

    pub const fn seconds(self) -> u8 {
        self.0
    }
}

impl Default for CycleDuration {
    fn default() -> Self {
        Self(30)
    }
}

impl FromStr for CycleDuration {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<u8>()
            .ok()
            .filter(|seconds| (12..=60).contains(seconds))
            .map(Self)
            .ok_or("expected whole seconds from 12 to 60")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Reverse,
    Forward,
}

impl FromStr for Direction {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "reverse" => Ok(Self::Reverse),
            "forward" => Ok(Self::Forward),
            _ => Err("expected reverse or forward"),
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Reverse => "reverse",
            Self::Forward => "forward",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cycle {
    pub duration: CycleDuration,
    pub direction: Direction,
}

impl Cycle {
    pub fn length(self) -> Duration {
        Duration::from_secs(u64::from(self.duration.seconds()))
    }

    pub fn ticks(self) -> u64 {
        self.length().as_secs() * FPS
    }

    pub fn tick(self, elapsed: Duration) -> Option<u64> {
        (elapsed < self.length()).then(|| elapsed.as_nanos() as u64 * FPS / NS_PER_SEC)
    }

    pub fn start_of_tick(self, tick: u64) -> Duration {
        debug_assert!(tick <= self.ticks());
        Duration::from_nanos((tick * NS_PER_SEC).div_ceil(FPS))
    }

    #[cfg(any(test, not(goodhand_embedded_playback)))]
    pub fn yaw(self, tick: u64) -> f64 {
        let degrees = match self.direction {
            Direction::Reverse => -360.0,
            Direction::Forward => 360.0,
        };
        degrees * tick as f64 / self.ticks() as f64
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Loop,
    Once,
}

impl fmt::Display for Repeat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Loop => "loop",
            Self::Once => "once",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Phase {
    pub turn: u64,
    pub tick: u64,
}

#[derive(Clone, Copy)]
pub struct Slot {
    pub phase: Phase,
    pub until_next_tick: Duration,
    pub until_join: Duration,
}

#[derive(Clone, Copy)]
pub struct Timeline {
    pub cycle: Cycle,
    pub repeat: Repeat,
}

impl Timeline {
    pub fn sample(self, elapsed: Duration) -> Option<Slot> {
        // Reduce whole seconds first: no accumulated tick count, converted
        // absolute nanoseconds, or repeated addition of cycle boundaries.
        let seconds = self.cycle.length().as_secs();
        let turn = elapsed.as_secs() / seconds;
        if self.repeat == Repeat::Once && turn != 0 {
            return None;
        }
        let within = Duration::new(elapsed.as_secs() % seconds, elapsed.subsec_nanos());
        let tick = self.cycle.tick(within)?;
        Some(Slot {
            phase: Phase { turn, tick },
            until_next_tick: self.cycle.start_of_tick(tick + 1) - within,
            until_join: self.cycle.length() - within,
        })
    }
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
    .map_err(|error| match error {
        PacketError::Stopped { cause, .. } | PacketError::Io(cause) => cause,
    })
}

#[derive(Debug)]
pub enum PacketError<E> {
    /// The caller stopped this packet before completion. An accepted prefix
    /// cannot be taken back and may include incomplete terminal sequences.
    Stopped {
        cause: E,
        written: usize,
    },
    Io(io::Error),
}

/// The output wait must be bounded so the next iteration can recheck
/// cancellation, viewport and wall-clock budget before writing again.
pub fn write_bounded_with_wait<E>(
    mut sink: impl Write,
    mut remaining: &[u8],
    mut check: impl FnMut() -> Result<(), E>,
    mut wait: impl FnMut() -> io::Result<()>,
) -> Result<(), PacketError<E>> {
    let len = remaining.len();
    while !remaining.is_empty() {
        if let Err(cause) = check() {
            return Err(PacketError::Stopped {
                cause,
                written: len - remaining.len(),
            });
        }
        match sink.write(remaining) {
            Ok(0) => return Err(PacketError::Io(io::ErrorKind::WriteZero.into())),
            Ok(count) => remaining = &remaining[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait().map_err(PacketError::Io)?;
            }
            Err(error) => return Err(PacketError::Io(error)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn exact_epoch_boundaries_for_each_supported_duration() {
        for seconds in 12..=60 {
            let cycle = Cycle {
                duration: CycleDuration(seconds),
                ..Cycle::default()
            };
            assert_eq!(cycle.ticks(), u64::from(seconds) * 24);
            assert_eq!(cycle.start_of_tick(cycle.ticks()), cycle.length());
            for tick in 0..cycle.ticks() {
                assert_eq!(cycle.tick(cycle.start_of_tick(tick)), Some(tick));
                assert_eq!(
                    cycle.tick(cycle.start_of_tick(tick + 1) - Duration::from_nanos(1)),
                    Some(tick)
                );
            }
            assert_eq!(cycle.tick(cycle.length()), None);
            assert_eq!(cycle.tick(Duration::MAX), None);
        }
    }

    #[test]
    fn procedural_yaw_preserves_default_and_reverses_about_opening_pose() {
        let reverse = Cycle::default();
        let forward = Cycle {
            direction: Direction::Forward,
            ..reverse
        };
        assert_eq!(reverse.ticks(), 720);
        for tick in 0..reverse.ticks() {
            assert_eq!(
                reverse.yaw(tick).to_bits(),
                (-360.0 * tick as f64 / 720.0).to_bits()
            );
            assert_eq!(forward.yaw(tick), -reverse.yaw(tick));
        }
        assert_eq!(forward.yaw(0), 0.0);
        assert!(forward.yaw(719) < 360.0);
    }

    #[test]
    fn timeline_wrap_is_continuous_and_uses_the_same_holds_each_turn() {
        let cycle = Cycle::default();
        let looping = Timeline {
            cycle,
            repeat: Repeat::Loop,
        };
        let once = Timeline {
            cycle,
            repeat: Repeat::Once,
        };
        for turn in [0, 1, 100_000_000] {
            let base = cycle.length() * turn;
            for tick in 0..cycle.ticks() {
                let slot = looping.sample(base + cycle.start_of_tick(tick)).unwrap();
                assert_eq!(
                    slot.phase,
                    Phase {
                        turn: u64::from(turn),
                        tick
                    }
                );
                assert_eq!(
                    slot.until_next_tick,
                    cycle.start_of_tick(tick + 1) - cycle.start_of_tick(tick)
                );
            }
            let last = looping
                .sample(base + cycle.length() - Duration::from_nanos(1))
                .unwrap();
            assert_eq!(last.phase.tick, 719);
            assert_eq!(last.until_next_tick, Duration::from_nanos(1));
            assert_eq!(last.until_join, Duration::from_nanos(1));
        }
        assert!(
            once.sample(cycle.length() - Duration::from_nanos(1))
                .is_some()
        );
        assert!(once.sample(cycle.length()).is_none());
        let next = looping.sample(cycle.length()).unwrap();
        assert_eq!(next.phase, Phase { turn: 1, tick: 0 });
        assert_eq!(next.until_join, cycle.length());
        let far = looping.sample(Duration::MAX).unwrap();
        assert!(far.phase.tick < cycle.ticks());
        assert!(far.until_next_tick <= far.until_join);
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
            || -> io::Result<()> {
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
    fn interrupted_packet_reports_exact_accepted_prefix() {
        for accepted in [0, 2] {
            let mut out = Script {
                steps: if accepted == 0 {
                    [Err(io::ErrorKind::WouldBlock.into())].into()
                } else {
                    [Ok(accepted), Err(io::ErrorKind::WouldBlock.into())].into()
                },
                ..Script::default()
            };
            let mut checks = 0;
            let err = write_bounded_with_wait(
                &mut out,
                b"abcdefg",
                || {
                    checks += 1;
                    if checks <= 1 + usize::from(accepted != 0) {
                        Ok(())
                    } else {
                        Err("expired")
                    }
                },
                || Ok(()),
            )
            .unwrap_err();
            assert!(matches!(
                err,
                PacketError::Stopped { cause: "expired", written } if written == accepted
            ));
            assert_eq!(out.written, b"abcdefg"[..accepted]);
        }
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
