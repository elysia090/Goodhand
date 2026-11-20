# Goodhand

**oops** is a rotating hand in its own window. A turn takes 30 seconds,
with 540 distinct artwork phases at 18 fps. The black canvas preserves the
original point grid and the familiar window frame belongs to your desktop.

![v0.0.1 planned, 18 fps, x64 and ARM64, MIT software](docs/release-badges.svg)

## Downloads

No v0.0.1 binaries have been published. Verified packages will appear on
[GitHub Releases](https://github.com/elysia090/Goodhand/releases). macOS
downloads are on hold; the AppKit source and its build checks stay in
the project.

| Desktop | Chip | Verification | Download |
| --- | --- | --- | --- |
| Linux | `x64` · Intel / AMD | Reviewed artwork ran on native Xfce | Not published |
| Linux | `ARM64` · 64-bit Arm | Synthetic CI only | Pending native check |
| Windows | `x64` · Intel / AMD | Synthetic CI only | Pending native check |
| Windows | `ARM64` · 64-bit Arm | Synthetic CI only | Pending native check |
| macOS | `x64` · Intel Mac | Synthetic CI only | On hold |
| macOS | `ARM64` · Apple silicon | Synthetic CI only | On hold |

Synthetic CI checks the player with generated test data, not the
release artwork. See the [release checklist](tools/oops/release/README.md)
for native verification and signing.

## Open oops

Once your package is available, extract it and open `oops.exe` on Windows
or `oops` on Linux. Linux needs a GTK 3 desktop runtime; you can also run
`./oops` from a shell.

Close the window, press **q** or **Esc**, or use `--once` for one turn.
Resize to fit without stretching. The desktop app needs no terminal,
video player, Python or network connection. The display and compositor may
miss a frame under load.

## Develop

This repository contains a Rust library for shape, point tones, timing and
placement; the `oops` player; and offline tooling. A clean checkout does
not contain the reviewed release artwork. The Linux x64 terminal developer
preview is separate from the reviewed native artwork.

- [Build and playback options](tools/oops/README.md)
- [Library, development checks and offline workflow](docs/development.md)
- [Design and quality gates](DESIGN.md)

## License

The software is [MIT](LICENSE). [Artwork and reference data](ARTWORK.md)
are not covered by that license. A distribution that embeds artwork must
include a separate artwork rights notice.
