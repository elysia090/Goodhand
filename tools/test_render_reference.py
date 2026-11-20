#!/usr/bin/env python3
"""Synthetic tests for fixed-grid reference redrawing; no original artwork."""

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import subprocess
import sys
import unittest
from unittest import mock

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
        "width": 1000, "height": 640, "fps": 24, "frames": 288,
        "period_seconds": 12,
        "grid": {"columns": 200, "rows": 120, "pitch_px": 5,
                 "dot_size_px": [2, 2], "dot_origin_px": [2, 42]},
    }
    source.with_suffix(".json").write_text(json.dumps(metadata))
    return source, gray, mask



def authored_fixture(folder):
    source, gray, mask = observation_fixture(folder)
    gray[:, 20, 10] = 0  # Occupied black, not an absent point.
    mask[:, 21, 11] = True
    gray[:, 21, 11] = 231
    np.savez_compressed(source, authored_gray=gray, mask=mask)
    path = source.with_suffix(".json")
    metadata = json.loads(path.read_text())
    metadata.update(output_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                    source_kind="authored", mask_status="accepted",
                    generation={"method": "synthetic fixture", "parent_observation_sha256": "a" * 64})
    path.write_text(json.dumps(metadata))
    return source, gray, mask


class AuthoredPreview(unittest.TestCase):
    def test_exact_pixels_mask_and_provenance_without_adoption(self):
        with tempfile.TemporaryDirectory() as folder:
            source, gray, mask = authored_fixture(folder)
            loaded_gray, loaded_mask, _, _, _ = renderer._SOURCE_IO.load_gray_source(
                source, kind="authored", profile="reference-v1")
            np.testing.assert_array_equal(loaded_gray, gray)
            np.testing.assert_array_equal(loaded_mask, mask)
            self.assertTrue(loaded_mask[0, 20, 10])
            self.assertEqual(loaded_gray[0, 20, 10], 0)
            out = Path(folder) / "preview"
            report = renderer.render(source, out, [287, 0], kind="authored")
            expected = np.zeros((640, 1000), dtype=np.uint8)
            expected[147:149, 57:59] = 231
            np.testing.assert_array_equal(np.asarray(Image.open(out / "reference-f000.png")), expected)
            self.assertEqual(report["source_kind"], "authored")
            self.assertNotIn("source_observation_sha256", report)
            self.assertEqual(report["source_npz_sha256"], hashlib.sha256(source.read_bytes()).hexdigest())
            self.assertEqual(report["source_sidecar_sha256"],
                             hashlib.sha256(source.with_suffix(".json").read_bytes()).hexdigest())
            self.assertEqual(report["generation_declaration"]["parent_observation_sha256"], "a" * 64)
            self.assertEqual(report["artifact_adoption"], "not_assessed")
            self.assertEqual(report["mask_status"], "accepted")
            self.assertEqual([f["frame"] for f in report["files"]], [287, 0])
            self.assertEqual([f["declared_yaw"] for f in report["files"]], [1.25, 0])
            self.assertEqual(set(p.name for p in out.iterdir()),
                             {"reference-f287.png", "reference-f000.png", "reference-redraw.json"})
            self.assertEqual(json.loads((out / "reference-redraw.json").read_text()), report)
            for item in report["files"]:
                self.assertEqual(item["sha256"], hashlib.sha256((out / item["name"]).read_bytes()).hexdigest())

    def test_role_mismatch_and_unknown_kind_leave_no_output(self):
        with tempfile.TemporaryDirectory() as folder:
            out = Path(folder) / "preview"
            source, _, _ = authored_fixture(folder)
            for call in (lambda: renderer.render(source, out, [0]),
                         lambda: renderer.load_observations(source),
                         lambda: renderer.render(source, out, [0], kind="auto")):
                with self.assertRaises(ValueError):
                    call()
                self.assertFalse(out.exists())
            source, _, _ = observation_fixture(folder)
            with self.assertRaises(ValueError):
                renderer.render(source, out, [0], kind="authored")
            self.assertFalse(out.exists())

    def test_authored_invalid_declaration_or_mixed_arrays_leave_no_output(self):
        with tempfile.TemporaryDirectory() as folder:
            for failure in ("missing generation", "invalid parent", "mixed arrays"):
                with self.subTest(failure=failure):
                    source, gray, mask = authored_fixture(folder)
                    path = source.with_suffix(".json")
                    metadata = json.loads(path.read_text())
                    if failure == "missing generation":
                        del metadata["generation"]
                    elif failure == "invalid parent":
                        metadata["generation"]["parent_observation_sha256"] = "not a hash"
                    else:
                        np.savez_compressed(source, authored_gray=gray, observed_gray=gray, mask=mask)
                        metadata["output_sha256"] = hashlib.sha256(source.read_bytes()).hexdigest()
                    path.write_text(json.dumps(metadata))
                    out = Path(folder) / "preview"
                    with self.assertRaises(ValueError):
                        renderer.render(source, out, [0], kind="authored")
                    self.assertFalse(out.exists())

    def test_authored_snapshot_survives_path_replacement(self):
        with tempfile.TemporaryDirectory() as folder:
            source, gray, mask = authored_fixture(folder)
            source_bytes, sidecar_bytes = source.read_bytes(), source.with_suffix(".json").read_bytes()
            original_read = renderer._SOURCE_IO.read_bounded_snapshot
            calls = []

            def read_once(path, limit):
                self.assertNotIn(path, calls)
                calls.append(path)
                snapshot = original_read(path, limit)
                Path(path).write_bytes(b"replaced")
                return snapshot

            with mock.patch.object(renderer._SOURCE_IO, "read_bounded_snapshot", read_once):
                out = Path(folder) / "preview"
                report = renderer.render(source, out, [0], kind="authored")
            self.assertEqual(len(calls), 2)
            self.assertEqual(report["source_npz_sha256"], hashlib.sha256(source_bytes).hexdigest())
            self.assertEqual(report["source_sidecar_sha256"], hashlib.sha256(sidecar_bytes).hexdigest())
            np.testing.assert_array_equal(np.asarray(Image.open(out / "reference-f000.png")),
                                          renderer.compose_frame(gray[0], mask[0]))

    def test_cli_observed_default_matches_explicit_role(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _ = observation_fixture(folder)
            reports = []
            for name, options in (("default", []), ("explicit", ["--kind", "observed"])):
                command = [sys.executable, str(Path(renderer.__file__)), str(source),
                           str(Path(folder) / name), "--frames", "0"] + options
                result = subprocess.run(command, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                reports.append(json.loads(result.stdout))
            self.assertEqual(reports[0], reports[1])
            self.assertEqual(reports[0]["source_kind"], "observed")
            self.assertEqual(reports[0]["source_observation_sha256"], reports[0]["source_npz_sha256"])
            self.assertNotIn("generation_declaration", reports[0])

    def test_cli_authored_requires_explicit_role(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _ = authored_fixture(folder)
            out = Path(folder) / "preview"
            command = [sys.executable, str(Path(renderer.__file__)), str(source), str(out), "--frames", "0"]
            failed = subprocess.run(command, capture_output=True, text=True)
            self.assertNotEqual(failed.returncode, 0)
            self.assertFalse(out.exists())
            success = subprocess.run(command + ["--kind", "authored"], capture_output=True, text=True)
            self.assertEqual(success.returncode, 0, success.stderr)
            self.assertEqual(json.loads(success.stdout)["source_kind"], "authored")
            repeated = subprocess.run(command + ["--kind", "authored"], capture_output=True, text=True)
            self.assertNotEqual(repeated.returncode, 0)

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

    def test_missing_or_mismatched_profile_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _ = observation_fixture(folder)
            path = source.with_suffix(".json")
            metadata = json.loads(path.read_text())
            for key in ("width", "height", "fps", "frames", "period_seconds", "grid"):
                for missing in (True, False):
                    with self.subTest(key=key, missing=missing):
                        changed = dict(metadata)
                        if missing:
                            del changed[key]
                        else:
                            changed[key] = {"columns": 200} if key == "grid" else 1
                        path.write_text(json.dumps(changed))
                        with self.assertRaises(ValueError):
                            renderer.load_observations(source)
            for key in metadata["grid"]:
                changed = dict(metadata)
                changed["grid"] = dict(metadata["grid"])
                changed["grid"][key] = 1
                path.write_text(json.dumps(changed))
                with self.subTest(grid_key=key), self.assertRaises(ValueError):
                    renderer.load_observations(source)

    def test_source_snapshot_hash_and_pixels_remain_consistent(self):
        with tempfile.TemporaryDirectory() as folder:
            source, gray, mask = observation_fixture(folder)
            source_bytes = source.read_bytes()
            original_read = renderer._SOURCE_IO.read_bounded_snapshot
            calls = []

            def read_once(path, limit):
                if path == source:
                    calls.append(path)
                    if len(calls) > 1:
                        raise AssertionError("Source must only be read once")
                    snapshot = original_read(path, limit)
                    # Simulate a concurrent replacement after the input snapshot.
                    source.write_bytes(b"different file after snapshot")
                    return snapshot
                return original_read(path, limit)

            with mock.patch.object(renderer._SOURCE_IO, "read_bounded_snapshot", read_once):
                out = Path(folder) / "rendered"
                report = renderer.render(source, out, [0])
            self.assertEqual(len(calls), 1)
            self.assertEqual(report["source_observation_sha256"],
                             hashlib.sha256(source_bytes).hexdigest())
            actual = np.asarray(Image.open(out / "reference-f000.png"))
            np.testing.assert_array_equal(actual, renderer.compose_frame(gray[0], mask[0]))

    def test_duplicate_or_outside_frame_rejected_without_outputs(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _ = observation_fixture(folder)
            for frames in ([0, 0], [-1], [288]):
                out = Path(folder) / "rendered"
                with self.assertRaises(ValueError):
                    renderer.render(source, out, frames)
                self.assertFalse(out.exists())

    def test_existing_directory_rejected_even_with_unselected_files(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _ = observation_fixture(folder)
            out = Path(folder) / "rendered"
            out.mkdir()
            unselected = out / "reference-f144.png"
            unselected.write_bytes(b"previous incomplete run")
            with self.assertRaises(FileExistsError):
                renderer.render(source, out, [0])
            self.assertEqual(unselected.read_bytes(), b"previous incomplete run")
            self.assertFalse((out / "reference-f000.png").exists())
            self.assertFalse((out / "reference-redraw.json").exists())

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

