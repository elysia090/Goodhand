# oops playback development

For normal use, see [playback and configuration](../tools/oops/README.md).
For package preparation and supported targets, see the
[release checklist](../tools/oops/release/README.md).

## Native presentation

The v0.0.4 build uses the exact original grayscale checkpoint (P3-2):
288 original poses, separate occupancy and gray, no additional grading
or interpolation. The release scheduler distributes poses uniformly over
a 45-second turn by default. The 22-second offline proof uses a separate
24 fps container, not the runtime schedule.

The default 595×290 client excludes native decorations. The desktop can
still tile or resize it. The 1016×445 composition aperture fits without
clipping or stretching. The opaque pane is cool near-black (`#0C1012`).

The real window frame and its hit targets are always present. Under
Xfwm on X11, the opaque pane hides the native decorations visually when
idle. Hover or use the keyboard to reveal the real title bar and buttons
without moving or resizing the artwork. Other Linux window managers,
Wayland, the HighContrast GTK theme and translucent modes keep their
usual visible decorations.

The Windows 11 candidate keeps the Win32 frame and uses a
click-through color mask over its caption at rest. Pointer and
keyboard use reveal the genuine caption buttons and system menu
without changing the client rectangle. Windows draws those controls.
The masked mode uses square native window corners to contain its
caption mask. Older Windows, high contrast and unsupported systems keep
their regular visible chrome. Interactive Windows testing is separate
from the build and CLI checks.

On a composited Linux desktop, press t to cycle appearances, or set
`window_appearance` in the config. Windows and macOS support the black
`safe` appearance; they report an error for the other appearances.
On Linux, `safe` is opaque near-black, `clear` is transparent, and
`framed` adds a lightly translucent neutral background. The native
frame is the same in all three modes.

In the Linux archived-artwork build, the opaque pane samples fractional
fits by the portion of each device pixel covered by a dot. This avoids
alternating thin and thick rows or columns without moving the aperture
or changing the stored tones. Whole-pixel fits use the existing Cairo
draw; transparent modes, nonmatching device transforms and unusually
large buffers also keep that path. Win32, AppKit and terminal rendering
are unchanged. A photographed or externally scaled display can still
show moiré.

## Terminal playback and recovery

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
`--measure` reports timing after terminal restoration. `written` counts
complete TTY packets; `held` counts ticks reusing a committed pose.
`handled` and the phase markers include both. Timing percentiles and
maxima describe frames submitted completely to the TTY. The stale counter counts completed renders discarded
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

## Developer builds

A build with no extra inputs uses the procedural developer scene.
Run that executable with `--terminal`; requesting Island gives a
diagnostic rather than silently switching to text. This is not the
reviewed desktop artwork. This source-only build does not use the checked-in offline references.

The optional archive build embeds a 200×120 GHSH occupancy and matching
GHT8 gray pair, with 288 poses at 24 Hz. This requires the absolute path
to an independently verified directory containing `shape.ghs` and
`tone.ght` at build time. A missing or invalid explicit input fails
the build. With a GTK3 desktop:

    GOODHAND_PLAYBACK_DIR=/absolute/path/to/verified-data \
      cargo build --manifest-path tools/oops/Cargo.toml --offline --release --locked --features desktop-window

Omit `--features desktop-window` for a terminal-only build. The baseline
window build uses the exact archive selected below, not a terminal
fixture or a private W4 input.

From the repository root, run the source-only checks without build-time
archive variables:

    cargo test --manifest-path tools/oops/Cargo.toml --offline --locked
    cargo clippy --manifest-path tools/oops/Cargo.toml --offline --locked --all-targets -- -D warnings
    cargo build --manifest-path tools/oops/Cargo.toml --offline --release --locked
    python3 tools/oops/tests/pty_smoke.py tools/oops/target/release/oops --full

## Native baseline build

From a full Git checkout, pack the exact historical artwork using
Python with NumPy. Use an absolute output path outside the checkout
and do not store or upload the generated pair separately.

```sh
artwork=/absolute/path/to/new-private-directory
python tools/oops/release/prepare_baseline.py \
  reference/source-study/pixels.npz "$artwork"
GOODHAND_BASELINE_ARTWORK_DIR="$artwork" \
  cargo build --manifest-path tools/oops/Cargo.toml --offline --release --locked --features desktop-window
```

In this build the baseline is the default native artwork; there is no
runtime artwork selector. Python and the artwork files are not needed
at runtime. See the [release checklist](../tools/oops/release/README.md) for the
separate artwork notice and distribution checks.

## Verification

The desktop-feature tests exercise raster coverage, pane compositing
and resize fit without a desktop session. The exact baseline build
tests all 288 distinct rasters, 45-second boundaries in both
directions, and pass-through of occupied gray and coverage. Historical
W5 desktop inputs retain their separate tests.
The shared archive tests check the original terminal hold schedule.
Kernel PTY checks separately exercise terminal geometry, deadlines and
teardown. Those checks do not establish compositor behavior, refresh,
or another platform’s behavior.

Historical development is documented separately in the [W5 experiment](historical-w5.md).
