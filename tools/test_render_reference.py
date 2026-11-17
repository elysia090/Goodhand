#!/usr/bin/env python3
"""Synthetic tests for fixed-grid reference redrawing; no original artwork."""

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

import numpy as np
from PIL import Image

spec = importlib.util.spec_from_file_location(
    "renderer", Path(__file__).with_name("render_reference.py")
)
renderer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(renderer)


def observation_fixture(folder):
    source = Path(folder) / "observations.npz"
    gray = np.full((288, 120, 200), 119, dtype=np.uint8)
    mask = np.zeros(gray.shape, dtype=np.bool_)
    mask[:, 20, 10] = True
    np.savez_compressed(source, observed_gray=gray, mask=mask)
    metadata = {
        "output_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "source_sha256": "synthetic",
        "mask_status": "observed_candidate",
        "direction": "reverse",
    }
    source.with_suffix(".json").write_text(json.dumps(metadata))
    return source, gray, mask


class ReferenceRedraw(unittest.TestCase):
    def frame(self):
        gray = np.full((120, 200), 119, dtype=np.uint8)
        mask = np.zeros(gray.shape, dtype=np.bool_)
        mask[20, 10] = True
        return gray, mask

    def test_fixed_dot_footprint_and_header(self):
        gray, mask = self.frame()
        canvas = renderer.compose_frame(gray, mask)
        expected = np.zeros((640, 1000), dtype=np.uint8)
        expected[142:144, 52:54] = 119
        np.testing.assert_array_equal(canvas, expected)

    def test_inputs_unchanged_and_background_ignored(self):
        gray, mask = self.frame()
        old_gray, old_mask = gray.copy(), mask.copy()
        renderer.compose_frame(gray, mask)
        np.testing.assert_array_equal(gray, old_gray)
        np.testing.assert_array_equal(mask, old_mask)

    def test_black_occupied_point_is_not_rejected_or_reclassified(self):
        gray, mask = self.frame()
        gray[20, 10] = 0
        self.assertTrue(mask[20, 10])
        self.assertFalse(renderer.compose_frame(gray, mask).any())
        self.assertTrue(mask[20, 10])

    def test_frame_shape_and_types_rejected(self):
        gray, mask = self.frame()
        for bad_gray, bad_mask in (
            (gray.astype(float), mask), (gray[:, :-1], mask),
            (gray, mask.astype(np.uint8)), (gray, mask[:-1]),
        ):
            with self.assertRaises(ValueError):
                renderer.compose_frame(bad_gray, bad_mask)

    def test_png_existing_file_preserved(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "existing.png"
            path.write_bytes(b"preserve")
            with self.assertRaises(FileExistsError):
                renderer.save_png_exclusive(np.zeros((640, 1000), dtype=np.uint8), path)
            self.assertEqual(path.read_bytes(), b"preserve")

    def test_rendered_pixels_and_candidate_status(self):
        with tempfile.TemporaryDirectory() as folder:
            source, gray, mask = observation_fixture(folder)
            out = Path(folder) / "rendered"
            report = renderer.render(source, out, [0, 144])
            self.assertEqual(report["mask_status"], "observed_candidate")
            self.assertEqual([item["frame"] for item in report["files"]], [0, 144])
            actual = np.asarray(Image.open(out / "reference-f000.png"))
            np.testing.assert_array_equal(actual, renderer.compose_frame(gray[0], mask[0]))

    def test_bad_hash_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _ = observation_fixture(folder)
            metadata = json.loads(source.with_suffix(".json").read_text())
            metadata["output_sha256"] = "wrong"
            source.with_suffix(".json").write_text(json.dumps(metadata))
            with self.assertRaises(ValueError):
                renderer.load_observations(source)

    def test_duplicate_or_outside_frame_rejected_without_outputs(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _ = observation_fixture(folder)
            for frames in ([0, 0], [-1], [288]):
                out = Path(folder) / "rendered"
                with self.assertRaises(ValueError):
                    renderer.render(source, out, frames)
                self.assertFalse(out.exists())

    def test_existing_output_report_preserved(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _ = observation_fixture(folder)
            out = Path(folder) / "rendered"
            out.mkdir()
            (out / "reference-redraw.json").write_bytes(b"preserve")
            with self.assertRaises(FileExistsError):
                renderer.render(source, out, [0])
            self.assertEqual((out / "reference-redraw.json").read_bytes(), b"preserve")
            self.assertFalse((out / "reference-f000.png").exists())


if __name__ == "__main__":
    unittest.main()
