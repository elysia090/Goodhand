#!/usr/bin/env python3
"""Pack neutral 2D occupancy as row runs, without baking tone into shape."""

import argparse
import hashlib
import io
import json
import pathlib
import struct

import numpy as np

HEADER = struct.Struct("<4sHHHHI")
MAX_FRAMES = 4096
MAX_FPS = 1000
MAX_ASSET_BYTES = 64 * 1024 * 1024


def encode(mask, fps=24):
    mask = np.asarray(mask)
    if mask.dtype != np.bool_ or mask.ndim != 3:
        raise ValueError("Expected a boolean frame,row,column occupancy array")
    frames, height, width = mask.shape
    if not (0 < frames <= MAX_FRAMES and 0 < height <= 255 and 0 < width <= 255):
        raise ValueError("Unsupported dimensions")
    if not (0 < fps <= MAX_FPS):
        raise ValueError("Unsupported clock")

    fixed_bytes = HEADER.size + 4 * (frames + 1)
    if fixed_bytes > MAX_ASSET_BYTES:
        raise ValueError("Shape exceeds the runtime asset limit")
    offsets = [0]
    payload = bytearray()
    for frame in mask:
        for row in frame:
            edges = np.flatnonzero(np.diff(np.r_[False, row, False].astype(np.int8)))
            runs = edges.reshape(-1, 2)
            if fixed_bytes + len(payload) + 1 + 2 * len(runs) > MAX_ASSET_BYTES:
                raise ValueError("Shape exceeds the runtime asset limit")
            payload.append(len(runs))
            for start, end in runs:
                payload.extend((int(start), int(end - start)))
        offsets.append(len(payload))

    header = HEADER.pack(b"GHSH", 1, width, height, fps, frames)
    index = struct.pack("<" + "I" * len(offsets), *offsets)
    return header + index + payload


def _runs(blob, left, right, width, height):
    position = left
    for row in range(height):
        if position >= right:
            raise ValueError("Truncated row")
        count = blob[position]
        position += 1
        previous_end = -1
        for _ in range(count):
            if position + 2 > right:
                raise ValueError("Truncated run")
            start, length = blob[position:position + 2]
            position += 2
            if not length or start <= previous_end or start + length > width:
                raise ValueError("Noncanonical run")
            yield row, start, length
            previous_end = start + length
    if position != right:
        raise ValueError("Trailing frame data")


def decode(blob):
    if len(blob) > MAX_ASSET_BYTES:
        raise ValueError("Shape exceeds the runtime asset limit")
    if len(blob) < HEADER.size:
        raise ValueError("Truncated header")
    magic, version, width, height, fps, frames = HEADER.unpack_from(blob)
    if magic != b"GHSH" or version != 1:
        raise ValueError("Unknown shape format")
    if not (0 < width <= 255 and 0 < height <= 255 and 0 < frames <= MAX_FRAMES):
        raise ValueError("Invalid dimensions")
    if not (0 < fps <= MAX_FPS):
        raise ValueError("Invalid clock")

    payload_start = HEADER.size + 4 * (frames + 1)
    if len(blob) < payload_start:
        raise ValueError("Truncated offsets")
    offsets = struct.unpack_from("<" + "I" * (frames + 1), blob, HEADER.size)
    payload_size = len(blob) - payload_start
    if offsets[0] != 0 or offsets[-1] != payload_size:
        raise ValueError("Invalid extent")

    # Validate all frames before allocating the expanded sequence.
    for left, right in zip(offsets, offsets[1:]):
        if not 0 <= left <= right <= payload_size:
            raise ValueError("Invalid offset")
        for _ in _runs(blob, payload_start + left, payload_start + right, width, height):
            pass

    mask = np.zeros((frames, height, width), dtype=np.bool_)
    for frame, (left, right) in enumerate(zip(offsets, offsets[1:])):
        for row, start, length in _runs(
            blob, payload_start + left, payload_start + right, width, height
        ):
            mask[frame, row, start:start + length] = True
    return mask, fps


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("observations", type=pathlib.Path)
    parser.add_argument("destination", type=pathlib.Path)
    parser.add_argument("--candidate", action="store_true",
                        help="Allow an unaccepted observed mask for diagnostics only")
    args = parser.parse_args()
    report_path = args.destination.with_suffix(".json")
    if args.destination.suffix != ".ghs":
        raise ValueError("Shape destination must end in .ghs")
    if args.destination.exists() or report_path.exists():
        raise FileExistsError("Refusing to overwrite existing shape or report")

    input_bytes = args.observations.read_bytes()
    input_hash = hashlib.sha256(input_bytes).hexdigest()
    input_report_path = args.observations.with_suffix(".json")
    input_report = json.loads(input_report_path.read_text()) if input_report_path.exists() else {}
    if input_report.get("output_sha256") not in (None, input_hash):
        raise ValueError("Observation bytes do not match their provenance report")
    accepted = input_report.get("mask_status") == "accepted"
    if not accepted and not args.candidate:
        raise ValueError("Mask is not accepted; use --candidate only for diagnostics")
    with np.load(io.BytesIO(input_bytes), allow_pickle=False) as observations:
        source = observations["mask"]
        ambiguity = observations["threshold_ambiguity"] if "threshold_ambiguity" in observations.files else None
    if ambiguity is not None and (ambiguity.dtype != np.bool_ or ambiguity.shape != source.shape):
        raise ValueError("Invalid per-cell threshold ambiguity map")
    blob = encode(source)
    recovered, fps = decode(blob)
    if not np.array_equal(source, recovered):
        raise RuntimeError("Shape round-trip changed occupancy")

    args.destination.parent.mkdir(parents=True, exist_ok=True)
    with args.destination.open("xb") as target:
        target.write(blob)
    report = {
        "format": "GHSH v1 independent row runs",
        "shape": [int(value) for value in source.shape],
        "fps": fps,
        "bytes": len(blob),
        "bitpacked_comparison_bytes": int(np.packbits(source).nbytes),
        "round_trip_differing_cells": int(np.count_nonzero(source != recovered)),
        "sha256": hashlib.sha256(blob).hexdigest(),
        "meaning": "2D occupancy only; no tone, depth, normals, or model topology",
        "mask_status": "accepted" if accepted else "observed_candidate",
        "input_observations_sha256": input_hash,
        "reference_sha256": input_report.get("source_sha256"),
        "extraction_threshold": input_report.get("threshold"),
        "threshold_ambiguity_cells": int(ambiguity.sum()) if ambiguity is not None else None,
        "warning": None if accepted else "Diagnostic candidate; round-trip equality is not visual approval",
    }
    with report_path.open("x") as target:
        json.dump(report, target, indent=2)
        target.write("\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()

