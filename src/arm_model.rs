//! Continuous model, orthographic sampling, and base gray for the Goodhand arm.
//!
//! Each view is traced from the same model at its requested sample locations.
//! Neither the shape nor the light reads from artwork, pose guides, prior
//! frames, or raster assets. Tone is not used to infer occupancy.

use core::fmt;

const CAMERA_DISTANCE: f64 = 1.65;
const FRAME_Y: f64 = 0.017;
const START_YAW_DEGREES: f64 = 9.25;
const PITCH_DEGREES: f64 = -2.55;
const HIT: f64 = 0.0016;
const TRACE_STEPS: usize = 96;
const GRAZING_LIMIT: usize = 320;
const GRAZING_DISTANCE: f64 = 0.015;
const HEEL_BLEND: f64 = 0.028;
const THUMB_BLEND: f64 = 0.018;
const HAND_BLEND: f64 = 0.019;
const WRIST_BLEND: f64 = 0.060;
// A broad material bank faces the lower, rear side of the tapered arm.
const FOREARM_BANK: Vec3 = Vec3::new(
    0.16034511339036536,
    -0.651_402_023_148_359_3,
    -0.741_596_149_430_439_8,
);

/// A view of the same modeled gesture at any finite yaw, on a caller's grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArmView {
    /// In degrees. Zero is the opening view; negative yaw turns in the
    /// reference playback direction. Angles wrap after 360 degrees.
    pub yaw_degrees: f64,
    pub width: usize,
    pub height: usize,
    /// Physical width divided by height of one sample, normally 1 for
    /// pixels. A cell renderer can pass its actual sample proportions.
    pub sample_aspect: f64,
    /// 1 fits the baseline framing without stretching it. Values above 1
    /// bring the arm closer and may crop it, without changing its proportions.
    pub zoom: f64,
}

impl ArmView {
    /// Start with square samples and the baseline framing.
    pub const fn new(yaw_degrees: f64, width: usize, height: usize) -> Self {
        Self {
            yaw_degrees,
            width,
            height,
            sample_aspect: 1.0,
            zoom: 1.0,
        }
    }

    /// Validate the view and return the number of bytes needed in each output.
    pub fn required_len(self) -> Result<usize, ArmRenderError> {
        self.validate().map(|view| view.len)
    }

    fn validate(self) -> Result<ProjectedGrid, ArmRenderError> {
        if self.width == 0 || self.height == 0 {
            return Err(ArmRenderError::InvalidGrid);
        }
        let len = self
            .width
            .checked_mul(self.height)
            .filter(|&len| len <= isize::MAX as usize)
            .ok_or(ArmRenderError::SizeOverflow)?;
        if !self.yaw_degrees.is_finite() {
            return Err(ArmRenderError::InvalidYaw);
        }
        if !self.sample_aspect.is_finite() || self.sample_aspect <= 0.0 {
            return Err(ArmRenderError::InvalidSampleAspect);
        }
        if !self.zoom.is_finite() || self.zoom <= 0.0 {
            return Err(ArmRenderError::InvalidZoom);
        }
        // Preserve the physical sample aspect and fit both reference spans.
        // Extra space in either axis simply adds margin around the same center.
        let width = self.width as f64;
        let height = self.height as f64;
        let horizontal = ArmSampler::FRAME_WIDTH / width;
        let vertical = ArmSampler::FRAME_HEIGHT / height;
        let x = horizontal.max(vertical * self.sample_aspect) / self.zoom;
        let y = vertical.max(horizontal / self.sample_aspect) / self.zoom;
        if x > 0.0 && y > 0.0 && (width * x).is_finite() && (height * y).is_finite() {
            Ok(ProjectedGrid { len, x, y })
        } else {
            Err(ArmRenderError::InvalidProjection)
        }
    }
}

struct ProjectedGrid {
    len: usize,
    x: f64,
    y: f64,
}

/// Optional authored surface attributes. The default is the base alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArmAttrs {
    /// Attenuate direct light in short hand-fold contacts. This never changes
    /// geometry, coverage, or the base ambient floor.
    pub contact_shadow: bool,
}

/// A prepared camera and the model it samples, without a fixed output grid.
///
/// Use one sampler per yaw for a custom aperture or projection. Samples are
/// independent; construction and sampling need no heap allocation or I/O.
pub struct ArmSampler {
    camera: Camera,
    model: Arm,
    bounds: ArmBounds,
    lights: SceneLights,
    attrs: ArmAttrs,
}

impl ArmSampler {
    /// Width of the baseline framing, in orthographic plane units.
    pub const FRAME_WIDTH: f64 = 2.0;

    /// Height of the baseline framing, in the same units as its width.
    pub const FRAME_HEIGHT: f64 = 1.2;

    /// Prepare to sample the given yaw, in degrees, with the same orientation
    /// and wrapping as [`ArmView`].
    pub fn new(yaw_degrees: f64) -> Result<Self, ArmRenderError> {
        Self::with_attrs(yaw_degrees, ArmAttrs::default())
    }

    /// Prepare a view with explicitly chosen surface attributes. The base
    /// form, ray positions, and intersection tests remain the same.
    pub fn with_attrs(yaw_degrees: f64, attrs: ArmAttrs) -> Result<Self, ArmRenderError> {
        if !yaw_degrees.is_finite() {
            return Err(ArmRenderError::InvalidYaw);
        }
        let camera = Camera::new(yaw_degrees);
        let model = Arm::new();
        let bounds = model.bounds();
        let lights = SceneLights::new(&camera);
        Ok(Self {
            camera,
            model,
            bounds,
            lights,
            attrs,
        })
    }

    /// Trace one point in the orthographic plane. The baseline frame is
    /// centered at (0, 0), with +x right and +y up. Use an equal unit on the
    /// two axes to preserve the model's proportions. The nominal frame is
    /// [-1, 1] × [-0.6, 0.6]; samples can also be taken outside it.
    ///
    /// `None` means the ray is empty or a coordinate is nonfinite. A returned
    /// value is occupied gray with the chosen surface attributes (if any),
    /// not composited gray or display intensity.
    pub fn sample(&self, x: f64, y: f64) -> Option<u8> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        self.sample_row(self.row_origin(y), x)
    }

    fn row_origin(&self, y: f64) -> Vec3 {
        self.camera.eye + self.camera.up * (y + FRAME_Y)
    }

    fn sample_row(&self, row_origin: Vec3, x: f64) -> Option<u8> {
        let origin = row_origin + self.camera.right * x;
        let point = self.camera.trace(origin, &self.model, self.bounds)?;
        let normal = self.model.normal(point, self.bounds);
        let receiver = self.model.palm_receiving_weight(point, normal);
        let (floor, reflected) = self.lights.base_light(point, normal, receiver);
        let direct_scale = if self.attrs.contact_shadow {
            self.model.contact_light_scale(point, normal)
        } else {
            1.0
        };
        Some((floor + reflected * direct_scale).round() as u8)
    }
}

/// Resample the model and its broad base lighting into separate caller buffers.
///
/// Both outputs are dense row-major fields, with row zero at the top.
/// Occupancy is 0 or 1. Background gray is 0; occupied base gray is supplied
/// independently of occupancy. Display mapping belongs to the consumer.
/// Validation occurs before any writes; unused tails stay unchanged. No
/// allocation, I/O, or reliance on earlier views is needed.
pub fn render_arm(
    view: ArmView,
    occupancy: &mut [u8],
    base_gray: &mut [u8],
) -> Result<(), ArmRenderError> {
    let ProjectedGrid {
        len,
        x: step_x,
        y: step_y,
    } = view.validate()?;
    if occupancy.len() < len {
        return Err(ArmRenderError::OccupancyBufferTooShort {
            required: len,
            actual: occupancy.len(),
        });
    }
    if base_gray.len() < len {
        return Err(ArmRenderError::GrayBufferTooShort {
            required: len,
            actual: base_gray.len(),
        });
    }

    let sampler = ArmSampler::new(view.yaw_degrees)?;
    let center_x = 0.5 * (view.width - 1) as f64;
    let center_y = 0.5 * (view.height - 1) as f64;
    let occupied = occupancy[..len].chunks_exact_mut(view.width);
    let gray = base_gray[..len].chunks_exact_mut(view.width);

    for (row, (mask_row, gray_row)) in occupied.zip(gray).enumerate() {
        let y = (center_y - row as f64) * step_y;
        let row_origin = sampler.row_origin(y);
        for (column, (mask, gray)) in mask_row.iter_mut().zip(gray_row).enumerate() {
            let x = (column as f64 - center_x) * step_x;
            if let Some(base) = sampler.sample_row(row_origin, x) {
                *mask = 1;
                *gray = base;
            } else {
                *mask = 0;
                *gray = 0;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmRenderError {
    InvalidGrid,
    SizeOverflow,
    InvalidYaw,
    InvalidSampleAspect,
    InvalidZoom,
    InvalidProjection,
    OccupancyBufferTooShort { required: usize, actual: usize },
    GrayBufferTooShort { required: usize, actual: usize },
}

impl fmt::Display for ArmRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid arm render input: {self:?}")
    }
}

impl std::error::Error for ArmRenderError {}

#[derive(Clone, Copy)]
struct Vec3 {
    x: f64,
    y: f64,
    z: f64,
}

impl Vec3 {
    const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    fn scale_axes(self, other: Self) -> Self {
        Self::new(self.x * other.x, self.y * other.y, self.z * other.z)
    }

    fn min(self, other: Self) -> Self {
        Self::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
        )
    }

    fn max(self, other: Self) -> Self {
        Self::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
        )
    }
}

impl core::ops::Add for Vec3 {
    type Output = Vec3;

    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl core::ops::Sub for Vec3 {
    type Output = Vec3;

    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl core::ops::Mul<f64> for Vec3 {
    type Output = Vec3;

    fn mul(self, rhs: f64) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}

impl Bounds {
    fn around(a: Vec3, b: Vec3, radius: Vec3) -> Self {
        Self {
            min: a.min(b) - radius,
            max: a.max(b) + radius,
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

struct Taper {
    start: Vec3,
    end: Vec3,
    scaled_start: Vec3,
    axis: Vec3,
    inverse_axis_length_squared: f64,
    radius_start: f64,
    radius_delta: f64,
    depth_scale: f64,
    inverse_depth_scale: f64,
    distance_scale: f64,
}

impl Taper {
    fn new(start: Vec3, end: Vec3, radius_start: f64, radius_end: f64, depth_scale: f64) -> Self {
        let inverse_depth_scale = depth_scale.recip();
        let scale = Vec3::new(1.0, 1.0, inverse_depth_scale);
        let scaled_start = start.scale_axes(scale);
        let axis = end.scale_axes(scale) - scaled_start;
        Self {
            start,
            end,
            scaled_start,
            axis,
            inverse_axis_length_squared: axis.dot(axis).recip(),
            radius_start,
            radius_delta: radius_end - radius_start,
            depth_scale,
            inverse_depth_scale,
            distance_scale: depth_scale.min(1.0),
        }
    }

    fn distance(&self, p: Vec3) -> f64 {
        let relative = Vec3::new(p.x, p.y, p.z * self.inverse_depth_scale) - self.scaled_start;
        let phase = (relative.dot(self.axis) * self.inverse_axis_length_squared).clamp(0.0, 1.0);
        let radius = self.radius_start + phase * self.radius_delta;
        ((relative - self.axis * phase).length() - radius) * self.distance_scale
    }

    fn bounds(&self, distance_limit: f64) -> Bounds {
        let padding = distance_limit / self.distance_scale;
        let radial = Vec3::new(1.0, 1.0, self.depth_scale);
        let start_radius = radial * (self.radius_start + padding);
        let end_radius = radial * (self.radius_start + self.radius_delta + padding);
        Bounds::around(self.start, self.start, start_radius)
            .union(Bounds::around(self.end, self.end, end_radius))
    }
}

struct Ellipsoid {
    center: Vec3,
    radius: Vec3,
    inverse_radius: Vec3,
    min_radius: f64,
}

impl Ellipsoid {
    fn new(center: Vec3, radius: Vec3) -> Self {
        Self {
            center,
            radius,
            inverse_radius: Vec3::new(radius.x.recip(), radius.y.recip(), radius.z.recip()),
            min_radius: radius.x.min(radius.y).min(radius.z),
        }
    }

    fn distance(&self, p: Vec3) -> f64 {
        ((p - self.center).scale_axes(self.inverse_radius).length() - 1.0) * self.min_radius
    }

    fn bounds(&self, distance_limit: f64) -> Bounds {
        Bounds::around(
            self.center,
            self.center,
            self.radius * (1.0 + distance_limit / self.min_radius),
        )
    }
}

struct Arm {
    forearm: Taper,
    palm: Ellipsoid,
    heel: Ellipsoid,
    thumb: Taper,
    lower_fold: Ellipsoid,
    upper_fold: Ellipsoid,
    inner_fold: Ellipsoid,
    raised_finger: Taper,
    extended_finger: Taper,
}

impl Arm {
    fn new() -> Self {
        // The long arm points right, up is +y, and the palm faces toward +z.
        // Wrist and palm sit behind the spread fingertips. Model that
        // depth here so foreshortening works in both directions at any yaw.
        Self {
            forearm: Taper::new(
                Vec3::new(-0.81, -0.192, -0.074),
                Vec3::new(0.13, -0.057, -0.067),
                0.159,
                0.108,
                1.24,
            ),
            palm: Ellipsoid::new(
                Vec3::new(0.335, 0.012, -0.048),
                Vec3::new(0.310, 0.171, 0.160),
            ),
            heel: Ellipsoid::new(
                Vec3::new(0.194, 0.050, -0.057),
                Vec3::new(0.177, 0.147, 0.140),
            ),
            thumb: Taper::new(
                Vec3::new(0.195, 0.054, 0.054),
                Vec3::new(0.422, -0.058, 0.105),
                0.077,
                0.067,
                0.95,
            ),
            // Two unequal, shallow pads overlap in depth at different
            // heights instead of merging into one round cross-section.
            // Recess the inner support into the thumb-side joint so it
            // does not form a third projecting pad.
            lower_fold: Ellipsoid::new(
                Vec3::new(0.489, -0.089, -0.059),
                Vec3::new(0.125, 0.063, 0.117),
            ),
            upper_fold: Ellipsoid::new(
                Vec3::new(0.481, -0.037, 0.049),
                Vec3::new(0.115, 0.058, 0.108),
            ),
            inner_fold: Ellipsoid::new(
                Vec3::new(0.325, -0.080, 0.043),
                Vec3::new(0.111, 0.099, 0.098),
            ),
            raised_finger: Taper::new(
                Vec3::new(0.40, 0.099, -0.027),
                Vec3::new(0.753, 0.336, -0.058),
                0.072,
                0.050,
                0.85,
            ),
            extended_finger: Taper::new(
                Vec3::new(0.408, 0.071, 0.013),
                Vec3::new(0.861, 0.141, 0.022),
                0.063,
                0.046,
                0.91,
            ),
        }
    }

    fn distance(&self, p: Vec3, bounds: ArmBounds) -> f64 {
        let arm = self.arm_distance(p);
        // Each primitive has an x-axis lower bound outside its grown
        // interval. The hand interval includes every possible reduction
        // from its smooth joins, so their union cannot get nearer than
        // this deliberately loose slope. When it stays outside the
        // forearm blend, returning arm is identical to evaluating the join.
        let left_gap = bounds.hand.min.x - p.x;
        // Keep extra slack for floating-point arithmetic near the blend.
        if left_gap > 0.0 && left_gap * bounds.hand_x_slope > arm + WRIST_BLEND + 1e-12 {
            return arm;
        }
        self.hand_distance(p, arm)
    }

    fn arm_distance(&self, p: Vec3) -> f64 {
        // Intersect with a plane to keep the proximal end intentionally cut.
        let cut = -p.x - 0.882 - 0.19 * (p.y + 0.19);
        self.forearm.distance(p).max(cut)
    }

    fn hand_distance(&self, p: Vec3, arm: f64) -> f64 {
        let hand = smooth_join(self.palm.distance(p), self.heel.distance(p), HEEL_BLEND);
        let hand = smooth_join(hand, self.thumb.distance(p), THUMB_BLEND);
        let hand = smooth_join(hand, self.lower_fold.distance(p), HAND_BLEND);
        let hand = smooth_join(hand, self.upper_fold.distance(p), HAND_BLEND);
        let hand = smooth_join(hand, self.inner_fold.distance(p), HAND_BLEND);
        let hand = smooth_join(hand, self.raised_finger.distance(p), HAND_BLEND);
        let hand = smooth_join(hand, self.extended_finger.distance(p), HAND_BLEND);
        smooth_join(arm, hand, WRIST_BLEND)
    }

    fn bounds(&self) -> ArmBounds {
        // A smooth join can lower the lesser input by at most width/4.
        // Allow for that displacement in every relevant primitive's bounds.
        let wrist = HIT + 0.25 * WRIST_BLEND;
        let grown = |remaining_joins: u8| wrist + 0.25 * HAND_BLEND * f64::from(remaining_joins);
        let thumb = grown(5) + 0.25 * THUMB_BLEND;
        let heel = thumb + 0.25 * HEEL_BLEND;
        let ellipsoids = [
            &self.palm,
            &self.heel,
            &self.lower_fold,
            &self.upper_fold,
            &self.inner_fold,
        ];
        let tapers = [&self.thumb, &self.raised_finger, &self.extended_finger];
        let hand_x_slope = ellipsoids
            .iter()
            .map(|e| e.min_radius * e.inverse_radius.x)
            .chain(tapers.iter().map(|t| t.distance_scale))
            .fold(1.0_f64, f64::min);
        ArmBounds {
            hand_x_slope,
            forearm: self.forearm.bounds(wrist),
            hand: self
                .palm
                .bounds(heel)
                .union(self.heel.bounds(heel))
                .union(self.thumb.bounds(thumb))
                .union(self.lower_fold.bounds(grown(5)))
                .union(self.upper_fold.bounds(grown(4)))
                .union(self.inner_fold.bounds(grown(3)))
                .union(self.raised_finger.bounds(grown(2)))
                .union(self.extended_finger.bounds(grown(1))),
        }
    }

    fn palm_receiving_weight(&self, point: Vec3, normal: Vec3) -> f64 {
        // Only the front bank of the palm, above the short folded contact,
        // receives this light. Use fractions of the palm axes to give the
        // bank a soft edge rather than making it another piece of geometry.
        if normal.z <= 0.17 {
            return 0.0;
        }
        let local = (point - self.palm.center).scale_axes(self.palm.inverse_radius);
        if local.z <= 0.34 {
            return 0.0;
        }
        let x = (local.x - 0.13) / 0.75;
        let y = (local.y - 0.35) / 0.84;
        let area = smooth_step(1.0 - x * x - y * y);
        let front = smooth_step((local.z - 0.34) / 0.44) * smooth_step((normal.z - 0.17) / 0.56);
        0.64 * area * front
    }

    fn contact_light_scale(&self, point: Vec3, normal: Vec3) -> f64 {
        let palm = smooth_join(
            self.palm.distance(point),
            self.heel.distance(point),
            HEEL_BLEND,
        );
        let fold = self
            .thumb
            .distance(point)
            .abs()
            .min(self.lower_fold.distance(point).abs())
            .min(self.upper_fold.distance(point).abs())
            .min(self.inner_fold.distance(point).abs());
        let proximity = 1.0 - smooth_step((palm.abs().max(fold) - 0.013) / 0.026);
        let lower_facing = smooth_step((0.22 - normal.y) / 0.59);
        let end = 1.0 - smooth_step((point.x - 0.52) / 0.12);
        1.0 - 0.19 * proximity * lower_facing * end
    }

    fn normal(&self, point: Vec3, bounds: ArmBounds) -> Vec3 {
        let x = Vec3::new(0.0015, 0.0, 0.0);
        let y = Vec3::new(0.0, 0.0015, 0.0);
        let z = Vec3::new(0.0, 0.0, 0.0015);
        let gradient = Vec3::new(
            self.distance(point + x, bounds) - self.distance(point - x, bounds),
            self.distance(point + y, bounds) - self.distance(point - y, bounds),
            self.distance(point + z, bounds) - self.distance(point - z, bounds),
        );
        gradient * gradient.length().max(1.0e-8).recip()
    }
}

fn smooth_join(a: f64, b: f64, width: f64) -> f64 {
    let overlap = (width - (a - b).abs()).max(0.0) / width;
    a.min(b) - 0.25 * width * overlap * overlap
}

#[derive(Clone, Copy)]
struct ArmBounds {
    forearm: Bounds,
    hand: Bounds,
    hand_x_slope: f64,
}

struct Camera {
    eye: Vec3,
    right: Vec3,
    up: Vec3,
    direction: Vec3,
    inverse_direction: Vec3,
}

impl Camera {
    fn new(yaw_degrees: f64) -> Self {
        // Reduce before the offset so huge finite inputs cannot erase the phase.
        let yaw = (yaw_degrees.rem_euclid(360.0) + START_YAW_DEGREES).rem_euclid(360.0);
        let (sin_yaw, cos_yaw) = yaw.to_radians().sin_cos();
        let (sin_pitch, cos_pitch) = PITCH_DEGREES.to_radians().sin_cos();
        let toward_eye = Vec3::new(-sin_yaw * cos_pitch, -sin_pitch, cos_yaw * cos_pitch);
        let direction = toward_eye * -1.0;
        Self {
            eye: toward_eye * CAMERA_DISTANCE,
            right: Vec3::new(cos_yaw, 0.0, sin_yaw),
            up: Vec3::new(-sin_yaw * sin_pitch, cos_pitch, cos_yaw * sin_pitch),
            direction,
            inverse_direction: Vec3::new(
                direction.x.recip(),
                direction.y.recip(),
                direction.z.recip(),
            ),
        }
    }

    fn model_direction(&self, scene_direction: Vec3) -> Vec3 {
        self.right * scene_direction.x + self.up * scene_direction.y
            - self.direction * scene_direction.z
    }

    fn trace(&self, origin: Vec3, model: &Arm, bounds: ArmBounds) -> Option<Vec3> {
        // Empty corners in the combined silhouette bounds can otherwise
        // cost many steps. Retain rays reaching the forearm or the hand.
        let (near, far) = match (
            self.clip(origin, bounds.forearm),
            self.clip(origin, bounds.hand),
        ) {
            (Some((a, b)), Some((c, d))) => (a.min(c), b.max(d)),
            (Some(span), None) | (None, Some(span)) => span,
            (None, None) => return None,
        };
        let mut travel = near;
        for step in 0..GRAZING_LIMIT {
            let probe = origin + self.direction * travel;
            let distance = model.distance(probe, bounds);
            if distance <= HIT {
                return Some(probe);
            }
            // Almost tangent rays can travel along a side plane for many
            // small steps. Keep tracing those near the form; this continues
            // the same ray, without repairing or filtering mask samples.
            if step == TRACE_STEPS && distance > GRAZING_DISTANCE {
                break;
            }
            // This factor leaves margin for the tapered segments and joins,
            // whose fields are not exact Euclidean signed distances.
            travel += HIT.max(0.62 * distance);
            if travel > far {
                break;
            }
        }
        None
    }

    fn clip(&self, origin: Vec3, bounds: Bounds) -> Option<(f64, f64)> {
        let mut near: f64 = 0.0;
        let mut far = 2.0 * CAMERA_DISTANCE;
        let inv = self.inverse_direction;
        if clip_axis(
            origin.x,
            inv.x,
            bounds.min.x,
            bounds.max.x,
            &mut near,
            &mut far,
        ) && clip_axis(
            origin.y,
            inv.y,
            bounds.min.y,
            bounds.max.y,
            &mut near,
            &mut far,
        ) && clip_axis(
            origin.z,
            inv.z,
            bounds.min.z,
            bounds.max.z,
            &mut near,
            &mut far,
        ) {
            Some((near, far))
        } else {
            None
        }
    }
}

fn clip_axis(origin: f64, inv: f64, min: f64, max: f64, near: &mut f64, far: &mut f64) -> bool {
    if !inv.is_finite() {
        return (min..=max).contains(&origin);
    }
    let a = (min - origin) * inv;
    let b = (max - origin) * inv;
    *near = near.max(a.min(b));
    *far = far.min(a.max(b));
    *near <= *far
}

struct SceneLights {
    key: Vec3,
    fill: Vec3,
    palm_key: f64,
    forearm_key: f64,
}

impl SceneLights {
    fn new(camera: &Camera) -> Self {
        // The scene lights remain fixed as the arm rotates. Transform them
        // into model coordinates once, not through a per-angle light curve.
        let (sin_azimuth, cos_azimuth) = (-36.2_f64).to_radians().sin_cos();
        let (sin_elevation, cos_elevation) = 1.0_f64.to_radians().sin_cos();
        let key = Vec3::new(
            sin_azimuth * cos_elevation,
            sin_elevation,
            cos_azimuth * cos_elevation,
        );
        let fill = Vec3::new(0.50, 0.42, 0.755);
        let key = camera.model_direction(key);
        // This broad material plane faces upward and back toward the wrist.
        // Its response follows the existing key, not the viewing angle.
        let palm_normal = Vec3::new(-0.665, 0.29, 0.6883);
        let palm_key = 0.85 * palm_normal.dot(key).clamp(0.0, 1.0).powf(1.8);
        // A fixed weak bounce from below. Compute the incident response
        // once for the broad bank; its footprint still follows each ray's
        // real hit and surface normal.
        let bounce = Vec3::new(0.30, -0.55, 0.78);
        let bounce = camera.model_direction(bounce * bounce.length().recip());
        let forearm_key = 38.0 * smooth_step((FOREARM_BANK.dot(bounce) - 0.78) / 0.22);
        Self {
            key,
            fill: camera.model_direction(fill),
            palm_key,
            forearm_key,
        }
    }

    fn forearm_bounce(&self, point: Vec3, normal: Vec3) -> f64 {
        if self.forearm_key == 0.0 || !(-0.875..-0.05).contains(&point.x) {
            return 0.0;
        }
        let facing = FOREARM_BANK.dot(normal);
        if facing <= 0.71 {
            return 0.0;
        }
        // Fade before the cut and wrist instead of ringing either edge.
        let along =
            smooth_step((point.x + 0.875) / 0.16) * (1.0 - smooth_step((point.x + 0.29) / 0.24));
        self.forearm_key * along * smooth_step((facing - 0.71) / 0.29)
    }

    fn base_light(&self, point: Vec3, normal: Vec3, receiver: f64) -> (f64, f64) {
        let key = ((normal.dot(self.key) - 0.034) / 0.966)
            .clamp(0.0, 1.0)
            .powf(1.38);
        let fill = ((normal.dot(self.fill) - 0.25) / 0.75)
            .clamp(0.0, 1.0)
            .powf(1.65);
        let palm = smooth_step((point.x - 0.10) / 0.28);
        let cap = smooth_step((-point.x - 0.72) / 0.13) * smooth_step((-normal.x - 0.62) / 0.38);
        let material = (1.0 - 0.119 * cap) * (1.0 - 0.091 * palm);
        let floor = 34.3 + 7.0 * smooth_step((normal.y + 0.19) / 0.90);
        let fill = (0.012 + 0.12 * palm) * fill;
        let incident = key.max(fill);
        // Keep the stronger response rather than adding strength to an
        // already-lit forearm. The ambient floor stays independent.
        let reflected = (116.43 * incident * material).max(self.forearm_bounce(point, normal));
        if receiver == 0.0 || floor + reflected <= 44.0 {
            return (floor, reflected);
        }
        // Redirect part of the existing key, not the fill or the shadow
        // floor. Ease in above deep shadow; any contact attr is later.
        let received = (key + (self.palm_key - key) * receiver).max(fill);
        let lit = smooth_step((floor + reflected - 44.0) / 31.0);
        let reflected = reflected + lit * (116.43 * (received - incident) * material);
        (floor, reflected)
    }
}

fn smooth_step(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forearm_bounce_reads_a_bank_without_changing_the_ambient_floor() {
        let lights = SceneLights::new(&Camera::new(-135.0));
        let off = SceneLights {
            forearm_key: 0.0,
            ..lights
        };
        let bank = Vec3::new(-0.58, -0.14, -0.16);
        let (floor, reflected) = lights.base_light(bank, FOREARM_BANK, 0.0);
        let (plain_floor, plain_reflected) = off.base_light(bank, FOREARM_BANK, 0.0);
        assert_eq!(floor, plain_floor);
        assert!(reflected > plain_reflected + 12.0);
        for (p, n) in [
            (bank, Vec3::new(0.0, 1.0, 0.0)),
            (bank, Vec3::new(-1.0, 0.0, 0.0)),
            (Vec3::new(0.1, -0.1, -0.1), FOREARM_BANK),
        ] {
            assert_eq!(lights.base_light(p, n, 0.0), off.base_light(p, n, 0.0));
        }
        let front = SceneLights::new(&Camera::new(0.0));
        assert_eq!(front.forearm_bounce(bank, FOREARM_BANK), 0.0);
    }

    #[test]
    fn hand_bound_matches_the_unpruned_field_through_the_wrist() {
        let arm = Arm::new();
        let bounds = arm.bounds();
        let mut skips = 0;
        for ix in -92..=45 {
            for iy in -24..=24 {
                for iz in -6..=6 {
                    let p = Vec3::new(
                        f64::from(ix) / 100.0,
                        f64::from(iy) / 50.0,
                        f64::from(iz) / 20.0,
                    );
                    let stock = arm.arm_distance(p);
                    let unpruned = arm.hand_distance(p, stock);
                    let gap = bounds.hand.min.x - p.x;
                    if gap > 0.0 && gap * bounds.hand_x_slope > stock + WRIST_BLEND + 1e-12 {
                        skips += 1;
                    }
                    assert_eq!(arm.distance(p, bounds).to_bits(), unpruned.to_bits());
                }
            }
        }
        assert!(skips > 1000);
    }

    #[test]
    fn foreshortened_forearm_keeps_a_shallow_capped_section() {
        let arm = ArmSampler::new(90.0).unwrap();
        // The far edge belongs to the cut arm, below the palm and fingers.
        // Keep its outer plane present without widening the end into a bulb.
        assert!(arm.sample(0.245, -0.255).is_some());
        assert!(arm.sample(0.265, -0.255).is_none());
    }

    #[test]
    fn folded_pads_stay_joined_to_the_palm() {
        let arm = Arm::new();
        assert!(arm.upper_fold.center.y > arm.lower_fold.center.y);
        for pad in [&arm.upper_fold, &arm.lower_fold] {
            assert!(pad.radius.y < pad.radius.z);
        }
        for fold in [&arm.upper_fold, &arm.lower_fold, &arm.inner_fold] {
            for i in 0..=8 {
                let t = f64::from(i) / 8.0;
                let point = fold.center + (arm.palm.center - fold.center) * t;
                assert!(arm.distance(point, arm.bounds()) < 0.0);
            }
        }
    }

    #[test]
    fn inner_fold_support_recedes_behind_the_visible_pads() {
        let arm = Arm::new();
        let bounds = arm.bounds();
        // The support still has volume under the thumb-side joint, but
        // does not make a third cap in front of the two folded pads.
        assert!(arm.distance(Vec3::new(0.325, -0.110, 0.116), bounds) < -HIT);
        assert!(arm.distance(Vec3::new(0.325, -0.110, 0.163), bounds) > HIT);
    }

    #[test]
    fn foreshortened_fold_shows_a_short_valley_between_receiving_pads() {
        // One witness at the side of the turn, not a lighting preset.
        // Each sample must still intersect a continuous physical gesture.
        let arm = ArmSampler::new(-101.25).unwrap();
        let band = |y| {
            [-0.165, -0.155, -0.145]
                .into_iter()
                .map(|x| u16::from(arm.sample(x, y).unwrap()))
                .sum::<u16>()
        };
        let upper = band(-0.075);
        let valley = band(-0.105);
        let lower = band(-0.125);
        assert!(upper > valley + 30 && lower > valley + 30);
    }

    #[test]
    fn palm_receiver_is_a_front_bank_with_a_soft_boundary() {
        let arm = Arm::new();
        let front = Vec3::new(0.0, 0.0, 1.0);
        let origin = arm.palm.center;
        let radius = arm.palm.radius;
        let bank = Vec3::new(0.13, 0.35, 1.0);
        let strength = |local: Vec3, normal| {
            arm.palm_receiving_weight(origin + local.scale_axes(radius), normal)
        };
        assert!(strength(bank, front) > 0.55);
        let near = strength(bank + Vec3::new(0.30, 0.0, 0.0), front);
        let edge = strength(bank + Vec3::new(0.71, 0.0, 0.0), front);
        assert!(strength(bank, front) > near && near > edge && edge > 0.0);
        assert_eq!(strength(bank + Vec3::new(0.76, 0.0, 0.0), front), 0.0);
        assert_eq!(strength(bank + Vec3::new(0.0, -0.85, 0.0), front), 0.0);
        assert_eq!(strength(bank + Vec3::new(0.0, 0.0, -1.0), front), 0.0);
        assert_eq!(strength(bank, front * -1.0), 0.0);
    }

    #[test]
    fn palm_receiver_changes_key_response_but_never_the_dark_floor() {
        let wrist_light = SceneLights::new(&Camera::new(0.0));
        let finger_light = SceneLights::new(&Camera::new(-90.0));
        assert!(wrist_light.palm_key > finger_light.palm_key);
        assert!(wrist_light.palm_key > 0.55);

        let point = Vec3::new(0.373, 0.09, 0.1);
        let front_bank = Vec3::new(-0.18, 0.62, 0.76);
        let (floor, direct) = wrist_light.base_light(point, front_bank, 0.0);
        let (received_floor, received) = wrist_light.base_light(point, front_bank, 0.6);
        assert_eq!(floor, received_floor);
        assert!(received > direct);

        let (floor, direct) = finger_light.base_light(point, front_bank, 0.0);
        let (received_floor, received) = finger_light.base_light(point, front_bank, 0.6);
        assert_eq!(floor, received_floor);
        assert!(received < direct);

        let dark = Vec3::new(0.0, -1.0, 0.0);
        let unmodified = wrist_light.base_light(point, dark, 0.0);
        assert!(unmodified.0 + unmodified.1 < 44.0);
        assert_eq!(wrist_light.base_light(point, dark, 0.6), unmodified);
    }
}
