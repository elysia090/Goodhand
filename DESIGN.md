# Design

## Purpose and fixed contract

Preserve the hand's pose, silhouette, composition, and intentional asymmetry while
allowing tone to be authored independently. A strong image at every angle and
lightweight terminal presentation are separate goals; neither follows from a
small file or a passing test suite. Reproduce the reference before changing it.

| Property | Reference contract |
| --- | --- |
| Loop | Reverse, 24 fps, 288 frames, 12 seconds |
| Angle | Frame `f`: `(-1.25 * f) mod 360` degrees |
| Canvas | 1000 × 640 pixels; artwork region begins at y = 40 |
| Logical grid | 200 columns × 120 rows |
| Dot placement | Origin (2, 42), pitch 5 pixels, 2 × 2 pixel dots |
| Front | Frame 0, two extended fingers on screen right |
| Opposite view | Frame 144, two extended fingers on screen left |
| Composition | Fixed size, position, and axis; no per-frame fitting |

Screen orientation does not establish anatomical surface ownership. Neutral
shape means observed 2D occupancy, not recovered depth or normals. Compressed
video cannot always distinguish black occupied points from background or recover
original coverage/palette. Preserve uncertainty instead of silently repairing it.

## Responsibilities

```text
Offline observations or authored gray + independent occupancy
    → validated snapshots → packed shape and occupied-order tone
    → absolute-time frame selection → fixed pixel surface
    → future terminal encoding → future application I/O
```

Use one small Rust crate. Video decoders, NumPy, and Pillow remain offline tools.
`gray_source.py` validates role-separated input snapshots; `pack_sequence.py`
encodes occupancy; `pack_tone.py` checks exact mask agreement before tone export;
`render_reference.py` previews either declared role without changing its values.
No terminal backend or application clock/I/O loop is implemented yet.

- `ShapeFrame` represents occupancy; a tone frame represents gray8 in occupied
  row/run order. Keep these meanings distinct even if storage changes.
- `Sequence::frame_index_at(elapsed)` uses absolute time, not accumulated deltas.
  The reference profile has a 12-second period; other valid sequences use their
  own frame count and fps.
- Tone composition and quantization belong offline. Keep baseline and authored
  versions independently comparable. Runtime relighting is not required.
- Tone validation checks dimensions, fps, frame count, and occupied value counts.
  Matching dimensions/counts alone cannot establish provenance; retain hashes.
- `write_surface` changes no shape. In gray8 output, occupied black and background
  are visually identical. Never reconstruct occupancy from this surface.
- A future terminal encoder should reuse caller buffers and perform no I/O.
  Resizing invalidates differential state; clearing and exit restoration belong
  to an explicit terminal/application contract.
- Pixels, logical points, and terminal character cells are not interchangeable.
  Preserve aspect ratio and specify cropping/scaling/suspension policy explicitly.
  ANSI/Braille compatibility output requires its own visual evaluation.

## Binary contracts

`GHSH` version 1 stores shape. Its little-endian 16-byte header is magic `GHSH`,
version u16, width u16, height u16, fps u16, frame_count u32. Next come
`frame_count + 1` u32 offsets relative to the payload. Each frame has `height`
rows, each with a u8 run count and `(start: u8, length: u8)` pairs.

Width/height are 1..255, fps 1..1000, frame_count 1..4096; assets are at most
64 MiB. Offsets start at zero, are nondecreasing and in bounds, and end at payload
length. Runs are nonempty, ordered, in bounds, and separated by at least one
unoccupied point. Packers merge adjacent runs. Trailing frame bytes are invalid.

`GHT8` version 1 uses the same header layout with magic `GHT8`, offset table, and
occupied-order gray8 payload. Every frame's value count must match the shape's
run-length sum. Zero is a valid occupied tone. Provenance and acceptance stay
outside the payload.

Validation is O(asset bytes) without allocation; lookup reads two offsets;
borrowed row iteration is O(rows + runs). Expanding an N-cell mask writes N cells.
The pixel writer clears its required canvas and writes occupied footprints.
These are costs of the chosen implementation, not universal lower bounds.

## Quality gates

Do not treat version 0.0.1 as finished until all gates pass:

1. **Shape/composition:** compare the full cycle, boundaries, coordinates, and
   uncertainty; include actual terminal text and margins. Reject per-frame drift.
2. **Baseline/large planes:** reproduce the reference before authorship; judge
   hierarchy and distant readability, not merely global gain or contrast.
3. **Intermediate planes/boundaries:** inspect native size before enlargement.
   Reject collapsed internal separation and meaningless isolated patches.
4. **Motion:** compare key views, all-angle atlas, and 24 fps playback including
   last-to-first. Separate silhouette entry/exit from internal flicker. State
   interpolation assumptions for angles not present in the 1.25-degree samples.
5. **Terminal/runtime:** test near/far viewing, resize, pause/resume, exit restore,
   bandwidth, memory, long frames, and missed deadlines on a real device.
6. **Independent review:** check artwork, API boundaries, dependencies, recovery,
   and necessary work. Numerical success cannot waive visual failure.

## Performance evidence

The reference grid has 24,000 cells; at 24 fps the budget is about 41.67 ms/frame.
A dense byte field costs 6,912,000 bytes per loop; a separate one-bit occupancy
field costs 864,000 bytes. Those are representation costs, not information-theory
minima. Compare row runs, shared references, changed-cell encoding, and on-demand
expansion before assuming full-grid work is necessary.

Measure startup, validation, expansion, frame selection, tone placement, encoding,
write acceptance, and visible presentation separately. Record input hashes,
hardware/OS/terminal, build, target, and protocol. Count cursor/color/UTF-8 costs;
measured bytes/throughput estimates only apply under those conditions. Smaller
compression may increase decode and cache costs. Do not infer speed from Rust,
Wasm build success, average timing alone, or comparison with only one candidate.
