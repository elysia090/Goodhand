from pathlib import Path
import tempfile
import unittest

import numpy as np

from render_cycle import FRAME_BYTES, NEUTRAL_CHROMA, frames, pose_holds, render


class TimingTests(unittest.TestCase):
    def test_exact_22_second_schedule(self):
        holds = pose_holds(288, 528)
        self.assertEqual(holds[:6], (2, 2, 2, 1, 2, 2))
        self.assertEqual(holds.count(1), 48)
        self.assertEqual(holds.count(2), 240)
        self.assertEqual(sum(holds), 528)
        elapsed = 0
        for pose, ticks in enumerate(holds, 1):
            elapsed += ticks
            self.assertLessEqual(abs(elapsed * 288 - pose * 528), 144)

    def test_never_drops_a_pose(self):
        for poses, ticks in ((0, 0), (288, 287), (-1, 1)):
            with self.subTest(poses=poses, ticks=ticks):
                with self.assertRaises(ValueError):
                    pose_holds(poses, ticks)
        self.assertEqual(pose_holds(288, 288), (1,) * 288)
        self.assertEqual(pose_holds(288, 576), (2,) * 288)

    def test_fixed_grid_and_hold_order(self):
        gray = np.zeros((2, 120, 200), dtype=np.uint8)
        mask = np.zeros((2, 120, 200), dtype=np.bool_)
        gray[0, 2, 3], mask[0, 2, 3] = 47, True
        gray[1, 2, 3], mask[1, 2, 3] = 85, True
        gray[0, 4, 3] = 200  # Not occupied.
        first, repeat, last = frames(gray, mask, (2, 1))
        self.assertEqual(len(first), FRAME_BYTES)
        self.assertEqual(first, repeat)
        self.assertEqual(first[640_000:], NEUTRAL_CHROMA)
        a, b = [np.frombuffer(x[:640_000], dtype=np.uint8).reshape(640, 1000) for x in (first, last)]
        self.assertEqual(np.argwhere(a).tolist(), [[52, 17], [52, 18], [53, 17], [53, 18]])
        self.assertEqual((a.max(), b.max()), (47, 85))

    def test_protects_existing_output_and_invalid_duration(self):
        with tempfile.TemporaryDirectory() as root:
            d = Path(root)
            destination = d / "existing.mp4"
            destination.write_bytes(b"protected")
            with self.assertRaises(FileExistsError):
                render(d / "missing.npz", destination, kind="authored", seconds=22)
            self.assertEqual(destination.read_bytes(), b"protected")
            with self.assertRaises(ValueError):
                render(d / "missing.npz", d / "new.mp4", kind="authored", seconds=11)


if __name__ == "__main__":
    unittest.main()
