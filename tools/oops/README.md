# oops 0.0.1 · terminal playback

The packaged review runs as a single executable. From a normal foreground
Xfce Terminal shell on GNU/Linux x86-64, run:

    ./oops --check
    ./oops

The minimum terminal is 92 columns × 45 rows with UTF-8, a single-cell
bullet and true color. Press q or Ctrl-C to stop early. The normal run
stops on its own after 30 seconds. No config, video, guide, external
artwork, Python, Rust installation or FFmpeg is required at runtime.
The ELF uses the host's standard GNU/Linux loader and shared libraries.

The review build holds its 288 native poses on the original 24 Hz clock
(three and two ticks alternately). Occupancy and gray remain separate:
black can be occupied. Nothing is fetched or reconstructed during
playback. The fixed source canvas is sampled onto the terminal's cell
grid with a uniform physical scale; no arbitrary stretching, contrast
curve or interpolation between poses is applied. The result is a cell
presentation, not pixel-exact video or a guarantee about every monitor,
font, frame submission, or screen refresh.

### Framing and configuration

    oops --check --zoom 85
    oops --zoom 120 --cell-aspect 0.4667
    oops --config oops.example.conf --measure

- `--zoom PERCENT` accepts an integer from 50 through 200. At 100 the
  complete source canvas fits the drawable grid. Below 100 the
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

`--check` prints the compiled content, resolved zoom, aspect source,
and drawing grid without entering the alternate screen. `--measure`
prints the grid, aspect and zoom of the last fully submitted frame
alongside process timings.

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

### Developer build and validation

The distribution build embeds only the reviewed 200×120 GHSH occupancy
and matching GHT8 gray, with 288 poses at 24 Hz. The ordered 30-second hold
schedule is fixed by the player. Supply the **absolute path** to a verified
private directory containing `shape.ghs` and `tone.ght` at build time:

    GOODHAND_PLAYBACK_DIR=/absolute/path/to/verified-data \
      cargo build --offline --release --locked

An explicit directory with missing or invalid data fails the build, not
falls back to a different scene. Check for `embedded review` in
`target/release/oops --help` before packaging. Do not include the build
input directory in the distribution. Source checks without that variable
use the procedural developer scene; that executable is not the reviewed
package. CI also exercises the embedded path with synthetic data.

    cargo test --offline --locked
    cargo clippy --offline --locked --all-targets -- -D warnings
    python3 tests/pty_smoke.py target/release/oops --full

Native lossless verification compares the ordered, decoded 2×2-dot raster
and its timestamps; kernel PTY checks separately exercise the terminal
geometry, deadlines and teardown. A passing headless check does not certify
the result on another terminal or physical display.
