"""Write a new authored snapshot with source provenance and independent occupancy."""

import hashlib
import json
from pathlib import Path

import numpy as np

from gray_source import REFERENCE_PROFILE


def write_authored(directory, gray, mask, original, generation):
    directory = Path(directory)
    path = directory / "pixels.npz"
    directory.mkdir(exist_ok=False)
    with path.open("xb") as target:
        np.savez_compressed(target, mask=mask, authored_gray=gray)
    metadata = {
        **REFERENCE_PROFILE, "direction": original["direction"],
        "mask_status": original["mask_status"], "source_kind": "authored",
        "output_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "generation": {
            "parent_observation_sha256": original["generation"]["parent_observation_sha256"],
            **generation,
        },
    }
    with path.with_suffix(".json").open("x", encoding="utf-8") as target:
        json.dump(metadata, target, indent=2)
        target.write("\n")
