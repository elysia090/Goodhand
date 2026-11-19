#!/usr/bin/env python3
"""Generate an offline tone ramp for the reviewed neutral gray (code 96).

Top: unchanged gray ramps. Bottom: the study on the same background.
No playback content, automatic backdrop detection, or graphics packages.
"""

import argparse
import itertools
import math
from pathlib import Path
import sys


BACKGROUND = 96
LUMA = (0.2126, 0.7152, 0.0722)
VALUE = (
    (0, 0.0),
    (10, 6.2),
    (17, 9.8),
    (20, 13.1),
    (24, 17.1),
    (34, 22.1),
    (45, 30.7),
    (56, 40.2),
    (62, 50.1),
    (65, 71.7),
    (69, 116.6),
    (77, 135.8),
    (83, 143.7),
    (96, 152.8),
    (111, 167.1),
    (132, 179.6),
    (150, 190.1),
    (177, 202.1),
    (189, 204.5),
    (218, 214.4),
    (255, 222.0),
)
CHROMA = (
    (0, 0.0, 0.0),
    (9, 0.0, 0.0),
    (16, -3.5, 25.5),
    (20, -10.0, 50.0),
    (30, -10.0, 49.0),
    (34, -11.0, 44.0),
    (44, -8.0, 14.0),
    (51, -5.4, -10.0),
    (60, -3.0, -20.0),
    (68, -2.6, -26.6),
    (75, -1.9, -26.5),
    (94, -2.5, -23.5),
    (110, -4.0, -11.5),
    (128, -1.2, -6.8),
    (150, 4.8, -14.0),
    (165, 6.4, -12.8),
    (191, 3.9, -3.1),
    (213, -1.0, 4.0),
    (255, -2.0, 6.0),
)


def lerp(tone, stops):
    for left, right in zip(stops, stops[1:]):
        if tone <= right[0]:
            t = (tone - left[0]) / (right[0] - left[0])
            return tuple(a + t * (b - a) for a, b in zip(left[1:], right[1:]))
    raise ValueError("expected gray8")


def smooth(t):
    t = max(0.0, min(t, 1.0))
    return t * t * (3.0 - 2.0 * t)


def add_chroma(rgb, red, blue):
    green = -(LUMA[0] * red + LUMA[2] * blue) / LUMA[1]
    offsets = (red, green, blue)
    scale = min(
        1.0,
        *((255.0 - v if d > 0.0 else v) / abs(d)
          for v, d in zip(rgb, offsets) if d),
    ) if any(offsets) else 1.0
    return tuple(max(0.0, min(v + d * scale, 255.0)) for v, d in zip(rgb, offsets))


def linear_srgb(code):
    x = code / 255
    return x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4


LINEAR = tuple(map(linear_srgb, range(256)))


def light(rgb):
    return (
        sum(w * c for w, c in zip((2126, 7152, 722), rgb)),
        sum(w * LINEAR[c] for w, c in zip(LUMA, rgb)),
    )


def ordered(a, b):
    return all(x <= y for x, y in zip(light(a), light(b)))


def rounded(rgb):
    return tuple(math.floor(v + 0.5) for v in rgb)


def in_order(rgb, previous):
    first = rounded(rgb)
    if ordered(previous, first):
        return first
    # Before choosing the nearest integer corner, raise it neutrally
    # if needed to preserve both measures of brightness.
    while not ordered(previous, tuple(map(math.ceil, rgb))):
        rgb = tuple(min(255.0, v + 1.0) for v in rgb)
    corners = itertools.product(*[(math.floor(v), math.ceil(v)) for v in rgb])
    return min(
        (candidate for candidate in corners if ordered(previous, candidate)),
        key=lambda candidate: sum((v - q) ** 2 for v, q in zip(rgb, candidate)),
    )


def palette():
    """Return the isolated neutral-gray study, not a runtime theme."""
    colors = []
    previous = (0, 0, 0)
    for gray in range(256):
        value, = lerp(gray, VALUE)
        red, blue = lerp(gray, CHROMA)
        red += 1.2 * smooth((gray - 5.0) / 15.0)
        previous = in_order(add_chroma((value,) * 3, red, blue), previous)
        colors.append(previous)
    # A slight greener note inside the dark band. Accept only local edits
    # that leave both neighbors ordered, to protect lighter tone roles.
    for gray in range(6, 45):
        note = smooth((gray - 5.0) / 10.0) * (1.0 - smooth((gray - 26.0) / 19.0))
        candidate = rounded(add_chroma(colors[gray], -2.0 * note, -note))
        if ordered(colors[gray - 1], candidate) and ordered(candidate, colors[gray + 1]):
            colors[gray] = candidate
    return colors


def render():
    """A P6 PPM showing raw gray above and the graded reference ramp below."""
    width, height = 1048, 112
    empty = bytes((BACKGROUND,) * 3)
    gap = empty * width
    bands = [
        empty * 12 + b"".join(bytes(color) * 3 + empty for color in colors) + empty * 12
        for colors in ([(v,) * 3 for v in range(256)], palette())
    ]
    return f"P6\n{width} {height}\n255\n".encode() + (
        gap * 16 + bands[0] * 32 + gap * 16 + bands[1] * 32 + gap * 16
    )


def render_reference(path, pose):
    """Before/after at the archive's native point grid and pixel positions."""
    import numpy as np

    # Reuse the checked-in snapshot validator, including provenance and
    # occupancy checks. NumPy is optional for the standalone ramp study.
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools"))
    from gray_source import REFERENCE_PROFILE, load_gray_source

    gray, mask, *_ = load_gray_source(path, kind="authored", profile="reference-v1")
    if not 0 <= pose < len(gray):
        raise ValueError(f"expected a pose between 0 and {len(gray) - 1}")
    tone, occupied = gray[pose], mask[pose]
    width, height = REFERENCE_PROFILE["width"], REFERENCE_PROFILE["height"]
    grid = REFERENCE_PROFILE["grid"]
    pitch = grid["pitch_px"]
    x, y = grid["dot_origin_px"]
    dx, dy = grid["dot_size_px"]
    before = np.full((height, width, 3), BACKGROUND, dtype=np.uint8)
    after = before.copy()
    raw = tone[occupied][:, None]
    graded = np.array(palette(), dtype=np.uint8)[tone[occupied]]
    for j in range(dy):
        for i in range(dx):
            before[y + j::pitch, x + i::pitch][occupied] = raw
            after[y + j::pitch, x + i::pitch][occupied] = graded
    return (f"P6\n{width} {2 * height}\n255\n".encode()
            + before.tobytes() + after.tobytes())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--rgb", action="store_true", help="write the 768 palette bytes")
    mode.add_argument("--reference", type=Path, help="grade a public authored gray/mask NPZ")
    parser.add_argument("--pose", type=int, help="reference pose, default 0")
    args = parser.parse_args()
    if args.pose is not None and args.reference is None:
        parser.error("--pose requires --reference")
    if args.rgb:
        output = bytes(channel for color in palette() for channel in color)
    elif args.reference is not None:
        output = render_reference(args.reference, args.pose or 0)
    else:
        output = render()
    sys.stdout.buffer.write(output)


if __name__ == "__main__":
    main()
