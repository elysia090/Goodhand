# Goodhand development
Shape, tone, timing, and display placement have separate contracts.
Offline Python tools prepare and inspect data; they are not runtime dependencies.
The [visual baseline](../reference/visual-baseline/) is the approved appearance
for comparison. The [source study](../reference/source-study/) preserves the
earlier authored gray and the independent occupancy shared by both snapshots.
They are offline references, not runtime inputs. Source imagery is not included.

The v0.0.4 package embeds the original 288-pose grayscale artwork and
runs a 45-second turn by default. The source-only developer build
renders a separate generative terminal preview. Neither reads the
offline reference files at runtime. See
[player development](playback-development.md), the
[historical W5 experiment](historical-w5.md),
[design and quality gates](../DESIGN.md), and the [tone input contract](tone.md).

## Library

- `frame.rs`: borrowed occupancy rows/runs and caller-buffer mask expansion.
- `sequence.rs`: validated shape sequence and absolute-time frame selection.
- `tone.rs`: independently validated occupied-order gray8 values.
- `surface.rs`: fixed pixel placement, without terminal cells or I/O.

```rust
let shape = goodhand::Sequence::from_bytes(&shape_bytes)?;
let tones = goodhand::ToneSequence::from_bytes(&tone_bytes, shape)?;
let spec = goodhand::RasterSpec::REFERENCE;
let mut pixels = vec![0u8; spec.required_len()?];
let f = shape.frame_index_at(elapsed);
goodhand::write_surface(shape.frame(f).unwrap(), tones.frame(f).unwrap(), spec, &mut pixels)?;
```

Readers validate the complete asset before exposing frames. Parsing and borrowed
row iteration do not allocate. `write_mask` writes only the required prefix and
leaves a short buffer unchanged. `write_surface` validates before writing,
reuses the caller's allocation, clears the required canvas, and preserves any
unused tail. An occupied black value is not background occupancy.

## Generative model

`src/arm_model.rs` samples the same continuous arm shape at a chosen
yaw and physical sample aspect. It writes separate occupancy and gray
to caller-owned buffers, with no frame data or video input. The new
model is an approximation of the visual baseline, not a reconstruction
of its underlying geometry. See the [base and attr design](generative-base.md).

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo test --locked --release
cargo build --locked --release --lib
# Requires the installed target:
cargo build --locked --release --lib --target wasm32-unknown-unknown
# Offline tools require Python 3, NumPy, and Pillow:
python3 -m unittest discover -s tools -p 'test_*.py' -v
```

CI separates synthetic Python tests, Rust 1.100 quality/build checks, and Rust
1.70 compatibility. A Wasm library build does not establish browser performance.

## Offline workflow

Extraction requires `ffmpeg` and `ffprobe` on PATH. It validates the selected
source hash, first video stream, frame timestamps, and reference profile, then
stores occupancy candidates, sampled gray, and located threshold ambiguity
separately. Observed gray is not a recovered palette, depth, or normal field.

```sh
python3 tools/extract_reference.py source.mp4 observations.npz \
  --expected-sha256 SHA256 --direction reverse
python3 tools/pack_sequence.py observations.npz candidate.ghs --candidate
python3 tools/render_reference.py observations.npz observed-preview --frames 0 144
python3 tools/render_reference.py authored.npz authored-preview --kind authored --frames 0 144
python3 tools/pack_tone.py authored.npz candidate.ghs authored-export \
  --kind authored --profile reference-v1
```

`--candidate` explicitly allows an unaccepted mask for diagnostics; shape packing
otherwise requires acceptance. A lossless round trip preserves the chosen mask,
not proof that it is correct. Review extraction settings, ambiguity, and shape
before recording acceptance.

Previewing defaults to `observed` for existing callers. Authored preview requires
`--kind authored` (Python: `render(source, destination, frames, kind="authored")`).
Both use the same strict snapshot validator. No curves, relighting, repair,
interpolation, or adoption decisions occur. Preview PNGs retain the historical
`reference-fNNN.png` filenames and `reference-redraw.json` report name for either
role; the report identifies the role explicitly.

If you already have a lossless fixed-grid review, use
[review recovery](review-recovery.md) to recover its gray with the
original independent occupancy. No pose guide or authoring recipe is used.

Use a new output directory for every preview or tone export. Existing directories,
even empty ones, are rejected. Partial outputs after an I/O failure are not a
completed export; use a fresh destination when retrying. Source validation happens
before directory creation. Output hashes, input snapshot hashes, and declared
provenance are recorded. Mask acceptance never implies artwork adoption.
