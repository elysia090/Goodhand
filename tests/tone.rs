use goodhand::{Sequence, ToneError, ToneSequence};

fn asset(magic: &[u8; 4], width: u16, height: u16, fps: u16, frames: &[&[u8]]) -> Vec<u8> {
    let mut bytes = magic.to_vec();
    for value in [1, width, height, fps] {
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

fn set_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn set_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn fixed_wire_fixture_borrows_occupied_gray_including_black() {
    let shape_bytes = asset(b"GHSH", 4, 2, 24, &[&[1, 1, 2, 0], &[0, 1, 0, 1]]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    let bytes = b"GHT8\x01\0\x04\0\x02\0\x18\0\x02\0\0\0\0\0\0\0\x02\0\0\0\x03\0\0\0\0\xff\x77";
    let before = *bytes;
    let tones = ToneSequence::from_bytes(bytes, shape).unwrap();
    assert_eq!(tones.frame_count(), 2);
    assert_eq!(tones.frame(0), Some(&[0, 255][..]));
    assert_eq!(tones.frame(1), Some(&[119][..]));
    assert_eq!(tones.frame(0).unwrap().as_ptr(), bytes[28..].as_ptr());
    assert!(tones.frame(2).is_none());
    assert!(tones.frame(u32::MAX).is_none());
    assert_eq!(*bytes, before);
    let mut mask = [0; 8];
    shape.frame(0).unwrap().write_mask(&mut mask).unwrap();
    assert_eq!(mask, [0, 1, 1, 0, 0, 0, 0, 0]);
}

#[test]
fn empty_occupied_frames_have_zero_length_tone_slices() {
    let shape_bytes = asset(b"GHSH", 2, 1, 24, &[&[0], &[0]]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    let bytes = asset(b"GHT8", 2, 1, 24, &[&[], &[]]);
    let tones = ToneSequence::from_bytes(&bytes, shape).unwrap();
    assert_eq!(tones.frame(0), Some(&[][..]));
    assert_eq!(tones.frame(1), Some(&[][..]));
}

#[test]
fn maximum_grid_and_frame_count_stay_bounded() {
    let mut rows = Vec::new();
    for _ in 0..255 {
        rows.extend_from_slice(&[1, 0, 255]);
    }
    let shape_bytes = asset(b"GHSH", 255, 255, 1000, &[&rows]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    let gray = vec![255; 255 * 255];
    let bytes = asset(b"GHT8", 255, 255, 1000, &[&gray]);
    assert_eq!(
        ToneSequence::from_bytes(&bytes, shape)
            .unwrap()
            .frame(0)
            .unwrap(),
        gray
    );

    let frames: Vec<&[u8]> = vec![&[0]; 4096];
    let shape_bytes = asset(b"GHSH", 1, 1, 1000, &frames);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    let frames: Vec<&[u8]> = vec![&[]; 4096];
    let bytes = asset(b"GHT8", 1, 1, 1000, &frames);
    let tones = ToneSequence::from_bytes(&bytes, shape).unwrap();
    assert_eq!(tones.frame_count(), 4096);
    assert_eq!(tones.frame(4095), Some(&[][..]));
}

#[test]
fn every_short_header_is_rejected() {
    let shape_bytes = asset(b"GHSH", 1, 1, 24, &[&[0]]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    for size in 0..16 {
        assert_eq!(
            ToneSequence::from_bytes(&vec![0; size], shape).unwrap_err(),
            ToneError::TruncatedHeader
        );
    }
}

#[test]
fn rejects_magic_and_version() {
    let shape_bytes = asset(b"GHSH", 1, 1, 24, &[&[0]]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    let mut bytes = asset(b"BAD!", 1, 1, 24, &[&[]]);
    assert_eq!(
        ToneSequence::from_bytes(&bytes, shape).unwrap_err(),
        ToneError::BadMagic
    );
    bytes[..4].copy_from_slice(b"GHT8");
    set_u16(&mut bytes, 4, 2);
    assert_eq!(
        ToneSequence::from_bytes(&bytes, shape).unwrap_err(),
        ToneError::UnsupportedVersion(2)
    );
}

#[test]
fn rejects_mismatched_dimensions_and_extreme_values_before_arithmetic() {
    let shape_bytes = asset(b"GHSH", 1, 1, 24, &[&[0]]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    for (width, height) in [(0, 1), (1, 0), (256, 1), (1, 256), (u16::MAX, u16::MAX)] {
        let bytes = asset(b"GHT8", width, height, 24, &[&[]]);
        assert_eq!(
            ToneSequence::from_bytes(&bytes, shape).unwrap_err(),
            ToneError::DimensionsMismatch {
                expected: (1, 1),
                actual: (width, height)
            }
        );
    }
}

#[test]
fn rejects_mismatched_fps_without_adding_a_clock() {
    let shape_bytes = asset(b"GHSH", 1, 1, 24, &[&[0]]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    for fps in [0, 25, 1001, u16::MAX] {
        let bytes = asset(b"GHT8", 1, 1, fps, &[&[]]);
        assert_eq!(
            ToneSequence::from_bytes(&bytes, shape).unwrap_err(),
            ToneError::FpsMismatch {
                expected: 24,
                actual: fps
            }
        );
    }
}

#[test]
fn rejects_extreme_frame_counts_before_table_size_calculation() {
    let shape_bytes = asset(b"GHSH", 1, 1, 24, &[&[0]]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    for count in [0, 2, 4097, u32::MAX] {
        let mut bytes = asset(b"GHT8", 1, 1, 24, &[&[]]);
        set_u32(&mut bytes, 12, count);
        assert_eq!(
            ToneSequence::from_bytes(&bytes, shape).unwrap_err(),
            ToneError::FrameCountMismatch {
                expected: 1,
                actual: count
            }
        );
    }
}

#[test]
fn rejects_truncated_offset_table() {
    let shape_bytes = asset(b"GHSH", 1, 1, 24, &[&[0]]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    let bytes = asset(b"GHT8", 1, 1, 24, &[&[]]);
    for size in 16..24 {
        assert_eq!(
            ToneSequence::from_bytes(&bytes[..size], shape).unwrap_err(),
            ToneError::TruncatedOffsetTable
        );
    }
}

#[test]
fn rejects_nonzero_outside_nonmonotone_and_inexact_offsets() {
    let shape_bytes = asset(b"GHSH", 1, 1, 24, &[&[1, 0, 1], &[1, 0, 1]]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    let valid = asset(b"GHT8", 1, 1, 24, &[&[11], &[22]]);
    let mut bytes = valid.clone();
    set_u32(&mut bytes, 16, 1);
    assert_eq!(
        ToneSequence::from_bytes(&bytes, shape).unwrap_err(),
        ToneError::FirstOffsetNotZero
    );
    let mut bytes = valid.clone();
    set_u32(&mut bytes, 20, u32::MAX);
    assert_eq!(
        ToneSequence::from_bytes(&bytes, shape).unwrap_err(),
        ToneError::OffsetOutOfRange {
            index: 1,
            offset: u32::MAX
        }
    );
    let mut bytes = valid.clone();
    set_u32(&mut bytes, 20, 2);
    set_u32(&mut bytes, 24, 1);
    assert_eq!(
        ToneSequence::from_bytes(&bytes, shape).unwrap_err(),
        ToneError::NonMonotoneOffsets { index: 2 }
    );
    let mut bytes = valid;
    set_u32(&mut bytes, 24, 1);
    assert_eq!(
        ToneSequence::from_bytes(&bytes, shape).unwrap_err(),
        ToneError::FinalOffsetMismatch
    );
}

#[test]
fn rejects_later_frame_count_mismatch_before_any_frame_is_exposed() {
    let shape_bytes = asset(b"GHSH", 1, 1, 24, &[&[1, 0, 1], &[1, 0, 1]]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    for values in [&[][..], &[1, 2][..]] {
        let bytes = asset(b"GHT8", 1, 1, 24, &[&[7], values]);
        assert_eq!(
            ToneSequence::from_bytes(&bytes, shape).unwrap_err(),
            ToneError::OccupiedCountMismatch {
                index: 1,
                expected: 1,
                actual: values.len()
            }
        );
    }
}

#[test]
fn rejects_trailing_tone_bytes() {
    let shape_bytes = asset(b"GHSH", 1, 1, 24, &[&[1, 0, 1]]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    let mut bytes = asset(b"GHT8", 1, 1, 24, &[&[11]]);
    bytes.push(22);
    assert_eq!(
        ToneSequence::from_bytes(&bytes, shape).unwrap_err(),
        ToneError::FinalOffsetMismatch
    );
    set_u32(&mut bytes, 20, 2);
    assert_eq!(
        ToneSequence::from_bytes(&bytes, shape).unwrap_err(),
        ToneError::OccupiedCountMismatch {
            index: 0,
            expected: 1,
            actual: 2
        }
    );
}

#[test]
fn rejects_over_64_mib_before_header_access() {
    let shape_bytes = asset(b"GHSH", 1, 1, 24, &[&[0]]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    let bytes = vec![0; goodhand::sequence::MAX_ASSET_BYTES + 1];
    assert_eq!(
        ToneSequence::from_bytes(&bytes, shape).unwrap_err(),
        ToneError::AssetTooLarge
    );
}

#[test]
fn every_truncation_and_single_byte_mutation_is_safe_to_expose() {
    let shape_bytes = asset(b"GHSH", 4, 2, 24, &[&[1, 1, 2, 0], &[0, 1, 0, 1]]);
    let shape = Sequence::from_bytes(&shape_bytes).unwrap();
    let valid = asset(b"GHT8", 4, 2, 24, &[&[0, 255], &[119]]);
    let exercise = |bytes: &[u8]| {
        if let Ok(tones) = ToneSequence::from_bytes(bytes, shape) {
            assert_eq!(tones.frame(0).unwrap().len(), 2);
            assert_eq!(tones.frame(1).unwrap().len(), 1);
            assert!(tones.frame(2).is_none());
            assert!(tones.frame(u32::MAX).is_none());
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
