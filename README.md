# Goodhand

**oops** is a rotating hand in its own window. A turn takes 30 seconds,
with 540 distinct artwork phases at 18 fps. The black canvas preserves the
original point grid and the familiar window frame belongs to your desktop.

**v0.0.1** · **18 fps** · **MIT software**

## Download

The v0.0.1 packages are being prepared. There are no v0.0.1 binaries to
download yet. Once verified and published, find them under
[Releases](https://github.com/elysia090/Goodhand/releases).

| Desktop | x64 · 64-bit Intel / AMD | ARM64 · 64-bit Arm |
| --- | --- | --- |
| Windows | `oops-v0.0.1-windows-x64.zip` | `oops-v0.0.1-windows-arm64.zip` |
| Linux | `oops-v0.0.1-linux-x64.tar.gz` | `oops-v0.0.1-linux-arm64.tar.gz` |
| macOS | `oops-v0.0.1-macos-x64.zip` · Intel Mac | `oops-v0.0.1-macos-arm64.zip` · Apple silicon |

The table names the planned artifacts; it does not claim verification on
every host. Desktop builds target GTK 3 on GNU/Linux, Win32 on Windows,
and AppKit on macOS. See the [release checklist](tools/oops/release/README.md)
for the verification and signing required per platform.

## Open oops

- **Windows:** extract the ZIP and open `oops.exe`.
- **Linux:** extract the archive and open `oops`, or run `./oops`. A GTK 3
  desktop runtime is required.
- **macOS:** extract the ZIP and open `oops.app`.

Close the window, press **q** or **Esc**, or use `--once` for one turn.
Resize to fit without stretching. The desktop app needs no terminal,
video player, Python or network connection. The display and compositor may
miss a frame under load.

## Develop

This repository contains a Rust library for shape, point tones, timing and
placement; the `oops` player; and offline tooling. A clean checkout does
not contain the reviewed release artwork. The Linux x64 terminal developer
preview is separate from the released native artwork.

- [Build and playback options](tools/oops/README.md)
- [Library, development checks and offline workflow](docs/development.md)
- [Design and quality gates](DESIGN.md)

## License

The software is [MIT](LICENSE). [Artwork and reference data](ARTWORK.md)
are not covered by that license. A distribution that embeds artwork must
include a separate artwork rights notice.
