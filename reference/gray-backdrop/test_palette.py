import unittest

import render as study


class GrayBackdropStudyTest(unittest.TestCase):
    def test_calibration_anchors_and_order(self):
        rgb = study.palette()
        self.assertEqual(len(rgb), 256)
        self.assertEqual(rgb[0], (0, 0, 0))
        self.assertTrue(all(study.ordered(a, b) for a, b in zip(rgb, rgb[1:])))
        self.assertTrue(all(0 <= c <= 255 for color in rgb for c in color))
        self.assertLess(study.light(rgb[55])[0], 96 * 10_000)
        self.assertGreater(study.light(rgb[90])[0], 96 * 10_000)
        self.assertNotEqual(rgb[100], (100, 100, 100))

    def test_deterministic_comparison_preserves_all_mark_positions(self):
        data = study.render()
        head, pixels = data.split(b"\n255\n", 1)
        self.assertEqual(head, b"P6\n1048 112")
        self.assertEqual(len(pixels), 1048 * 112 * 3)
        self.assertEqual(data, study.render())
        pixel = lambda x, y: tuple(pixels[(1048 * y + x) * 3: (1048 * y + x + 1) * 3])
        palette = study.palette()
        for tone in range(256):
            x = 12 + tone * 4
            self.assertEqual(pixel(x, 16), (tone,) * 3)
            self.assertEqual(pixel(x, 64), palette[tone])
            self.assertEqual(pixel(x + 3, 16), (96,) * 3)
            self.assertEqual(pixel(x + 3, 64), (96,) * 3)
