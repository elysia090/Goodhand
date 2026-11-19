#!/usr/bin/env python3
"""Produce synthetic review-format inputs for the embedded-build test."""
import argparse
from pathlib import Path
import struct


def asset(magic, frames):
    offsets = [0]
    for frame in frames:
        offsets.append(offsets[-1] + len(frame))
    return (struct.pack("<4sHHHHI", magic, 1, 200, 120, 24, len(frames))
            + struct.pack(f"<{len(offsets)}I", *offsets)
            + b"".join(frames))


def write_fixture(path):
    path.mkdir()
    shape, tone = [], []
    for pose in range(288):
        left = 27 + pose % 15
        # Moving, high-contrast synthetic rectangle, big enough for the
        # PTY to exercise backpressure. Never derived from artwork.
        shape.append(bytes(32) + bytes((1, left, 126)) * 64 + bytes(24))
        tone.append(bytes(25 + (index * 13 + pose * 7) % 160 for index in range(126 * 64)))
    (path / "shape.ghs").write_bytes(asset(b"GHSH", shape))
    (path / "tone.ght").write_bytes(asset(b"GHT8", tone))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="new directory")
    write_fixture(parser.parse_args().destination)
