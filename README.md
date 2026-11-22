# Goodhand

**oops** is a rotating hand in its own window. The v0.0.4 source plays
the original 288-pose grayscale artwork on the cool near-black pane,
without later arm corrections. One turn takes 45 seconds by default.

[![Source version: v0.0.4](https://img.shields.io/badge/source-v0.0.4-lightgrey?style=flat)](https://github.com/elysia090/Goodhand/releases)
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
| Linux | `x64` · Intel / AMD | Exact-artwork Cairo renderer tests; see release notes | See Releases |
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
`.\oops.exe --once` in PowerShell on Windows. Use `--duration 22` for
a faster turn. The v0.0.4 build schedules its 288 original poses
at 6.4 per second by default; v0.0.2 uses a different 540-pose archive.

The app does not need a terminal to play, a video player, or a network
connection. A busy desktop compositor can skip displayed frames.

The exact-artwork native client defaults to 595×290 before the desktop
places it. Linux evens dot coverage at fractional sizes; this release
does not change the Windows renderer's scaling.

## Develop

This repository contains a Rust library for shape, point tones, timing
and placement; the `oops` player; and offline tooling. The full Git
checkout contains the historical baseline snapshot. The generated tag
source ZIP and tarball omit artwork data. The Linux x64 terminal
developer preview is separate from the native artwork.

- [Build and playback options](tools/oops/README.md)
- [Library, development checks and offline workflow](docs/development.md)
- [Design and quality gates](DESIGN.md)

## License

The software is [MIT](LICENSE). [Artwork and reference data](ARTWORK.md)
are not covered by that license. A distribution that embeds artwork must
include a separate artwork rights notice.
