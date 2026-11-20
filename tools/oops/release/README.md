# oops 0.0.1 release checklist

The release is not a terminal recorder. It shows the reviewed black
`oops` artwork in a native 1016×445 logical-pixel window, with a
30-second turn and 540 phases at 18 Hz by default. GTK 3 can also use
its two existing translucent appearances. The Win32 and AppKit ports
use the standard chrome and the black artwork only.

## Release status

No v0.0.1 package is published. Linux x64 has run with the reviewed
artwork on native Xfce. Linux ARM64 and both Windows architectures have
passed synthetic CI but still need native release checks. macOS
downloads are on hold for both Intel and Apple silicon; AppKit source
and synthetic CI remain in place.

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

For v0.0.1, put the approved Linux x64 binary archive on the
`v0.0.1` draft release; do not upload the extracted art. Manually run
**Windows x64 release build** on `main` with numeric IDs for the draft
and its Linux asset. The job's read-only token must be able to read that
draft. If it cannot, stop rather than making the draft public or adding
a long-lived credential.

The job checks the Linux archive and executable SHA-256, recovers the
approved final 540 phases within the runner, and builds Windows x64
with MSVC. Its artifact contains only the Windows ZIP and companion
checksum, not the intermediate files. CLI and build tests do not
certify a desktop launch. Download the artifact and test the enclosed
`oops.exe` before attaching the Windows ZIP to the release. This job
does not publish or modify the draft.

## Verify and package

1. Launch the final executable on every OS and CPU you intend to list.
   Check window title, initial aperture, first pose, native controls,
   q/Esc, resize, minimize/restore and `--once`. Compare the client
   against the reviewed Linux bitmap. On a normal desktop, check 540
   different phases in order and a seamless wrap; if the compositor
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
   `0644` for notices and checksum files. Reopen from a fresh extraction. Attach only the archives
   for the targets that passed and their matching checksums. The app
   must work without Python or a terminal for the native flow.

Before publishing, verify each package included in the release and its
artwork, signing and clean-download requirements. Omit held targets
rather than listing unavailable downloads.
