# Study: neutral gray backdrop

This is an offline study for a known sRGB `#606060` backdrop. It is not
a runtime theme, does not inspect the display or the desktop, and is not
linked into `oops`. The grade separates luma and chroma, retains the
slight warmth of the gray look, and confines the greener note to shadows.

From the repository root, write a dependency-free before/after ramp:

    python3 reference/gray-backdrop/render.py > /tmp/goodhand-gray-ramp.ppm

Or apply the same map to the existing public visual baseline (requires
NumPy for the repository's validated snapshot reader):

    python3 reference/gray-backdrop/render.py --reference reference/visual-baseline/pixels.npz --pose 0 > /tmp/goodhand-gray-arm.ppm

Both comparisons put ungraded gray above the study on the same backdrop.
The public-hand comparison uses the existing native pitch and
independent occupancy. It is not a reconstruction of the private review
animation. The input snapshot is not modified; no render or extra media
is checked in.

## Reproduction check

The 768 bytes written by `--rgb` matched all 256 RGB entries of the
reviewed gray study, including its green shadow adjustment. The
reproducible palette SHA-256 is
`ef8331e7dc7bf6755a0553f6b4c57885ecce4ca4dfb0f40d44bb15cf5b2b9c50`.
It is a grade check, not a video or terminal screenshot hash.

    python3 reference/gray-backdrop/render.py --rgb | sha256sum
    python3 -m unittest discover -s reference/gray-backdrop
