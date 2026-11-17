use goodhand::{write_surface, RasterSpec, Sequence, SurfaceError};

fn asset(width: u16, height: u16, frames: &[&[u8]]) -> Vec<u8> {
    let mut bytes = b"GHSH".to_vec();
    for value in [1, width, height, 24] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&(frames.len() as u32).to_le_bytes());
    let mut offset = 0u32;
    bytes.extend_from_slice(&offset.to_le_bytes());
    for frame in frames {
        offset += frame.len() as u32;
        bytes.extend_from_slice(&offset.to_le_bytes());
    }
    for frame in frames {
        bytes.extend_from_slice(frame);
    }
    bytes
}

fn small_spec() -> RasterSpec {
    RasterSpec {
        width: 7,
        height: 8,
        origin: [1, 2],
        pitch: [3, 4],
        dot_size: [2, 1],
    }
}

#[test]
fn reference_profile_matches_first_and_last_grid_footprints() {
    let mut rows = vec![1, 0, 1];
    rows.extend_from_slice(&[0; 118]);
    rows.extend_from_slice(&[1, 199, 1]);
    let bytes = asset(200, 120, &[&rows]);
    let frame = Sequence::from_bytes(&bytes).unwrap().frame(0).unwrap();
    let spec = RasterSpec::REFERENCE;
    assert_eq!(spec.required_len(), Ok(640_000));
    let mut output = vec![7; 640_003];
    write_surface(frame, &[17, 203], spec, &mut output).unwrap();
    let mut expected = vec![0; 640_003];
    for y in 42..44 {
        expected[y * 1000 + 2..y * 1000 + 4].fill(17);
    }
    for y in 637..639 {
        expected[y * 1000 + 997..y * 1000 + 999].fill(203);
    }
    expected[640_000..].fill(7);
    assert_eq!(output, expected);
}

#[test]
fn occupied_values_follow_rows_runs_and_points_without_fit() {
    let bytes = asset(2, 2, &[&[1, 1, 1, 1, 0, 2]]);
    let frame = Sequence::from_bytes(&bytes).unwrap().frame(0).unwrap();
    let tones = [20, 30, 40];
    let before = bytes.clone();
    let mut output = [9; 56];
    write_surface(frame, &tones, small_spec(), &mut output).unwrap();
    let mut expected = [0; 56];
    expected[2 * 7 + 4..2 * 7 + 6].fill(20);
    expected[6 * 7 + 1..6 * 7 + 3].fill(30);
    expected[6 * 7 + 4..6 * 7 + 6].fill(40);
    assert_eq!(output, expected);
    assert_eq!(tones, [20, 30, 40]);
    assert_eq!(bytes, before);
}

#[test]
fn separated_runs_and_later_rows_preserve_gray_order() {
    let bytes = asset(5, 2, &[&[2, 0, 1, 3, 2, 1, 1, 1]]);
    let frame = Sequence::from_bytes(&bytes).unwrap().frame(0).unwrap();
    let spec = RasterSpec {
        width: 10,
        height: 4,
        origin: [0, 0],
        pitch: [2, 2],
        dot_size: [1, 1],
    };
    let mut output = [9; 40];
    write_surface(frame, &[10, 20, 30, 40], spec, &mut output).unwrap();
    let mut expected = [0; 40];
    expected[0] = 10;
    expected[6] = 20;
    expected[8] = 30;
    expected[22] = 40;
    assert_eq!(output, expected);
}

#[test]
fn next_frame_clears_old_dots_and_nonoccupied_background() {
    let bytes = asset(2, 2, &[&[1, 1, 1, 0], &[0, 0]]);
    let shape = Sequence::from_bytes(&bytes).unwrap();
    let mut output = [9; 58];
    write_surface(shape.frame(0).unwrap(), &[125], small_spec(), &mut output).unwrap();
    assert_eq!(output[2 * 7 + 4], 125);
    write_surface(shape.frame(1).unwrap(), &[], small_spec(), &mut output).unwrap();
    assert_eq!(&output[..56], &[0; 56]);
    assert_eq!(&output[56..], &[9; 2]);
}

#[test]
fn black_is_consumed_as_an_occupied_value_not_background_metadata() {
    let bytes = asset(2, 2, &[&[1, 1, 1, 0]]);
    let frame = Sequence::from_bytes(&bytes).unwrap().frame(0).unwrap();
    let mut output = [9; 56];
    write_surface(frame, &[0], small_spec(), &mut output).unwrap();
    assert_eq!(output, [0; 56]);
    let mut occupancy = [0; 4];
    frame.write_mask(&mut occupancy).unwrap();
    assert_eq!(occupancy, [0, 1, 0, 0]);
}

#[test]
fn short_destination_is_unchanged() {
    let bytes = asset(2, 2, &[&[1, 1, 1, 0]]);
    let frame = Sequence::from_bytes(&bytes).unwrap().frame(0).unwrap();
    let mut output = [9; 55];
    assert_eq!(
        write_surface(frame, &[125], small_spec(), &mut output),
        Err(SurfaceError::BufferTooShort {
            required: 56,
            actual: 55
        })
    );
    assert_eq!(output, [9; 55]);
}

#[test]
fn short_or_extra_gray_is_rejected_before_clearing() {
    let bytes = asset(2, 2, &[&[1, 1, 1, 0]]);
    let frame = Sequence::from_bytes(&bytes).unwrap().frame(0).unwrap();
    for gray in [&[][..], &[1, 2][..]] {
        let mut output = [9; 56];
        assert_eq!(
            write_surface(frame, gray, small_spec(), &mut output),
            Err(SurfaceError::GrayCountMismatch {
                expected: 1,
                actual: gray.len()
            })
        );
        assert_eq!(output, [9; 56]);
    }
}

#[test]
fn rejects_zero_dimensions_pitch_or_footprint_and_overlapping_dots() {
    let bytes = asset(2, 2, &[&[0, 0]]);
    let frame = Sequence::from_bytes(&bytes).unwrap().frame(0).unwrap();
    let base = small_spec();
    for spec in [
        RasterSpec { width: 0, ..base },
        RasterSpec { height: 0, ..base },
        RasterSpec {
            pitch: [0, 4],
            ..base
        },
        RasterSpec {
            pitch: [3, 0],
            ..base
        },
        RasterSpec {
            dot_size: [0, 1],
            ..base
        },
        RasterSpec {
            dot_size: [2, 0],
            ..base
        },
        RasterSpec {
            dot_size: [4, 1],
            ..base
        },
        RasterSpec {
            dot_size: [2, 5],
            ..base
        },
    ] {
        let mut output = [9; 56];
        assert_eq!(
            write_surface(frame, &[], spec, &mut output),
            Err(SurfaceError::InvalidRaster)
        );
        assert_eq!(output, [9; 56]);
    }
}

#[test]
fn checked_canvas_and_grid_arithmetic_rejects_overflow_unchanged() {
    let bytes = asset(2, 2, &[&[0, 0]]);
    let frame = Sequence::from_bytes(&bytes).unwrap().frame(0).unwrap();
    let base = small_spec();
    for spec in [
        RasterSpec {
            width: usize::MAX,
            height: 2,
            ..base
        },
        RasterSpec {
            origin: [usize::MAX, 2],
            ..base
        },
        RasterSpec {
            origin: [1, usize::MAX],
            ..base
        },
        RasterSpec {
            pitch: [usize::MAX, 4],
            ..base
        },
        RasterSpec {
            pitch: [3, usize::MAX],
            ..base
        },
    ] {
        let mut output = [9; 56];
        assert_eq!(
            write_surface(frame, &[], spec, &mut output),
            Err(SurfaceError::SizeOverflow)
        );
        assert_eq!(output, [9; 56]);
    }
}

#[test]
fn three_point_grid_rejects_pitch_multiplication_overflow() {
    let bytes = asset(3, 3, &[&[0, 0, 0]]);
    let frame = Sequence::from_bytes(&bytes).unwrap().frame(0).unwrap();
    let base = RasterSpec {
        width: 10,
        height: 12,
        ..small_spec()
    };
    for pitch in [[usize::MAX, 4], [3, usize::MAX]] {
        let mut output = [9; 120];
        assert_eq!(
            write_surface(frame, &[], RasterSpec { pitch, ..base }, &mut output),
            Err(SurfaceError::SizeOverflow)
        );
        assert_eq!(output, [9; 120]);
    }
}

#[test]
fn whole_grid_must_fit_even_when_all_points_are_absent() {
    let bytes = asset(2, 2, &[&[0, 0]]);
    let frame = Sequence::from_bytes(&bytes).unwrap().frame(0).unwrap();
    let base = small_spec();
    for spec in [
        RasterSpec { width: 5, ..base },
        RasterSpec { height: 6, ..base },
        RasterSpec {
            origin: [7, 2],
            ..base
        },
        RasterSpec {
            origin: [1, 8],
            ..base
        },
    ] {
        let mut output = [9; 56];
        assert_eq!(
            write_surface(frame, &[], spec, &mut output),
            Err(SurfaceError::GridOutsideCanvas)
        );
        assert_eq!(output, [9; 56]);
    }
}
