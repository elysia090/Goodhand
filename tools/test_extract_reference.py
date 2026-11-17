#!/usr/bin/env python3
"""Output-protection tests for offline reference extraction."""

import importlib.util
import pathlib
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "extract", pathlib.Path(__file__).with_name("extract_reference.py")
)
extract = importlib.util.module_from_spec(spec)
spec.loader.exec_module(extract)


class ObservationProtection(unittest.TestCase):
    def test_valid_timestamps(self):
        extract.validate_timestamps([f"{frame/24:.6f}" for frame in range(288)])

    def test_bad_timestamps(self):
        valid = [f"{frame/24:.6f}" for frame in range(288)]
        for bad in (valid[:-1], [None] + valid[1:], ["0.01"] + valid[1:], ["nan"] + valid[1:],
                    valid[:12] + [valid[11]] + valid[13:]):
            with self.assertRaises(ValueError):
                extract.validate_timestamps(bad)

    def test_suffix_cannot_redirect_numpy_to_existing_file(self):
        with tempfile.TemporaryDirectory() as folder:
            destination = pathlib.Path(folder) / "observation"
            existing = destination.with_suffix(".npz")
            existing.write_bytes(b"preserve")
            with self.assertRaises(ValueError):
                extract.extract(pathlib.Path("unused.mp4"), destination)
            self.assertEqual(existing.read_bytes(), b"preserve")

    def test_existing_observations_are_preserved(self):
        with tempfile.TemporaryDirectory() as folder:
            destination = pathlib.Path(folder) / "observation.npz"
            destination.write_bytes(b"preserve")
            with self.assertRaises(FileExistsError):
                extract.extract(pathlib.Path("unused.mp4"), destination)
            self.assertEqual(destination.read_bytes(), b"preserve")

    def test_existing_report_is_preserved(self):
        with tempfile.TemporaryDirectory() as folder:
            destination = pathlib.Path(folder) / "observation.npz"
            report = destination.with_suffix(".json")
            report.write_bytes(b"preserve")
            with self.assertRaises(FileExistsError):
                extract.extract(pathlib.Path("unused.mp4"), destination)
            self.assertEqual(report.read_bytes(), b"preserve")
            self.assertFalse(destination.exists())


if __name__ == "__main__":
    unittest.main()
