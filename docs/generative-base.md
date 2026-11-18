# Generative arm base

The visual baseline is a comparison, not an input to the generator.
The generator traces a newly modeled forearm, wrist, palm and fingers
at the requested yaw. Neither it nor the terminal needs the baseline
movie, recovered frames, a pose guide, a mesh or per-angle table.

- **Base shape:** tapered sections and rounded planes join around the
  wrist and the folded thumb. The palm and forearm sit farther back
  than the fingertips. This depth is part of the shape, so the sides
  foreshorten without any angle-specific translations. The cut is a
  plane, not a spherical end. The two visible folded pads sit at
  different heights and use shallow cross-sections, so their overlap
  can read on edge without a painted crease. The inner fold still
  connects them to the palm. The open-finger gap is real empty space,
  not black paint.
- **Base tone:** the same fixed keys light the changing physical
  normals. The cut face's matte material and the gentle floor in
  shadow keep the light from becoming a uniform rim. The front palm bank
  redirects a portion of the existing key toward a broad, wrist-facing
  plane. This holds a receiving gray next to the thumb without lifting
  the dark floor, outlining the fingers, or affecting the back of the
  hand. Its support is expressed in palm coordinates, not screen cells
  or an angle table. The light response is an authored model, not a
  reconstruction of the reference capture or its original lighting.
- **Attrs:** a correction must identify a part of the live model,
  what it changes and where it abstains. Keep it separable while it is
  being compared. If the effect is reproducible and useful, put the
  rule in shape or material and retire the correction so it does not
  act a second time. The modeled heel and folded thumb were absorbed
  on the shape side; the receiving bank is part of the material.
  `ArmSampler::with_attrs` exposes an opt-in
  short contact shadow where two modeled surfaces meet. It changes
  reflected light, not the shadow floor or occupancy. It is off by
  default; the separate broad-forearm study is not part of the base.
- **Projection:** sample the same continuous world-space model at any
  finite yaw and requested viewport. Fit the physical aspect instead
  of stretching the figure; a zoom can crop but cannot distort it.
  Gray and occupancy are independent. A renderer may estimate edge
  coverage separately; a very small grid cannot show every gap.
- **Presentation:** let the consumer decide dot placement, background,
  display curve and time. Do not use the old frame count or decoded
  video clock as a requirement for arbitrary continuous yaw.

In a separate image study, the coat and hand in [Rembrandt's
self-portrait](https://www.nga.gov/artworks/79-self-portrait) help explain
why the whole dark area need not open to keep a small connection
legible. In [John Martin's proof](https://www.nga.gov/artworks/124846-joshua-commanding-sun-stand-still),
the dark rock still contains weak lines that follow its shape.
The [study of a hand attributed to Greuze](https://www.metmuseum.org/art/collection/search/785410)
keeps a short dark under the finger beside a much broader quieter tone.
Those observations motivate curved sections and small overlap tests,
not a noise layer, indiscriminate darkening or a new outline. In the
two views of [Rodin's clenched left hand](https://www.metmuseum.org/art/collection/search/207496),
distinct pads remain legible where they meet without a drawn seam.
The gesture is not copied. No museum image is read by the code.
