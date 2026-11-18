#!/usr/bin/env python3
"""Preview fixed-grid observed or authored gray without changing tone or geometry."""

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path

import numpy as np
from PIL import Image

FRAME_COUNT = 288
GRID_SHAPE = (120, 200)
CANVAS_SHAPE = (640, 1000)

# Keep file-based imports used by external QA working without changing sys.path.
_SOURCE_SPEC = importlib.util.spec_from_file_location(
    "gray_source", Path(__file__).with_name("gray_source.py")
)
_SOURCE_IO = importlib.util.module_from_spec(_SOURCE_SPEC)
_SOURCE_SPEC.loader.exec_module(_SOURCE_IO)


def load_observations(source):
    gray, mask, metadata, digest, _ = _SOURCE_IO.load_gray_source(
        source, kind="observed", profile="reference-v1"
    )
    return gray, mask, metadata, digest


def compose_frame(gray, mask):
    if gray.dtype != np.uint8 or gray.shape != GRID_SHAPE:
        raise ValueError("Invalid gray frame")
    if mask.dtype != np.bool_ or mask.shape != GRID_SHAPE:
        raise ValueError("Invalid occupancy frame")
    # Black is a valid occupied tone. Occupancy is never inferred from gray.
    foreground = np.where(mask, gray, 0)
    canvas = np.zeros(CANVAS_SHAPE, dtype=np.uint8)
    for dy in (0, 1):
        for dx in (0, 1):
            canvas[42 + dy::5, 2 + dx::5] = foreground
    return canvas


def save_png_exclusive(canvas, destination):
    with destination.open("xb") as output:
        Image.fromarray(canvas).save(output, format="PNG")


def render(source, destination, frames, *, kind="observed"):
    """Render a validated source snapshot; role selection never implies adoption."""
    gray, mask, metadata, digest, sidecar_digest = _SOURCE_IO.load_gray_source(
        source, kind=kind, profile="reference-v1"
    )
    if len(set(frames)) != len(frames):
        raise ValueError("Duplicate frame selection")
    if any(frame < 0 or frame >= FRAME_COUNT for frame in frames):
        raise ValueError("Frame outside the reference loop")
    paths = [destination / f"reference-f{frame:03}.png" for frame in frames]
    report_path = destination / "reference-redraw.json"
    if report_path.exists() or any(path.exists() for path in paths):
        raise FileExistsError("Refusing to replace existing reference outputs")

    destination.mkdir(parents=True, exist_ok=False)
    files = []
    for frame, path in zip(frames, paths):
        save_png_exclusive(compose_frame(gray[frame], mask[frame]), path)
        files.append({"name": path.name, "frame": frame,
                      "declared_yaw": (-1.25 * frame) % 360,
                      "sha256": hashlib.sha256(path.read_bytes()).hexdigest()})
    report = {
        "meaning": ("Observed grayscale reference redraw, not new illumination or 3D/palette recovery"
                    if kind == "observed" else
                    "Authored grayscale preview, not an adoption or geometry/lighting validation"),
        "source_kind": kind,
        "source_npz_sha256": digest,
        "source_sidecar_sha256": sidecar_digest,
        "artifact_adoption": "not_assessed",
        "source_reference_sha256": metadata.get("source_sha256"),
        "mask_status": metadata["mask_status"],
        "sample_method": metadata.get("sample_method"),
        "canvas_px": [1000, 640], "grid": [200, 120],
        "dot_origin_px": [2, 42], "pitch_px": 5, "dot_size_px": [2, 2],
        "fps": 24, "period_seconds": 12, "files": files,
        "caveats": [
            "Each uniform 2x2 dot uses the selected saved gray, not recovered original pixels",
            "Black occupied dots and black background are indistinguishable in PNG; retain the source mask",
            "The mask remains a candidate unless its source records acceptance",
            "Source annotation header is not part of the artwork and is not copied",
        ],
    }
    if kind == "observed":
        # Preserve the original report key only for genuine observed input.
        report["source_observation_sha256"] = digest
    else:
        report["generation_declaration"] = metadata["generation"]
        report["caveats"].append(
            "Generation method and parent hash are declarations, not verified ancestry"
        )
    with report_path.open("x") as output:
        json.dump(report, output, indent=2)
        output.write("\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="NPZ source with matching JSON sidecar")
    parser.add_argument(
        "--kind", choices=("observed", "authored"), default="observed",
        help="select the input role (default: observed)",
    )
    parser.add_argument("destination", type=Path)
    parser.add_argument("--frames", nargs="+", type=int)
    args = parser.parse_args()
    frames = args.frames if args.frames is not None else list(range(FRAME_COUNT))
    print(json.dumps(render(args.source, args.destination, frames, kind=args.kind), indent=2))


if __name__ == "__main__":
    main()

