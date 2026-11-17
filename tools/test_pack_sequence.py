#!/usr/bin/env python3
"""Contract tests for the offline occupancy packer. No project artwork required."""
import importlib.util, pathlib, struct, unittest
import numpy as np
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
