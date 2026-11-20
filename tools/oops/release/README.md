# oops 0.0.3 release checklist

Build and verify Windows x64 and Linux x64 together. macOS stays on
hold; ARM64 downloads are out of scope. Preserve existing tags and
packages. The default 663×290 native client plays the exact historical
288-pose grayscale artwork over 45 seconds on the `#0C1012` pane.
No later arm correction or interpolation is applied.

## Distribution boundary

The software is MIT. The separate artwork grant must cover the exact
288-phase baseline pair, including extraction from the executable.
Include [the separate artwork notice](ARTWORK-BASELINE.txt) with each
package. Do not attach raw art inputs, source footage, guides or a
private review archive. Published W5 package permission is separate.

The exact previously archived source snapshot can be read from a full
checkout. Tag source ZIP/tar downloads use `export-ignore` to exclude
both historical artwork datasets; verify the tag's generated archives
before publishing. The only new artwork bytes sent outside the build
runner must be inside the finished executable.

## Build on the target architecture

| Target | Native build | Test scope |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | GNU/Linux x64, GTK 3 | X11 and/or Wayland path actually supported by the package |
| `aarch64-unknown-linux-gnu` | GNU/Linux ARM64, GTK 3 | Native ARM64 chip, not x64 emulation |
| `x86_64-pc-windows-msvc` | Windows x64, MSVC/Windows SDK | Tested x64 desktop versions; Win32 API floor Windows 10 1703 |
| `aarch64-pc-windows-msvc` | Windows ARM64, MSVC/Windows SDK | Native Windows 11 ARM64 |
| `x86_64-apple-darwin` | Intel Mac, Xcode CLI tools | Native Intel Mac, not Rosetta |
| `aarch64-apple-darwin` | Apple silicon, Xcode CLI tools | Native Apple silicon |

Use Rust 1.100.0 (the CI toolchain). Build Linux packages against a
deliberately chosen glibc baseline. This branch checks Ubuntu 24.04;
don't advertise older distributions without running them. Linux needs
GTK 3, GDK and Cairo at runtime. On macOS set
`MACOSX_DEPLOYMENT_TARGET=11.0` for both CPU builds; deployment version
is not a substitute for native testing. The Win32 host requires the
matching architecture's Visual Studio Developer shell. For the digest
check, make GNU `sha256sum` available (Git for Windows supplies it);
macOS uses system `shasum -a 256`.

Prepare the exact archive from the full checkout with Python and NumPy,
then set `GOODHAND_BASELINE_ARTWORK_DIR` to its absolute directory.
Do not set the historical W4/W5 build variables. Remap builder-specific
paths and use a fresh target directory. On each target host:

```sh
artwork=/absolute/path/to/new-private-directory
python tools/oops/release/prepare_baseline.py \
  reference/source-study/pixels.npz "$artwork"
export GOODHAND_BASELINE_ARTWORK_DIR="$artwork"
cargo +1.100.0 test --manifest-path tools/oops/Cargo.toml --locked --offline --release --features desktop-window
cargo +1.100.0 build --manifest-path tools/oops/Cargo.toml --locked --offline --release --features desktop-window
```

Synthetic desktop CI never ingests private inputs or publishes
binaries. Passing it is a build gate, not desktop verification.

## Windows x64 release build in Actions

Manually run **Windows x64 release build** on `release/oops-v0.0.3` or
`main` with the exact source commit SHA. Enable
`artwork_scope_confirmed` only after distribution approval has been
recorded. The job reads the pinned archive already in source history,
packs it only on the runner, and uploads just the finished Windows
archive and its SHA-256. It uses read-only permissions and does not
publish or change releases.

Check the artifact's source SHA and both checksums. Build and CLI
checks do not establish real Windows hover behavior. Record the
interactive results separately; distinguish tested and unverified
environments in release notes.

## Verify and package

1. Launch the final executable on every OS and CPU you intend to list.
   Check window title, initial aperture, first pose, native controls,
   q/Esc, resize, minimize/restore and `--once`. Compare the client
   against the reviewed Linux bitmap. Verify that the real native chrome
   blends at rest, reveals on hover and remains accessible by keyboard,
   without a geometry jump. Check OS-native fallback separately. On a
   normal desktop, check all 288 phases in order and a seamless wrap; if the compositor
   drops a phase, note the observation instead of certifying cadence
   solely from a compressed recording or automated build.
2. On macOS create `oops.app/Contents/MacOS/oops` from the executable
   and copy [Info.plist](Info.plist) to `oops.app/Contents/Info.plist`.
   Verify the architecture, then Developer ID sign, notarize and staple
   the app before packaging. Reverify the extracted signed app. On
   Windows sign Authenticode as applicable and test the downloaded ZIP
   on a machine without the developer tools. Do not instruct recipients
   to disable platform security checks.
3. Run the packager on the native interactive host. It checks the CPU,
   GUI subsystem on Windows, release build identity, embedded artwork
   bytes, version, and whole-turn exit. Use the signed bundle executable
   on macOS:

```sh
python3 tools/oops/release/package.py \
  --executable /path/to/oops \
  --artwork /path/to/cleared-final-pair \
  --artwork-notice tools/oops/release/ARTWORK-BASELINE.txt \
  --output /path/to/new-package-directory
```

4. Check the archive's companion SHA-256 and the internal executable's
   checksum. Linux archives use `0755` for directories and `oops`, and
   `0644` for notices and checksum files. Reopen from a fresh
   extraction. Attach both x64 archives and matching checksums. The app
   must work without Python or a terminal for the native flow.

Before publishing, verify each package included in the release and its
artwork, signing and clean-download requirements. Omit held targets
rather than listing unavailable downloads.
