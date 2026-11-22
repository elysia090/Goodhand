# Goodhand

**oops** is a rotating hand in its own window. v0.0.4 loops the
288 original grayscale poses over 45 seconds on a near-black
background. It requests a 595×290 client area, excluding decorations.

## Downloads

Look for Linux x64 and Windows x64 packages on
[GitHub Releases](https://github.com/elysia090/Goodhand/releases).
x64 is for 64-bit Intel and AMD. This version does not provide
ARM64 or macOS downloads.

Extract the archive. Open `oops` on Linux or `oops.exe` on Windows.
Linux requires a GTK 3 desktop runtime and glibc 2.35 or later.
Native playback does not need a terminal, video player, Python,
a network connection or separate artwork files.

Close the window, or press **q** or **Esc**, to stop.
Run `./oops --once` on Linux or `.\oops.exe --once` in PowerShell
to play a single turn. Use `--duration 22` for a faster turn.
See [playback and configuration](tools/oops/README.md) for the
optional config and Linux terminal preview.

## Develop

- [Build, test and understand player behavior](docs/playback-development.md)
- [Rust library and offline tools](docs/development.md)
- [Design and quality gates](DESIGN.md)
- [Release checklist and verification scope](tools/oops/release/README.md)

## License

The software is [MIT](LICENSE). [Artwork and reference data](ARTWORK.md)
are not covered by that license. Executables that embed artwork require
a separate artwork notice.
