#!/usr/bin/env python3
"""Package a reviewed native distribution locally; never upload anything."""
import argparse
import gzip
import hashlib
import platform
import shutil
import struct
import subprocess
import tarfile
import tempfile
import time
import zipfile
from pathlib import Path

VERSION = "0.0.1"
HOSTS = {
    ("Linux", "x86_64"): ("linux", "x64"),
    ("Linux", "aarch64"): ("linux", "arm64"),
    ("Darwin", "x86_64"): ("macos", "x64"),
    ("Darwin", "arm64"): ("macos", "arm64"),
    ("Windows", "AMD64"): ("windows", "x64"),
    ("Windows", "ARM64"): ("windows", "arm64"),
}
MOTION_DIGESTS = {
    "shape.ghs": "e44b22610dd3a02024181fd1b856cc1e33c15f13821e72ddda5d24784dffa6ff",
    "tone.ght": "52e71a2731a14902ba2bb8db25d141f4f619aa6b7b584eb2be1e47c9550f46e0",
}
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def executable_target(data):
    if data[:6] == b"\x7fELF\x02\x01":
        cpu = struct.unpack_from("<H", data, 18)[0]
        return ("linux", {62: "x64", 183: "arm64"}.get(cpu))
    if data[:4] == b"\xcf\xfa\xed\xfe":
        cpu = struct.unpack_from("<I", data, 4)[0]
        return ("macos", {0x1000007: "x64", 0x100000C: "arm64"}.get(cpu))
    if data[:2] == b"MZ":
        pe = struct.unpack_from("<I", data, 0x3C)[0]
        if data[pe:pe + 4] == b"PE\0\0":
            cpu = struct.unpack_from("<H", data, pe + 4)[0]
            kind = struct.unpack_from("<H", data, pe + 24)[0]
            subsystem = struct.unpack_from("<H", data, pe + 92)[0]
            if kind == 0x20B and subsystem == 2:
                return ("windows", {0x8664: "x64", 0xAA64: "arm64"}.get(cpu))
    raise ValueError("expected a 64-bit native desktop executable")


def validate_executable(executable, artwork, target):
    data = executable.read_bytes()
    if executable_target(data) != target:
        raise ValueError("executable platform or architecture does not match the packaging host")
    for filename, expected in MOTION_DIGESTS.items():
        payload = (artwork / filename).read_bytes()
        if hashlib.sha256(payload).hexdigest() != expected:
            raise ValueError(f"{filename} is not the reviewed final artwork")
        if payload not in data:
            raise ValueError(f"{filename} is not embedded in this executable")
    return hashlib.sha256(data).hexdigest()


def local_smoke(executable):
    version = subprocess.run(
        [str(executable), "--version"], check=True, capture_output=True,
        text=True, timeout=10,
    )
    if version.stdout.strip() != f"oops {VERSION}":
        raise ValueError("executable version is not oops " + VERSION)
    help_text = subprocess.run(
        [str(executable), "--help"], check=True, capture_output=True,
        text=True, timeout=10,
    ).stdout
    if "Desktop: reviewed W5 native artwork." not in help_text:
        raise ValueError("expected the final-art-only build, not the W4 review/terminal build")
    # Do not treat version output or metadata alone as a runnable app.
    start = time.monotonic()
    subprocess.run([str(executable), "--once"], check=True, timeout=40)
    if not 29.0 <= time.monotonic() - start <= 37.0:
        raise ValueError("native window did not complete a full 30-second run")


def linux_archive(stage, archive):
    def portable(entry):
        if entry.isdir():
            entry.mode = 0o755
        elif entry.isfile():
            entry.mode = 0o755 if Path(entry.name).name == "oops" else 0o644
        else:
            raise ValueError(f"unsupported archive entry: {entry.name}")
        entry.uid = entry.gid = entry.mtime = 0
        entry.uname = entry.gname = ""
        return entry

    # No dependence on the builder's umask, account name, or clock.
    with archive.open("wb") as outer:
        with gzip.GzipFile(filename="", fileobj=outer, mode="wb", mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as out:
                out.add(stage, arcname=stage.name, filter=portable)


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--executable", type=Path, required=True)
    cli.add_argument("--artwork", type=Path, required=True,
                     help="separately cleared final shape.ghs and tone.ght directory")
    cli.add_argument("--artwork-notice", type=Path, required=True,
                     help="artwork rights notice authorized for inclusion in the archive")
    cli.add_argument("--output", type=Path, required=True,
                     help="new output directory (must not already exist)")
    args = cli.parse_args()

    host = HOSTS.get((platform.system(), platform.machine()))
    if not host:
        cli.error("this build host is not in the x64/ARM64 native release matrix")
    os_name, cpu = host
    if os_name == "macos":
        translated = subprocess.run(
            ["sysctl", "-in", "sysctl.proc_translated"],
            text=True, capture_output=True,
        )
        if translated.stdout.strip() == "1":
            cli.error("package Intel macOS builds on Intel, not under Rosetta")
    notice = args.artwork_notice.read_bytes()
    if len(notice) > 16_384 or not notice.decode("utf-8").strip():
        cli.error("provide a non-empty UTF-8 artwork rights notice (max 16 KiB)")
    if args.output.exists():
        cli.error("output directory already exists")

    executable = args.executable.resolve(strict=True)
    # macOS expects a finished app that can be signed and notarized before
    # packaging. Copy it intact rather than mutating signed bundle contents.
    if os_name == "macos":
        app = executable.parent.parent.parent
        if executable.parts[-3:] != ("Contents", "MacOS", "oops") or app.name != "oops.app":
            cli.error("for macOS, use the executable inside the finished oops.app")
        allowed = {"Contents/Info.plist", "Contents/MacOS/oops",
                   "Contents/_CodeSignature/CodeResources", "Contents/CodeResources"}
        for item in app.rglob("*"):
            if item.is_symlink() or (
                not item.is_dir() and item.relative_to(app).as_posix() not in allowed
            ):
                cli.error(f"unexpected bundle entry: {item.relative_to(app)}")
        subprocess.run(["codesign", "--verify", "--strict", str(app)], check=True)
        subprocess.run(["xcrun", "stapler", "validate", str(app)], check=True)
    checksum = validate_executable(executable, args.artwork, host)
    local_smoke(executable)

    basename = f"oops-v{VERSION}-{os_name}-{cpu}"
    ext = ".tar.gz" if os_name == "linux" else ".zip"
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        stage = work / basename
        stage.mkdir()
        if os_name == "macos":
            subprocess.run(["ditto", str(app), str(stage / "oops.app")], check=True)
            archive_exe = "oops.app/Contents/MacOS/oops"
        else:
            archive_exe = "oops.exe" if os_name == "windows" else "oops"
            shutil.copy2(executable, stage / archive_exe)
        shutil.copyfile(ROOT / "LICENSE", stage / "LICENSE")
        shutil.copyfile(HERE / "README.txt", stage / "README.txt")
        (stage / "ARTWORK-LICENSE.txt").write_bytes(notice)
        (stage / "EXECUTABLE.sha256").write_text(
            f"{checksum}  {archive_exe}\n", encoding="ascii",
        )
        archive = work / (basename + ext)
        if os_name == "linux":
            linux_archive(stage, archive)
        elif os_name == "macos":
            subprocess.run(
                ["ditto", "-c", "-k", "--sequesterRsrc", "--keepParent",
                 str(stage), str(archive)], check=True,
            )
        else:
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as out:
                for path in sorted(stage.iterdir()):
                    out.write(path, arcname=f"{basename}/{path.name}")
        sha256 = hashlib.sha256(archive.read_bytes()).hexdigest()
        args.output.mkdir()
        shutil.move(archive, args.output / archive.name)
        (args.output / f"{archive.name}.sha256").write_text(
            f"{sha256}  {archive.name}\n", encoding="ascii",
        )


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, struct.error, subprocess.SubprocessError) as error:
        raise SystemExit(f"package: {error}") from error
