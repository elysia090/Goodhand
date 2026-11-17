#!/usr/bin/env python3
"""Contract tests for the offline occupancy packer. No project artwork required."""
import importlib.util, pathlib, struct, unittest
import contextlib, hashlib, io, json, tempfile
from types import SimpleNamespace
import numpy as np
from unittest import mock
spec=importlib.util.spec_from_file_location("pack",pathlib.Path(__file__).with_name("pack_sequence.py"))
pack=importlib.util.module_from_spec(spec);spec.loader.exec_module(pack)

def raw_frame(payload,width=5,height=1):
    return pack.HEADER.pack(b"GHSH",1,width,height,24,1)+struct.pack("<II",0,len(payload))+bytes(payload)

class ShapeContract(unittest.TestCase):
    def test_round_trip_random(self):
        random=np.random.default_rng(9)
        for dims in ((1,1,1),(3,7,17),(288,2,8),(2,3,255)):
            source=random.random(dims)>.6
            output,fps=pack.decode(pack.encode(source))
            np.testing.assert_array_equal(source,output);self.assertEqual(fps,24)
    def test_empty_and_full(self):
        for value in (False,True):
            source=np.full((2,3,5),value,dtype=bool)
            output,_=pack.decode(pack.encode(source));np.testing.assert_array_equal(source,output)
    def test_occupancy_has_no_tone(self):
        source=np.array([[[True,False,True]]])
        output,_=pack.decode(pack.encode(source));self.assertEqual(output.dtype,np.bool_)
        np.testing.assert_array_equal(source,output)
    def test_bad_dimensions(self):
        for dims in ((0,2,2),(1,0,2),(1,2,0),(1,256,2),(1,2,256),(4097,1,1)):
            with self.assertRaises(ValueError):pack.encode(np.zeros(dims,dtype=bool))
    def test_bad_clock(self):
        for fps in (0,1001):
            with self.assertRaises(ValueError):pack.encode(np.zeros((1,1,1)),fps)
    def test_nonboolean_rejected(self):
        with self.assertRaises(ValueError):pack.encode(np.ones((1,1,1),dtype=np.uint8))
    def test_invalid_frame_validated_before_allocation(self):
        with mock.patch.object(pack.np,"zeros",side_effect=AssertionError("allocation before validation")):
            with self.assertRaises(ValueError):pack.decode(raw_frame([1,0,0]))
    def test_runtime_asset_limit(self):
        with mock.patch.object(pack,"MAX_ASSET_BYTES",25):
            with self.assertRaises(ValueError):pack.encode(np.ones((1,1,5),dtype=bool))
            with self.assertRaises(ValueError):pack.decode(bytes(26))
    def test_candidate_gate_and_provenance(self):
        with tempfile.TemporaryDirectory() as folder:
            source=pathlib.Path(folder)/"observations.npz"
            np.savez_compressed(source,mask=np.ones((1,1,5),dtype=bool))
            metadata={"mask_status":"observed_candidate",
                      "output_sha256":hashlib.sha256(source.read_bytes()).hexdigest()}
            source.with_suffix(".json").write_text(json.dumps(metadata))
            destination=pathlib.Path(folder)/"shape.ghs"
            args=SimpleNamespace(observations=source,destination=destination,candidate=False)
            with mock.patch.object(pack.argparse.ArgumentParser,"parse_args",return_value=args):
                with self.assertRaises(ValueError):pack.main()
            self.assertFalse(destination.exists())
            args.candidate=True
            with mock.patch.object(pack.argparse.ArgumentParser,"parse_args",return_value=args):
                with contextlib.redirect_stdout(io.StringIO()):pack.main()
            report=json.loads(destination.with_suffix(".json").read_text())
            self.assertEqual(report["mask_status"],"observed_candidate")
            self.assertEqual(report["input_observations_sha256"],metadata["output_sha256"])
    def test_changed_observations_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            source=pathlib.Path(folder)/"observations.npz"
            np.savez_compressed(source,mask=np.ones((1,1,5),dtype=bool))
            source.with_suffix(".json").write_text(json.dumps({"output_sha256":"wrong"}))
            destination=pathlib.Path(folder)/"shape.ghs"
            args=SimpleNamespace(observations=source,destination=destination,candidate=True)
            with mock.patch.object(pack.argparse.ArgumentParser,"parse_args",return_value=args):
                with self.assertRaises(ValueError):pack.main()
            self.assertFalse(destination.exists())
    def test_cli_uses_same_snapshot_for_hash_mask_and_ambiguity(self):
        with tempfile.TemporaryDirectory() as folder:
            source = pathlib.Path(folder) / "observations.npz"
            destination = pathlib.Path(folder) / "shape.ghs"
            original_mask = np.array([[[True, False, False]]])
            changed_mask = np.array([[[False, True, False]]])
            np.savez_compressed(source, mask=original_mask,
                                threshold_ambiguity=np.ones(original_mask.shape, dtype=bool))
            snapshot = source.read_bytes()
            metadata = {"mask_status": "observed_candidate",
                        "output_sha256": hashlib.sha256(snapshot).hexdigest()}
            source.with_suffix(".json").write_text(json.dumps(metadata))
            changed = io.BytesIO()
            np.savez_compressed(changed, mask=changed_mask,
                                threshold_ambiguity=np.zeros(changed_mask.shape, dtype=bool))
            changed_bytes = changed.getvalue()
            args = SimpleNamespace(observations=source, destination=destination, candidate=True)
            original_read = pathlib.Path.read_bytes
            original_load = pack.np.load
            reads, contexts = [], []

            def read_then_replace(path):
                if path == source:
                    reads.append(path)
                    if len(reads) > 1:
                        raise AssertionError("Source must be read once")
                    captured = original_read(path)
                    path.write_bytes(changed_bytes)
                    return captured
                return original_read(path)

            def load_snapshot(value, *positional, **keywords):
                self.assertIsInstance(value, io.BytesIO)
                self.assertEqual(value.getvalue(), snapshot)
                self.assertEqual(keywords, {"allow_pickle": False})
                archive = original_load(value, *positional, **keywords)
                context = mock.MagicMock()
                context.__enter__.return_value = archive
                context.__exit__.side_effect = archive.__exit__
                contexts.append(context)
                return context

            with mock.patch.object(pack.argparse.ArgumentParser, "parse_args", return_value=args), \
                    mock.patch.object(pathlib.Path, "read_bytes", read_then_replace), \
                    mock.patch.object(pack.np, "load", side_effect=load_snapshot), \
                    contextlib.redirect_stdout(io.StringIO()):
                pack.main()
            recovered, fps = pack.decode(destination.read_bytes())
            np.testing.assert_array_equal(recovered, original_mask)
            self.assertEqual(fps, 24)
            self.assertEqual(len(reads), 1)
            self.assertEqual(len(contexts), 1)
            contexts[0].__exit__.assert_called_once_with(None, None, None)
            report = json.loads(destination.with_suffix(".json").read_text())
            self.assertEqual(report["input_observations_sha256"], metadata["output_sha256"])
            self.assertEqual(report["threshold_ambiguity_cells"], 3)
            self.assertEqual(source.read_bytes(), changed_bytes)

    def test_mask_only_cli_without_sidecar_keeps_generic_dimensions(self):
        for mask in [np.ones((1, 1, 5), dtype=bool),
                     np.arange(42).reshape(2, 3, 7) % 2 == 0]:
            with tempfile.TemporaryDirectory() as folder:
                source = pathlib.Path(folder) / "mask-only.npz"
                destination = pathlib.Path(folder) / "shape.ghs"
                np.savez_compressed(source, mask=mask)
                args = SimpleNamespace(observations=source, destination=destination, candidate=True)
                with mock.patch.object(pack.argparse.ArgumentParser, "parse_args", return_value=args), \
                        contextlib.redirect_stdout(io.StringIO()):
                    pack.main()
                recovered, fps = pack.decode(destination.read_bytes())
                np.testing.assert_array_equal(recovered, mask)
                self.assertEqual(fps, 24)

    def test_cli_closes_npz_context_when_array_lookup_fails(self):
        with tempfile.TemporaryDirectory() as folder:
            source = pathlib.Path(folder) / "missing-mask.npz"
            destination = pathlib.Path(folder) / "shape.ghs"
            np.savez_compressed(source, not_mask=np.zeros((1, 1, 3), dtype=bool))
            args = SimpleNamespace(observations=source, destination=destination, candidate=True)
            original_load = pack.np.load
            contexts = []

            def load_snapshot(value, *positional, **keywords):
                self.assertIsInstance(value, io.BytesIO)
                self.assertEqual(keywords, {"allow_pickle": False})
                archive = original_load(value, *positional, **keywords)
                context = mock.MagicMock()
                context.__enter__.return_value = archive
                context.__exit__.side_effect = archive.__exit__
                contexts.append(context)
                return context

            with mock.patch.object(pack.argparse.ArgumentParser, "parse_args", return_value=args), \
                    mock.patch.object(pack.np, "load", side_effect=load_snapshot):
                with self.assertRaises(KeyError):
                    pack.main()
            self.assertEqual(len(contexts), 1)
            contexts[0].__exit__.assert_called_once()
            self.assertIs(contexts[0].__exit__.call_args.args[0], KeyError)
            self.assertFalse(destination.exists())
    def test_bad_shape_rank(self):
        with self.assertRaises(ValueError):pack.encode(np.zeros((2,2)))
    def test_short_header(self):
        for size in range(16):
            with self.assertRaises(ValueError):pack.decode(bytes(size))
    def test_magic_version(self):
        source=bytearray(raw_frame([0]))
        for offset,value in ((0,0),(4,2)):
            bad=source.copy();bad[offset]=value
            with self.assertRaises(ValueError):pack.decode(bad)
    def test_truncated_offsets(self):
        source=raw_frame([0])
        with self.assertRaises(ValueError):pack.decode(source[:20])
    def test_invalid_extent(self):
        source=bytearray(raw_frame([0]))
        for offset,value in ((16,1),(20,2)):
            bad=source.copy();struct.pack_into("<I",bad,offset,value)
            with self.assertRaises(ValueError):pack.decode(bad)
    def test_truncated_rows_runs(self):
        for payload in ([],[1],[1,0]):
            with self.assertRaises(ValueError):pack.decode(raw_frame(payload))
    def test_noncanonical_runs(self):
        for payload in ([1,0,0],[1,4,2],[2,0,2,1,1],[2,0,2,2,1],[2,3,1,0,1]):
            with self.assertRaises(ValueError):pack.decode(raw_frame(payload))
    def test_trailing_frame_data(self):
        with self.assertRaises(ValueError):pack.decode(raw_frame([0,0]))
    def test_nonmonotone_offsets(self):
        source=bytearray(pack.encode(np.zeros((2,1,1),dtype=bool)))
        struct.pack_into("<I",source,20,3)
        with self.assertRaises(ValueError):pack.decode(source)

if __name__=="__main__":unittest.main()

