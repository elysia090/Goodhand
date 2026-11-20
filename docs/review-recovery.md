# Developer evidence: recover a reviewed fixed-grid snapshot

This is an offline provenance and validation workflow, not a playback package.
The supported terminal playback entry point is the packaged [oops executable](../tools/oops/README.md).

Use this when the reviewed **lossless** MP4 is already available to you.
It recovers its exact 8-bit gray values without a pose guide. It does not
reconstruct the edits, a model, or any hidden data from the movie.

For the visual baseline, use the original 30-second review movie and the
independent occupancy of the source study. The review kept that silhouette. A video
cannot tell apart black on an occupied point and black outside the shape.
The mask remains a separate input and is carried over unchanged. A movie
whose outline changed needs its own matching occupancy source.

```sh
python tools/recover_reviewed_gray.py reviewed.mp4 reference/source-study/pixels.npz recovered-review \
  --seconds 30 --expected-sha256 SHA256_FROM_YOUR_REVIEW_RECORD
python tools/render_cycle.py recovered-review/pixels.npz review-check.mp4 \
  --kind authored --seconds 30
```

The tool needs NumPy, Pillow, FFmpeg, and FFprobe. It binds the decode to
the specified input hash and validates the reference video clock. For
every retained pose it checks its scheduled repeated ticks, neutral
chroma, all four pixels of each dot and the surrounding black. It does
not accept an arbitrary transcode, resized clip, edited cadence or
approximate threshold fit. It writes a new NPZ and matching JSON with
declared input provenance. Neither input is modified. Keep generated
outputs separate from source control.

This workflow was checked against the retained 30-second review:
all **720** decoded frames matched the expected raster, and the recovered
gray and mask arrays matched the original **288** authored poses exactly.
The repository provides the [recovered visual baseline](../reference/visual-baseline/)
but not the source review movie. The snapshot can be rendered without
the movie. Recovery is not an acceptance of a new artwork, a terminal
preview or a formula to recreate the reviewed appearance from the source study alone.
