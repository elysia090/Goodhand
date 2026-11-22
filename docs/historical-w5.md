# Historical W5 experiment

This opt-in developer experiment is not part of the v0.0.4 package or
the default playback experience. Its source is kept for reference; the
required private W4 and W5 inputs are not distributed.

W5 groups the upper forearm return into a quieter dark plane and gives
the lower bearing plane more weight. It is a build-time opt-in layered on
the **exact reviewed W4 occupancy and tone pair**. The source W5
correction changes gray codes at the reviewed angles, not occupancy.
In the native window, only the signed correction is transferred from
the calibrated view to occupied native dots. For 30- and 45-second
turns, the private W5 motion archive supplies additional sampled
shapes and tones between reviewed phases. The motion archive must
match the approved window bounds and uses the same display grade.

From the repository root on GNU/Linux, with `sha256sum` and the
verified W4 and W5 motion pairs available locally (each directory
contains `shape.ghs` and `tone.ght`):

    GOODHAND_TONE_PROFILE=w5 \
      GOODHAND_PLAYBACK_DIR=/absolute/path/to/verified-w4 \
      GOODHAND_NATIVE_MOTION_DIR=/absolute/path/to/verified-w5-motion \
      cargo build --manifest-path tools/oops/Cargo.toml --offline --release --locked --features desktop-window

The build verifies the exact inputs and embeds them. It rejects a
missing or different motion pair for the reviewed W5 desktop build
instead of silently showing repeated poses for the default turn. To
build W5 for the terminal only, omit both `GOODHAND_NATIVE_MOTION_DIR`
and `--features desktop-window`. The terminal keeps the 288 reviewed
poses and its original 24 Hz timing.

This repository supplies the W5 rules and coefficients, not the
required W4 or motion archive. Neither the public offline references
nor a clean source-only build reproduce the reviewed W5 image. With no
archive, terminal playback uses the procedural developer scene and
native playback reports that the window is unavailable. With an
archive but no tone profile, playback uses its original tones and
hold schedule.

In a 116-column × 41-row terminal with a dark background, check and
play the W5 build from a normal prompt:

    ./tools/oops/target/release/oops --cell-aspect 0.5 --check
    ./tools/oops/target/release/oops --terminal --cell-aspect 0.5 --once

`--check` reports whether W5 is active for the planned layout. The
reviewed view uses a 115 × 27 Braille drawing grid, 0.5 cell aspect, 30-second
reverse turn, default zoom and brightness, and zero offsets. Other
settings or drawing grids use the original embedded tones. There is no
asset, checksum utility, or source dependency at runtime.
