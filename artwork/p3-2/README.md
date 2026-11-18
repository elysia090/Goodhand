# P3-2 working artwork

This selected, project-authored pixel snapshot is the starting point for
the next visual review, not a finished artwork release. It contains 288
reverse-rotation poses, an independent occupancy mask, and authored 8-bit
gray. It does not contain the source capture, source photograph, mesh,
third-party reference images, or in-progress retouches.

Generate the 22-second standalone proof from the repository root with Python,
NumPy, Pillow, FFmpeg, and FFprobe (including FFmpeg's libx264 encoder):

```sh
python tools/render_cycle.py artwork/p3-2/pixels.npz goodhand-p3-2.mp4 \
  --kind authored --seconds 22
```

The source sidecar preserves the 12-second reference sampling contract.
The preview retains all 288 poses at 24 output frames per second using
240 two-tick holds and 48 one-tick holds. It does not interpolate, drop poses,
or change the artwork's appearance. The renderer verifies decoded pixel
values and timestamps before making the output available.

Known 22-second proof: 528 frames at 1000 × 640. The decoded full-range
YUV420 byte stream has SHA-256
`7c5e10446257fd164f961dd73b84b02f61c394140d7672e672e96f8f7cbb8864`.
The video container's bytes can vary by encoder version. This proof is not
a terminal backend or a real-device playback test.
