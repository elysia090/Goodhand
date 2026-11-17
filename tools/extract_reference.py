#!/usr/bin/env python3
"""Recover observed 2D samples, never original 3D or original palette, from MP4s.

Preserves source bytes. The output stores lossy decoded samples separately from
binary silhouette observations. No denoising, retouching, or relighting occurs.
"""
import argparse, hashlib, json, pathlib, subprocess
import numpy as np

def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def extract(source, destination):
    if destination.exists() or destination.with_suffix(".json").exists():
        raise FileExistsError("Refusing to overwrite existing observations or report")
    probe = json.loads(subprocess.check_output([
        "ffprobe", "-v", "error", "-select_streams", "v:0",
        "-show_streams", "-of", "json", str(source)]))["streams"][0]
    width, height = int(probe["width"]), int(probe["height"])
    if (width, height, probe["r_frame_rate"]) != (1000, 640, "24/1"):
        raise ValueError("Input does not match the preserved video contract")
    process = subprocess.Popen(["ffmpeg", "-nostdin", "-v", "error", "-i",
        str(source), "-pix_fmt", "gray", "-f", "rawvideo", "-"],
        stdout=subprocess.PIPE)
    samples = []
    while True:
        raw = process.stdout.read(width * height)
        if not raw:
            break
        if len(raw) != width * height:
            raise ValueError("Truncated decoder frame")
        frame = np.frombuffer(raw, dtype=np.uint8).reshape(height, width)
        # Observed 2x2 dot footprint, not an invented full-cell average.
        dots = np.stack([frame[42+dy::5, 2+dx::5] for dy in (0,1) for dx in (0,1)])
        samples.append(dots.max(axis=0))
    if process.wait() != 0:
        raise RuntimeError("Video decoding failed")
    samples = np.stack(samples)
    if samples.shape != (288, 120, 200):
        raise ValueError(f"Unexpected recovered array shape: {samples.shape}")
    masks = samples > 12
    # Video artifacts may perturb intensity. Check occupancy remains stable under
    # several thresholds, and record rather than silently repair discrepancies.
    stability = {str(t): int(np.count_nonzero((samples > t) != masks))
                 for t in (5, 8, 10, 15, 20, 25, 30)}
    destination.parent.mkdir(parents=True, exist_ok=True)
    np.savez_compressed(destination, observed_gray=samples, mask=masks)
    report = {
        "source": source.name, "source_sha256": sha256(source),
        "output": destination.name, "output_sha256": sha256(destination),
        "meaning": "decoded 2D observations; not a recovered mesh or original palette",
        "width": width, "height": height, "fps": 24, "frames": 288,
        "period_seconds": 12, "direction": "reverse",
        "grid": {"columns":200,"rows":120,"pitch_px":5,
                 "dot_size_px":[2,2],"dot_origin_px":[2,42]},
        "sample_method":"maximum of decoded 2x2 footprint; not original palette",
        "threshold":12,"mask_disagreements_vs_other_thresholds":stability,
        "active_dot_count": masks.sum(axis=(1,2)).tolist(),
        "yaw_degrees": ((-np.arange(288)*1.25)%360).tolist()
    }
    destination.with_suffix(".json").write_text(json.dumps(report, indent=2)+"\n")
    return report

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=pathlib.Path)
    parser.add_argument("destination", type=pathlib.Path)
    args = parser.parse_args()
    print(json.dumps(extract(args.source, args.destination), indent=2))
