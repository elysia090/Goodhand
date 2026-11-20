import struct
import unittest

from package import executable_target


class Architecture(unittest.TestCase):
    def test_64_bit_linux_architecture(self):
        elf = bytearray(b"\x7fELF\x02\x01" + bytes(58))
        for cpu, code in (("x64", 62), ("arm64", 183)):
            struct.pack_into("<H", elf, 18, code)
            self.assertEqual(executable_target(elf), ("linux", cpu))
        elf[4] = 1
        with self.assertRaises(ValueError):
            executable_target(elf)

    def test_64_bit_macos_architecture(self):
        for cpu, code in (("x64", 0x1000007), ("arm64", 0x100000C)):
            macho = b"\xcf\xfa\xed\xfe" + struct.pack("<I", code)
            self.assertEqual(executable_target(macho), ("macos", cpu))

    def test_windows_rejects_console_build_and_accepts_gui_cpu(self):
        pe = bytearray(256)
        pe[:2] = b"MZ"
        struct.pack_into("<I", pe, 0x3C, 64)
        pe[64:68] = b"PE\0\0"
        struct.pack_into("<H", pe, 88, 0x20B)
        struct.pack_into("<H", pe, 156, 2)
        for cpu, code in (("x64", 0x8664), ("arm64", 0xAA64)):
            struct.pack_into("<H", pe, 68, code)
            self.assertEqual(executable_target(pe), ("windows", cpu))
        struct.pack_into("<H", pe, 156, 3)
        with self.assertRaises(ValueError):
            executable_target(pe)
