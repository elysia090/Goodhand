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
picture is centered in the drawing area (excluding the labels and the
last column) and letterboxed without stretching. The grid is capped at
220 columns and 80 rows to bound ray tracing. Resizing the window or
changing the terminal font zoom recomputes the fit from the geometry
reported by the terminal. The camera center and playback clock are not
reset. No sub-cell detail or presentation cadence is guaranteed. Gray
and occupancy remain separate.

### Framing and configuration

    oops --check --zoom 85
    oops --zoom 120 --cell-aspect 0.4667
    oops --config oops.example.conf --measure

- `--zoom PERCENT` accepts an integer from 50 through 200. At 100 the
  full model framing rectangle fits the drawable grid. Below 100 the
  drawing gets smaller around the same camera center; above 100 both
  axes enlarge uniformly and the edges can intentionally crop. The
  percentage is relative to the current fit, not to the terminal font
  size or a fixed cell count. The setting applies for the whole run.
- `--cell-aspect auto` (the default) derives cell width divided by cell
  height from TTY pixel geometry if it reports at least 3 × 5 pixels per
  cell with a ratio in 0.30–0.85. Otherwise it assumes 0.50 (1:2).
  If your terminal omits usable pixels and your font looks stretched,
  pass a measured ratio in 0.30–0.85. A manual value takes precedence
  over TTY measurements throughout the run. Recheck it after a font
  change that changes the cell proportions.
- No config is required or searched for. Copy
  [`oops.example.conf`](oops.example.conf), adjust the settings, and
  pass the path with `--config PATH`. The file is UTF-8, at most 8 KiB,
  with optional `zoom` and `cell_aspect` keys, one `key = value` per
  line. Blank lines, whitespace around entries, and `#` comments are
  allowed. The precedence per setting is command line, explicit file,
  built-in default. Unknown or duplicate keys, invalid values (even
  when overridden), and missing or unreadable explicit files stop
  startup before the app enters its alternate screen.

`--check` prints the resolved zoom, aspect source, and model grid
without entering the alternate screen. `--measure` prints those same
settings for the last fully submitted frame, alongside process timings.

### Playback and recovery

Only changed cell runs are written. The app temporarily turns off canonical
input and echo without changing terminal signals. Normal completion, q,
handled signals and controlled errors restore the original attributes and
leave the owned alternate screen. Forced kill, disconnection, and a wedged
terminal do not guarantee visual cleanup. A resize below the minimum exits
without forcing a corrupt draw. --measure reports timings after terminal
restoration. Timing percentiles and maxima describe frames submitted
completely to the tty. The stale counter counts completed renders discarded
before submission. The late-write counter counts completed writes that
crossed their tick boundary. When a nonblocking write would block, the
frame writer waits briefly for output readiness instead of imposing a
fixed delay before every retry. If a frame exhausts its write budget, the
player drops it and waits for output readiness without rendering a queue.
The 30-second deadline, q, resize checks and handled signals still apply.
If any prefix reached the terminal, the next current frame cancels an
incomplete terminal sequence, clears the owned alternate screen and
draws the full image rather than continuing the old diff. A usable
resize during submission also drops that frame and redraws at the new size.
After a reported viewport change, the next successfully submitted frame
uses a fresh canvas, even if the render grid does not change.
The dropped-write count includes these discarded submissions; partial-write
is the subset that wrote some bytes. Timing is submission to the tty,
not compositor or display timing.

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
