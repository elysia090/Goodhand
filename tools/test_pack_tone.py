#!/usr/bin/env python3
"""Synthetic source/packing contracts; no project artwork or curves."""

import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest import mock
import warnings
import zipfile

import numpy as np

spec = importlib.util.spec_from_file_location("tone_packer", Path(__file__).with_name("pack_tone.py"))
pack = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pack)
source_io = pack.source_io


def source_fixture(folder, kind="observed", *, ambiguity=False):
    source = Path(folder) / "source.npz"
    shape_path = Path(folder) / "shape.ghs"
    gray = np.full((288, 120, 200), 201, dtype=np.uint8)
    mask = np.zeros(gray.shape, dtype=np.bool_)
    mask[:, 0, 0] = True
    mask[:, 1, 2] = True
    mask[1, 119, 199] = True
    gray[:, 0, 0] = 0
    gray[:, 1, 2] = 17
    gray[1, 119, 199] = 255
    key = "observed_gray" if kind == "observed" else "authored_gray"
    arrays = {key: gray, "mask": mask}
    if ambiguity:
        uncertain = np.zeros_like(mask)
        uncertain[0, 0, 1] = True
        arrays["threshold_ambiguity"] = uncertain
    np.savez_compressed(source, **arrays)
    metadata = copy.deepcopy(source_io.REFERENCE_PROFILE)
    metadata.update({"output_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                     "source_sha256": "a" * 64, "direction": "reverse",
                     "mask_status": "observed_candidate"})
    if kind == "authored":
        metadata.update({"source_kind": "authored", "generation": {
            "method": "synthetic already-quantized values; not an executable curve",
            "parent_observation_sha256": "b" * 64,
        }})
    source.with_suffix(".json").write_text(json.dumps(metadata))
    shape_path.write_bytes(pack.shape_codec.encode(mask, fps=24))
    return source, shape_path, gray, mask, metadata


def update_metadata(source, metadata):
    source.with_suffix(".json").write_text(json.dumps(metadata))


def replace_archive(source, entries, metadata):
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", UserWarning)
        with zipfile.ZipFile(source, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for name, data in entries:
                archive.writestr(name, data)
    metadata = copy.deepcopy(metadata)
    metadata["output_sha256"] = hashlib.sha256(source.read_bytes()).hexdigest()
    update_metadata(source, metadata)


def archive_entries(source):
    with zipfile.ZipFile(source) as archive:
        return [(item.filename, archive.read(item)) for item in archive.infolist()]


def fake_npy(shape, dtype, payload_bytes=288 * 120 * 200):
    output = io.BytesIO()
    np.lib.format.write_array_header_1_0(output, {
        "descr": np.dtype(dtype).str, "fortran_order": False, "shape": shape,
    })
    output.write(bytes(payload_bytes))
    return output.getvalue()


def directory_bomb(declared_count):
    name = b"observed_gray.npy"
    local = struct.pack("<4s5H3I2H", b"PK\x03\x04", 20, 0, 0, 0, 0, 0, 0, 0, len(name), 0) + name
    entry = struct.pack("<4s6H3I5H2I", b"PK\x01\x02", 20, 20, 0, 0, 0, 0,
                        0, 0, 0, len(name), 0, 0, 0, 0, 0, 0) + name
    directory = entry * 50000
    footer = struct.pack("<4s4H2IH", b"PK\x05\x06", 0, 0, declared_count,
                         declared_count, len(directory), len(local), 0)
    return local + directory + footer


class ToneEncoding(unittest.TestCase):
    def test_tiny_golden_is_profile_independent_and_preserves_black(self):
        mask = np.zeros((2, 2, 5), dtype=bool)
        mask[0, 0, [0, 3, 4]] = True
        mask[0, 1, 1] = True
        gray = np.full(mask.shape, 201, dtype=np.uint8)
        gray[0, 0, [0, 3, 4]] = [0, 17, 255]
        gray[0, 1, 1] = 63
        shape = pack.shape_codec.encode(mask, fps=3)
        old_gray, old_mask = gray.copy(), mask.copy()
        actual = pack.encode_tone(shape, gray, mask)
        expected = (b"GHT8\x01\0\x05\0\x02\0\x03\0\x02\0\0\0"
                    b"\0\0\0\0\x04\0\0\0\x04\0\0\0\0\x11\xff\x3f")
        self.assertEqual(actual, expected)
        np.testing.assert_array_equal(gray, old_gray)
        np.testing.assert_array_equal(mask, old_mask)

    def test_c_order_for_strided_gray_and_fortran_mask(self):
        gray = np.arange(40, dtype=np.uint8).reshape(2, 2, 10)[:, :, ::2]
        mask = np.asfortranarray(gray % 3 == 0)
        shape = pack.shape_codec.encode(mask, fps=7)
        payload = pack.encode_tone(shape, gray, mask)[28:]
        expected = bytes(int(gray[f, y, x]) for f in range(2) for y in range(2)
                         for x in range(5) if mask[f, y, x])
        self.assertEqual(payload, expected)

    def test_all_uint8_values_are_preserved_without_palette_or_curve(self):
        gray = np.arange(256, dtype=np.uint8).reshape(1, 2, 128)
        mask = np.ones(gray.shape, dtype=bool)
        shape = pack.shape_codec.encode(mask, fps=9)
        self.assertEqual(pack.encode_tone(shape, gray, mask)[24:], bytes(range(256)))

    def test_empty_frames_have_empty_payload_and_repeated_offsets(self):
        gray = np.full((2, 1, 3), 255, dtype=np.uint8)
        mask = np.zeros(gray.shape, dtype=bool)
        shape = pack.shape_codec.encode(mask, fps=2)
        blob = pack.encode_tone(shape, gray, mask)
        self.assertEqual(len(blob), 28)
        self.assertEqual(struct.unpack_from("<III", blob, 16), (0, 0, 0))

    def test_dtype_rank_and_shape_are_not_coerced(self):
        gray = np.zeros((1, 1, 3), dtype=np.uint8)
        mask = np.ones(gray.shape, dtype=bool)
        shape = pack.shape_codec.encode(mask)
        for bad_gray, bad_mask in [(gray.astype(float), mask), (gray.astype(object), mask),
                                   (gray, mask.astype(np.uint8)), (gray[0], mask[0]),
                                   (gray[:, :, :-1], mask)]:
            with self.subTest(dtype=str(bad_gray.dtype), dims=bad_gray.shape):
                with self.assertRaises(ValueError):
                    pack.encode_tone(shape, bad_gray, bad_mask)

    def test_equal_occupied_counts_at_different_positions_are_rejected(self):
        gray = np.zeros((1, 1, 3), dtype=np.uint8)
        original = np.array([[[True, False, False]]])
        shifted = np.array([[[False, True, False]]])
        shape = pack.shape_codec.encode(original)
        with self.assertRaisesRegex(ValueError, "every cell"):
            pack.encode_tone(shape, gray, shifted)

    def test_bad_shape_bytes_and_offsets_are_rejected(self):
        gray = np.zeros((1, 1, 3), dtype=np.uint8)
        mask = np.ones(gray.shape, dtype=bool)
        valid = pack.shape_codec.encode(mask)
        cases = [valid[:15], b"BAD!" + valid[4:], valid[:4] + b"\2\0" + valid[6:],
                 valid[:16] + struct.pack("<II", 1, 3) + valid[24:]]
        for value in cases:
            with self.assertRaises(ValueError):
                pack.encode_tone(value, gray, mask)
        with self.assertRaises(ValueError):
            pack.encode_tone(bytearray(valid), gray, mask)

    def test_dimension_mismatch_rejected_before_dense_shape_decode(self):
        gray = np.zeros((1, 1, 3), dtype=np.uint8)
        mask = np.ones(gray.shape, dtype=bool)
        shape = bytearray(pack.shape_codec.encode(mask))
        struct.pack_into("<H", shape, 6, 65535)
        with mock.patch.object(pack.shape_codec, "decode", side_effect=AssertionError("must not allocate")):
            with self.assertRaises(ValueError):
                pack.encode_tone(bytes(shape), gray, mask)

    def test_runtime_size_limit_checked_before_payload_allocation(self):
        gray = np.zeros((3, 2, 20), dtype=np.uint8)
        mask = np.ones(gray.shape, dtype=bool)
        shape = pack.shape_codec.encode(mask)
        limit = 16 + 4 * 4 + 120
        with mock.patch.object(pack, "MAX_ASSET_BYTES", limit):
            self.assertEqual(len(pack.encode_tone(shape, gray, mask)), limit)
        with mock.patch.object(pack, "MAX_ASSET_BYTES", limit - 1):
            with mock.patch.object(pack.shape_codec, "decode", side_effect=AssertionError("must not allocate")):
                with self.assertRaisesRegex(ValueError, "runtime asset limit"):
                    pack.encode_tone(shape, gray, mask)


class ToneSources(unittest.TestCase):
    def load(self, source, kind="observed"):
        return source_io.load_gray_source(source, kind=kind, profile="reference-v1")

    def test_observed_source_accepts_and_checks_optional_ambiguity(self):
        with tempfile.TemporaryDirectory() as folder:
            source, shape, gray, mask, _ = source_fixture(folder, ambiguity=True)
            loaded_gray, loaded_mask, _, digest, _ = self.load(source)
            np.testing.assert_array_equal(loaded_gray, gray)
            np.testing.assert_array_equal(loaded_mask, mask)
            self.assertFalse(loaded_mask[0, 0, 1])
            self.assertEqual(digest, hashlib.sha256(source.read_bytes()).hexdigest())
            report = pack.pack_tone(source, shape, Path(folder) / "export", kind="observed", profile="reference-v1")
            self.assertEqual(report["source_kind"], "observed")
            self.assertEqual(report["artifact_adoption"], "not_assessed")

    def test_authored_generation_is_recorded_not_applied_or_parent_verified(self):
        with tempfile.TemporaryDirectory() as folder:
            source, shape, gray, mask, metadata = source_fixture(folder, "authored")
            metadata["generation"]["gain_not_to_execute"] = 0.1
            update_metadata(source, metadata)
            out = Path(folder) / "export"
            report = pack.pack_tone(source, shape, out, kind="authored", profile="reference-v1")
            self.assertEqual((out / "tone.ght").read_bytes(), pack.encode_tone(shape.read_bytes(), gray, mask))
            self.assertEqual(report["declared_generation"], metadata["generation"])
            self.assertIn("declaration only", report["parent_hash_verification"])
            self.assertEqual(report["output_sha256"], hashlib.sha256((out / "tone.ght").read_bytes()).hexdigest())

    def test_identical_values_have_identical_payload_but_different_source_roles(self):
        with tempfile.TemporaryDirectory() as folder:
            blobs = []
            for kind in ("observed", "authored"):
                directory = Path(folder) / kind
                directory.mkdir()
                source, shape, _, _, _ = source_fixture(directory, kind)
                out = directory / "export"
                report = pack.pack_tone(source, shape, out, kind=kind, profile="reference-v1")
                self.assertEqual(report["source_kind"], kind)
                blobs.append((out / "tone.ght").read_bytes())
            self.assertEqual(*blobs)

    def test_opposite_and_ambiguous_array_names_are_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, metadata = source_fixture(folder)
            with self.assertRaises(ValueError):
                self.load(source, "authored")
            entries = archive_entries(source)
            gray_bytes = next(raw for name, raw in entries if name == "observed_gray.npy")
            for extra_name in ("authored_gray.npy", "gray.npy", "lut.npy"):
                replace_archive(source, entries + [(extra_name, gray_bytes)], metadata)
                with self.assertRaisesRegex(ValueError, "NPZ member"):
                    self.load(source)

    def test_authored_sidecar_requires_explicit_role_and_generation(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, metadata = source_fixture(folder, "authored")
            changes = [dict(metadata, source_kind="observed"), dict(metadata, source_kind=None),
                       dict(metadata, generation=None), dict(metadata, generation={}),
                       dict(metadata, generation={"method": "", "parent_observation_sha256": "b" * 64}),
                       dict(metadata, generation={"method": "synthetic", "parent_observation_sha256": "not-a-hash"})]
            missing_kind = dict(metadata)
            del missing_kind["source_kind"]
            changes.append(missing_kind)
            for changed in changes:
                update_metadata(source, changed)
                with self.assertRaises(ValueError):
                    self.load(source, "authored")

    def test_authored_metadata_cannot_be_labelled_observed(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, metadata = source_fixture(folder)
            for changed in [dict(metadata, source_kind="authored"), dict(metadata, generation={"method": "author"})]:
                update_metadata(source, changed)
                with self.assertRaises(ValueError):
                    self.load(source)

    def test_kind_and_profile_are_required_not_inferred(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, _ = source_fixture(folder)
            for kind, profile in [("auto", "reference-v1"), ("observed", "other")]:
                with self.assertRaises(ValueError):
                    source_io.load_gray_source(source, kind=kind, profile=profile)
            with self.assertRaises(TypeError):
                source_io.load_gray_source(source)

    def test_bad_hash_direction_status_and_profile_are_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, metadata = source_fixture(folder)
            for key, value in [("output_sha256", "wrong"), ("direction", "forward"),
                               ("mask_status", "approved-tone"), ("fps", 30), ("grid", {})]:
                update_metadata(source, dict(metadata, **{key: value}))
                with self.assertRaises(ValueError):
                    self.load(source)

    def test_unknown_or_duplicate_members_rejected_before_numpy_load(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, metadata = source_fixture(folder)
            entries = archive_entries(source)
            for extra in [("unknown.npy", b"unused"), entries[0]]:
                replace_archive(source, entries + [extra], metadata)
                with mock.patch.object(source_io.np, "load", side_effect=AssertionError("must not allocate")):
                    with self.assertRaisesRegex(ValueError, "NPZ member"):
                        self.load(source)

    def test_large_and_underreported_directories_rejected_before_zipfile(self):
        for declared_count in (50000, 2):
            raw = directory_bomb(declared_count)
            self.assertEqual(len(raw), 3150069)
            with mock.patch.object(source_io.zipfile, "ZipFile", side_effect=AssertionError("must not construct index")):
                with self.assertRaises(ValueError):
                    source_io._validate_members(raw, "observed_gray")

    def test_eocd_disk_counts_sizes_and_boundary_rejected_before_zipfile(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, _ = source_fixture(folder)
            raw = source.read_bytes()
            original = list(struct.unpack("<4s4H2IH", raw[-22:]))
            changes = [(1, 1), (2, 1), (3, 3), (4, 1), (3, 65535),
                       (5, 0), (5, 0xFFFFFFFF), (6, 0xFFFFFFFF),
                       (5, original[5] - 1), (6, original[6] + 1)]
            for field, value in changes:
                fields = original.copy()
                fields[field] = value
                changed = raw[:-22] + struct.pack("<4s4H2IH", *fields)
                with mock.patch.object(source_io.zipfile, "ZipFile", side_effect=AssertionError("must not construct index")):
                    with self.assertRaises(ValueError):
                        source_io._validate_members(changed, "observed_gray")

    def test_comments_trailing_bytes_and_zip64_directory_locator_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, _ = source_fixture(folder)
            raw = source.read_bytes()
            fields = list(struct.unpack("<4s4H2IH", raw[-22:]))
            commented = fields.copy()
            commented[7] = 4
            located = fields.copy()
            located[5] += 20
            cases = [b"", raw + b"x", raw[:-22] + struct.pack("<4s4H2IH", *commented) + b"note",
                     raw[:-22] + b"PK\x06\x07" + bytes(16) + struct.pack("<4s4H2IH", *located)]
            for changed in cases:
                with mock.patch.object(source_io.zipfile, "ZipFile", side_effect=AssertionError("must not construct index")):
                    with self.assertRaises(ValueError):
                        source_io._validate_members(changed, "observed_gray")

    def test_directory_byte_limit_is_inclusive(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, _ = source_fixture(folder)
            raw = source.read_bytes()
            size = struct.unpack("<4s4H2IH", raw[-22:])[5]
            with mock.patch.object(source_io, "MAX_CENTRAL_DIRECTORY_BYTES", size):
                source_io._validate_members(raw, "observed_gray")
            with mock.patch.object(source_io, "MAX_CENTRAL_DIRECTORY_BYTES", size - 1):
                with mock.patch.object(source_io.zipfile, "ZipFile", side_effect=AssertionError("must not construct index")):
                    with self.assertRaises(ValueError):
                        source_io._validate_members(raw, "observed_gray")

    def test_actual_directory_count_must_match_declared_count(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, _ = source_fixture(folder, ambiguity=True)
            raw = source.read_bytes()
            fields = list(struct.unpack("<4s4H2IH", raw[-22:]))
            fields[3] = fields[4] = 2
            changed = raw[:-22] + struct.pack("<4s4H2IH", *fields)
            with self.assertRaisesRegex(ValueError, "member count differs"):
                source_io._validate_members(changed, "observed_gray")

    def test_numpy_stored_and_deflated_with_local_zip64_extra_are_compatible(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, gray, mask, metadata = source_fixture(folder)
            for writer, method in [(np.savez, zipfile.ZIP_STORED),
                                   (np.savez_compressed, zipfile.ZIP_DEFLATED)]:
                writer(source, observed_gray=gray, mask=mask, threshold_ambiguity=np.zeros_like(mask))
                raw = source.read_bytes()
                name_length, extra_length = struct.unpack_from("<HH", raw, 26)
                extra_at = 30 + name_length
                self.assertGreaterEqual(extra_length, 4)
                self.assertEqual(struct.unpack_from("<H", raw, extra_at)[0], 1)
                with zipfile.ZipFile(io.BytesIO(raw)) as archive:
                    self.assertEqual({item.compress_type for item in archive.infolist()}, {method})
                metadata["output_sha256"] = hashlib.sha256(raw).hexdigest()
                update_metadata(source, metadata)
                actual_gray, actual_mask, _, _, _ = self.load(source)
                np.testing.assert_array_equal(actual_gray, gray)
                np.testing.assert_array_equal(actual_mask, mask)

    def test_unsupported_compression_rejected_before_any_member_open(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, _ = source_fixture(folder)
            raw = bytearray(source.read_bytes())
            directory_at = struct.unpack_from("<4s4H2IH", raw, len(raw) - 22)[6]
            struct.pack_into("<H", raw, directory_at + 10, zipfile.ZIP_LZMA)
            with mock.patch.object(source_io.zipfile.ZipFile, "open", side_effect=AssertionError("must not open codec")):
                with self.assertRaisesRegex(ValueError, "compression method"):
                    source_io._validate_members(bytes(raw), "observed_gray")

    def test_missing_selected_member_is_not_guessed(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, metadata = source_fixture(folder)
            entries = [(name, raw) for name, raw in archive_entries(source) if name != "observed_gray.npy"]
            replace_archive(source, entries, metadata)
            with self.assertRaisesRegex(ValueError, "NPZ member"):
                self.load(source)

    def test_npy_dtype_and_huge_or_wrong_shape_rejected_before_allocation(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, metadata = source_fixture(folder)
            entries = archive_entries(source)
            masks = [(name, raw) for name, raw in entries if name == "mask.npy"]
            bad = [fake_npy((288, 120, 200), np.float64),
                   fake_npy((288, 120, 200), object),
                   fake_npy((10**15, 120, 200), np.uint8),
                   fake_npy((288, 120, 201), np.uint8)]
            for raw in bad:
                replace_archive(source, [("observed_gray.npy", raw)] + masks, metadata)
                with mock.patch.object(source_io.np, "load", side_effect=AssertionError("must not allocate")):
                    with self.assertRaisesRegex(ValueError, "NPY header"):
                        self.load(source)

    def test_optional_ambiguity_header_is_checked_not_silently_ignored(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, metadata = source_fixture(folder)
            entries = archive_entries(source)
            replace_archive(source, entries + [("threshold_ambiguity.npy", fake_npy((288, 120, 200), np.uint8))], metadata)
            with self.assertRaisesRegex(ValueError, "NPY header"):
                self.load(source)

    def test_truncated_trailing_and_unknown_version_npy_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            source, _, _, _, metadata = source_fixture(folder)
            entries = archive_entries(source)
            name, original = entries[0]
            for raw in [original[:-1], original + b"x", original[:6] + b"\3\0" + original[8:]]:
                changed = [(name, raw)] + entries[1:]
                replace_archive(source, changed, metadata)
                with self.assertRaises(ValueError):
                    self.load(source)

    def test_bounded_snapshot_reads_limit_plus_one_once_before_rejecting(self):
        stream = mock.Mock()
        stream.read.return_value = b"x" * 9
        with mock.patch.object(Path, "open") as opened:
            opened.return_value.__enter__.return_value = stream
            with self.assertRaisesRegex(ValueError, "snapshot exceeds"):
                source_io.read_bounded_snapshot(Path("growing.npz"), 8)
            opened.assert_called_once_with("rb")
            stream.read.assert_called_once_with(9)

    def test_bounded_snapshot_accepts_exact_limit_without_extra_read(self):
        stream = mock.Mock()
        stream.read.return_value = b"x" * 8
        with mock.patch.object(Path, "open") as opened:
            opened.return_value.__enter__.return_value = stream
            self.assertEqual(source_io.read_bounded_snapshot(Path("exact.npz"), 8), b"x" * 8)
            stream.read.assert_called_once_with(9)

    def test_shape_snapshot_is_limited_before_decode_and_output(self):
        with tempfile.TemporaryDirectory() as folder:
            source, shape, _, _, _ = source_fixture(folder)
            original_read = source_io.read_bounded_snapshot
            out = Path(folder) / "export"

            def reject_shape(path, limit):
                if path == shape:
                    self.assertEqual(limit, pack.MAX_ASSET_BYTES)
                    raise ValueError("simulated oversized shape snapshot")
                return original_read(path, limit)

            with mock.patch.object(source_io, "read_bounded_snapshot", reject_shape):
                with mock.patch.object(pack.shape_codec, "decode", side_effect=AssertionError("must not allocate")):
                    with self.assertRaisesRegex(ValueError, "oversized shape"):
                        pack.pack_tone(source, shape, out, kind="observed", profile="reference-v1")
            self.assertFalse(out.exists())

    def test_input_snapshots_are_read_once_and_remain_consistent_after_replacement(self):
        with tempfile.TemporaryDirectory() as folder:
            source, shape, gray, mask, _ = source_fixture(folder)
            sidecar = source.with_suffix(".json")
            snapshots = {path: path.read_bytes() for path in (source, sidecar, shape)}
            calls = {path: 0 for path in snapshots}
            expected = pack.encode_tone(snapshots[shape], gray, mask)
            original_read = source_io.read_bounded_snapshot

            def read_once(path, limit):
                if path in snapshots:
                    calls[path] += 1
                    if calls[path] > 1:
                        raise AssertionError("Input read more than once")
                    snapshot = original_read(path, limit)
                    path.write_bytes(b"changed after captured snapshot")
                    return snapshot
                return original_read(path, limit)

            out = Path(folder) / "export"
            with mock.patch.object(source_io, "read_bounded_snapshot", read_once):
                report = pack.pack_tone(source, shape, out, kind="observed", profile="reference-v1")
            self.assertEqual(list(calls.values()), [1, 1, 1])
            self.assertEqual((out / "tone.ght").read_bytes(), expected)
            for key, path in [("source_snapshot_sha256", source), ("source_sidecar_sha256", sidecar), ("shape_sha256", shape)]:
                self.assertEqual(report[key], hashlib.sha256(snapshots[path]).hexdigest())

    def test_accepted_mask_or_input_tone_label_does_not_adopt_export(self):
        with tempfile.TemporaryDirectory() as folder:
            source, shape, _, _, metadata = source_fixture(folder)
            metadata.update(mask_status="accepted", artifact_adoption="accepted", tone_adoption="accepted")
            update_metadata(source, metadata)
            report = pack.pack_tone(source, shape, Path(folder) / "export", kind="observed", profile="reference-v1")
            self.assertEqual(report["source_mask_status"], "accepted")
            self.assertEqual(report["artifact_adoption"], "not_assessed")
            self.assertNotIn("tone_adoption", report)

    def test_existing_directories_and_files_are_never_overwritten(self):
        with tempfile.TemporaryDirectory() as folder:
            source, shape, _, _, _ = source_fixture(folder)
            for name, content in [("empty", None), ("mixed", b"keep")]:
                out = Path(folder) / name
                out.mkdir()
                if content is not None:
                    (out / "old.ght").write_bytes(content)
                with self.assertRaises(FileExistsError):
                    pack.pack_tone(source, shape, out, kind="observed", profile="reference-v1")
                self.assertFalse((out / "tone.ght").exists())
                if content is not None:
                    self.assertEqual((out / "old.ght").read_bytes(), content)

    def test_invalid_mask_or_shape_fps_creates_no_output(self):
        with tempfile.TemporaryDirectory() as folder:
            source, shape, _, mask, _ = source_fixture(folder)
            for changed, fps in [(np.roll(mask, 1, axis=2), 24), (mask, 30)]:
                shape.write_bytes(pack.shape_codec.encode(changed, fps=fps))
                out = Path(folder) / "export"
                with self.assertRaises(ValueError):
                    pack.pack_tone(source, shape, out, kind="observed", profile="reference-v1")
                self.assertFalse(out.exists())

    def test_report_failure_leaves_incomplete_directory_not_reused(self):
        with tempfile.TemporaryDirectory() as folder:
            source, shape, _, _, _ = source_fixture(folder)
            out = Path(folder) / "export"
            original_open = Path.open

            def fail_report(path, *args, **kwargs):
                if path == out / "tone.json":
                    raise OSError("simulated report failure")
                return original_open(path, *args, **kwargs)

            with mock.patch.object(Path, "open", fail_report):
                with self.assertRaises(OSError):
                    pack.pack_tone(source, shape, out, kind="observed", profile="reference-v1")
            before = (out / "tone.ght").read_bytes()
            self.assertFalse((out / "tone.json").exists())
            with self.assertRaises(FileExistsError):
                pack.pack_tone(source, shape, out, kind="observed", profile="reference-v1")
            self.assertEqual((out / "tone.ght").read_bytes(), before)

    def test_nonfinite_generation_exponent_rejected_before_output_dir(self):
        with tempfile.TemporaryDirectory() as folder:
            source, shape, _, _, metadata = source_fixture(folder, "authored")
            metadata["generation"]["unrepresentable"] = "NONFINITE_PLACEHOLDER"
            raw = json.dumps(metadata).replace('"NONFINITE_PLACEHOLDER"', "1e999")
            source.with_suffix(".json").write_text(raw)
            out = Path(folder) / "export"
            with self.assertRaises(ValueError):
                pack.pack_tone(source, shape, out, kind="authored", profile="reference-v1")
            self.assertFalse(out.exists())

    def test_cli_requires_both_role_and_profile(self):
        for options in [[], ["--kind", "observed"], ["--profile", "reference-v1"]]:
            with mock.patch("sys.argv", ["pack_tone.py", "a.npz", "b.ghs", "out", *options]):
                with mock.patch("sys.stderr", io.StringIO()):
                    with self.assertRaises(SystemExit) as failure:
                        pack.main()
                    self.assertEqual(failure.exception.code, 2)


if __name__ == "__main__":
    unittest.main()
