use goodhand::{DecodeError, FrameError, MaskBufferTooShort, Sequence};
use std::time::Duration;

fn asset(width: u16, height: u16, fps: u16, frames: &[&[u8]]) -> Vec<u8> {
    let mut bytes = b"GHSH".to_vec();
    for field in [1, width, height, fps] {
        bytes.extend_from_slice(&field.to_le_bytes());
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

fn set_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn set_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn error(bytes: &[u8]) -> DecodeError {
    Sequence::from_bytes(bytes).unwrap_err()
}

fn frame_error(encoded: &[u8]) -> FrameError {
    match error(&asset(8, 1, 24, &[encoded])) {
        DecodeError::InvalidFrame { index: 0, error } => error,
        other => panic!("expected a frame-local error, got {other:?}"),
    }
}

#[test]
fn valid_tiny_sequence_borrows_rows_and_reuses_mask_buffer() {
    let bytes = asset(8, 3, 2, &[&[2, 0, 2, 4, 2, 0, 1, 7, 1], &[0, 1, 1, 6, 0]]);
    let sequence = Sequence::from_bytes(&bytes).unwrap();
    assert_eq!((sequence.width(), sequence.height()), (8, 3));
    assert_eq!((sequence.fps(), sequence.frame_count()), (2, 2));
    assert_eq!(sequence.frames().len(), 2);
    assert!(sequence.frame(2).is_none());
    assert!(sequence.frame(u32::MAX).is_none());

    let frame = sequence.frame(0).unwrap();
    assert_eq!(
        (frame.width(), frame.height(), frame.mask_len()),
        (8, 3, 24)
    );
    let mut rows = frame.rows();
    assert_eq!(rows.len(), 3);
    let row = rows.next().unwrap();
    assert_eq!(row.y(), 0);
    let mut runs = row.runs();
    assert_eq!(runs.len(), 2);
    let first = runs.next().unwrap();
    assert_eq!((first.start(), first.length(), first.end()), (0, 2, 2));
    assert_eq!(runs.len(), 1);
    let second = runs.next().unwrap();
    assert_eq!((second.start(), second.length(), second.end()), (4, 2, 6));
    assert!(runs.next().is_none());
    assert!(runs.next().is_none());
    assert_eq!(rows.next().unwrap().runs().len(), 0);
    assert_eq!(rows.next().unwrap().y(), 2);
    assert!(rows.next().is_none());
    assert!(rows.next().is_none());

    let mut output = [9; 26];
    frame.write_mask(&mut output).unwrap();
    assert_eq!(
        output,
        [1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 9, 9]
    );
    sequence
        .frame_at(Duration::from_millis(500))
        .write_mask(&mut output)
        .unwrap();
    assert_eq!(
        output,
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9, 9]
    );
    let reversed: Vec<_> = sequence.frames().rev().collect();
    reversed[0].write_mask(&mut output).unwrap();
    assert_eq!(&output[8..16], &[0, 1, 1, 1, 1, 1, 1, 0]);
}

#[test]
fn too_short_mask_buffer_is_unchanged() {
    let bytes = asset(8, 1, 24, &[&[1, 2, 3]]);
    let frame = Sequence::from_bytes(&bytes).unwrap().frame(0).unwrap();
    let mut output = [7; 7];
    assert_eq!(
        frame.write_mask(&mut output),
        Err(MaskBufferTooShort {
            required: 8,
            actual: 7
        })
    );
    assert_eq!(output, [7; 7]);
}

#[test]
fn black_surface_does_not_mean_background() {
    let bytes = asset(2, 1, 24, &[&[1, 1, 1]]);
    let frame = Sequence::from_bytes(&bytes).unwrap().frame(0).unwrap();
    let mut occupancy = [9; 2];
    frame.write_mask(&mut occupancy).unwrap();
    // Future tone data can contain identical zero (black) values at both points.
    // The independent occupancy still distinguishes surface from background.
    let tone = [0u8, 0];
    let points: Vec<_> = occupancy
        .iter()
        .zip(tone)
        .map(|(&present, gray)| (present != 0, gray))
        .collect();
    assert_eq!(points, [(false, 0), (true, 0)]);
}

#[test]
fn twelve_second_loop_uses_exact_24fps_intervals() {
    let frames: Vec<&[u8]> = vec![&[0]; 288];
    let bytes = asset(1, 1, 24, &frames);
    let sequence = Sequence::from_bytes(&bytes).unwrap();
    assert_eq!(sequence.frame_index_at(Duration::ZERO), 0);
    assert_eq!(sequence.frame_index_at(Duration::from_nanos(41_666_666)), 0);
    assert_eq!(sequence.frame_index_at(Duration::from_nanos(41_666_667)), 1);
    assert_eq!(sequence.frame_index_at(Duration::from_secs(1)), 24);
    assert_eq!(sequence.frame_index_at(Duration::new(11, 999_999_999)), 287);
    assert_eq!(sequence.frame_index_at(Duration::from_secs(12)), 0);
    assert_eq!(sequence.frame_index_at(Duration::from_secs(120)), 0);
    // Every first representable nanosecond in a frame, and the preceding one.
    for frame in 1u64..=288 {
        let first_ns = (frame * 1_000_000_000 + 23) / 24;
        assert_eq!(
            sequence.frame_index_at(Duration::from_nanos(first_ns)),
            (frame % 288) as u32
        );
        assert_eq!(
            sequence.frame_index_at(Duration::from_nanos(first_ns - 1)),
            (frame - 1) as u32
        );
    }
}

#[test]
fn non_integral_second_loop_and_huge_elapsed_do_not_overflow() {
    let bytes = asset(1, 1, 3, &[&[0], &[0], &[0], &[0], &[0]]);
    let sequence = Sequence::from_bytes(&bytes).unwrap();
    for elapsed in [
        Duration::ZERO,
        Duration::from_nanos(333_333_333),
        Duration::from_nanos(333_333_334),
        Duration::from_secs(2),
        Duration::new(u64::MAX, 999_999_999),
        Duration::MAX,
    ] {
        let ticks = (u128::from(elapsed.as_secs()) * 1_000_000_000
            + u128::from(elapsed.subsec_nanos()))
            * 3
            / 1_000_000_000;
        assert_eq!(sequence.frame_index_at(elapsed), (ticks % 5) as u32);
        assert_eq!(sequence.frame_at(elapsed).mask_len(), 1);
    }
    let frames: Vec<&[u8]> = vec![&[0]; 288];
    let bytes = asset(1, 1, 24, &frames);
    let sequence = Sequence::from_bytes(&bytes).unwrap();
    let expected = ((u128::from(u64::MAX) * 24 + 23) % 288) as u32;
    assert_eq!(sequence.frame_index_at(Duration::MAX), expected);
}

#[test]
fn one_frame_and_largest_supported_grid_are_valid() {
    let mut rows = Vec::new();
    for _ in 0..255 {
        rows.extend_from_slice(&[1, 0, 255]);
    }
    let bytes = asset(255, 255, 1_000, &[&rows]);
    let sequence = Sequence::from_bytes(&bytes).unwrap();
    assert_eq!(sequence.frame_index_at(Duration::MAX), 0);
    let mut mask = vec![0; 255 * 255];
    sequence
        .frame_at(Duration::MAX)
        .write_mask(&mut mask)
        .unwrap();
    assert!(mask.iter().all(|&point| point == 1));
}

#[test]
fn maximum_frame_count_and_alternating_row_are_valid() {
    let mut row = vec![128];
    for start in (0..=254).step_by(2) {
        row.extend_from_slice(&[start, 1]);
    }
    let frames = vec![row.as_slice(); 4_096];
    let bytes = asset(255, 1, 1_000, &frames);
    let sequence = Sequence::from_bytes(&bytes).unwrap();
    assert_eq!(sequence.frames().len(), 4_096);
    let frame = sequence.frame(4_095).unwrap();
    assert_eq!(frame.rows().next().unwrap().runs().len(), 128);
    let mut mask = [9; 255];
    frame.write_mask(&mut mask).unwrap();
    for (x, present) in mask.into_iter().enumerate() {
        assert_eq!(present, u8::from(x % 2 == 0));
    }
    let expected = ((u128::from(u64::MAX) * 1_000 + 999) % 4_096) as u32;
    assert_eq!(sequence.frame_index_at(Duration::MAX), expected);
}

#[test]
fn rejects_truncated_header_bad_magic_and_version() {
    let valid = asset(8, 1, 24, &[&[0]]);
    for len in 0..16 {
        assert_eq!(error(&valid[..len]), DecodeError::TruncatedHeader);
    }
    let mut bytes = valid.clone();
    bytes[0] = b'X';
    assert_eq!(error(&bytes), DecodeError::BadMagic);
    set_u16(&mut bytes, 4, 2);
    bytes[0] = b'G';
    assert_eq!(error(&bytes), DecodeError::UnsupportedVersion(2));
}

#[test]
fn rejects_zero_and_oversized_dimensions_fps_and_frame_count() {
    for (width, height) in [(0, 1), (1, 0), (256, 1), (1, 256), (u16::MAX, u16::MAX)] {
        assert_eq!(
            error(&asset(width, height, 24, &[&[0]])),
            DecodeError::InvalidDimensions { width, height }
        );
    }
    for fps in [0, 1_001, u16::MAX] {
        assert_eq!(
            error(&asset(8, 1, fps, &[&[0]])),
            DecodeError::InvalidFps(fps)
        );
    }
    for count in [0, 4_097, u32::MAX] {
        let mut bytes = asset(8, 1, 24, &[&[0]]);
        set_u32(&mut bytes, 12, count);
        assert_eq!(error(&bytes), DecodeError::InvalidFrameCount(count));
    }
}

#[test]
fn rejects_oversized_asset_before_reading_the_header() {
    let bytes = vec![0; goodhand::sequence::MAX_ASSET_BYTES + 1];
    assert_eq!(error(&bytes), DecodeError::AssetTooLarge);
}

#[test]
fn rejects_offset_table_truncation_and_nonzero_origin() {
    let valid = asset(8, 1, 24, &[&[0]]);
    for len in 16..24 {
        assert_eq!(error(&valid[..len]), DecodeError::TruncatedOffsetTable);
    }
    let mut bytes = valid;
    set_u32(&mut bytes, 16, 1);
    assert_eq!(error(&bytes), DecodeError::FirstOffsetNotZero);
}

#[test]
fn rejects_out_of_range_nonmonotone_and_inexact_final_offsets() {
    let mut bytes = asset(8, 1, 24, &[&[0], &[0]]);
    set_u32(&mut bytes, 20, 3);
    assert_eq!(
        error(&bytes),
        DecodeError::OffsetOutOfRange {
            index: 1,
            offset: 3
        }
    );
    set_u32(&mut bytes, 20, 2);
    set_u32(&mut bytes, 24, 1);
    assert_eq!(error(&bytes), DecodeError::NonMonotoneOffsets { index: 2 });
    set_u32(&mut bytes, 20, 1);
    assert_eq!(error(&bytes), DecodeError::FinalOffsetMismatch);
    set_u32(&mut bytes, 24, u32::MAX);
    assert_eq!(
        error(&bytes),
        DecodeError::OffsetOutOfRange {
            index: 2,
            offset: u32::MAX
        }
    );
}

#[test]
fn rejects_empty_frames_and_validates_later_frames_before_exposure() {
    assert_eq!(frame_error(&[]), FrameError::TruncatedRow { row: 0 });
    let bytes = asset(8, 2, 24, &[&[0]]);
    assert_eq!(
        error(&bytes),
        DecodeError::InvalidFrame {
            index: 0,
            error: FrameError::TruncatedRow { row: 1 }
        }
    );
    let bytes = asset(8, 1, 24, &[&[0], &[1, 0, 0]]);
    assert_eq!(
        error(&bytes),
        DecodeError::InvalidFrame {
            index: 1,
            error: FrameError::ZeroLengthRun { row: 0, run: 0 }
        }
    );
}

#[test]
fn rejects_truncated_run_count_data_zero_lengths_and_outside_runs() {
    for encoded in [&[1][..], &[1, 0][..], &[2, 0, 1][..]] {
        let run = if encoded[0] == 2 { 1 } else { 0 };
        assert_eq!(
            frame_error(encoded),
            FrameError::TruncatedRun { row: 0, run }
        );
    }
    assert_eq!(
        frame_error(&[1, 0, 0]),
        FrameError::ZeroLengthRun { row: 0, run: 0 }
    );
    for encoded in [&[1, 7, 2][..], &[1, 8, 1][..], &[1, 255, 255][..]] {
        assert_eq!(
            frame_error(encoded),
            FrameError::RunOutsideGrid { row: 0, run: 0 }
        );
    }
}

#[test]
fn rejects_out_of_order_overlapping_adjacent_runs_and_trailing_data() {
    for encoded in [
        &[2, 4, 1, 0, 1][..], // out of order
        &[2, 1, 3, 2, 1][..], // overlap
        &[2, 1, 2, 3, 1][..], // adjacency
    ] {
        assert_eq!(
            frame_error(encoded),
            FrameError::NonCanonicalRun { row: 0, run: 1 }
        );
    }
    assert_eq!(frame_error(&[0, 99]), FrameError::TrailingBytes);
}

#[test]
fn every_truncation_and_single_byte_mutation_is_safe_to_parse() {
    let valid = asset(8, 3, 24, &[&[2, 0, 2, 4, 2, 0, 1, 7, 1], &[0, 1, 1, 6, 0]]);
    let exercise = |bytes: &[u8]| {
        if let Ok(sequence) = Sequence::from_bytes(bytes) {
            let mut mask = vec![9; usize::from(sequence.width()) * usize::from(sequence.height())];
            for frame in sequence.frames() {
                frame.write_mask(&mut mask).unwrap();
                assert!(mask.iter().all(|&point| point <= 1));
                for row in frame.rows() {
                    for run in row.runs() {
                        assert!(run.length() > 0);
                        assert!(run.end() <= sequence.width());
                    }
                }
            }
        }
    };
    for end in 0..=valid.len() {
        exercise(&valid[..end]);
    }
    for index in 0..valid.len() {
        for value in 0..=u8::MAX {
            let mut bytes = valid.clone();
            bytes[index] = value;
            exercise(&bytes);
        }
    }
}
