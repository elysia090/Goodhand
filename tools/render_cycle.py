#!/usr/bin/env python3
"""Render an observed or authored cycle at a new duration without altering poses."""
import argparse
from collections.abc import Iterator
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

import numpy as np

from gray_source import FRAME_COUNT, PROFILE_NAME, REFERENCE_PROFILE, load_gray_source
from render_reference import compose_frame

FPS = REFERENCE_PROFILE["fps"]
WIDTH, HEIGHT = REFERENCE_PROFILE["width"], REFERENCE_PROFILE["height"]
LUMA_BYTES = WIDTH * HEIGHT
NEUTRAL_CHROMA = bytes([128]) * (LUMA_BYTES // 2)
FRAME_BYTES = LUMA_BYTES + len(NEUTRAL_CHROMA)
MAX_SECONDS = 60


def pose_holds(source_count: int, output_count: int) -> tuple[int, ...]:
    """Round cumulative pose boundaries half-up, retaining every pose."""
    if source_count <= 0 or output_count < source_count:
        raise ValueError("A pose-preserving schedule requires at least one tick per pose")
    boundaries = [
        (2 * pose * output_count + source_count) // (2 * source_count)
        for pose in range(source_count + 1)
    ]
    return tuple(end - start for start, end in zip(boundaries, boundaries[1:]))


def frames(gray: np.ndarray, mask: np.ndarray, holds: tuple[int, ...]) -> Iterator[bytes]:
    """Compose each unique frame once, then emit its presentation ticks."""
    if gray.shape != mask.shape or len(gray) != len(holds) or any(count < 1 for count in holds):
        raise ValueError("Each input frame must have one or more output ticks and a matching mask")
    for tone, occupied, count in zip(gray, mask, holds):
        raw = compose_frame(tone, occupied).tobytes() + NEUTRAL_CHROMA
        for _ in range(count):
            yield raw


def encode(destination: Path, gray: np.ndarray, mask: np.ndarray, holds: tuple[int, ...]) -> None:
    """Write a lossless video-only preview in the existing reference grid."""
    args = [
        "ffmpeg", "-nostdin", "-v", "error", "-f", "rawvideo",
        "-pixel_format", "yuvj420p", "-video_size", f"{WIDTH}x{HEIGHT}",
        "-framerate", str(FPS), "-i", "pipe:0", "-an", "-c:v", "libx264",
        "-threads", "4", "-preset", "fast", "-qp", "0", "-pix_fmt", "yuvj420p",
        "-color_range", "pc", "-colorspace", "bt709", "-color_primaries", "bt709",
        "-color_trc", "iec61966-2-1", "-movflags", "+faststart", str(destination),
    ]
    with subprocess.Popen(args, stdin=subprocess.PIPE) as process:
        assert process.stdin is not None
        for frame in frames(gray, mask, holds):
            process.stdin.write(frame)
        process.stdin.close()
        if process.wait():
            raise RuntimeError("FFmpeg did not finish encoding the preview")


def verify_clock(path: Path, count: int) -> None:
    """Check the stream shape and every output timestamp."""
    args = [
        "ffprobe", "-v", "error", "-show_entries",
        "format=duration:stream=codec_type,width,height,avg_frame_rate:frame=best_effort_timestamp_time",
        "-of", "json", str(path),
    ]
    parsed = json.loads(subprocess.check_output(args))
    streams = parsed["streams"]
    if len(streams) != 1:
        raise ValueError("Expected one video-only stream")
    video = streams[0]
    if (video["codec_type"], video["width"], video["height"], video["avg_frame_rate"]) != (
        "video", WIDTH, HEIGHT, f"{FPS}/1"
    ):
        raise ValueError("Unexpected video profile")
    timestamps = parsed["frames"]
    if len(timestamps) != count or abs(float(parsed["format"]["duration"]) - count / FPS) >= 1e-6:
        raise ValueError("Unexpected video duration or frame count")
    for index, entry in enumerate(timestamps):
        if abs(float(entry["best_effort_timestamp_time"]) - index / FPS) >= 1e-6:
            raise ValueError(f"Unexpected timestamp at frame {index}")


def verify(path: Path, gray: np.ndarray, mask: np.ndarray, holds: tuple[int, ...]) -> str:
    """Compare every decoded luma and chroma byte, then validate the clock."""
    args = [
        "ffmpeg", "-nostdin", "-v", "error", "-threads", "4", "-i", str(path),
        "-map", "0:v:0", "-pix_fmt", "yuvj420p", "-f", "rawvideo", "pipe:1",
    ]
    digest = hashlib.sha256()
    with subprocess.Popen(args, stdout=subprocess.PIPE) as process:
        assert process.stdout is not None
        for index, expected in enumerate(frames(gray, mask, holds)):
            actual = process.stdout.read(FRAME_BYTES)
            if actual != expected:
                process.kill()
                raise ValueError(f"Decoded pixel mismatch at frame {index}")
            digest.update(actual)
        if process.stdout.read(1):
            process.kill()
            raise ValueError("Unexpected trailing frame data")
        if process.wait():
            raise RuntimeError("FFmpeg did not finish decoding the preview")
    verify_clock(path, sum(holds))
    return digest.hexdigest()


def render(source: Path, destination: Path, *, kind: str, seconds: int) -> dict:
    """Validate the input once; publish the output path only after verification."""
    if not FRAME_COUNT <= FPS * seconds <= FPS * MAX_SECONDS:
        raise ValueError(f"Choose a duration from {FRAME_COUNT // FPS} through {MAX_SECONDS} seconds")
    if destination.suffix.lower() != ".mp4":
        raise ValueError("Video output must end in .mp4")
    if destination.exists():
        raise FileExistsError("Refusing to overwrite a preview")
    gray, mask, _, source_digest, _ = load_gray_source(source, kind=kind, profile=PROFILE_NAME)
    holds = pose_holds(FRAME_COUNT, seconds * FPS)
    with tempfile.TemporaryDirectory(dir=destination.parent, prefix=".goodhand-cycle-") as tmp:
        candidate = Path(tmp) / "preview.mp4"
        encode(candidate, gray, mask, holds)
        decoded_digest = verify(candidate, gray, mask, holds)
        # Both paths are on the same filesystem. An existing destination fails atomically.
        os.link(candidate, destination)
    return {
        "source_npz_sha256": source_digest,
        "source_kind": kind,
        "source_poses": FRAME_COUNT,
        "output_frames": sum(holds),
        "fps": FPS,
        "duration_seconds": seconds,
        "decoded_yuvj420p_sha256": decoded_digest,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="NPZ with a matching JSON sidecar")
    parser.add_argument("destination", type=Path, help="new MP4 path")
    parser.add_argument("--kind", choices=("observed", "authored"), required=True)
    parser.add_argument("--seconds", type=int, required=True)
    args = parser.parse_args()
    print(json.dumps(render(args.source, args.destination, kind=args.kind, seconds=args.seconds), indent=2))


if __name__ == "__main__":
    main()
