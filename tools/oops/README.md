# oops 0.0.1 · playback pilot

### Island: an independent desktop window

Run `oops` from a normal shell in the optional Linux GTK3 desktop build.
`--window` and `--island` explicitly select the same window. Press q or
Esc to close, or use `oops --once` to stop after one turn. Successful
playback does not write to the shell. This path has been checked on an
Xfce composited GNU/Linux x86-64 desktop; it requires a composited RGBA
visual, GTK3/GDK and Cairo.

The default pane is pure black. The foreground uses its black-canvas
grade on the 200×120 source grid: at the starting size, a 2×2 mark per
occupied point and a five-pixel pitch. Occupied marks are opaque,
including black points. Inside the artwork only, a restrained black
matte fills some gaps between occupied points, fading out toward the
silhouette and open finger spaces. It doesn't change the dots' RGB or alpha, and it is invisible
when composited over pure black. The app neither blurs the image nor
reads the desktop.

- The one-pixel outline appears under the pointer and disappears when it
  leaves.
- To move the window, left-drag the upper middle about 24 pixels down
  from the rim, outside the buttons.
- To resize, left-drag the upper-left or lower-right corner, or an edge
  where the resize cursor appears. GTK delegates the gesture to the
  window manager. The clear pane's minimum window size is 840×420.
  The aperture keeps its aspect without clipping or stretching, even in
  a differently shaped window. Resizing changes only its display scale,
  using nearest-neighbor transport, not the source grid or timing.
- At the top right, hover for maximize/restore and close. At the bottom
  left, hover for appearance. Those controls do not resize or move the
  artwork.

The reviewed black pane opens in a 1024×576 client with a 1016×462
visible box inside. It uses tighter framing, a fixed leftward and
slightly upward placement, and 5 px rounded corners on both fill and
hover outline. Drag the visible black rim. A faint supporting gray
inside the lower forearm is exclusive to the black pane. It changes
only the tone of existing interior dots, not the source grid, native
scale or time. A larger embedded scene can retain the wide framing
rather than clip.

Use the appearance controls or set `window_appearance` in the explicit
config. `safe` is the default pure black pane. `clear` is the 1120×560
transparent pane; `framed` uses the same size and a light translucent
neutral fill, without blur. `--config`, `--once`, `--duration` and
`--direction` apply to Island. Terminal framing flags do not.

The default 30-second turn schedules all 288 poses on its 24 Hz clock,
holding them for three and two ticks in turn. Display refresh and actual
submissions depend on the compositor. The [gray backdrop study](../../study/gray-backdrop/)
is offline and does not change the appearance of Island.

### Terminal fallback

In a foreground Xfce/VTE-compatible GNU/Linux x86-64 terminal:

    oops --check
    oops --terminal
    oops --terminal --alternate-buffer
    oops --terminal --dots

The terminal path is explicit. `--terminal` draws below the command,
retaining the existing screen and scrollback. `--alternate-buffer` uses
the alternate screen in that same terminal. It does not open a window.
Use a dark background and at least 80 columns × 24 rows with UTF-8 and
true color. A larger viewport preserves more of the dot detail. Press q
or Ctrl-C to return to the shell, or pass `--once` for one turn.

The terminal maps a fixed, padded full-turn envelope onto the cell grid.
The embedded archive keeps all 288 scheduled poses on its regular 24 Hz
timeline. The terminal may miss submissions, and its shared cell colors
cannot preserve every independent native dot value. Neither Island nor
terminal playback needs external video, Python, or FFmpeg once built.

### Framing and configuration

    oops --check --zoom 85
    oops --terminal --zoom 120 --cell-aspect 0.4667
    oops --terminal --duration 24 --direction forward --offset-x -10 --brightness 85
    oops --config oops.example.conf --measure
    oops --terminal --once --duration 30

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
  on resize, and can crop at the drawing grid's edges.
- Braille is the default in both terminal buffers. Each cell
  samples a 2×4 grid and displays the rounded mean gray of its occupied
  dots. This shows more of the authored silhouette at the same size,
  but cannot preserve a different gray for every dot or add detail
  beyond the archive. Use `--dots` for the original one-bullet-per-cell
  renderer if the font does not draw single-cell Braille correctly. The
  asset-free procedural developer scene limits ray samples on large
  grids; the embedded artwork takes every Braille sample directly.
- `--brightness PERCENT` applies a linear gain of 0 through 200 to
  each displayed gray code, with integer rounding (ties up) and
  clipping to 255. The default, 100, applies no additional gain. At 0,
  all marks are black but retain their occupancy. This does not change
  terminal brightness, background, or text.
- No config is required or searched for. Copy
  [`oops.example.conf`](oops.example.conf), adjust the settings, and
  pass the path with `--config PATH`. The file is UTF-8, at most 8 KiB,
  with optional `zoom`, `cell_aspect`, `duration`, `direction`,
  `offset_x`, `offset_y`, `brightness`, and `renderer` keys, one `key = value`
  per line. Blank lines, whitespace around entries, and `#` comments are
  allowed. The precedence per setting is command line, explicit file,
  built-in default. Set `renderer = braille` or `renderer = dots`;
  `--dots` overrides either value. Unknown or duplicate keys, invalid
  values (even when overridden), and missing or unreadable explicit
  files stop startup before the app draws or reserves rows.

`--check` prints the compiled content, resolved settings, aspect source,
and drawing grid without starting playback. `--measure`
prints the playback settings and timing after exit, including the grid,
aspect, and zoom of the last fully submitted frame. Use `--once` with
`--measure` to automatically report after a single turn. Loop counters
cover the whole run; timing percentiles and maxima cover up to one
turn's worth of the most recent successfully submitted frames.
`written` counts completed TTY packets; `held` counts ticks that reused
the committed pose. `handled` and the phase markers include both.

### Playback and recovery

Only changed cell runs are written. A held archived pose on the same
viewport is neither rendered nor written again. The app temporarily turns off canonical
input and echo without changing terminal signals. Normal completion, q, and
handled signals restore the original keyboard attributes and cursor
visibility. Inline returns the prompt below the command and erases only
its reserved rows. Alternate-screen playback leaves its buffer so the
original shell is shown again. Inline
asks VTE for its two cursor positions, not existing screen text. As a
window or font resize settles, it re-fits the visible reserved area.
Ordinary width-only changes can redraw during the drag when the earlier
drawing is known not to wrap. Larger changes can cause VTE to reflow
rows before oops can redraw; it verifies the reserved positions first.
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

A build with no extra inputs uses the procedural developer scene.
Run that executable with `--terminal`; requesting Island gives a
diagnostic rather than silently switching to text. This is not the
reviewed desktop artwork. The checked-in offline references are not
used to build or run the player.

The optional archive build embeds a 200×120 GHSH occupancy and matching
GHT8 gray pair, with 288 poses at 24 Hz. This requires the absolute path
to an independently verified directory containing `shape.ghs` and
`tone.ght` at build time. A missing or invalid explicit input fails
the build. With a composited GTK3 desktop:

    GOODHAND_PLAYBACK_DIR=/absolute/path/to/verified-data \
      cargo build --offline --release --locked --features desktop-window

Omit `--features desktop-window` for a terminal-only build. To reproduce
the current reviewed black-canvas art, use the W5 inputs and switch below.
This project does not distribute the required W4 pair or a desktop binary.

    cargo test --offline --locked
    cargo clippy --offline --locked --all-targets -- -D warnings
    cargo build --offline --release --locked
    python3 tests/pty_smoke.py target/release/oops --full

### W5 shading build

W5 groups the upper forearm return into a quieter dark plane and gives
the lower bearing plane more weight. It is a build-time opt-in layered on
the **exact reviewed W4 occupancy and tone pair**. It changes gray codes
only at the reviewed angles, not occupancy or the 24 Hz timeline. In
Island, only the signed correction is transferred from the calibrated
view to occupied native dots; their original values are not averaged
into terminal-cell tones.

From the repository root on GNU/Linux, with `sha256sum` on the build host and the previously
verified W4 `shape.ghs` and `tone.ght` already available locally:

    GOODHAND_TONE_PROFILE=w5 \
      GOODHAND_PLAYBACK_DIR=/absolute/path/to/verified-w4 \
      cargo build --manifest-path tools/oops/Cargo.toml --offline --release --locked --features desktop-window

The build verifies both inputs before embedding them. It rejects other
archives rather than applying W5 to unrelated images. This repository
supplies the W5 rules and coefficients, not the required W4 archive.
Neither the public offline references nor a clean source-only build
reproduce the reviewed W5 image. With no archive, the executable runs the
procedural developer scene. With an archive but no tone profile, it
uses the archive's original tones, as before.

In a 116-column × 41-row terminal with a dark background, check and
play the W5 build from a normal prompt:

    ./tools/oops/target/release/oops --cell-aspect 0.5 --check
    ./tools/oops/target/release/oops --terminal --cell-aspect 0.5 --once

`--check` reports whether W5 is active for the planned layout. The
reviewed view uses a 115 × 27 Braille drawing grid, 0.5 cell aspect, 30-second
reverse turn, default zoom and brightness, and zero offsets. Other
settings or drawing grids use the original embedded tones. There is no
asset, checksum utility, or source dependency at runtime.

### Verification

The desktop-feature tests exercise raster coverage, opaque black,
interior matte containment, resize fit and pointer-control geometry
without a desktop session. The shared
archive tests check the hold schedule.
Kernel PTY checks separately exercise terminal geometry, deadlines and
teardown. Those checks do not establish compositor behavior, refresh,
or another platform’s behavior.
