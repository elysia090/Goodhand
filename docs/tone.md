# Tone and preview contract

## Input roles

Tone packing and previewing consume an NPZ with a same-stem JSON sidecar.
The fixed `reference-v1` profile is 288 frames of 120 × 200 cells, reverse
24 fps / 12 seconds, with the pixel placement defined in [DESIGN](../DESIGN.md).

- Observed: `observed_gray:uint8[288,120,200]` and independent `mask:bool` of the same shape.
- Authored: `authored_gray:uint8[288,120,200]` and the same mask contract.
- Optional `threshold_ambiguity:bool` is validated, never used to repair occupancy.
- Opposite-role arrays, ambiguous `gray`, extra diagnostic arrays, mixed roles,
  implicit dtype conversion, and clamping are rejected.

The sidecar requires NPZ `output_sha256`, the full reference profile,
`direction:"reverse"`, and `mask_status:"observed_candidate"` or `"accepted"`.
Authored input additionally requires `source_kind:"authored"`, and `generation`
with a nonempty `method` and lowercase 64-hex `parent_observation_sha256`.
Observed input may omit `source_kind` for compatibility or declare `observed`,
but cannot include an authored generation declaration.

These labels and parent hashes are declarations, not verified ancestry. Never
replace observed values with authored results under the observed key. Mask
acceptance does not imply artwork adoption.

## Validation and resource bounds

`gray_source.py` reads one bounded snapshot each of NPZ and JSON; validation and
hashing use those same bytes. Limits are 64 MiB for NPZ, 1 MiB for JSON, and
10,240 bytes per NPY header. Member headers, dtype, shape, and expanded length
are checked before array loading. Pickle, duplicates, unknown arrays, and trailing
data are rejected.

The container permits NumPy's stored/deflated members, a comment-free single-disk
ZIP32 central directory with 2..3 entries and at most 4 KiB. NumPy local-header
ZIP64 extras remain supported. This is a bounded profile, not a general ZIP
importer or a guarantee of whole-process memory usage or execution time.

## PNG preview

```sh
python3 tools/render_reference.py observations.npz observed-preview --frames 0 144
python3 tools/render_reference.py authored.npz authored-preview --kind authored --frames 0 144
```

Python callers use `render(source, destination, frames, kind="authored")`.
Omitting `kind` preserves the observed default; `load_observations(source)` stays
observed-only. Both roles use the same strict `reference-v1` snapshot validator.

The tool places saved gray at fixed dot coordinates without curves, relighting,
interpolation, geometry reconstruction, or adoption. Background samples are ignored.
Occupied black remains valid in the source mask, although the black PNG cannot
distinguish it from background. Retain the independent mask downstream.

Use a new destination directory; existing directories, even empty ones, are
rejected. Selected frames retain caller order and use `reference-fNNN.png` for
both roles, alongside `reference-redraw.json`. Duplicate/out-of-range frames are
rejected. Omitting CLI `--frames` renders all 288 frames. Validation occurs before
directory creation. An I/O failure may leave partial output; retry in a new folder.

Reports record `source_kind`, `source_npz_sha256`, `source_sidecar_sha256`, mask
status, declared reference hash/sample method, fixed placement/timing, and PNG
hashes, frame numbers, and declared yaw. Observed reports retain the legacy
`source_observation_sha256` key. Authored reports instead include
`generation_declaration`, explicitly not verified ancestry. `artifact_adoption`
is always `not_assessed`, even for an accepted mask.

## Runtime export

```sh
python3 tools/pack_tone.py authored.npz shape.ghs authored-export \
  --kind authored --profile reference-v1
```

Packing requires explicit role/profile, compares the independent mask to GHSH at
every cell, and exports occupied-order `tone.ght` with `tone.json`. Black zero is
preserved. NPZ, sidecar, and shape use bounded snapshots; reports retain input and
output hashes, occupancy counts, and declarations. No curve, repair, or adoption
is added. Existing output directories are rejected.

## Authoring and review

Preserve pose, silhouette, composition, point coordinates, and timing. Reproduce
the reference before authorship. Video samples are not recovered palettes or
normals; image correspondence is not material tracking.

Before changing values, state the angle/region, lost visual distinction, separate
causal hypothesis, proposed operation, protected conditions, and refutation
criteria. Compare native-size baseline and candidate before enlargement or metrics.
Do not assign anatomy or contact ownership to an ambiguous shadow by assumption.

Design large dark/intermediate/light planes before detail. Favor intentional
asymmetry and hierarchy rather than uniform contrast or equal areas. A point
should join a plane, separate a boundary, or retain light within a shadow. Compare
local structural hypotheses against a global-contrast control. Review broad
plane relationships, local folds and contact, then selective finish in separate
passes. Keep each comparison baseline and apply the next change only after
reviewing the current pass.

Measure occupancy separately from apparent contour loss, and occupied-dot errors
rather than only background-dominated whole-image error. Connected regions and
boundary widths are diagnostics, not artistic objectives. Distinguish silhouette
entry/exit from internal flicker. Compare native views, enlargements, all-angle
atlases, 24 fps playback including the seam, and real-terminal composition.
Keep observed, authored, and uncertain tracks separate. Occlusion/reappearance
must not be bridged without evidence; smoothness alone cannot prove attachment.

## Research directions, not adopted algorithms

These references motivate comparisons, not claims of implementation or direct
transfer to fixed bright points on black:

- [Weighted Voronoi Stippling](https://www.cs.ubc.ca/labs/imager/tr/2002/secord2002b/secord.2002b.pdf):
  density approximates tone, but moving dark points on white is a different representation.
- [Real-Time Hatching](https://hhoppe.com/hatching.pdf) and
  [Real-Time Animated Stippling](https://www.cs.mun.ca/~omeruvia/research/publications/realTimeStippling.pdf):
  investigate persistence without assuming recovered surface coordinates.
- [Scalar Spatiotemporal Blue Noise Masks](https://arxiv.org/html/2112.09629v1):
  temporal-integration benefits do not prove the best direct-view 24 fps result.
- [Keyframe-Based Tracking for Rotoscoping and Animation](https://grail.cs.washington.edu/projects/rotoscoping/roto.pdf)
  and [Stroke Surfaces](https://purehost.bath.ac.uk/ws/portalfiles/portal/560911/CSBU-2003-01.pdf):
  distinguish observed keys, interpolation, disappearance, splitting, and merging.
  An image-plane curve normal is not a surface normal.
- [GuiltyGear Xrd production presentation](https://media.gdcvault.com/gdc2015/presentations/Motomura_Junya_GuiltyGearXrd.pdf):
  investigate intentional plane/line design without assuming an unavailable mesh.
