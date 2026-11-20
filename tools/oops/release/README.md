# oops 0.0.2 release checklist

Build Windows x64 and Linux x64 packages for this release. Both use a
compact native pane (663×290 logical client for the reviewed artwork)
and schedule the same 540 phases over one 45-second turn. The reviewed
wrist tone is applied by default in the opaque pane. GTK 3 also supports
its two existing translucent appearances. The Win32 and AppKit ports
use the native frame and the opaque artwork.

Both ARM64 downloads are out of scope for v0.0.2, and macOS downloads
remain on hold for both Intel and Apple silicon. Preserve the published
v0.0.1 tag and its packages. Do not publish only one of the two v0.0.2
packages.

## Distribution boundary

The software is MIT. Confirm permission for the exact final
540-phase shape and tone files, including extraction from the executable.
Keep the record of permission separately from the release. Prepare the
authorized artwork notice to include in the archive. A development-only
grant does not authorize a binary release.

The distribution build embeds only the final artwork pair. It refuses
`GOODHAND_PLAYBACK_DIR`, `GOODHAND_TONE_PROFILE` and
`GOODHAND_NATIVE_MOTION_DIR` in this mode, and validates the final pair
against the reviewed format and digests. Never transfer a private seed,
guide, source movie, intermediate asset or fixture to a release runner.

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

Set `GOODHAND_RELEASE_ARTWORK_DIR` to the absolute directory holding
verified `shape.ghs` and `tone.ght`. Leave the three developer variables
unset. Set a new clean `CARGO_TARGET_DIR` and remap absolute source
paths with rustc's `--remap-path-prefix` so packages do not expose local
build directories. Then run on each host:

```sh
cargo +1.100.0 test --manifest-path tools/oops/Cargo.toml --locked --offline --release --features desktop-window
cargo +1.100.0 build --manifest-path tools/oops/Cargo.toml --locked --offline --release --features desktop-window
```

The synthetic desktop CI workflow never ingests private assets or
publishes binaries. Passing it is only a build gate.

## Windows x64 release build in Actions

Manually run **Windows x64 release build** on `release/oops-v0.0.2`
before merge with its exact source commit SHA. Run it again on `main`
after merge if its release commit differs. The job fetches the existing v0.0.1
Linux x64 archive and verifies its pinned archive and executable SHA-256.
The final approved 540-phase artwork bytes have not changed. This uses
no draft release credentials and publishes no release.

The job extracts only that approved pair and its notice inside the
ephemeral runner, builds with MSVC and packages the real Windows ZIP
and checksum. Neither artwork input goes into the CI artifact. Confirm
the source SHA and the archive checksum from Actions. Desktop behavior
is separate from the CLI and build checks. Do not merge or ship until
the artifact's window controls and playback have been tested interactively on
Windows 11 x64. Run the hover and keyboard checks on the packaged build
with the pointer away at rest, after minimize/restore and on mixed-DPI
monitors. Also check accessibility focus and the contrast-theme fallback.

## Verify and package

1. Launch the final executable on every OS and CPU you intend to list.
   Check window title, initial aperture, first pose, native controls,
   q/Esc, resize, minimize/restore and `--once`. Compare the client
   against the reviewed Linux bitmap. Verify that the real native chrome
   blends at rest, reveals on hover and remains accessible by keyboard,
   without a geometry jump. Check OS-native fallback separately. On a
   normal desktop, check 540 phases in order and a seamless wrap; if the compositor
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
  --artwork-notice /path/to/authorized-artwork-notice.txt \
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
