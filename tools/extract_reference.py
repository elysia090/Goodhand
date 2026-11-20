#!/usr/bin/env python3
"""Extract observed 2D samples, never original 3D or palette, from fixed-loop MP4s.

Source bytes remain unchanged. Decoded samples and occupancy are distinct.
No denoising, retouching, invented depth, or relighting occurs.
"""

import argparse
import hashlib
import json
import math
import pathlib
import subprocess

import numpy as np

FRAME_COUNT = 288


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_timestamps(timestamps):
    if len(timestamps) != FRAME_COUNT:
        raise ValueError("Expected exactly 288 observed timestamps")
    for frame, timestamp in enumerate(timestamps):
        value = float(timestamp) if timestamp is not None else float("nan")
        if not math.isfinite(value) or abs(value - frame / 24) > 0.000001:
            raise ValueError("Video timestamps do not match zero-based constant 24 fps")


def extract(source, destination, *, expected_sha256=None, direction=None):
    report_path = destination.with_suffix(".json")
    if destination.suffix != ".npz":
        raise ValueError("Observation destination must end in .npz")
    if destination.exists() or report_path.exists():
        raise FileExistsError("Refusing to overwrite existing observations or report")

    source_hash = sha256(source)
    if expected_sha256 is None or source_hash != expected_sha256:
        raise ValueError("Source hash does not match the explicitly selected reference")
    if direction not in ("reverse", "forward"):
        raise ValueError("Playback direction must be explicitly declared")

    probe = json.loads(subprocess.check_output([
        "ffprobe", "-v", "error", "-select_streams", "v:0",
        "-show_streams", "-of", "json", str(source),
    ]))["streams"][0]
    width, height = int(probe["width"]), int(probe["height"])
    if (width, height, probe["r_frame_rate"], probe["avg_frame_rate"]) != (1000, 640, "24/1", "24/1"):
        raise ValueError("Input does not match the reference video contract")

    frame_probe = json.loads(subprocess.check_output([
        "ffprobe", "-v", "error", "-select_streams", "v:0",
        "-read_intervals", "%+#289", "-show_frames",
        "-show_entries", "frame=best_effort_timestamp_time", "-of", "json", str(source),
    ]))
    timestamps = [frame.get("best_effort_timestamp_time") for frame in frame_probe["frames"]]
    validate_timestamps(timestamps)

    process = subprocess.Popen([
        "ffmpeg", "-nostdin", "-v", "error", "-i", str(source),
        "-map", "0:v:0", "-fps_mode", "passthrough",
        "-pix_fmt", "gray", "-f", "rawvideo", "-",
    ], stdout=subprocess.PIPE)
    samples = []
    try:
        while True:
            raw = process.stdout.read(width * height)
            if not raw:
                break
            if len(raw) != width * height:
                raise ValueError("Truncated decoder frame")
            if len(samples) >= FRAME_COUNT:
                raise ValueError("Input exceeds the fixed-loop frame count")
            frame = np.frombuffer(raw, dtype=np.uint8).reshape(height, width)
            # A footprint observation, not a full-cell area average.
            dots = np.stack([
                frame[42 + dy::5, 2 + dx::5]
                for dy in (0, 1) for dx in (0, 1)
            ])
            samples.append(dots.max(axis=0))
        if process.wait() != 0:
            raise RuntimeError("Video decoding failed")
    finally:
        process.stdout.close()
        if process.poll() is None:
            process.kill()
            process.wait()

    if len(samples) != FRAME_COUNT:
        raise ValueError("Input does not contain exactly 288 frames")
    samples = np.stack(samples)
    if samples.shape != (FRAME_COUNT, 120, 200):
        raise ValueError(f"Unexpected observation array shape: {samples.shape}")
    masks = samples > 12
    threshold_ambiguity = (samples > 5) & (samples <= 30)
    stability = {
        str(threshold): int(np.count_nonzero((samples > threshold) != masks))
        for threshold in (5, 8, 10, 15, 20, 25, 30)
    }

    destination.parent.mkdir(parents=True, exist_ok=True)
    # Passing an exclusive file handle prevents NumPy from appending a suffix
    # or overwriting a name that appeared after the preflight check.
    with destination.open("xb") as output:
        np.savez_compressed(
            output, observed_gray=samples, mask=masks,
            threshold_ambiguity=threshold_ambiguity,
        )
    report = {
        "source": source.name,
        "source_sha256": source_hash,
        "output": destination.name,
        "output_sha256": sha256(destination),
        "meaning": "decoded 2D observations; not a recovered mesh or original palette",
        "width": width, "height": height, "fps": 24, "frames": FRAME_COUNT,
        "period_seconds": 12,
        "direction": direction,
        "direction_origin": "explicitly declared reference contract, not inferred from pixels",
        "video_stream": "0:v:0",
        "observed_timestamp_seconds": timestamps,
        "mask_status": "observed_candidate",
        "threshold_ambiguity": "per-cell stored map; stability band only, not true geometric confidence",
        "grid": {
            "columns": 200, "rows": 120, "pitch_px": 5,
            "dot_size_px": [2, 2], "dot_origin_px": [2, 42],
        },
        "sample_method": "maximum of decoded 2x2 footprint; not original palette",
        "threshold": 12,
        "mask_disagreements_vs_other_thresholds": stability,
        "active_dot_count": masks.sum(axis=(1, 2)).tolist(),
        "declared_yaw_degrees": (
            np.arange(FRAME_COUNT) * (-1.25 if direction == "reverse" else 1.25) % 360
        ).tolist(),
    }
    if sha256(source) != source_hash:
        raise RuntimeError("Source changed while observations were being decoded")
    with report_path.open("x") as output:
        json.dump(report, output, indent=2)
        output.write("\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=pathlib.Path)
    parser.add_argument("destination", type=pathlib.Path)
    parser.add_argument("--expected-sha256", required=True)
    parser.add_argument("--direction", choices=("reverse", "forward"), required=True)
    args = parser.parse_args()
    print(json.dumps(extract(
        args.source, args.destination,
        expected_sha256=args.expected_sha256, direction=args.direction,
    ), indent=2))


if __name__ == "__main__":
    main()
