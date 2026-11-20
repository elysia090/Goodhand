//! The shared native artwork raster, schedule and aperture.
mod bearing_tone;
#[cfg(target_os = "linux")]
mod gtk;
mod interior_matte;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod portable;
#[cfg(goodhand_native_motion)]
mod wrist_tone;

#[cfg(target_os = "linux")]
pub(crate) use gtk::run;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) use portable::run;

use self::interior_matte::InteriorMatte;
use crate::black_grade::{self, BACKDROP_RGB};
use crate::playback::{Cycle, Repeat, Timeline};
use crate::reference_frame::ReferenceFrame;
use crate::settings::WindowAppearance;
use goodhand::RasterSpec;
use std::ffi::c_int;
use std::io;
use std::time::Duration;

const WIDTH: usize = RasterSpec::REFERENCE.width;
const HEIGHT: usize = RasterSpec::REFERENCE.height;
const PANEL_WIDTH: usize = 1120;
const PANEL_HEIGHT: usize = 560;
const BLACK_WIDTH: usize = 1016;
const BLACK_HEIGHT: usize = 462;
const BLACK_BOTTOM: i32 = 8;
const BLACK_EXTRA_TOP: i32 = 8;
const BLACK_SHIFT: [i32; 2] = [-32, -8];
#[cfg(any(target_os = "linux", test))]
const MIN_SCALE: f64 = 0.60;

#[derive(Clone, Copy, Eq, PartialEq)]
struct Canvas {
    width: usize,
    height: usize,
    crop_top: i32,
    offset_x: i32,
}

impl Canvas {
    fn frame(
        (lo, hi): ([i32; 2], [i32; 2]),
        [width, height]: [usize; 2],
        shift: [i32; 2],
        vertical_gutter: i32,
    ) -> Option<Self> {
        let left = (WIDTH as i32 - width as i32) / 2 - shift[0];
        let top = (lo[1] + hi[1] - height as i32).div_euclid(2) - shift[1];
        if lo[0] < left
            || hi[0] > left + width as i32
            || lo[1] - top < vertical_gutter
            || top + height as i32 - hi[1] < vertical_gutter
        {
            return None;
        }
        Some(Self {
            width,
            height,
            crop_top: top,
            offset_x: shift[0],
        })
    }

    #[cfg(any(target_os = "linux", test))]
    fn client_at(self, scale: f64) -> (c_int, c_int) {
        (
            (scale * self.width as f64).round() as c_int,
            (scale * self.height as f64).round() as c_int,
        )
    }

    fn sized_by_height(self, height: usize) -> (c_int, c_int) {
        let width = (height * self.width).div_ceil(self.height);
        (width as c_int, height as c_int)
    }

    fn compact(self) -> (c_int, c_int) {
        // About an 80×24 terminal footprint, without changing the aperture.
        self.sized_by_height((13 * self.height).div_ceil(20))
    }

    fn minimum(self) -> (c_int, c_int) {
        // At 0.60× even 2×2 source dots retain a nearest-neighbor sample.
        self.sized_by_height((3 * self.height).div_ceil(5))
    }
}

struct Apertures {
    standard: Canvas,
    black: Canvas,
}

impl Apertures {
    fn new(bounds: ([i32; 2], [i32; 2])) -> io::Result<Self> {
        let standard =
            Canvas::frame(bounds, [PANEL_WIDTH, PANEL_HEIGHT], [0, 0], 16).ok_or_else(|| {
                io::Error::other("this archive cannot fit the wide panel without cropping")
            })?;
        // Fixed compact presentation for the reviewed black artwork. If
        // another embedded archive is too large, keep the wider-safe framing.
        let black = Canvas::frame(bounds, [BLACK_WIDTH, BLACK_HEIGHT], BLACK_SHIFT, 6)
            .map(|mut aperture| {
                // Keep the horizontal composition and the lower edge.
                // Add headroom above the original black-pane framing.
                let height = bounds.1[1] - aperture.crop_top + BLACK_BOTTOM;
                aperture.crop_top -= BLACK_EXTRA_TOP;
                aperture.height = aperture.height.min(height as usize) + BLACK_EXTRA_TOP as usize;
                aperture
            })
            .unwrap_or(standard);
        Ok(Self { standard, black })
    }

    fn for_appearance(&self, appearance: WindowAppearance) -> Canvas {
        match appearance {
            WindowAppearance::Safe => self.black,
            WindowAppearance::Clear | WindowAppearance::Framed => self.standard,
        }
    }
}
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy)]
struct Placement {
    size: (c_int, c_int),
    scale: f64,
    origin: (c_int, c_int),
}

#[cfg(any(target_os = "linux", test))]
impl Placement {
    fn new(canvas: Canvas, width: c_int, height: c_int) -> Self {
        // Fit the selected aperture's aspect without stretching the artwork.
        // Integer arithmetic preserves the original 2:1 path exactly.
        let available_width = width.max(0);
        let available_height = height.max(0);
        let width_limited_height =
            (i64::from(available_width) * canvas.height as i64 / canvas.width as i64) as c_int;
        let panel_height = available_height.min(width_limited_height);
        let panel_width =
            (i64::from(panel_height) * canvas.width as i64 / canvas.height as i64) as c_int;
        let size = (panel_width, panel_height);
        Self {
            size,
            scale: f64::from(panel_height) / canvas.height as f64,
            origin: ((width - size.0) / 2, (height - size.1) / 2),
        }
    }

    fn raster_origin(self, canvas: Canvas) -> (f64, f64) {
        (
            (f64::from(self.size.0) - WIDTH as f64 * self.scale) / 2.0
                + f64::from(canvas.offset_x) * self.scale,
            -f64::from(canvas.crop_top) * self.scale,
        )
    }
}

struct Artwork {
    renderer: ReferenceFrame,
    apertures: Apertures,
    appearance: WindowAppearance,
    light: Vec<u8>,
    alpha: Vec<u8>,
    argb: Vec<u32>,
    matte: InteriorMatte,
    foreground: [u32; 256],
    timeline: Timeline,
    pose: u32,
    #[cfg(goodhand_native_motion)]
    wrist: Option<wrist_tone::WristTone>,
}

impl Artwork {
    fn new(repeat: Repeat, appearance: WindowAppearance, cycle: Cycle) -> io::Result<Self> {
        let renderer = ReferenceFrame::new(cycle)?;
        let apertures = Apertures::new(renderer.occupied_bounds()?)?;
        let art = Self {
            renderer,
            apertures,
            appearance,
            light: vec![0; WIDTH * HEIGHT],
            alpha: vec![0; WIDTH * HEIGHT],
            argb: vec![0; WIDTH * HEIGHT],
            matte: InteriorMatte::new(),
            foreground: if cfg!(goodhand_baseline_artwork) {
                std::array::from_fn(|gray| argb([gray as u8; 3], true))
            } else {
                black_grade::palette().map(|rgb| argb(rgb, true))
            },
            timeline: Timeline { cycle, repeat },
            pose: 0,
            #[cfg(goodhand_native_motion)]
            wrist: None,
        };
        #[cfg(goodhand_native_motion)]
        let art = {
            let mut art = art;
            if art.renderer.uses_reviewed_motion() {
                // Base the gain on the opaque look even if Linux starts in
                // a translucent mode; switching back later stays consistent.
                art.appearance = WindowAppearance::Safe;
                art.wrist = Some(wrist_tone::WristTone::prepare(&mut art)?);
                art.appearance = appearance;
                art.update(0)?;
            }
            art
        };
        Ok(art)
    }

    fn canvas(&self) -> Canvas {
        self.apertures.for_appearance(self.appearance)
    }

    fn follow(&mut self, elapsed: Duration) -> io::Result<Frame> {
        let Some(tick) = self.renderer.tick_for(self.timeline, elapsed) else {
            return Ok(Frame::Finished);
        };
        let pose = self
            .renderer
            .pose_at_tick(self.timeline.cycle, tick)
            .ok_or_else(|| io::Error::other("invalid native playback phase"))?;
        if pose == self.pose {
            return Ok(Frame::Unchanged);
        }
        self.update(pose)?;
        Ok(Frame::Changed)
    }

    fn update(&mut self, pose: u32) -> io::Result<()> {
        self.renderer
            .render(pose, &mut self.light, Some(&mut self.alpha))?;
        self.pose = pose;
        self.paint();
        Ok(())
    }

    fn paint(&mut self) {
        paint_dots(&self.light, &self.alpha, &self.foreground, &mut self.argb);
        if cfg!(goodhand_baseline_artwork) {
            // Archived support and tone are the whole image. Do not add
            // support shading, a display curve, or a local wrist grade.
            return;
        }
        if self.appearance == WindowAppearance::Safe {
            // Only dots color the opaque pane. Keep the depth measurement for
            // the approved bearing tone, without adding a second backdrop.
            self.matte.measure_depth(&self.alpha);
            bearing_tone::apply(&self.matte, &self.light, &mut self.argb);
            #[cfg(goodhand_native_motion)]
            if let Some(wrist) = &self.wrist {
                wrist.apply(self.pose, &mut self.argb);
            }
        } else {
            self.matte.apply(&self.light, &self.alpha, &mut self.argb);
        }
    }
}

fn paint_dots(gray: &[u8], occupied: &[u8], palette: &[u32; 256], output: &mut [u32]) {
    let spec = RasterSpec::REFERENCE;
    output.fill(0);
    // ReferenceFrame writes one uniform gray and occupancy per source dot.
    // Visit dot origins, not the empty pixels between the fixed footprints.
    for y in 0..usize::from(crate::archive_profile::HEIGHT) {
        let start = (spec.origin[1] + y * spec.pitch[1]) * spec.width + spec.origin[0];
        for x in 0..usize::from(crate::archive_profile::WIDTH) {
            let first = start + x * spec.pitch[0];
            if occupied[first] == 0 {
                continue;
            }
            let color = palette[usize::from(gray[first])];
            for row in 0..spec.dot_size[1] {
                let left = first + row * spec.width;
                output[left..left + spec.dot_size[0]].fill(color);
            }
        }
    }
}

fn argb([r, g, b]: [u8; 3], present: bool) -> u32 {
    if present {
        u32::from_be_bytes([255, r, g, b])
    } else {
        0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Frame {
    Unchanged,
    Changed,
    Finished,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn copy_black_client(argb: &[u32], canvas: Canvas, output: &mut [u32]) {
    let backdrop = backdrop_argb();
    // Integer coordinates at 1×, identical to the Cairo aperture. Native
    // backends receive only the opaque client and scale it if resized.
    output.fill(backdrop);
    let offset_x = (canvas.width as i32 - WIDTH as i32) / 2 + canvas.offset_x;
    let offset_y = -canvas.crop_top;
    let src_x = (-offset_x).max(0) as usize;
    let src_y = (-offset_y).max(0) as usize;
    let dst_x = offset_x.max(0) as usize;
    let dst_y = offset_y.max(0) as usize;
    let width = WIDTH
        .saturating_sub(src_x)
        .min(canvas.width.saturating_sub(dst_x));
    let height = HEIGHT
        .saturating_sub(src_y)
        .min(canvas.height.saturating_sub(dst_y));
    for y in 0..height {
        let src = (src_y + y) * WIDTH + src_x;
        let dst = (dst_y + y) * canvas.width + dst_x;
        for (to, &pixel) in output[dst..dst + width]
            .iter_mut()
            .zip(&argb[src..src + width])
        {
            *to = over_backdrop(pixel, backdrop);
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn backdrop_argb() -> u32 {
    argb(BACKDROP_RGB, true)
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn over_backdrop(pixel: u32, backdrop: u32) -> u32 {
    let src = pixel.to_be_bytes();
    if src[0] == 255 {
        return pixel;
    }
    let op = 255 - u16::from(src[0]);
    let bg = backdrop.to_be_bytes();
    let channel = |i: usize| src[i] + ((u16::from(bg[i]) * op + 127) / 255) as u8;
    u32::from_be_bytes([255, channel(1), channel(2), channel(3)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tinted_backdrop_does_not_modify_opaque_artwork() {
        let bg = backdrop_argb();
        assert_eq!(bg, 0xff0c_1012);
        assert_eq!(over_backdrop(0, bg), bg);
        assert_eq!(over_backdrop(0xff02_1118, bg), 0xff02_1118);
        assert_eq!(over_backdrop(0xc000_0000, bg), 0xff03_0404);
        assert_eq!(over_backdrop(0x8000_0000, bg), 0xff06_0809);
    }

    #[test]
    fn client_pixels_match_each_approved_placement_on_black() {
        let src: Vec<u32> = (0..(WIDTH * HEIGHT) as u32)
            .map(|value| 0xff00_0000 | value)
            .collect();
        let apertures = Apertures::new(([47, 137], [954, 549])).unwrap();
        for canvas in [apertures.black, apertures.standard] {
            let mut out = vec![0; canvas.width * canvas.height];
            copy_black_client(&src, canvas, &mut out);
            let (x, y) = Placement::new(canvas, canvas.width as i32, canvas.height as i32)
                .raster_origin(canvas);
            for (j, row) in out.chunks_exact(canvas.width).enumerate() {
                for (i, &pixel) in row.iter().enumerate() {
                    let ox = i as i32 - x as i32;
                    let oy = j as i32 - y as i32;
                    let expected =
                        if (0..WIDTH as i32).contains(&ox) && (0..HEIGHT as i32).contains(&oy) {
                            src[oy as usize * WIDTH + ox as usize]
                        } else {
                            backdrop_argb()
                        };
                    assert_eq!(pixel, expected);
                }
            }
        }
    }

    #[test]
    fn forty_five_second_turn_shows_exactly_540_uniform_phases() {
        use crate::playback::CycleDuration;
        use crate::reference_frame::{motion_pose_at_tick, motion_tick};
        let cycle = Cycle {
            duration: CycleDuration::WINDOW_DEFAULT,
            ..Cycle::default()
        };
        let timeline = Timeline {
            cycle,
            repeat: Repeat::Loop,
        };
        for pose in 0..540 {
            let step = Duration::from_nanos((pose * 1_000_000_000_u64).div_ceil(12));
            assert_eq!(motion_tick(timeline, step), Some(pose));
            assert_eq!(motion_pose_at_tick(cycle, pose), Some(pose as u32));
            assert_eq!(
                motion_tick(timeline, Duration::from_secs(45) + step),
                Some(pose)
            );
            if pose > 0 {
                assert_eq!(
                    motion_tick(timeline, step - Duration::from_nanos(1)),
                    Some(pose - 1)
                );
            }
        }
        assert_eq!(motion_tick(timeline, Duration::from_secs(45)), Some(0));
        assert_eq!(
            motion_tick(
                Timeline {
                    repeat: Repeat::Once,
                    ..timeline
                },
                Duration::from_secs(45)
            ),
            None
        );
    }

    #[test]
    fn native_motion_takes_every_scheduled_index_in_both_directions() {
        use crate::archive_profile::{NATIVE_MOTION_FPS, NATIVE_MOTION_POSES};
        use crate::playback::Direction;
        use crate::reference_frame::{motion_pose_at_tick, motion_tick};
        let reverse = Cycle::default();
        let forward = Cycle {
            direction: Direction::Forward,
            ..reverse
        };
        let ticks = u64::from(NATIVE_MOTION_POSES);
        let fps = u64::from(NATIVE_MOTION_FPS);
        assert_eq!(ticks, 30 * fps);
        let looping = Timeline {
            cycle: reverse,
            repeat: Repeat::Loop,
        };
        let once = Timeline {
            cycle: reverse,
            repeat: Repeat::Once,
        };
        let turn = Duration::from_secs(30);
        for tick in 0..ticks {
            let instant = Duration::from_nanos((tick * 1_000_000_000).div_ceil(fps));
            assert_eq!(motion_tick(once, instant), Some(tick));
            assert_eq!(motion_tick(looping, turn + instant), Some(tick));
            if tick > 0 {
                assert_eq!(
                    motion_tick(once, instant - Duration::from_nanos(1)),
                    Some(tick - 1)
                );
            }
            assert_eq!(motion_pose_at_tick(reverse, tick), Some(tick as u32));
            assert_eq!(
                motion_pose_at_tick(forward, tick),
                Some(((ticks - tick) % ticks) as u32)
            );
        }
        assert_eq!(motion_tick(once, turn), None);
        assert_eq!(motion_tick(looping, turn), Some(0));
        assert_eq!(motion_pose_at_tick(reverse, ticks), None);
        assert_eq!(motion_pose_at_tick(forward, ticks), None);
        let twelve = Cycle {
            duration: "12".parse().unwrap(),
            ..reverse
        };
        assert_eq!(motion_pose_at_tick(twelve, 0), Some(0));
        assert_eq!(motion_pose_at_tick(twelve, 1), Some(2));
        assert_eq!(motion_pose_at_tick(twelve, 216), None);
    }

    #[cfg(any(goodhand_native_motion, goodhand_baseline_artwork))]
    fn black_point_signature(argb: &[u32], canvas: Canvas) -> u64 {
        let spec = RasterSpec::REFERENCE;
        let view = Placement::new(canvas, 1016, 445);
        let (xoff, yoff) = view.raster_origin(canvas);
        let mut signature = 0xcbf2_9ce4_8422_2325u64;
        for v in 0..usize::from(crate::archive_profile::HEIGHT) {
            for u in 0..usize::from(crate::archive_profile::WIDTH) {
                let x = spec.origin[0] + u * spec.pitch[0];
                let y = spec.origin[1] + v * spec.pitch[1];
                if x as f64 + xoff < 0.0
                    || y as f64 + yoff < 0.0
                    || x as f64 + xoff >= 1016.0
                    || y as f64 + yoff >= 445.0
                {
                    continue;
                }
                for &byte in &argb[y * WIDTH + x].to_be_bytes()[1..] {
                    signature = (signature ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
                }
            }
        }
        signature
    }

    #[cfg(goodhand_native_motion)]
    #[test]
    fn motion_preserves_sampled_anchors_and_visible_poses_are_unique() {
        use crate::archive_profile::NATIVE_MOTION_POSES;
        use std::collections::HashSet;
        let mut motion =
            Artwork::new(Repeat::Once, WindowAppearance::Safe, Cycle::default()).unwrap();
        #[cfg(goodhand_embedded_playback)]
        let custom = Cycle {
            duration: "12".parse().unwrap(),
            ..Cycle::default()
        };
        #[cfg(goodhand_embedded_playback)]
        let mut reviewed = Artwork::new(Repeat::Once, WindowAppearance::Safe, custom).unwrap();
        assert_eq!(
            motion.renderer.occupied_bounds().unwrap(),
            ([47, 137], [954, 549])
        );
        assert_eq!(motion.canvas().client_at(1.0), (1016, 445));
        #[cfg(goodhand_embedded_playback)]
        assert_eq!(reviewed.renderer.pose_at_tick(custom, 1), Some(1));
        let mut signatures = HashSet::new();
        for tick in 0..NATIVE_MOTION_POSES {
            motion.update(tick).unwrap();
            let hash = black_point_signature(&motion.argb, motion.canvas());
            assert!(
                signatures.insert(hash),
                "same visible artwork at tick {tick}"
            );
            // Stock W5 anchors land precisely on every fifteenth new sample.
            #[cfg(goodhand_embedded_playback)]
            if tick % 15 == 0 {
                reviewed.update(tick * 8 / 15).unwrap();
                assert_eq!(motion.light, reviewed.light, "raw tone at {tick}");
                assert_eq!(motion.alpha, reviewed.alpha, "independent alpha at {tick}");
                assert_eq!(motion.argb, reviewed.argb, "final ARGB at {tick}");
            }
        }
    }

    #[cfg(goodhand_baseline_artwork)]
    #[test]
    fn baseline_native_uses_exact_gray_coverage_and_all_288_uniform_phases() {
        use crate::archive_profile::POSES;
        use crate::playback::{CycleDuration, Direction};
        use std::collections::HashSet;
        let cycle = Cycle {
            duration: CycleDuration::WINDOW_DEFAULT,
            ..Cycle::default()
        };
        let mut artwork = Artwork::new(Repeat::Once, WindowAppearance::Safe, cycle).unwrap();
        let timeline = artwork.timeline;
        assert_eq!(
            artwork.renderer.occupied_bounds().unwrap(),
            ([47, 137], [954, 549])
        );
        assert_eq!(artwork.canvas().compact(), (663, 290));
        let mut known = HashSet::new();
        for pose in 0..POSES {
            let time = Duration::from_nanos(u64::from(pose) * 156_250_000);
            assert_eq!(
                artwork.renderer.tick_for(timeline, time),
                Some(u64::from(pose))
            );
            if pose > 0 {
                assert_eq!(
                    artwork
                        .renderer
                        .tick_for(timeline, time - Duration::from_nanos(1)),
                    Some(u64::from(pose - 1))
                );
            }
            assert_eq!(
                artwork.renderer.pose_at_tick(cycle, u64::from(pose)),
                Some(pose)
            );
            let forward = Cycle {
                direction: Direction::Forward,
                ..cycle
            };
            assert_eq!(
                artwork.renderer.pose_at_tick(forward, u64::from(pose)),
                Some((POSES - pose) % POSES)
            );
            artwork.update(pose).unwrap();
            assert!(known.insert(black_point_signature(&artwork.argb, artwork.canvas())));
            assert!(
                artwork
                    .argb
                    .iter()
                    .zip(artwork.light.iter().zip(&artwork.alpha))
                    .all(|(&actual, (&gray, &alpha))| {
                        actual == if alpha == 0 { 0 } else { argb([gray; 3], true) }
                    })
            );
        }
        assert_eq!(
            artwork.renderer.tick_for(timeline, Duration::from_secs(45)),
            None
        );
        let looping = Timeline {
            repeat: Repeat::Loop,
            ..timeline
        };
        assert_eq!(
            artwork.renderer.tick_for(looping, Duration::from_secs(45)),
            Some(0)
        );
        for seconds in [12, 22, 30, 60] {
            let cycle = Cycle {
                duration: seconds.to_string().parse().unwrap(),
                ..cycle
            };
            let timeline = Timeline {
                cycle,
                repeat: Repeat::Once,
            };
            let last = cycle.length() - Duration::from_nanos(1);
            assert_eq!(
                artwork.renderer.tick_for(timeline, last),
                Some(u64::from(POSES - 1))
            );
            assert_eq!(artwork.renderer.tick_for(timeline, cycle.length()), None);
        }
    }

    #[test]
    fn default_black_aperture_adds_headroom_without_moving_the_bottom() {
        let a = Apertures::new(([47, 137], [954, 549])).unwrap();
        let black = a.for_appearance(WindowAppearance::default());
        assert_eq!(
            a.for_appearance(WindowAppearance::Clear).client_at(1.0),
            (1120, 560)
        );
        assert_eq!(
            (black.width, black.height, black.crop_top),
            (1016, 445, 112)
        );
        assert_eq!(black.client_at(1.0), (1016, 445));
        let p = Placement::new(black, 1016, 445);
        assert_eq!((p.size, p.scale, p.origin), ((1016, 445), 1.0, (0, 0)));
        assert_eq!(p.raster_origin(black), (-24., -112.));
        assert_eq!(137 - black.crop_top, 17 + BLACK_EXTRA_TOP);
        assert_eq!(black.crop_top + black.height as i32 - 549, BLACK_BOTTOM);
        let narrower = Placement::new(black, 1015, 445);
        let shorter = Placement::new(black, 1016, 444);
        assert!(narrower.scale < 1.0 && shorter.scale < 1.0);
        let (min_width, min_height) = black.minimum();
        assert_eq!((min_width, min_height), (610, 267));
        let (w, h) = black.compact();
        assert_eq!((w, h), (663, 290));
        assert_eq!(Placement::new(black, w, h).scale, 290.0 / 445.0);
        assert!(Placement::new(black, min_width, min_height).scale >= MIN_SCALE);
    }

    #[test]
    fn transparent_geometry_is_unchanged_and_oversize_black_art_stays_wide() {
        let a = Apertures::new(([47, 90], [954, 590])).unwrap();
        let clear = a.for_appearance(WindowAppearance::Clear);
        assert_eq!(clear.client_at(1.0), (1120, 560));
        assert_eq!(clear.minimum(), (672, 336));
        assert!(a.for_appearance(WindowAppearance::Framed) == clear);
        assert!(a.for_appearance(WindowAppearance::Safe) == clear);
    }

    #[test]
    fn resize_fits_reference_pane_in_any_viewport_without_stretching() {
        let canvas = Canvas {
            width: PANEL_WIDTH,
            height: PANEL_HEIGHT,
            crop_top: 40,
            offset_x: 0,
        };
        for (width, height, panelsize, origin, scale) in [
            (1120, 560, (1120, 560), (0, 0), 1.0),
            (840, 420, (840, 420), (0, 0), 0.75),
            (900, 500, (900, 450), (0, 25), 45. / 56.),
            (1100, 430, (860, 430), (120, 0), 43. / 56.),
            (1600, 800, (1600, 800), (0, 0), 10. / 7.),
        ] {
            let p = Placement::new(canvas, width, height);
            assert_eq!((p.size, p.origin, p.scale), (panelsize, origin, scale));
            let (x, y) = p.raster_origin(canvas);
            assert!((x + WIDTH as f64 * scale / 2.0 - f64::from(p.size.0) / 2.0).abs() < 1e-12);
            assert_eq!(y + f64::from(canvas.crop_top) * scale, 0.);
        }
    }

    #[test]
    fn nearest_downscale_keeps_a_sample_in_each_source_mark() {
        let spec = RasterSpec::REFERENCE;
        let a = Apertures::new(([47, 137], [954, 549])).unwrap();
        for canvas in [a.standard, a.black] {
            for height in canvas.minimum().1 as usize..=canvas.height {
                let width = (height * canvas.width).div_ceil(canvas.height);
                let p = Placement::new(canvas, width as i32, height as i32);
                let ox = p.raster_origin(canvas);
                for (axis, offset, count) in [(0, ox.0, 200), (1, ox.1, 120)] {
                    for point in 0..count {
                        let source = spec.origin[axis] + point * spec.pitch[axis];
                        let start = offset + source as f64 * p.scale;
                        let end = start + spec.dot_size[axis] as f64 * p.scale;
                        // Every footprint has an output pixel center before
                        // clipping. Canvas has already verified the artwork fits.
                        let first_center = (start - 0.5).ceil() + 0.5;
                        assert!(first_center < end, "{axis}, {point}, {height}");
                    }
                }
            }
        }
    }

    #[test]
    fn black_support_changes_only_the_interior_of_existing_dots() {
        let mut state =
            Artwork::new(Repeat::Loop, WindowAppearance::Clear, Cycle::default()).unwrap();
        state.update(0).unwrap();
        let clear = state.argb.clone();
        state.appearance = WindowAppearance::Safe;
        state.paint();
        let spec = RasterSpec::REFERENCE;
        let mut changes = 0;
        for (i, (&black, &before)) in state.argb.iter().zip(&clear).enumerate() {
            if black == before {
                continue;
            }
            changes += 1;
            let [aa, ar, ag, ab] = before.to_be_bytes();
            if aa < 255 {
                // Opaque mode does not shade the plain backdrop around dots.
                assert_eq!(black, 0);
                continue;
            }
            let [ba, br, bg, bb] = black.to_be_bytes();
            assert_eq!((aa, ba), (255, 255));
            let gain = br - ar;
            assert!((1..=9).contains(&gain));
            assert_eq!((bg - ag, bb - ab), (gain, gain));
            let (x, y) = (i % WIDTH, i / WIDTH);
            let col = (x - spec.origin[0]) / spec.pitch[0];
            let row = (y - spec.origin[1]) / spec.pitch[1];
            assert!(state.matte.dot_depth(col, row) > 2);
        }
        if cfg!(goodhand_w5_profile) {
            assert!(changes > 0);
        }
        state.appearance = WindowAppearance::Clear;
        state.paint();
        assert_eq!(state.argb, clear);
    }

    #[test]
    fn dot_palette_matches_dense_paint_and_clears_the_previous_frame() {
        let spec = RasterSpec::REFERENCE;
        let mut gray = vec![0; WIDTH * HEIGHT];
        let mut occupied = vec![0; WIDTH * HEIGHT];
        let palette = black_grade::palette().map(|rgb| argb(rgb, true));
        for (x, y, value) in [(0, 0, 0), (1, 2, 51), (198, 117, 13), (199, 119, 240)] {
            let col = spec.origin[0] + x * spec.pitch[0];
            let row = spec.origin[1] + y * spec.pitch[1];
            for dy in 0..spec.dot_size[1] {
                let start = (row + dy) * WIDTH + col;
                gray[start..start + spec.dot_size[0]].fill(value);
                occupied[start..start + spec.dot_size[0]].fill(255);
            }
        }
        let mut output = vec![u32::MAX; WIDTH * HEIGHT];
        paint_dots(&gray, &occupied, &palette, &mut output);
        for (i, (&actual, (&gray, &alpha))) in
            output.iter().zip(gray.iter().zip(&occupied)).enumerate()
        {
            let expected = if alpha != 0 {
                palette[usize::from(gray)]
            } else {
                0
            };
            assert_eq!(actual, expected, "pixel {i}");
        }
        occupied.fill(0);
        paint_dots(&gray, &occupied, &palette, &mut output);
        assert!(output.iter().all(|&pixel| pixel == 0));
    }

    #[test]
    fn black_stays_opaque_and_the_matte_never_changes_black_compositing() {
        assert_eq!(argb([0; 3], true), 0xff00_0000);
        assert_eq!(argb([49, 50, 51], true), 0xff31_3233);
        assert_eq!(argb([0; 3], false), 0);

        // The raster path is independent of the GTK handle.
        let mut state =
            Artwork::new(Repeat::Loop, WindowAppearance::Clear, Cycle::default()).unwrap();
        state.update(0).unwrap();
        assert_eq!((state.canvas().width, state.canvas().height), (1120, 560));
        let local = Placement::new(state.canvas(), 1120, 560);
        assert_eq!((local.scale, local.origin), (1.0, (0, 0)));
        assert!(state.alpha.contains(&0));
        assert!(state.alpha.contains(&255));
        let palette = state.foreground;
        for ((&pixel, &gray), &alpha) in state.argb.iter().zip(&state.light).zip(&state.alpha) {
            assert!(alpha == 0 || alpha == 255);
            if alpha != 0 {
                assert_eq!(pixel, palette[usize::from(gray)]);
            } else {
                // Compositing over black is identical to the original image.
                assert_eq!(pixel & 0xff_ff_ff, 0);
                assert!(pixel >> 24 <= u32::from(interior_matte::PEAK));
            }
        }
    }
}
