use goodhand::{render_arm, ArmAttrs, ArmRenderError, ArmSampler, ArmView};

fn draw(view: ArmView) -> (Vec<u8>, Vec<u8>) {
    let count = view.required_len().unwrap();
    let mut mask = vec![0; count];
    let mut tone = vec![0; count];
    render_arm(view, &mut mask, &mut tone).unwrap();
    (mask, tone)
}

fn occupied(mask: &[u8]) -> usize {
    mask.iter().filter(|&&sample| sample == 1).count()
}

#[test]
fn prepared_points_match_dense_render_coordinates_and_coverage() {
    let yaw = -52.6;
    let sampler = ArmSampler::new(yaw).unwrap();
    assert!(matches!(
        ArmSampler::new(f64::NAN),
        Err(ArmRenderError::InvalidYaw)
    ));
    assert_eq!(sampler.sample(f64::INFINITY, 0.0), None);
    assert_eq!(sampler.sample(0.0, f64::NAN), None);
    assert_eq!(sampler.sample(3.0, 0.0), None);
    assert_eq!(sampler.sample(f64::MAX, f64::MAX), None);
    assert!(sampler.sample(0.0, 0.0).is_some());

    let (width, height) = (61, 37);
    let (mask, gray) = draw(ArmView::new(yaw, width, height));
    let unit =
        (ArmSampler::FRAME_WIDTH / width as f64).max(ArmSampler::FRAME_HEIGHT / height as f64);
    let center_x = (width / 2) as f64;
    let center_y = (height / 2) as f64;
    for row in 0..height {
        let y = (center_y - row as f64) * unit;
        for column in 0..width {
            let x = (column as f64 - center_x) * unit;
            let index = row * width + column;
            assert_eq!(
                sampler.sample(x, y),
                (mask[index] == 1).then_some(gray[index])
            );
        }
    }
}

#[test]
fn optional_contact_shadow_changes_only_local_reflected_light() {
    let mut darkened = 0;
    let mut floors = 0;
    let mut occupied = 0;
    for yaw in [0.0, -44.25, -98.75, -174.5, -252.0] {
        let base = ArmSampler::new(yaw).unwrap();
        let default = ArmSampler::with_attrs(yaw, ArmAttrs::default()).unwrap();
        let contact = ArmSampler::with_attrs(
            yaw,
            ArmAttrs {
                contact_shadow: true,
            },
        )
        .unwrap();
        for row in -32..=32 {
            let y = f64::from(row) * 0.016;
            for col in -62..=62 {
                let x = f64::from(col) * 0.016;
                let unshaded = base.sample(x, y);
                let shaded = contact.sample(x, y);
                assert_eq!(unshaded, default.sample(x, y));
                assert_eq!(unshaded.is_some(), shaded.is_some());
                if let (Some(unshaded), Some(shaded)) = (unshaded, shaded) {
                    occupied += 1;
                    assert!((34..=unshaded).contains(&shaded));
                    if unshaded == 34 {
                        floors += 1;
                        assert_eq!(shaded, unshaded);
                    }
                    if yaw == 0.0 && x < -0.25 {
                        assert_eq!(shaded, unshaded, "forearm is outside the fold contact");
                    }
                    darkened += usize::from(shaded < unshaded);
                }
            }
        }
    }
    assert!(floors > 0);
    assert!((8..occupied / 4).contains(&darkened));
}

#[test]
fn all_yaws_have_occupancy_and_separate_base_gray() {
    for index in 0..24 {
        let yaw = -15.0 * f64::from(index) + 0.375;
        let (mask, gray) = draw(ArmView::new(yaw, 97, 61));
        assert!(
            (100..mask.len() / 2).contains(&occupied(&mask)),
            "invalid coverage at {yaw}"
        );
        for (cell, gray) in mask.into_iter().zip(gray) {
            match cell {
                0 => assert_eq!(gray, 0),
                1 => assert!((34..=158).contains(&gray)),
                _ => panic!("invalid occupancy: {cell}"),
            }
        }
    }
}

#[test]
fn wraps_with_no_special_case_at_the_loop_boundary() {
    let front = draw(ArmView::new(0.0, 121, 77));
    assert_eq!(front, draw(ArmView::new(-360.0, 121, 77)));
    assert_eq!(front, draw(ArmView::new(720.0, 121, 77)));

    let near = draw(ArmView::new(-0.2, 121, 77));
    assert_eq!(near, draw(ArmView::new(359.8, 121, 77)));
    let changed = front.0.iter().zip(&near.0).filter(|(a, b)| a != b).count();
    assert!(changed < occupied(&front.0) / 20);
    // Neighboring fractional yaws must not collapse to one keyed frame.
    assert_ne!(front.1, near.1);
    assert_ne!(front.0, draw(ArmView::new(180.0, 121, 77)).0);

    let huge = f64::MAX;
    assert_eq!(
        draw(ArmView::new(huge, 43, 33)),
        draw(ArmView::new(huge.rem_euclid(360.0), 43, 33))
    );
}

#[test]
fn grazing_side_plane_stays_connected_in_the_opening_arm_edge() {
    let width = 200;
    let (mask, _) = draw(ArmView::new(81.25, width, 120));
    // These six occupied grazing samples must not be removed or left as
    // an unconnected streak outside the rest of the projected forearm.
    for y in 62..=67 {
        assert_eq!(mask[y * width + 81], 1);
    }
    assert!((62..=67).any(|y| {
        let row = y * width;
        mask[row + 82..=row + 85] == [1, 1, 1, 1]
    }));
}

#[test]
fn a_larger_odd_grid_samples_new_points_instead_of_stretching_cells() {
    let (width, height) = (67, 41);
    let yaw = -37.53;
    let (coarse_mask, coarse_gray) = draw(ArmView::new(yaw, width, height));
    let (fine_mask, fine_gray) = draw(ArmView::new(yaw, 3 * width, 3 * height));

    let mut new_boundary = false;
    for y in 0..height {
        for x in 0..width {
            let coarse = y * width + x;
            let fine_start = 3 * y * (3 * width) + 3 * x;
            let fine_center = fine_start + 3 * width + 1;
            // Odd scaling keeps the same world location at every third center.
            assert_eq!(coarse_mask[coarse], fine_mask[fine_center]);
            assert_eq!(coarse_gray[coarse], fine_gray[fine_center]);
            for sub_y in 0..3 {
                for sub_x in 0..3 {
                    let fine = fine_start + sub_y * (3 * width) + sub_x;
                    new_boundary |= fine_mask[fine] != fine_mask[fine_center];
                }
            }
        }
    }
    assert!(
        new_boundary,
        "fine occupancy looks like enlarged coarse cells"
    );
}

#[test]
fn non_square_samples_preserve_physical_coordinates() {
    // Both views are height-limited. The wider samples land on every other
    // square-sample center, without changing vertical coverage or lighting.
    let (wide, narrow, height) = (127, 63, 71);
    let yaw = -122.8;
    let (square_mask, square_gray) = draw(ArmView::new(yaw, wide, height));
    let (oblong_mask, oblong_gray) = draw(ArmView {
        sample_aspect: 2.0,
        ..ArmView::new(yaw, narrow, height)
    });
    for y in 0..height {
        for x in 0..narrow {
            let square = y * wide + 2 * x + 1;
            let oblong = y * narrow + x;
            assert_eq!(square_mask[square], oblong_mask[oblong]);
            assert_eq!(square_gray[square], oblong_gray[oblong]);
        }
    }
}

#[test]
fn zoom_repositions_samples_about_the_same_center() {
    let (width, height) = (83, 51);
    let yaw = -282.25;
    let normal = ArmView::new(yaw, width, height);
    let (mask, gray) = draw(normal);
    let (zoom_mask, zoom_gray) = draw(ArmView {
        zoom: 2.0,
        ..normal
    });
    let (mid_x, mid_y) = (width / 2, height / 2);
    for y in 0..height {
        for x in 0..width {
            if x % 2 != mid_x % 2 || y % 2 != mid_y % 2 {
                continue;
            }
            let original = ((y + mid_y) / 2) * width + (x + mid_x) / 2;
            let zoomed = y * width + x;
            assert_eq!(mask[original], zoom_mask[zoomed]);
            assert_eq!(gray[original], zoom_gray[zoomed]);
        }
    }
    assert!(occupied(&zoom_mask) > occupied(&mask));
}

#[test]
fn accepts_small_grids_and_preserves_caller_buffer_tails() {
    let (mask, gray) = draw(ArmView::new(-9.25, 1, 1));
    assert_eq!(mask, [1]);
    assert!(gray[0] > 0);

    let first = ArmView::new(4.6, 101, 59);
    let second = ArmView::new(-137.3, 79, 49);
    let len = first.required_len().unwrap();
    let second_len = second.required_len().unwrap();
    let mut mask = vec![73; len + 3];
    let mut gray = vec![195; len + 6];
    render_arm(first, &mut mask, &mut gray).unwrap();
    let (first_mask, first_gray) = (mask.clone(), gray.clone());
    assert_eq!(&mask[len..], &[73; 3]);
    assert_eq!(&gray[len..], &[195; 6]);

    render_arm(second, &mut mask, &mut gray).unwrap();
    let (expected_mask, expected_gray) = draw(second);
    assert_eq!(&mask[..second_len], &expected_mask);
    assert_eq!(&gray[..second_len], &expected_gray);
    assert_eq!(&mask[second_len..], &first_mask[second_len..]);
    assert_eq!(&gray[second_len..], &first_gray[second_len..]);

    render_arm(first, &mut mask, &mut gray).unwrap();
    assert_eq!(mask, first_mask);
    assert_eq!(gray, first_gray);
}

#[test]
fn invalid_view_or_one_short_output_does_not_write_either_buffer() {
    let ordinary = ArmView::new(0.0, 2, 3);
    let bad = [
        (ArmView::new(0.0, 0, 6), ArmRenderError::InvalidGrid),
        (ArmView::new(0.0, 6, 0), ArmRenderError::InvalidGrid),
        (
            ArmView::new(0.0, usize::MAX, 2),
            ArmRenderError::SizeOverflow,
        ),
        (
            ArmView::new(0.0, usize::MAX, 1),
            ArmRenderError::SizeOverflow,
        ),
        (ArmView::new(f64::NAN, 2, 3), ArmRenderError::InvalidYaw),
        (
            ArmView::new(f64::INFINITY, 2, 3),
            ArmRenderError::InvalidYaw,
        ),
        (
            ArmView {
                sample_aspect: 0.0,
                ..ordinary
            },
            ArmRenderError::InvalidSampleAspect,
        ),
        (
            ArmView {
                sample_aspect: f64::NAN,
                ..ordinary
            },
            ArmRenderError::InvalidSampleAspect,
        ),
        (
            ArmView {
                zoom: -3.0,
                ..ordinary
            },
            ArmRenderError::InvalidZoom,
        ),
        (
            ArmView {
                zoom: f64::INFINITY,
                ..ordinary
            },
            ArmRenderError::InvalidZoom,
        ),
        (
            ArmView {
                sample_aspect: f64::MIN_POSITIVE / 8.0,
                ..ordinary
            },
            ArmRenderError::InvalidProjection,
        ),
    ];
    let mut mask = [17; 8];
    let mut gray = [235; 8];
    for (view, error) in bad {
        assert_eq!(view.required_len(), Err(error));
        assert_eq!(render_arm(view, &mut mask, &mut gray), Err(error));
        assert_eq!(mask, [17; 8]);
        assert_eq!(gray, [235; 8]);
    }
    assert_eq!(
        render_arm(ordinary, &mut mask[..5], &mut gray),
        Err(ArmRenderError::OccupancyBufferTooShort {
            required: 6,
            actual: 5
        })
    );
    assert_eq!(
        render_arm(ordinary, &mut mask, &mut gray[..4]),
        Err(ArmRenderError::GrayBufferTooShort {
            required: 6,
            actual: 4
        })
    );
    assert_eq!(mask, [17; 8]);
    assert_eq!(gray, [235; 8]);
}
