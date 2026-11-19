# oops 0.0.1 · terminal playback

The packaged review runs as a single executable. From a normal foreground
Xfce Terminal shell on GNU/Linux x86-64, run:

    ./oops --check
    ./oops
    ./oops --island

By default, oops draws only the artwork in the current shell, immediately
below the command. It leaves prior shell output in the normal screen and
scrollback, reserving at most three quarters of the visible rows (and no
more than 80 rows). Use a dark terminal background to see the original
gray levels. `--island` draws only the artwork in a separate terminal
buffer, using the current window size; it does not maximize the window.
The minimum is 80 columns × 24 rows with UTF-8, a single-cell bullet, and
true color. Drawings at the minimum size have fewer dots; enlarge the
terminal or use Island for more detail. Playback loops by default, taking 30 seconds per
turn. Press q or Ctrl-C to return to the shell, or run `./oops --once`
to stop automatically at the end of the first turn. No config, video, guide, external
artwork, Python, Rust installation or FFmpeg is required at runtime.
The ELF uses the host's standard GNU/Linux loader and shared libraries.

By default the review build holds its 288 native poses on the 24 Hz clock
for three and two ticks alternately, in the reviewed direction. Occupancy and gray remain separate:
black can be occupied. Nothing is fetched or reconstructed during
playback. A single envelope around all occupied samples in the full turn, plus
five sample tiles on each side, is fit onto the terminal grid at a
uniform scale. The framing and center are fixed across poses. This
keeps the art larger without cropping at the default zoom. At small
sizes, some detail is necessarily lost to the coarser cell grid.
The original reviewed canvas, pixels, and pose timing are unchanged. The result is a cell
presentation, not pixel-exact video or a guarantee about every monitor,
font, frame submission, or screen refresh.

### Framing and configuration

    oops --check --zoom 85
    oops --zoom 120 --cell-aspect 0.4667
    oops --duration 24 --direction forward --offset-x -10 --brightness 85
    oops --config oops.example.conf --measure
    oops --once --duration 30

- `--zoom PERCENT` accepts an integer from 50 through 200. At 100 the
  full-turn envelope and its margin fit the drawable grid. Below 100 the
  drawing gets smaller around the same center; above 100 both
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
- `--duration SECONDS` selects 12 through 60 whole seconds for one turn.
  The timeline always runs at 24 Hz. Each archived pose starts at the
  nearest tick to its equally spaced time (ties round up), so every
  pose has at least one scheduled tick. A slow terminal may still miss
  submissions. The procedural developer scene samples the corresponding
  yaw on each tick.
- `--direction reverse` (the default) follows the reviewed order.
  `--direction forward` uses the same opening pose and visits the other
  poses in reverse order, with the same hold schedule. It does not
  mirror the image. The last pose precedes the wrap; the opening pose
  is played at the next turn's first tick without a pause or extra hold.
- `--offset-x PERCENT` and `--offset-y PERCENT` shift the artwork from
  center by signed whole percentages, from -50 to 50, of the current
  drawing grid's width and height, respectively. Positive x moves right;
  positive y moves down. Shifts are independent of zoom, stay proportional
  on resize, and can crop at the drawing grid's edges. The header and
  footer do not move with the picture.
- `--brightness PERCENT` applies a linear gain of 0 through 200 to
  each occupied gray code, with integer rounding (ties up) and clipping
  to 255. The default, 100, is unchanged. At 0, every dot is black;
  the dots still have occupancy. This does not change terminal
  brightness, background, or text, and does not restore clipped
  highlights.
- No config is required or searched for. Copy
  [`oops.example.conf`](oops.example.conf), adjust the settings, and
  pass the path with `--config PATH`. The file is UTF-8, at most 8 KiB,
  with optional `zoom`, `cell_aspect`, `duration`, `direction`,
  `offset_x`, `offset_y`, and `brightness` keys, one `key = value`
  per line. Blank lines, whitespace around entries, and `#` comments are
  allowed. The precedence per setting is command line, explicit file,
  built-in default. Unknown or duplicate keys, invalid values (even
  when overridden), and missing or unreadable explicit files stop
  startup before the app draws or reserves rows.

`--check` prints the compiled content, resolved settings, aspect source,
and drawing grid without starting playback. `--measure`
prints the playback settings and timing after exit, including the grid,
aspect, and zoom of the last fully submitted frame. Use `--once` with
`--measure` to automatically report after a single turn. Loop counters
cover the whole run; timing percentiles and maxima cover up to one
turn's worth of the most recent successfully submitted frames.

### Playback and recovery

Only changed cell runs are written. The app temporarily turns off canonical
input and echo without changing terminal signals. Normal completion, q, and
handled signals restore the original keyboard attributes. Inline returns
the prompt below the command and erases only its reserved rows. Island
leaves the separate buffer so the original shell is shown again. Inline
asks VTE for its two cursor positions, not existing screen text. As a
window or font resize settles, it re-fits the visible reserved area.
Normal terminal scrolling can move older commands or previously drawn
cells into scrollback; oops does not erase scrollback. If a resize
interrupts a partial inline write or the anchors cannot be verified,
playback stops without erasing uncertain rows. A resize below the
minimum also stops. Forced kill,
disconnection, and a wedged terminal do not guarantee visual cleanup.
`--measure` reports timings after terminal restoration. Timing percentiles and maxima describe frames submitted
completely to the tty. The stale counter counts completed renders discarded
before submission. The late-write counter counts completed writes that
crossed their tick boundary. When a nonblocking write would block, the
frame writer waits briefly for output readiness instead of imposing a
fixed delay before every retry. If a frame exhausts its write budget, the
player drops it and waits for output readiness without rendering a queue.
Loop joins stay on the original clock: no screen restart, reset delay,
or queue of missed poses. A pending write that runs into a join is
discarded if it has not completed. q, resize checks and handled signals
still apply; `--once` also imposes a final deadline.
If any prefix reached the terminal, the next current frame cancels an
incomplete terminal sequence, clears only the area it owns, and draws the
full image rather than continuing the old diff. A usable
resize during submission also drops that frame and redraws at the new size.
After a reported viewport change, the next successfully submitted frame
uses a fresh canvas, even if the render grid does not change.
The dropped-write count includes these discarded submissions; partial-write
is the subset that wrote some bytes. Timing is submission to the tty,
not compositor or display timing.

### Developer build and validation

The distribution build embeds only the reviewed 200×120 GHSH occupancy
and matching GHT8 gray, with 288 poses at 24 Hz. The default player schedule
matches the ordered 30-second review. Supply the **absolute path** to a verified
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
