"""Build an immutable Q1 authority from pinned upstream evaluation entries.

This module intentionally accepts an already extracted six-file slice.  It
does not claim to have verified the unavailable 7 GiB archive as a whole; the
receipt distinguishes the official HTTP object identity from the byte-exact
entries that were actually retained and checked.
"""

from __future__ import annotations

import ctypes
import dataclasses
import errno
import hashlib
import json
import os
import pathlib
import shutil
import struct
import sys
import tempfile
import zlib
from typing import Any


SCHEMA = "gsplat-q1-product-quality-evaluation-authority/v1"
AUTHORITY_CLASS = "upstream_evaluation_images"
ARCHIVE_URL = (
    "https://repo-sam.inria.fr/fungraph/3d-gaussian-splatting/"
    "evaluation/images.zip"
)
ARCHIVE_HTTP_BYTES = 7_064_286_140
EXPECTED_PNG = (979, 546, 8, 2, 0)
MAX_RECEIPT_BYTES = 64 * 1024


class EvaluationAuthorityError(ValueError):
    pass


@dataclasses.dataclass(frozen=True)
class EntrySpec:
    entry: str
    local_path: str
    bytes: int
    sha256: str
    media_type: str


OFFICIAL_ENTRIES = (
    EntrySpec(
        "truck/results.json",
        "results.json",
        690,
        "258da6be7ebeb2baf8152448704f46c6e09651e9c4b9d97b5d75ba6b15b82c73",
        "application/json",
    ),
    EntrySpec(
        "truck/per_view.json",
        "per_view.json",
        21_670,
        "a1977057d7d7db4cfd522201d413b876dfbca70f740d61e2f649f9ed9d49191f",
        "application/json",
    ),
    EntrySpec(
        "truck/test/ours_30000/gt/000001.png",
        "gt/000001.png",
        927_433,
        "3ef29744b6eee63ff916d3492d128dc6b469d590d295283825e8ca6444740cbf",
        "image/png",
    ),
    EntrySpec(
        "truck/test/ours_30000/gt/000009.png",
        "gt/000009.png",
        938_895,
        "59c990681da77f9a78dbdf6cb006100ddca2ecf1d6a1641b96373448320e1e2f",
        "image/png",
    ),
    EntrySpec(
        "truck/test/ours_30000/renders/000001.png",
        "renders/000001.png",
        824_420,
        "04a47e072b90fae759b18760d0e4fa62f8fc6129f43563f44c76fdade2266cfe",
        "image/png",
    ),
    EntrySpec(
        "truck/test/ours_30000/renders/000009.png",
        "renders/000009.png",
        837_130,
        "95547f65614f9084a74c7598e9120120f6ed810f6dd86ca380a98167a154a46c",
        "image/png",
    ),
)


def fail(message: str) -> None:
    raise EvaluationAuthorityError(message)


def canonical_json(value: Any) -> str:
    try:
        return json.dumps(
            value, allow_nan=False, ensure_ascii=False, separators=(",", ":"), sort_keys=True
        )
    except (TypeError, ValueError) as error:
        fail(f"authority is not canonical JSON: {error}")


def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            fail(f"JSON repeats key {key!r}")
        result[key] = value
    return result


def load_unique_json(path: pathlib.Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"{path.name}: invalid JSON: {error}")


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def validate_specs(entries: tuple[EntrySpec, ...]) -> None:
    archive_paths: set[str] = set()
    local_paths: set[str] = set()
    for spec in entries:
        entry = pathlib.PurePosixPath(spec.entry)
        local = pathlib.PurePosixPath(spec.local_path)
        if (
            not spec.entry
            or not spec.local_path
            or entry.is_absolute()
            or local.is_absolute()
            or ".." in entry.parts
            or ".." in local.parts
            or "." in entry.parts
            or "." in local.parts
            or "\\" in spec.entry
            or "\\" in spec.local_path
            or any(ord(character) < 32 for character in spec.entry + spec.local_path)
        ):
            fail("entry spec contains a path escape")
        if spec.entry in archive_paths or spec.local_path in local_paths:
            fail("entry spec contains a duplicate path")
        archive_paths.add(spec.entry)
        local_paths.add(spec.local_path)
        if (
            spec.bytes <= 0
            or len(spec.sha256) != 64
            or any(character not in "0123456789abcdef" for character in spec.sha256)
            or spec.media_type not in {"application/json", "image/png"}
        ):
            fail("entry spec contains an invalid byte identity")


def checked_path(root: pathlib.Path, spec: EntrySpec) -> pathlib.Path:
    relative = pathlib.PurePosixPath(spec.local_path)
    cursor = root
    if root.is_symlink() or not root.is_dir():
        fail("source directory must be a real directory")
    for part in relative.parts:
        cursor = cursor / part
        if cursor.is_symlink():
            fail(f"source path contains a symlink: {spec.local_path}")
    if not cursor.is_file():
        fail(f"source entry is missing: {spec.local_path}")
    try:
        cursor.resolve(strict=True).relative_to(root.resolve(strict=True))
    except (OSError, ValueError):
        fail(f"source path escapes root: {spec.local_path}")
    if cursor.stat().st_size != spec.bytes or sha256(cursor) != spec.sha256:
        fail(f"source entry byte identity drifted: {spec.local_path}")
    return cursor


def inspect_rgb8_png(path: pathlib.Path) -> dict[str, int]:
    data = path.read_bytes()
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        fail(f"{path.name}: invalid PNG signature")
    offset = 8
    chunks: list[bytes] = []
    ihdr: tuple[int, int, int, int, int] | None = None
    while offset + 12 <= len(data):
        length = struct.unpack_from(">I", data, offset)[0]
        kind = data[offset + 4 : offset + 8]
        end = offset + 12 + length
        if end > len(data):
            fail(f"{path.name}: truncated PNG chunk")
        payload = data[offset + 8 : offset + 8 + length]
        expected_crc = struct.unpack_from(">I", data, offset + 8 + length)[0]
        if zlib.crc32(kind + payload) & 0xFFFFFFFF != expected_crc:
            fail(f"{path.name}: PNG CRC mismatch")
        chunks.append(kind)
        if len(chunks) == 1:
            if kind != b"IHDR" or length != 13:
                fail(f"{path.name}: PNG must start with IHDR")
            width, height, bit_depth, color_type, compression, filtering, interlace = struct.unpack(
                ">IIBBBBB", payload
            )
            if compression != 0 or filtering != 0:
                fail(f"{path.name}: unsupported PNG compression or filter method")
            ihdr = (width, height, bit_depth, color_type, interlace)
        offset = end
        if kind == b"IEND":
            break
    if offset != len(data) or not chunks or chunks[-1] != b"IEND" or b"IDAT" not in chunks:
        fail(f"{path.name}: incomplete PNG")
    if ihdr != EXPECTED_PNG:
        fail(
            f"{path.name}: expected 979x546 non-interlaced RGB8, got {ihdr!r}"
        )
    return {
        "width": ihdr[0],
        "height": ihdr[1],
        "bit_depth": ihdr[2],
        "color_type": ihdr[3],
        "interlace": ihdr[4],
    }


def validate_metrics(results_path: pathlib.Path, per_view_path: pathlib.Path) -> dict[str, Any]:
    results = load_unique_json(results_path)
    per_view = load_unique_json(per_view_path)
    try:
        aggregate = results["ours_30000"]
        per_view_method = per_view["ours_30000"]
        selected = {
            name: {
                metric.lower(): float(per_view_method[metric][f"{name}.png"])
                for metric in ("SSIM", "PSNR", "LPIPS")
            }
            for name in ("000001", "000009")
        }
        aggregate_selected = {
            metric.lower(): float(aggregate[metric])
            for metric in ("SSIM", "PSNR", "LPIPS")
        }
    except (KeyError, TypeError, ValueError) as error:
        fail(f"evaluation metrics are incomplete: {error}")
    return {"method": "ours_30000", "aggregate": aggregate_selected, "views": selected}


def publish_directory_noreplace(staging: pathlib.Path, output: pathlib.Path) -> None:
    libc = ctypes.CDLL(None, use_errno=True)
    source = os.fsencode(staging)
    destination = os.fsencode(output)
    if sys.platform == "darwin" and hasattr(libc, "renamex_np"):
        function = libc.renamex_np
        function.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
        function.restype = ctypes.c_int
        result = function(source, destination, 0x00000004)  # RENAME_EXCL
    elif sys.platform.startswith("linux") and hasattr(libc, "renameat2"):
        function = libc.renameat2
        function.argtypes = [
            ctypes.c_int,
            ctypes.c_char_p,
            ctypes.c_int,
            ctypes.c_char_p,
            ctypes.c_uint,
        ]
        function.restype = ctypes.c_int
        result = function(-100, source, -100, destination, 0x00000001)
    else:
        fail("atomic no-replace directory publication is unsupported")
    if result == 0:
        return
    error = ctypes.get_errno()
    if error in {errno.EEXIST, errno.ENOTEMPTY}:
        fail(f"output already exists: {output}")
    raise OSError(error, os.strerror(error), output)


def authority_receipt(source_dir: pathlib.Path, entries: tuple[EntrySpec, ...]) -> dict[str, Any]:
    validate_specs(entries)
    checked = {spec.local_path: checked_path(source_dir, spec) for spec in entries}
    pngs = {
        spec.local_path: inspect_rgb8_png(checked[spec.local_path])
        for spec in entries
        if spec.media_type == "image/png"
    }
    metrics = validate_metrics(checked["results.json"], checked["per_view.json"])
    return {
        "schema": SCHEMA,
        "authority_class": AUTHORITY_CLASS,
        "scene": "truck",
        "archive": {
            "url": ARCHIVE_URL,
            "http_content_length_bytes": ARCHIVE_HTTP_BYTES,
            "local_archive_present": False,
            "archive_sha256_verified": False,
            "verification_scope": "pinned_extracted_entries_only",
        },
        "entries": [
            {
                "archive_entry": spec.entry,
                "source_relative_path": spec.local_path,
                "retained_relative_path": f"source/{spec.local_path}",
                "bytes": spec.bytes,
                "sha256": spec.sha256,
                "media_type": spec.media_type,
                **({"png": pngs[spec.local_path]} if spec.media_type == "image/png" else {}),
            }
            for spec in entries
        ],
        "published_baseline": metrics,
        "qualification": {
            "product_quality": "Deferred",
            "performance_authorized": False,
            "endpoint_output": False,
            "self_generated_authority": False,
        },
    }


def validate_authority(
    root: pathlib.Path,
    entries: tuple[EntrySpec, ...] = OFFICIAL_ENTRIES,
) -> dict[str, Any]:
    if root.is_symlink() or not root.is_dir():
        fail("authority root must be a real directory")
    expected_paths = {"authority.json", *(f"source/{spec.local_path}" for spec in entries)}
    actual_paths: set[str] = set()
    for path in root.rglob("*"):
        if path.is_symlink():
            fail("authority tree contains a symlink")
        if path.is_file():
            actual_paths.add(path.relative_to(root).as_posix())
    if actual_paths != expected_paths:
        fail("authority tree file set does not match the pinned receipt")
    receipt_path = root / "authority.json"
    if receipt_path.is_symlink() or not receipt_path.is_file():
        fail("authority.json is missing or a symlink")
    if receipt_path.stat().st_size > MAX_RECEIPT_BYTES:
        fail("authority.json exceeds the bounded receipt size")
    receipt = load_unique_json(receipt_path)
    if receipt.get("schema") != SCHEMA or receipt.get("authority_class") != AUTHORITY_CLASS:
        fail("authority schema or class mismatch")
    if receipt.get("authority_class") in {"endpoint", "self_generated", "endpoint_generated"}:
        fail("endpoint/self-generated authority is forbidden")
    expected = authority_receipt(root / "source", entries)
    if canonical_json(receipt) != canonical_json(expected):
        fail("authority receipt does not match retained pinned entries")
    return receipt


def build_authority(
    source_dir: pathlib.Path,
    output: pathlib.Path,
    entries: tuple[EntrySpec, ...] = OFFICIAL_ENTRIES,
) -> dict[str, Any]:
    source_dir = pathlib.Path(os.path.abspath(source_dir))
    output = pathlib.Path(os.path.abspath(output))
    if output.exists() or output.is_symlink():
        fail(f"output already exists: {output}")
    parent = output.parent
    if parent.is_symlink() or not parent.is_dir():
        fail("output parent must be a real existing directory")
    receipt = authority_receipt(source_dir, entries)
    staging = pathlib.Path(tempfile.mkdtemp(prefix=f".{output.name}.staging-", dir=parent))
    try:
        for spec in entries:
            source = checked_path(source_dir, spec)
            destination = staging / "source" / spec.local_path
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination, follow_symlinks=False)
        (staging / "authority.json").write_text(
            canonical_json(receipt) + "\n", encoding="utf-8"
        )
        validate_authority(staging, entries)
        publish_directory_noreplace(staging, output)
        validate_authority(output, entries)
        return receipt
    finally:
        if staging.exists():
            shutil.rmtree(staging)
