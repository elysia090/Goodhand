# oops 0.0.1 · procedural terminal pilot

This pilot renders the Goodhand arm directly from its Rust model. It
does not read or embed footage, raster frames, archived surfaces, or pose
guides. The CLI samples the default base only; authored attributes are off.

From a normal foreground Xfce Terminal shell on GNU/Linux x86-64, run:

    oops --check
    oops

The minimum foreground terminal is 92 columns × 45 rows with a UTF-8
single-cell bullet and true-color support. Maximize or zoom out if the
window is too small. q and Ctrl-C stop playback; the normal run stops on
its own after 30 seconds.

One 24 Hz timeline turn decreases yaw from 0° to −360°. The current model
is sampled at each available tick; nothing is loaded from old frames or
replayed to catch up. There is one orthographic ray per active cell. The
picture is centered and letterboxed without stretching, capped at 220
columns and 80 rows to bound ray tracing. Cell pitch is estimated from
TTY pixel geometry when usable, otherwise width:height 1:2 is assumed.
This preserves modeled proportions on the calibrated profile, not for
every font or emulator. No sub-cell detail or presentation cadence is
guaranteed. Gray and occupancy remain separate.

Only changed cell runs are written. The app temporarily turns off canonical
input and echo without changing terminal signals. Normal completion, q,
handled signals and controlled errors restore the original attributes and
leave the owned alternate screen. Forced kill, disconnection, and a wedged
terminal do not guarantee visual cleanup. A resize below the minimum exits
without forcing a corrupt draw. --measure reports timings after terminal
restoration. Timing percentiles and maxima describe frames submitted
completely to the tty. The stale counter counts completed renders discarded
before submission. The late-write counter counts completed writes that
crossed their tick boundary. The measured write step is submission to the
tty, not compositor or display timing. Work can be dropped under load,
and the CLI never queues stale frames to catch up.

In one GNU/Linux x86-64 Xfce Terminal run per size (VTE 0.80.1, default
base, width:height pitch assumed 1:2), the pilot observed:

| Terminal | Model grid | Elapsed | Submitted | Trace / tty write p95 |
| --- | --- | --- | --- | --- |
| 165 × 55 | 164 × 49 | 30.006 s | 708 / 720 | 13.951 / 9.443 ms |
| 290 × 97 | 220 × 66 | 30.005 s | 711 / 720 | 18.810 / 12.839 ms |

Both runs and separate q and Ctrl-C tests restored the shell and its tty
settings. These measurements describe one shared host and terminal. They do
not guarantee every tick, 24 visible frames per second, the look at every
font/size, or performance on other hardware.

The pilot builds into a single dynamically linked GNU/Linux x86-64 ELF
with no runtime assets or external executable dependencies. The repository's
Goodhand Cargo crate supplies the model at build time.

    cargo build --offline --release --locked
    cargo test --offline --locked
    cargo clippy --offline --locked --all-targets -- -D warnings
    python3 tests/pty_smoke.py target/release/oops --full
