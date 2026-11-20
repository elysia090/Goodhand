#!/usr/bin/env python3
"""Recover gray from a verified lossless fixed-grid review, not its authoring rules."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

import numpy as np

from authored_export import write_authored
from gray_source import (
    FRAME_COUNT, GRID_SHAPE, MAX_SOURCE_BYTES, PROFILE_NAME, REFERENCE_PROFILE,
    load_gray_source, read_bounded_snapshot,
)
from render_cycle import (
    FPS, FRAME_BYTES, HEIGHT, LUMA_BYTES, MAX_SECONDS, NEUTRAL_CHROMA, WIDTH,
    pose_holds, verify_clock,
)
from render_reference import compose_frame

GRID = REFERENCE_PROFILE["grid"]
X_START, Y_START = GRID["dot_origin_px"]
PITCH = GRID["pitch_px"]


def recover_frames(raw, mask, holds):
    """Verify every repeated tick, all dot pixels, background and neutral chroma."""
    if mask.dtype != bool or mask.shape != (len(holds), *GRID_SHAPE) or not all(x >= 1 for x in holds):
        raise ValueError("Expected one independent occupancy mask per scheduled pose")
    stream = iter(raw)
    gray = np.zeros(mask.shape, dtype=np.uint8)
    for f, (occupied, count) in enumerate(zip(mask, holds, strict=True)):
        first = next(stream, b"")
        if len(first) != FRAME_BYTES or first[LUMA_BYTES:] != NEUTRAL_CHROMA:
            raise ValueError(f"Pose {f} does not contain a complete neutral reference frame")
        luma = np.frombuffer(first, dtype=np.uint8, count=LUMA_BYTES).reshape(HEIGHT, WIDTH)
        samples = luma[Y_START::PITCH, X_START::PITCH]
        gray[f] = np.where(occupied, samples, 0)
        if not np.array_equal(compose_frame(gray[f], occupied), luma):
            raise ValueError(f"Pose {f} differs from the declared occupied dot raster")
        for _ in range(1, count):
            if next(stream, b"") != first:
                raise ValueError(f"Pose {f} changes or is truncated within its repeated ticks")
    if next(stream, None) is not None:
        raise ValueError("Unexpected trailing video frames")
    return gray


def decode(path, occupied, holds):
    args = [
        "ffmpeg", "-nostdin", "-v", "error", "-protocol_whitelist", "file,pipe",
        "-threads", "2", "-f", "mov", "-i", str(path), "-map", "0:v:0",
        "-pix_fmt", "yuvj420p", "-f", "rawvideo", "pipe:1",
    ]
    with subprocess.Popen(args, stdout=subprocess.PIPE) as process:
        assert process.stdout is not None
        frames = iter(lambda: process.stdout.read(FRAME_BYTES), b"")
        try:
            gray = recover_frames(frames, occupied, holds)
        except BaseException:
            if process.poll() is None:
                process.kill()
            raise
        if process.wait():
            raise ValueError("FFmpeg did not finish a valid video decode")
    return gray


def recover(movie, mask_source, destination, *, seconds, expected_sha256):
    movie, destination = Path(movie), Path(destination)
    if movie.suffix.lower() != ".mp4" or not FRAME_COUNT <= FPS*seconds <= FPS*MAX_SECONDS:
        raise ValueError("Expected a supported MP4 review duration")
    if destination.exists():
        raise FileExistsError(destination)
    video = read_bounded_snapshot(movie, MAX_SOURCE_BYTES)
    digest = hashlib.sha256(video).hexdigest()
    if digest != expected_sha256:
        raise ValueError("Review video bytes do not match the expected SHA-256")
    _, mask, original, parent, _ = load_gray_source(mask_source, kind="authored", profile=PROFILE_NAME)
    holds = pose_holds(len(mask), seconds*FPS)
    # Reading the movie once binds validation and decoding to the same bytes.
    with tempfile.TemporaryDirectory(prefix="goodhand-review-") as tmp:
        snapshot = Path(tmp) / "source.mp4"
        snapshot.write_bytes(video)
        verify_clock(snapshot, sum(holds))
        gray = decode(snapshot, mask, holds)
    write_authored(destination, gray, mask, original, {
        "method": "Lossless review dot-raster extraction with independent original occupancy",
        "status": "recovered_review_pixels_not_authoring_recipe",
        "reference_video_sha256": digest,
        "parent_occupancy_npz_sha256": parent,
    })
    return {
        "video_sha256": digest, "source_poses": len(mask), "verified_ticks": sum(holds),
        "gray_sha256": hashlib.sha256(gray.tobytes()).hexdigest(),
        "occupancy_sha256": hashlib.sha256(mask.tobytes()).hexdigest(),
    }


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("movie", type=Path)
    p.add_argument("mask_source", type=Path, help="existing authored NPZ with its independent mask")
    p.add_argument("destination", type=Path)
    p.add_argument("--seconds", type=int, required=True)
    p.add_argument("--expected-sha256", required=True)
    args = p.parse_args()
    print(json.dumps(recover(args.movie, args.mask_source, args.destination,
                             seconds=args.seconds, expected_sha256=args.expected_sha256), indent=2))


if __name__ == "__main__":
    main()
