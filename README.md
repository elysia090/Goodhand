# Goodhand

**oops** is a rotating hand in its own window. In v0.0.2 the compact
window defaults to a 45-second turn with 540 distinct artwork phases.
The neutral artwork and native window frame keep the focus on the hand.

[![Source version: v0.0.2](https://img.shields.io/badge/source-v0.0.2-lightgrey?style=flat)](https://github.com/elysia090/Goodhand/releases)
[![Playback: 45 s per turn](https://img.shields.io/badge/playback-45%20s%20per%20turn-blue?style=flat)](#open-oops)
[![Chip targets: x64 and ARM64](https://img.shields.io/badge/targets-x64%20%7C%20ARM64-informational?style=flat)](#downloads)
[![Software license: MIT](https://img.shields.io/badge/license-MIT-green?style=flat)](LICENSE)

## Downloads

Available packages are listed on
[GitHub Releases](https://github.com/elysia090/Goodhand/releases). macOS
downloads are on hold; the AppKit source and its build checks stay in
the project.

| Desktop | Chip | Verification | Download |
| --- | --- | --- | --- |
| Linux | `x64` · Intel / AMD | Reviewed artwork ran on native Xfce | See Releases |
| Linux | `ARM64` · 64-bit Arm | Synthetic CI only | Pending native check |
| Windows | `x64` · Intel / AMD | Build and CLI checks; interactive behavior not verified | See Releases |
| Windows | `ARM64` · 64-bit Arm | Synthetic CI only | Pending native check |
| macOS | `x64` · Intel Mac | Synthetic CI only | On hold |
| macOS | `ARM64` · Apple silicon | Synthetic CI only | On hold |

Synthetic CI checks the player with generated test data, not the
release artwork. See the [release checklist](tools/oops/release/README.md)
for native verification and signing.

## Open oops

1. Open [GitHub Releases](https://github.com/elysia090/Goodhand/releases).
   Choose **x64** for 64-bit Intel or AMD, or **ARM64** for a 64-bit Arm
   chip, when a package is listed for your OS.
2. Extract the archive. On Windows, open `oops.exe` from the extracted
   folder. On Linux, open `oops` or run `./oops` from that folder.
   The Linux x64 build requires a GTK 3 desktop runtime and glibc 2.35
   or later.
3. Resize the window to fit. Close it, or press **q** or **Esc**, to stop.

To play one 45-second turn and exit, run `./oops --once` on Linux or
`.\oops.exe --once` in PowerShell on Windows. Add `--duration 30` to
use the earlier 30-second pace. The 540 phases are scheduled at 12 per
second by default or 18 per second for the 30-second turn.

The app does not need a terminal to play, a video player, or a network
connection. A busy desktop compositor can skip displayed frames.

## Develop

This repository contains a Rust library for shape, point tones, timing and
placement; the `oops` player; and offline tooling. A clean checkout does
not contain the reviewed release artwork. The Linux x64 terminal developer
preview is separate from the reviewed native artwork.

The compact dark pane uses Material dark-theme foreground references
and a localized wrist tone adjustment, without changing the artwork
archive. The original v0.0.1 release remains available.

- [Build and playback options](tools/oops/README.md)
- [Library, development checks and offline workflow](docs/development.md)
- [Design and quality gates](DESIGN.md)

## License

The software is [MIT](LICENSE). [Artwork and reference data](ARTWORK.md)
are not covered by that license. A distribution that embeds artwork must
include a separate artwork rights notice.
