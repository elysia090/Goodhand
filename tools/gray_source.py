"""Read one bounded-profile gray source snapshot; never author or adopt tone.

Observed and authored arrays have different names. Source labels and parent hashes
are declarations, not proof that the pixels came from a particular author/video.
The NPZ profile uses a comment-free, single-disk ZIP32 directory of 2..3 entries
and at most 4KiB. Local-file ZIP64 extras used by NumPy remain supported. Only
NumPy's normal stored/deflated compression methods are accepted.
"""

import hashlib
import io
import json
from pathlib import Path
import struct
import zipfile

import numpy as np

FRAME_COUNT = 288
GRID_SHAPE = (120, 200)
CANVAS_SHAPE = (640, 1000)
PROFILE_NAME = "reference-v1"
MAX_SOURCE_BYTES = 64 * 1024 * 1024
MAX_SIDECAR_BYTES = 1024 * 1024
MAX_NPY_HEADER_BYTES = 10240
MAX_CENTRAL_DIRECTORY_BYTES = 4096
_EOCD = struct.Struct("<4s4H2IH")

REFERENCE_PROFILE = {
    "width": 1000, "height": 640, "fps": 24, "frames": FRAME_COUNT,
    "period_seconds": 12,
    "grid": {"columns": 200, "rows": 120, "pitch_px": 5,
             "dot_size_px": [2, 2], "dot_origin_px": [2, 42]},
}


def _sha256(value):
    return hashlib.sha256(value).hexdigest()


def read_bounded_snapshot(path, limit):
    """Open once and read at most limit+1 bytes, even if the file grows."""
    if limit < 0:
        raise ValueError("Invalid snapshot byte limit")
    with Path(path).open("rb") as source:
        snapshot = source.read(limit + 1)
    if len(snapshot) > limit:
        raise ValueError("Input snapshot exceeds its byte limit")
    return snapshot


def _reject_json_constant(value):
    raise ValueError(f"Non-finite JSON constant: {value}")


def _is_sha256(value):
    return (isinstance(value, str) and len(value) == 64
            and all(character in "0123456789abcdef" for character in value))


def _validate_metadata(metadata, kind, digest):
    if not isinstance(metadata, dict):
        raise ValueError("Expected a gray-source metadata object")
    if metadata.get("output_sha256") != digest:
        raise ValueError("Observation bytes do not match their provenance report")
    if any(metadata.get(key) != value for key, value in REFERENCE_PROFILE.items()):
        raise ValueError("Observation metadata does not match the fixed reference profile")
    if metadata.get("direction") != "reverse":
        raise ValueError("This reference profile requires declared reverse rotation")
    if metadata.get("mask_status") not in ("observed_candidate", "accepted"):
        raise ValueError("Missing observation/acceptance status")
    if kind == "observed":
        # Existing extraction reports predate source_kind. The caller must still
        # declare observed explicitly; never infer it from numeric gray values.
        if metadata.get("source_kind") not in (None, "observed") or "generation" in metadata:
            raise ValueError("Authored generation cannot be labelled as observed gray")
    else:
        if metadata.get("source_kind") != "authored":
            raise ValueError("Authored gray requires source_kind=authored")
        generation = metadata.get("generation")
        if not isinstance(generation, dict):
            raise ValueError("Authored gray requires a generation declaration")
        if not isinstance(generation.get("method"), str) or not generation["method"].strip():
            raise ValueError("Authored generation requires a nonempty method declaration")
        if not _is_sha256(generation.get("parent_observation_sha256")):
            raise ValueError("Authored generation requires a declared parent observation SHA256")


def _preflight_npz_index(source_bytes):
    """Bound directory materialization before ZipFile constructs ZipInfo objects.

    This is a small profile check, not ZIP validation. Comments, trailing data,
    split disks and ZIP64 directory locators are outside this input profile.
    ZipFile still validates the directory and all opened members afterwards.
    """
    if len(source_bytes) < _EOCD.size:
        raise ValueError("Missing NPZ ZIP32 directory footer")
    footer_at = len(source_bytes) - _EOCD.size
    signature, disk, directory_disk, disk_count, count, size, offset, comment = _EOCD.unpack_from(
        source_bytes, footer_at
    )
    if signature != b"PK\x05\x06" or comment != 0:
        raise ValueError("NPZ requires a final comment-free ZIP32 directory footer")
    if disk != 0 or directory_disk != 0 or disk_count != count or count not in (2, 3):
        raise ValueError("NPZ member index requires a single-disk directory with 2..3 declared members")
    if (size == 0xFFFFFFFF or offset == 0xFFFFFFFF
            or (footer_at >= 20 and source_bytes[footer_at - 20:footer_at - 16] == b"PK\x06\x07")):
        raise ValueError("ZIP64 central directories are outside the NPZ profile")
    if not 46 * count <= size <= MAX_CENTRAL_DIRECTORY_BYTES:
        raise ValueError("NPZ central directory exceeds its small index limit")
    if offset + size != footer_at:
        raise ValueError("NPZ directory extent does not end at its footer")
    return count


def _validate_members(source_bytes, gray_key):
    declared_count = _preflight_npz_index(source_bytes)
    expected_shape = (FRAME_COUNT, *GRID_SHAPE)
    required = {f"{gray_key}.npy", "mask.npy"}
    allowed = required | {"threshold_ambiguity.npy"}
    with zipfile.ZipFile(io.BytesIO(source_bytes)) as archive:
        members = archive.infolist()
        if len(members) != declared_count:
            raise ValueError("NPZ directory member count differs from its footer")
        names = [member.filename for member in members]
        if len(names) != len(set(names)):
            raise ValueError("Duplicate NPZ member")
        if not required.issubset(names) or set(names) - allowed:
            raise ValueError("Missing, unknown, or opposite-kind NPZ member")
        if any(member.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED)
               for member in members):
            raise ValueError("Unsupported NPZ compression method")
        for member in members:
            dtype = np.dtype(np.uint8 if member.filename == f"{gray_key}.npy" else np.bool_)
            data_bytes = int(np.prod(expected_shape)) * dtype.itemsize
            if not data_bytes < member.file_size <= data_bytes + MAX_NPY_HEADER_BYTES:
                raise ValueError("NPY member exceeds the fixed profile size")
            with archive.open(member) as stream:
                version = np.lib.format.read_magic(stream)
                if version == (1, 0):
                    shape, _, actual_dtype = np.lib.format.read_array_header_1_0(stream)
                elif version == (2, 0):
                    shape, _, actual_dtype = np.lib.format.read_array_header_2_0(stream)
                else:
                    raise ValueError("Unsupported NPY header version")
                if shape != expected_shape or actual_dtype != dtype or actual_dtype.hasobject:
                    raise ValueError("NPY header does not match the selected gray profile")
                if stream.tell() + data_bytes != member.file_size:
                    raise ValueError("NPY member has truncated or trailing data")


def load_gray_source(source, *, kind, profile):
    """Return gray, mask, metadata, NPZ SHA, sidecar SHA from immutable snapshots.

    No dtype conversion, thresholding, curve, or adoption decision occurs here.
    The optional ambiguity map is validated but never used to repair the mask.
    """
    if kind not in ("observed", "authored"):
        raise ValueError("Gray kind must be explicitly observed or authored")
    if profile != PROFILE_NAME:
        raise ValueError("Unknown gray-source profile")
    source = Path(source)
    if source.suffix != ".npz":
        raise ValueError("Gray source must end in .npz")
    sidecar = source.with_suffix(".json")
    sidecar_bytes = read_bounded_snapshot(sidecar, MAX_SIDECAR_BYTES)
    source_bytes = read_bounded_snapshot(source, MAX_SOURCE_BYTES)
    metadata = json.loads(sidecar_bytes, parse_constant=_reject_json_constant)
    digest = _sha256(source_bytes)
    _validate_metadata(metadata, kind, digest)
    gray_key = "observed_gray" if kind == "observed" else "authored_gray"
    _validate_members(source_bytes, gray_key)
    with np.load(io.BytesIO(source_bytes), allow_pickle=False) as data:
        gray, mask = data[gray_key], data["mask"]
        ambiguity = data["threshold_ambiguity"] if "threshold_ambiguity" in data.files else None
    expected_shape = (FRAME_COUNT, *GRID_SHAPE)
    if gray.dtype != np.uint8 or gray.shape != expected_shape:
        raise ValueError("Expected 288 frames of 120x200 uint8 gray")
    if mask.dtype != np.bool_ or mask.shape != expected_shape:
        raise ValueError("Expected an independent boolean occupancy array")
    if ambiguity is not None and (ambiguity.dtype != np.bool_ or ambiguity.shape != expected_shape):
        raise ValueError("Invalid threshold ambiguity map")
    return gray, mask, metadata, digest, _sha256(sidecar_bytes)
