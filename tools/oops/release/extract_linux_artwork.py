"""Recover the pinned historical W5 artwork from v0.0.1; not a v0.0.3 input."""

import argparse
import hashlib
import io
import struct
import tarfile
from pathlib import Path

LEGACY_MOTION_DIGESTS = {
    "shape.ghs": "e44b22610dd3a02024181fd1b856cc1e33c15f13821e72ddda5d24784dffa6ff",
    "tone.ght": "52e71a2731a14902ba2bb8db25d141f4f619aa6b7b584eb2be1e47c9550f46e0",
}

# The existing v0.0.1 Linux x64 binary carries the same approved artwork.
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
    verified = {name: extract_input(blob, magic, LEGACY_MOTION_DIGESTS[name])
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
    print("Verified both final 540-phase artwork inputs and the included notice")


if __name__ == "__main__":
    main()
