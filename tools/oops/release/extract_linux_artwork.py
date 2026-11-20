"""Stage the reviewed final artwork inside the Windows release job."""

import argparse
import hashlib
import io
import struct
import tarfile
from pathlib import Path

from package import MOTION_DIGESTS

ARCHIVE_SHA256 = "378c1e3830a17b821755e2bf5079077346376782c88cc7f004ad025dc479fd31"
EXECUTABLE_SHA256 = "56961c0c275e53bbeeaff6b92eb63f7f367a26a4a30746643ab4d7baf85eb750"
PREFIX = "oops-v0.0.1-linux-x64"
LIMIT = 16 * 1024 * 1024
HEADER = struct.Struct("<4sHHHHI")
INPUTS = {b"GHSH": "shape.ghs", b"GHT8": "tone.ght"}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def exact_member(tar, name, maximum):
    members = [m for m in tar.getmembers() if m.name == f"{PREFIX}/{name}"]
    if len(members) != 1 or not members[0].isfile() or members[0].size > maximum:
        raise ValueError(f"invalid release member: {name}")
    return tar.extractfile(members[0]).read()


def extract_input(blob, magic, expected):
    poses = 540
    data_off = HEADER.size + (poses + 1) * 4
    search = 0
    found = None
    while (start := blob.find(magic, search)) != -1:
        search = start + len(magic)
        if len(blob) - start < data_off:
            continue
        if HEADER.unpack_from(blob, start) != (magic, 1, 200, 120, 18, poses):
            continue
        length, = struct.unpack_from("<I", blob, start + data_off - 4)
        if length > len(blob) - start - data_off:
            continue
        raw = blob[start:start + data_off + length]
        if digest(raw) == expected:
            if found is not None:
                raise ValueError(f"duplicate approved {magic.decode()} input")
            found = raw
    if found is None:
        raise ValueError(f"missing exact approved {magic.decode()} input")
    return found


def prepare(source, output):
    # Check the exact archive before interpreting any contents. Read only the
    # executable and its existing rights notice, never extract arbitrary paths.
    with source.open("rb") as f:
        archive = f.read(LIMIT + 1)
    if len(archive) > LIMIT or digest(archive) != ARCHIVE_SHA256:
        raise ValueError("Linux release archive does not match the approved SHA-256")
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
        blob = exact_member(tar, "oops", LIMIT)
        notice = exact_member(tar, "ARTWORK-LICENSE.txt", 16384)
    if digest(blob) != EXECUTABLE_SHA256:
        raise ValueError("Linux executable does not match the approved SHA-256")
    verified = {name: extract_input(blob, magic, MOTION_DIGESTS[name])
                for magic, name in INPUTS.items()}
    output.mkdir(mode=0o700)
    for name, data in verified.items():
        with (output / name).open("xb") as f:
            f.write(data)
    (output / "ARTWORK-LICENSE.txt").write_bytes(notice)


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("archive", type=Path)
    cli.add_argument("output", type=Path)
    args = cli.parse_args()
    try:
        prepare(args.archive, args.output)
    except (OSError, ValueError, tarfile.TarError) as err:
        cli.exit(1, f"artwork: {err}\n")
    print("Verified both final 18 Hz artwork inputs and the included notice")


if __name__ == "__main__":
    main()
