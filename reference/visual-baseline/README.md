# Visual baseline

This is the approved appearance as fixed-grid display data: 288 poses
of 8-bit gray plus the original, independent occupancy. It is the
visual baseline for the next Goodhand studies, not a shipped terminal
asset or final release. It contains no pose guide, source capture, movie,
mesh, or reference images.

The gray was recovered from the lossless 30-second review movie.
[The source study](../source-study/) supplied the matching occupancy, not replacement gray.
The [recovery workflow](../../docs/review-recovery.md) confirmed every
review frame and reproduced the retained 288-pose arrays exactly.
This snapshot preserves the result, not an explanation of its
authoring decisions or a recipe for deriving them from the source study.

To render the 30-second review without the original movie or a guide:

```sh
python tools/render_cycle.py reference/visual-baseline/pixels.npz goodhand-visual-baseline.mp4 \
  --kind authored --seconds 30
```

This produces 720 frames at 24 fps, reverse, with two or three ticks per
original pose. No interpolation, geometry or display palette is added.
The JSON sidecar's 12-second period describes the input grid's sampling
contract, not the review playback duration.

| Verification | SHA-256 |
| --- | --- |
| Reviewed gray array | `ee05c10a97c45990d016ae648e649d3fde3b11e95011fa2d624e18889daaf522` |
| Original occupancy array | `ebef068c46953ba28026cca3842a33a309a59e82ebd054a831605bb9f23acaf4` |
| 720 decoded full-range YUV420 frames | `9b60364dd9341bf03b640ea82aa392f9c20f57c6184ee6cf9e477a335aa75afd` |
