import argparse
import hashlib
import os
import struct
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from extract_linux_artwork import extract_input
from package import VERSION, executable_target, linux_archive, local_smoke


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


class LocalSmoke(unittest.TestCase):
    def test_current_default_runs_one_45_second_turn(self):
        results = [
            argparse.Namespace(stdout=f"oops {VERSION}\n"),
            argparse.Namespace(stdout="Desktop: original 288-pose grayscale artwork."),
            argparse.Namespace(stdout=""),
        ]
        with patch("package.subprocess.run", side_effect=results) as run:
            with patch("package.time.monotonic", side_effect=[10, 55]):
                local_smoke(Path("oops"))
            self.assertEqual(run.call_args.args[0], ["oops", "--once"])

    def test_previous_duration_is_rejected_even_with_current_version(self):
        results = [
            argparse.Namespace(stdout=f"oops {VERSION}\n"),
            argparse.Namespace(stdout="Desktop: original 288-pose grayscale artwork."),
            argparse.Namespace(stdout=""),
        ]
        with patch("package.subprocess.run", side_effect=results):
            with patch("package.time.monotonic", side_effect=[10, 40]):
                with self.assertRaisesRegex(ValueError, "45-second"):
                    local_smoke(Path("oops"))


    def test_old_artwork_build_is_rejected_even_with_current_version(self):
        results = [
            argparse.Namespace(stdout=f"oops {VERSION}\n"),
            argparse.Namespace(stdout="Desktop: reviewed W5 native artwork."),
        ]
        with patch("package.subprocess.run", side_effect=results):
            with self.assertRaisesRegex(ValueError, "exact grayscale baseline"):
                local_smoke(Path("oops"), launch_window=False)


class LinuxArchive(unittest.TestCase):
    def test_permissions_and_digest_are_independent_of_build_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            stage = root / f"oops-v{VERSION}-linux-x64"
            stage.mkdir(mode=0o700)
            (stage / "oops").write_bytes(b"executable")
            doc = stage / "LICENSE"
            doc.write_text("license", encoding="ascii")
            first, second = root / "first.tar.gz", root / "second.tar.gz"
            linux_archive(stage, first)
            os.utime(doc, (9, 9))
            if os.name != "nt":
                stage.chmod(0o755)
                doc.chmod(0o600)
            linux_archive(stage, second)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            with tarfile.open(first) as archive:
                self.assertEqual(archive.getmember(stage.name).mode, 0o755)
                self.assertEqual(archive.getmember(f"{stage.name}/oops").mode, 0o755)
                notice = archive.getmember(f"{stage.name}/LICENSE")
                self.assertEqual(notice.mode, 0o644)
                self.assertEqual((notice.uname, notice.gname), ("", ""))
                self.assertEqual(archive.extractfile(notice).read(), b"license")


class EmbeddedInput(unittest.TestCase):
    def test_extractor_requires_exact_unique_bytes(self):
        magic = b"GHSH"
        offsets = bytes(540 * 4) + struct.pack("<I", 5)
        raw = struct.pack("<4sHHHHI", magic, 1, 200, 120, 18, 540) + offsets + b"shape"
        approved = hashlib.sha256(raw).hexdigest()
        self.assertEqual(extract_input(b"noise" + raw + b"tail", magic, approved), raw)
        with self.assertRaisesRegex(ValueError, "missing"):
            extract_input(raw[:-1] + b"!", magic, approved)
        with self.assertRaisesRegex(ValueError, "duplicate"):
            extract_input(raw + raw, magic, approved)
