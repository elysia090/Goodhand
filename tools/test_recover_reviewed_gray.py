"""Test recovery and its rejection paths without requiring a video or pose guide."""

from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import numpy as np

from gray_source import GRID_SHAPE
from recover_reviewed_gray import recover, recover_frames
from render_cycle import LUMA_BYTES, WIDTH, frames


class RecoveryContracts(unittest.TestCase):
    def setUp(self):
        self.mask = np.zeros((2, *GRID_SHAPE), dtype=bool)
        self.gray = np.zeros(self.mask.shape, dtype=np.uint8)
        self.mask[:, 0, :2] = True
        self.mask[1, 3, 4] = True
        self.gray[:, 0, 1] = [97, 155]
        self.gray[1, 3, 4] = 71
        self.holds = (3, 2)
        self.raw = list(frames(self.gray, self.mask, self.holds))

    def test_exact_gray_recovery_includes_occupied_black(self):
        original_shape = self.mask.copy()
        np.testing.assert_array_equal(recover_frames(self.raw, self.mask, self.holds), self.gray)
        np.testing.assert_array_equal(self.mask, original_shape)
        self.assertTrue(self.mask[0, 0, 0])
        self.assertEqual(self.gray[0, 0, 0], 0)

    def test_neutral_chroma_is_required(self):
        bad = self.raw.copy()
        raw = bytearray(bad[0])
        raw[LUMA_BYTES] = 129
        bad[0] = bytes(raw)
        with self.assertRaisesRegex(ValueError, "neutral reference frame"):
            recover_frames(bad, self.mask, self.holds)

    def test_wrong_background_or_one_wrong_dot_pixel_is_detected(self):
        for location in (0, 43*WIDTH + 8):
            with self.subTest(location=location):
                bad = self.raw.copy()
                raw = bytearray(bad[0])
                raw[location] = (raw[location] + 1) % 256
                bad[0] = bytes(raw)
                with self.assertRaisesRegex(ValueError, "declared occupied dot raster"):
                    recover_frames(bad, self.mask, self.holds)

    def test_missing_extra_or_disagreeing_repeated_frames_are_rejected(self):
        for data in (self.raw[:-1], self.raw[:1] + self.raw[3:4] + self.raw[2:]):
            with self.assertRaisesRegex(ValueError, "repeated ticks"):
                recover_frames(data, self.mask, self.holds)
        with self.assertRaisesRegex(ValueError, "trailing video frames"):
            recover_frames(self.raw + [self.raw[-1]], self.mask, self.holds)

    def test_invalid_input_digest_fails_before_subprocess_or_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            movie = root / "review.mp4"
            movie.write_bytes(b"not the expected movie")
            with patch("recover_reviewed_gray.subprocess.Popen") as popen:
                with self.assertRaisesRegex(ValueError, "expected SHA-256"):
                    recover(movie, root / "missing.npz", root / "out", seconds=30, expected_sha256="b"*64)
                popen.assert_not_called()
            self.assertFalse((root / "out").exists())
