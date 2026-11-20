#!/usr/bin/env python3
"""Export explicitly observed/authored gray8 to GHT8; never apply or adopt art."""

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct

import numpy as np


def _load_tool(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(f"{name}.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


source_io = _load_tool("gray_source")
shape_codec = _load_tool("pack_sequence")
HEADER = struct.Struct("<4sHHHHI")
MAX_ASSET_BYTES = shape_codec.MAX_ASSET_BYTES


def encode_tone(shape_bytes, gray, mask):
    """Encode without a reference profile, curve, source label, or second clock.

    Every input mask cell must match GHSH, not merely its occupied count. Values
    follow C-order occupied positions, equivalent to canonical GHSH row/run order.
    """
    if not isinstance(shape_bytes, bytes):
        raise ValueError("Expected an immutable shape byte snapshot")
    gray, mask = np.asarray(gray), np.asarray(mask)
    if gray.dtype != np.uint8 or mask.dtype != np.bool_:
        raise ValueError("Expected uint8 gray and boolean mask without conversion")
    if gray.ndim != 3 or gray.shape != mask.shape:
        raise ValueError("Expected matching frame,row,column gray and mask arrays")
    if len(shape_bytes) > MAX_ASSET_BYTES or len(shape_bytes) < HEADER.size:
        raise ValueError("Shape size is outside the runtime contract")
    _, _, width, height, fps, frames = shape_codec.HEADER.unpack_from(shape_bytes)
    if gray.shape != (frames, height, width):
        raise ValueError("Gray dimensions do not match shape metadata")
    if not (0 < frames <= shape_codec.MAX_FRAMES and 0 < height <= 255 and 0 < width <= 255):
        raise ValueError("Unsupported shape dimensions")
    fixed_bytes = HEADER.size + 4 * (frames + 1)
    payload_bytes = int(np.count_nonzero(mask))
    if fixed_bytes + payload_bytes > MAX_ASSET_BYTES:
        raise ValueError("Tone exceeds the 64MiB runtime asset limit")
    shape_mask, decoded_fps = shape_codec.decode(shape_bytes)
    if not np.array_equal(mask, shape_mask):
        raise ValueError("Input mask does not match shape at every cell")
    offsets = [0]
    payload = bytearray()
    for frame in range(frames):
        selected = gray[frame].ravel(order="C")[mask[frame].ravel(order="C")]
        payload.extend(selected.tobytes())
        offsets.append(len(payload))
    header = HEADER.pack(b"GHT8", 1, width, height, decoded_fps, frames)
    return header + struct.pack("<" + "I" * len(offsets), *offsets) + payload


def pack_tone(source, shape, destination, *, kind, profile):
    """Write a new private-capable export directory, with provenance separate."""
    destination = Path(destination)
    if destination.exists():
        raise FileExistsError("Tone export requires a new dedicated directory")
    gray, mask, metadata, source_hash, sidecar_hash = source_io.load_gray_source(
        source, kind=kind, profile=profile
    )
    shape_bytes = source_io.read_bounded_snapshot(shape, MAX_ASSET_BYTES)
    shape_hash = hashlib.sha256(shape_bytes).hexdigest()
    blob = encode_tone(shape_bytes, gray, mask)
    fps = HEADER.unpack_from(blob)[4]
    if fps != metadata["fps"]:
        raise ValueError("Shape fps does not match the explicit source profile")
    report = {
        "format": "GHT8 v1 occupied row/run gray8",
        "source_kind": kind,
        "source_kind_verification": "explicit declaration and named-array contract, not origin authentication",
        "profile": profile,
        "source_snapshot_sha256": source_hash,
        "source_sidecar_sha256": sidecar_hash,
        "shape_sha256": shape_hash,
        "output_sha256": hashlib.sha256(blob).hexdigest(),
        "shape": [int(value) for value in mask.shape],
        "fps": fps,
        "bytes": len(blob),
        "payload_bytes": int(np.count_nonzero(mask)),
        "frame_occupied_counts": mask.sum(axis=(1, 2)).tolist(),
        "mask_matches_shape_all_cells": True,
        "source_mask_status": metadata["mask_status"],
        "artifact_adoption": "not_assessed",
        "declared_generation": metadata.get("generation") if kind == "authored" else None,
        "declared_source_reference_sha256": metadata.get("source_sha256") if kind == "observed" else None,
        "parent_hash_verification": "declaration only; parent bytes were not supplied or checked",
        "original_palette_or_3d_recovered": False,
        "meaning": "Lossless packing of selected occupied gray; no curve, mask repair, or adoption decision",
    }
    report_json = json.dumps(report, indent=2, allow_nan=False) + "\n"
    destination.mkdir(parents=True, exist_ok=False)
    with (destination / "tone.ght").open("xb") as output:
        output.write(blob)
    with (destination / "tone.json").open("x") as output:
        output.write(report_json)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("shape", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--kind", choices=("observed", "authored"), required=True)
    parser.add_argument("--profile", choices=(source_io.PROFILE_NAME,), required=True)
    args = parser.parse_args()
    print(json.dumps(pack_tone(args.source, args.shape, args.destination,
                               kind=args.kind, profile=args.profile), indent=2))


if __name__ == "__main__":
    main()
