#!/usr/bin/env python3
"""Pack the exact historical grayscale artwork for a local native build."""
import argparse
import hashlib
import sys
from pathlib import Path

from package import BASELINE_DIGESTS

TOOLS = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(TOOLS))
import gray_source
import pack_sequence
import pack_tone

SOURCE_SHA256 = "d1d6dccaa2ddaac86dec506db80e42df6faf4c8ff8b4f441b0375a48baddf996"


def prepare(source, output):
    gray, mask, _, digest, _ = gray_source.load_gray_source(
        source, kind="authored", profile=gray_source.PROFILE_NAME
    )
    if digest != SOURCE_SHA256:
        raise ValueError("expected the exact archived grayscale checkpoint")
    shape = pack_sequence.encode(mask, fps=24)
    tone = pack_tone.encode_tone(shape, gray, mask)
    blobs = {"shape.ghs": shape, "tone.ght": tone}
    for name, data in blobs.items():
        if hashlib.sha256(data).hexdigest() != BASELINE_DIGESTS[name]:
            raise ValueError(f"{name} no longer matches the pinned baseline")
    output.mkdir(mode=0o700)
    for name, data in blobs.items():
        with (output / name).open("xb") as target:
            target.write(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        prepare(args.source, args.output)
    except (OSError, ValueError) as error:
        parser.exit(1, f"baseline: {error}\n")
    print("Verified and packed the 288 exact grayscale phases for a local build.")


if __name__ == "__main__":
    main()
