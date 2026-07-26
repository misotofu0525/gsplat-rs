#!/usr/bin/env python3
"""Validate one fail-closed ``gsplat-balanced-image-gate/v1`` artifact.

This validator is intentionally independent from renderer and platform code. It
checks full-membership/SH/resolution receipts, decodes the retained RGBA8 PNGs,
recomputes every frozen B0 image metric, and verifies the moving 0 -> 1 -> 0
temporal receipts. It never supplies a missing field or substitutes a summary.
"""

from __future__ import annotations

import argparse
import binascii
import hashlib
import importlib.util
import json
import math
import pathlib
import re
import struct
import sys
import zlib
from dataclasses import dataclass
from typing import Any, Iterable


SCHEMA = "gsplat-balanced-image-gate/v1"
VALIDATOR_VERSION = 1
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
RESOLUTION_STAGES = ("requested", "surface", "internal_render", "presented")
EXACT_COUNT_FIELDS = (
    "source_splat_count",
    "decoded_splat_count",
    "encoded_splat_count",
    "resident_splat_count",
    "addressable_splat_count",
)
FRAME_METRIC_LIMITS = {
    "ssim_luma_srgb_window8": ("minimum", 0.99),
    "rgb_mae_normalized": ("maximum", 0.005),
    "rgb_bad_pixel_fraction_over_3": ("maximum", 0.02),
    "alpha_mae_normalized": ("maximum", 0.001),
    "alpha_bad_pixel_fraction_over_1": ("maximum", 0.005),
}
TEMPORAL_METRIC = "temporal_rgb_residual_mae_normalized"
TEMPORAL_LIMIT = 0.005
METRIC_RECEIPT_TOLERANCE = 1.0e-9
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
TRACE_VALIDATOR_PATH = REPO_ROOT / "tests/perf/trace/validate_trace_v1.py"
BENCHMARK_VALIDATOR_PATH = REPO_ROOT / "tests/perf/validate-benchmark-artifacts.py"
EVIDENCE_CLASSES = {"contract_fixture", "formal_quality"}
FORMAL_MIN_WIDTH = 1920
FORMAL_MIN_HEIGHT = 1080
LIFECYCLE_GENERATIONS = (
    "scene_generation",
    "camera_generation",
    "viewport_generation",
    "contract_generation",
    "plan_generation",
    "presentation_generation",
)
MATCHED_LIFECYCLE_GENERATIONS = (
    "scene_generation",
    "camera_generation",
    "viewport_generation",
    "contract_generation",
    "plan_generation",
    "presentation_generation",
)
DEPTH_PRECISION_PROFILES = {
    "exact": "ExactFull32",
    "candidate": "CandidateStable24",
}


class ValidationError(ValueError):
    pass


@dataclass(frozen=True)
class DecodedImage:
    width: int
    height: int
    rgba: bytes
    path: pathlib.Path | None = None


@dataclass(frozen=True)
class FramePixels:
    capture_index: int
    trace_frame_index: int
    exact: DecodedImage
    candidate: DecodedImage


@dataclass(frozen=True)
class ValidationResult:
    evidence_class: str
    frame_count: int
    transition_count: int
    validator_sha256: str


@dataclass(frozen=True)
class Authority:
    dataset_id: str
    dataset_asset_sha256: str
    dataset_asset_bytes: int
    source_splat_count: int
    source_sh_degree: int
    trace_id: str
    trace_content_sha256: str
    trace: dict[str, Any]


def fail(message: str) -> None:
    raise ValidationError(message)


def reject_constant(value: str) -> None:
    fail(f"non-finite JSON number is forbidden: {value}")


def reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            fail(f"duplicate JSON key is forbidden: {key}")
        result[key] = value
    return result


def load_json(path: pathlib.Path) -> dict[str, Any]:
    try:
        with path.open(encoding="utf-8") as handle:
            value = json.load(
                handle,
                parse_constant=reject_constant,
                object_pairs_hook=reject_duplicate_keys,
            )
    except ValidationError:
        raise
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")
    if not isinstance(value, dict):
        fail(f"{path} must contain a JSON object")
    return value


def require_object(parent: dict[str, Any], key: str, context: str) -> dict[str, Any]:
    value = parent.get(key)
    if not isinstance(value, dict):
        fail(f"{context}.{key} must be an object")
    return value


def require_array(parent: dict[str, Any], key: str, context: str) -> list[Any]:
    value = parent.get(key)
    if not isinstance(value, list):
        fail(f"{context}.{key} must be an array")
    return value


def require_string(parent: dict[str, Any], key: str, context: str) -> str:
    value = parent.get(key)
    if not isinstance(value, str) or not value:
        fail(f"{context}.{key} must be a non-empty string")
    return value


def require_bool(parent: dict[str, Any], key: str, context: str) -> bool:
    value = parent.get(key)
    if not isinstance(value, bool):
        fail(f"{context}.{key} must be boolean")
    return value


def require_int(
    parent: dict[str, Any], key: str, context: str, *, positive: bool = False
) -> int:
    value = parent.get(key)
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        fail(f"{context}.{key} must be a non-negative integer")
    if positive and value == 0:
        fail(f"{context}.{key} must be positive")
    return value


def require_number(parent: dict[str, Any], key: str, context: str) -> float:
    value = parent.get(key)
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        fail(f"{context}.{key} must be a number")
    result = float(value)
    if not math.isfinite(result):
        fail(f"{context}.{key} must be finite")
    return result


def require_sha256(parent: dict[str, Any], key: str, context: str) -> str:
    value = require_string(parent, key, context)
    if SHA256_RE.fullmatch(value) is None:
        fail(f"{context}.{key} must be a lowercase SHA-256")
    return value


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError as error:
        fail(f"cannot hash {path}: {error}")
    return digest.hexdigest()


def resolve_artifact_file(root: pathlib.Path, value: str, context: str) -> pathlib.Path:
    relative = pathlib.PurePosixPath(value)
    if relative.is_absolute() or not relative.parts or ".." in relative.parts:
        fail(f"{context} must be a relative path inside the artifact directory")
    resolved_root = root.resolve()
    try:
        path = root.joinpath(*relative.parts).resolve(strict=True)
    except OSError as error:
        fail(f"cannot resolve {context}: {error}")
    try:
        path.relative_to(resolved_root)
    except ValueError:
        fail(f"{context} escapes the artifact directory")
    if not path.is_file():
        fail(f"{context} does not name an artifact file")
    return path


def resolve_artifact_directory(root: pathlib.Path, value: str, context: str) -> pathlib.Path:
    relative = pathlib.PurePosixPath(value)
    if relative.is_absolute() or not relative.parts or ".." in relative.parts:
        fail(f"{context} must be a relative path inside the artifact directory")
    resolved_root = root.resolve()
    try:
        path = root.joinpath(*relative.parts).resolve(strict=True)
    except OSError as error:
        fail(f"cannot resolve {context}: {error}")
    try:
        path.relative_to(resolved_root)
    except ValueError:
        fail(f"{context} escapes the artifact directory")
    if not path.is_dir():
        fail(f"{context} does not name an artifact directory")
    return path


def artifact_directory_sha256(directory: pathlib.Path) -> str:
    """Hash one immutable artifact tree by relative name, length, and bytes."""

    digest = hashlib.sha256()
    try:
        entries = sorted(
            directory.rglob("*"),
            key=lambda path: path.relative_to(directory).as_posix(),
        )
    except OSError as error:
        fail(f"cannot enumerate benchmark artifact {directory}: {error}")
    files: list[pathlib.Path] = []
    for path in entries:
        if path.is_symlink():
            fail(f"benchmark artifact must not contain symlinks: {path}")
        if path.is_dir():
            continue
        if not path.is_file():
            fail(f"benchmark artifact contains a non-file entry: {path}")
        files.append(path)
    if not files:
        fail(f"benchmark artifact directory is empty: {directory}")
    for path in files:
        relative = path.relative_to(directory).as_posix().encode("utf-8")
        try:
            data = path.read_bytes()
        except OSError as error:
            fail(f"cannot hash benchmark artifact file {path}: {error}")
        digest.update(struct.pack(">Q", len(relative)))
        digest.update(relative)
        digest.update(struct.pack(">Q", len(data)))
        digest.update(data)
    return digest.hexdigest()


def load_trace_validator() -> Any:
    spec = importlib.util.spec_from_file_location(
        "balanced_gate_trace_validator", TRACE_VALIDATOR_PATH
    )
    if spec is None or spec.loader is None:
        fail(f"cannot load trace validator {TRACE_VALIDATOR_PATH}")
    module = importlib.util.module_from_spec(spec)
    trace_directory = str(TRACE_VALIDATOR_PATH.parent)
    sys.path.insert(0, trace_directory)
    try:
        spec.loader.exec_module(module)
    finally:
        sys.path.remove(trace_directory)
    return module


def load_benchmark_validator() -> Any:
    spec = importlib.util.spec_from_file_location(
        "balanced_gate_benchmark_validator", BENCHMARK_VALIDATOR_PATH
    )
    if spec is None or spec.loader is None:
        fail(f"cannot load benchmark validator {BENCHMARK_VALIDATOR_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def canonical_sha256(value: Any) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")
    return sha256_bytes(encoded)


def validate_authority(manifest: dict[str, Any], evidence_class: str) -> Authority:
    authority = require_object(manifest, "authority", "manifest")

    dataset_receipt = require_object(authority, "dataset_manifest", "manifest.authority")
    dataset_path = resolve_artifact_file(
        REPO_ROOT,
        require_string(dataset_receipt, "path", "manifest.authority.dataset_manifest"),
        "manifest.authority.dataset_manifest.path",
    )
    try:
        dataset_path.relative_to((REPO_ROOT / "tests/perf/datasets").resolve())
    except ValueError:
        fail("authoritative dataset manifest must come from tests/perf/datasets")
    if sha256_file(dataset_path) != require_sha256(
        dataset_receipt, "sha256", "manifest.authority.dataset_manifest"
    ):
        fail("manifest.authority.dataset_manifest SHA-256 mismatch")
    dataset = load_json(dataset_path)
    if dataset.get("schema") != "gsplat-dataset/v1":
        fail("authoritative dataset manifest schema must be 'gsplat-dataset/v1'")
    if evidence_class == "formal_quality" and dataset.get("qualification_status") != "qualified":
        fail("formal_quality requires a qualified authoritative dataset manifest")
    dataset_id = require_string(dataset, "id", "authoritative dataset manifest")
    asset_sha256 = require_sha256(dataset, "sha256", "authoritative dataset manifest")
    dataset_local_path = require_string(
        dataset, "local_path", "authoritative dataset manifest"
    )
    if require_string(
        dataset_receipt, "dataset_id", "manifest.authority.dataset_manifest"
    ) != dataset_id:
        fail("manifest.authority.dataset_manifest.dataset_id mismatch")
    if require_sha256(
        dataset_receipt, "asset_sha256", "manifest.authority.dataset_manifest"
    ) != asset_sha256:
        fail("manifest.authority.dataset_manifest.asset_sha256 mismatch")
    source_splat_count = require_int(
        dataset, "splat_count", "authoritative dataset manifest", positive=True
    )
    dataset_asset_bytes = require_int(
        dataset, "bytes", "authoritative dataset manifest", positive=True
    )
    source_sh_degree = require_int(dataset, "sh_degree", "authoritative dataset manifest")
    if evidence_class == "formal_quality":
        if not dataset_local_path.startswith("tests/datasets/external/"):
            fail("formal_quality forbids minimal contract fixture datasets")

    trace_receipt = require_object(authority, "trace", "manifest.authority")
    trace_path = resolve_artifact_file(
        REPO_ROOT,
        require_string(trace_receipt, "path", "manifest.authority.trace"),
        "manifest.authority.trace.path",
    )
    try:
        trace_path.relative_to((REPO_ROOT / "tests/perf/trace/fixtures").resolve())
    except ValueError:
        fail("authoritative camera trace must come from tests/perf/trace/fixtures")
    if sha256_file(trace_path) != require_sha256(
        trace_receipt, "sha256", "manifest.authority.trace"
    ):
        fail("manifest.authority.trace SHA-256 mismatch")
    trace = load_json(trace_path)
    trace_validator = load_trace_validator()
    try:
        trace_validator.validate(trace)
    except trace_validator.ValidationError as error:
        fail(f"authoritative camera trace is invalid: {error}")
    trace_id = require_string(trace, "trace_id", "authoritative camera trace")
    trace_content_sha256 = require_sha256(
        trace, "content_sha256", "authoritative camera trace"
    )
    if require_string(trace_receipt, "trace_id", "manifest.authority.trace") != trace_id:
        fail("manifest.authority.trace.trace_id mismatch")
    if require_sha256(
        trace_receipt, "content_sha256", "manifest.authority.trace"
    ) != trace_content_sha256:
        fail("manifest.authority.trace.content_sha256 mismatch")
    if evidence_class == "formal_quality":
        derivation = require_object(trace, "derivation", "authoritative camera trace")
        expected_derivation = {
            "source_path": dataset_local_path,
            "source_sha256": asset_sha256,
            "source_splat_count": source_splat_count,
            "source_sh_degree": source_sh_degree,
        }
        for key, expected in expected_derivation.items():
            if derivation.get(key) != expected:
                fail(
                    f"authoritative trace derivation.{key} does not match the "
                    "authoritative dataset manifest"
                )

    return Authority(
        dataset_id=dataset_id,
        dataset_asset_sha256=asset_sha256,
        dataset_asset_bytes=dataset_asset_bytes,
        source_splat_count=source_splat_count,
        source_sh_degree=source_sh_degree,
        trace_id=trace_id,
        trace_content_sha256=trace_content_sha256,
        trace=trace,
    )


def paeth_predictor(left: int, above: int, upper_left: int) -> int:
    estimate = left + above - upper_left
    distance_left = abs(estimate - left)
    distance_above = abs(estimate - above)
    distance_upper_left = abs(estimate - upper_left)
    if distance_left <= distance_above and distance_left <= distance_upper_left:
        return left
    if distance_above <= distance_upper_left:
        return above
    return upper_left


def unfilter_scanline(filter_type: int, source: bytes, previous: bytes) -> bytes:
    result = bytearray(len(source))
    bytes_per_pixel = 4
    for index, raw in enumerate(source):
        left = result[index - bytes_per_pixel] if index >= bytes_per_pixel else 0
        above = previous[index] if previous else 0
        upper_left = (
            previous[index - bytes_per_pixel]
            if previous and index >= bytes_per_pixel
            else 0
        )
        if filter_type == 0:
            value = raw
        elif filter_type == 1:
            value = raw + left
        elif filter_type == 2:
            value = raw + above
        elif filter_type == 3:
            value = raw + ((left + above) // 2)
        elif filter_type == 4:
            value = raw + paeth_predictor(left, above, upper_left)
        else:
            fail(f"unsupported PNG filter type: {filter_type}")
        result[index] = value & 0xFF
    return bytes(result)


def png_chunks(data: bytes) -> Iterable[tuple[bytes, bytes]]:
    if not data.startswith(PNG_SIGNATURE):
        fail("image is not a PNG")
    offset = len(PNG_SIGNATURE)
    saw_iend = False
    while offset < len(data):
        if len(data) - offset < 12:
            fail("PNG contains a truncated chunk")
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        chunk_type = data[offset + 4 : offset + 8]
        chunk_end = offset + 12 + length
        if chunk_end > len(data):
            fail("PNG chunk length exceeds the file")
        payload = data[offset + 8 : offset + 8 + length]
        expected_crc = struct.unpack(">I", data[offset + 8 + length : chunk_end])[0]
        actual_crc = binascii.crc32(chunk_type + payload) & 0xFFFFFFFF
        if actual_crc != expected_crc:
            fail(f"PNG {chunk_type!r} CRC mismatch")
        yield chunk_type, payload
        offset = chunk_end
        if chunk_type == b"IEND":
            saw_iend = True
            if offset != len(data):
                fail("PNG contains bytes after IEND")
            break
    if not saw_iend:
        fail("PNG is missing IEND")


def decode_rgba8_png(
    data: bytes,
    context: str,
    expected_dimensions: tuple[int, int] | None = None,
) -> DecodedImage:
    ihdr: bytes | None = None
    idat = bytearray()
    saw_iend = False
    for chunk_type, payload in png_chunks(data):
        if chunk_type == b"IHDR":
            if ihdr is not None or len(payload) != 13:
                fail(f"{context} has an invalid IHDR")
            ihdr = payload
        elif chunk_type == b"IDAT":
            if ihdr is None:
                fail(f"{context} has IDAT before IHDR")
            idat.extend(payload)
        elif chunk_type == b"IEND":
            if payload:
                fail(f"{context} has a non-empty IEND")
            saw_iend = True
        elif chunk_type[:1].isupper() and chunk_type not in {b"IHDR", b"IDAT", b"IEND"}:
            fail(f"{context} uses unsupported critical PNG chunk {chunk_type!r}")
    if ihdr is None or not idat or not saw_iend:
        fail(f"{context} is missing required PNG chunks")
    width, height, bit_depth, color_type, compression, filter_method, interlace = (
        struct.unpack(">IIBBBBB", ihdr)
    )
    if width == 0 or height == 0:
        fail(f"{context} dimensions must be positive")
    if expected_dimensions is not None and (width, height) != expected_dimensions:
        fail(f"{context} PNG dimensions do not match its receipt")
    if (bit_depth, color_type, compression, filter_method, interlace) != (8, 6, 0, 0, 0):
        fail(f"{context} must be non-interlaced RGBA8 PNG")
    row_bytes = width * 4
    expected_bytes = height * (row_bytes + 1)
    try:
        decompressor = zlib.decompressobj()
        filtered = bytearray()
        pending = bytes(idat)
        while pending:
            remaining = expected_bytes + 1 - len(filtered)
            if remaining <= 0:
                fail(f"{context} decompressed byte length exceeds its RGBA receipt")
            before = len(pending)
            filtered.extend(decompressor.decompress(pending, remaining))
            pending = decompressor.unconsumed_tail
            if len(filtered) > expected_bytes:
                fail(f"{context} decompressed byte length exceeds its RGBA receipt")
            if pending and len(pending) == before:
                fail(f"{context} compressed pixel stream made no progress")
    except zlib.error as error:
        fail(f"{context} has invalid compressed pixel data: {error}")
    if (
        len(filtered) != expected_bytes
        or not decompressor.eof
        or decompressor.unused_data
        or decompressor.unconsumed_tail
    ):
        fail(f"{context} decompressed byte length is invalid")
    rows: list[bytes] = []
    previous = b""
    offset = 0
    for _ in range(height):
        filter_type = filtered[offset]
        source = bytes(filtered[offset + 1 : offset + 1 + row_bytes])
        row = unfilter_scanline(filter_type, source, previous)
        rows.append(row)
        previous = row
        offset += row_bytes + 1
    return DecodedImage(width=width, height=height, rgba=b"".join(rows))


def load_image(
    root: pathlib.Path,
    receipt: dict[str, Any],
    expected_dimensions: tuple[int, int],
    context: str,
) -> DecodedImage:
    path_text = require_string(receipt, "path", context)
    expected_sha256 = require_sha256(receipt, "sha256", context)
    width = require_int(receipt, "width", context, positive=True)
    height = require_int(receipt, "height", context, positive=True)
    if (width, height) != expected_dimensions:
        fail(f"{context} dimensions must equal the full-resolution receipt")
    path = resolve_artifact_file(root, path_text, f"{context}.path")
    try:
        data = path.read_bytes()
    except OSError as error:
        fail(f"cannot read {path}: {error}")
    if sha256_bytes(data) != expected_sha256:
        fail(f"{context} SHA-256 mismatch")
    decoded = decode_rgba8_png(data, context, (width, height))
    return DecodedImage(
        width=decoded.width,
        height=decoded.height,
        rgba=decoded.rgba,
        path=path,
    )


def window_ssim(exact: list[float], candidate: list[float]) -> float:
    count = len(exact)
    mean_exact = sum(exact) / count
    mean_candidate = sum(candidate) / count
    variance_exact = 0.0
    variance_candidate = 0.0
    covariance = 0.0
    for exact_value, candidate_value in zip(exact, candidate, strict=True):
        exact_delta = exact_value - mean_exact
        candidate_delta = candidate_value - mean_candidate
        variance_exact += exact_delta * exact_delta
        variance_candidate += candidate_delta * candidate_delta
        covariance += exact_delta * candidate_delta
    denominator = max(count - 1, 1)
    variance_exact /= denominator
    variance_candidate /= denominator
    covariance /= denominator
    c1 = (0.01 * 255) ** 2
    c2 = (0.03 * 255) ** 2
    return (
        (2 * mean_exact * mean_candidate + c1) * (2 * covariance + c2)
    ) / (
        (mean_exact * mean_exact + mean_candidate * mean_candidate + c1)
        * (variance_exact + variance_candidate + c2)
    )


def compute_frame_metrics(exact: DecodedImage, candidate: DecodedImage) -> dict[str, float]:
    if (exact.width, exact.height) != (candidate.width, candidate.height):
        fail("Exact and candidate image dimensions differ")
    pixel_count = exact.width * exact.height
    rgb_absolute_error = 0
    alpha_absolute_error = 0
    rgb_bad_pixels = 0
    alpha_bad_pixels = 0
    for offset in range(0, len(exact.rgba), 4):
        rgb_errors = [
            abs(exact.rgba[offset + channel] - candidate.rgba[offset + channel])
            for channel in range(3)
        ]
        alpha_error = abs(exact.rgba[offset + 3] - candidate.rgba[offset + 3])
        rgb_absolute_error += sum(rgb_errors)
        alpha_absolute_error += alpha_error
        rgb_bad_pixels += int(any(error > 3 for error in rgb_errors))
        alpha_bad_pixels += int(alpha_error > 1)

    scores: list[float] = []
    for top in range(0, exact.height, 8):
        for left in range(0, exact.width, 8):
            exact_luma: list[float] = []
            candidate_luma: list[float] = []
            for y in range(top, min(top + 8, exact.height)):
                for x in range(left, min(left + 8, exact.width)):
                    offset = (y * exact.width + x) * 4
                    exact_luma.append(
                        0.2126 * exact.rgba[offset]
                        + 0.7152 * exact.rgba[offset + 1]
                        + 0.0722 * exact.rgba[offset + 2]
                    )
                    candidate_luma.append(
                        0.2126 * candidate.rgba[offset]
                        + 0.7152 * candidate.rgba[offset + 1]
                        + 0.0722 * candidate.rgba[offset + 2]
                    )
            scores.append(window_ssim(exact_luma, candidate_luma))
    return {
        "ssim_luma_srgb_window8": sum(scores) / len(scores),
        "rgb_mae_normalized": rgb_absolute_error / (255 * 3 * pixel_count),
        "rgb_bad_pixel_fraction_over_3": rgb_bad_pixels / pixel_count,
        "alpha_mae_normalized": alpha_absolute_error / (255 * pixel_count),
        "alpha_bad_pixel_fraction_over_1": alpha_bad_pixels / pixel_count,
    }


def compute_temporal_metric(previous: FramePixels, current: FramePixels) -> float:
    if (previous.exact.width, previous.exact.height) != (
        current.exact.width,
        current.exact.height,
    ):
        fail("moving-sequence image dimensions changed between frames")
    absolute_error = 0
    for offset in range(0, len(current.exact.rgba), 4):
        for channel in range(3):
            exact_delta = (
                current.exact.rgba[offset + channel]
                - previous.exact.rgba[offset + channel]
            )
            candidate_delta = (
                current.candidate.rgba[offset + channel]
                - previous.candidate.rgba[offset + channel]
            )
            absolute_error += abs(candidate_delta - exact_delta)
    pixel_count = current.exact.width * current.exact.height
    return absolute_error / (255 * 3 * pixel_count)


def verify_metric_receipt(
    receipt: dict[str, Any], key: str, actual: float, context: str
) -> None:
    declared = require_number(receipt, key, context)
    if not math.isclose(declared, actual, rel_tol=0.0, abs_tol=METRIC_RECEIPT_TOLERANCE):
        fail(f"{context}.{key} does not match recomputed RGBA8 bytes")


def validate_exactness(manifest: dict[str, Any], authority: Authority) -> None:
    exactness = require_object(manifest, "exactness", "manifest")
    counts = [
        require_int(exactness, key, "manifest.exactness", positive=True)
        for key in EXACT_COUNT_FIELDS
    ]
    if len(set(counts)) != 1:
        fail("manifest.exactness requires source=decoded=encoded=resident=addressable")
    if counts[0] != authority.source_splat_count:
        fail("manifest.exactness point count does not match authoritative dataset manifest")
    source_sh_degree = require_int(exactness, "source_sh_degree", "manifest.exactness")
    resident_sh_degree = require_int(exactness, "resident_sh_degree", "manifest.exactness")
    if resident_sh_degree != source_sh_degree:
        fail("manifest.exactness resident SH degree must equal source SH degree")
    if source_sh_degree != authority.source_sh_degree:
        fail("manifest.exactness SH degree does not match authoritative dataset manifest")
    required_values = {
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "sh_degree_policy": "source",
        "render_mode": "sorted_alpha",
    }
    for key, expected in required_values.items():
        if require_string(exactness, key, "manifest.exactness") != expected:
            fail(f"manifest.exactness.{key} must equal {expected!r}")
    if require_bool(exactness, "partial_scene_published", "manifest.exactness"):
        fail("manifest.exactness.partial_scene_published must be false")
    if not require_bool(exactness, "full_quality", "manifest.exactness"):
        fail("manifest.exactness.full_quality must be true")


def validate_resolution(
    manifest: dict[str, Any], trace: dict[str, Any], evidence_class: str
) -> tuple[int, int]:
    resolution = require_object(manifest, "resolution", "manifest")
    dimensions: list[tuple[int, int]] = []
    for stage in RESOLUTION_STAGES:
        dimensions.append(
            (
                require_int(resolution, f"{stage}_width", "manifest.resolution", positive=True),
                require_int(resolution, f"{stage}_height", "manifest.resolution", positive=True),
            )
        )
    if len(set(dimensions)) != 1:
        fail(
            "manifest.resolution requested/Surface/internal-render/presented "
            "dimensions must match"
        )
    if require_string(resolution, "dynamic_resolution", "manifest.resolution") != "disabled":
        fail("manifest.resolution.dynamic_resolution must equal 'disabled'")
    if require_string(resolution, "upscaling", "manifest.resolution") != "disabled":
        fail("manifest.resolution.upscaling must equal 'disabled'")
    if not require_bool(resolution, "full_resolution", "manifest.resolution"):
        fail("manifest.resolution.full_resolution must be true")
    trace_display = require_object(trace, "display", "authoritative camera trace")
    trace_dimensions = (
        require_int(trace_display, "width", "authoritative camera trace.display", positive=True),
        require_int(trace_display, "height", "authoritative camera trace.display", positive=True),
    )
    if dimensions[0] != trace_dimensions:
        fail("manifest.resolution does not match authoritative trace display")
    if evidence_class == "formal_quality" and (
        dimensions[0][0] < FORMAL_MIN_WIDTH or dimensions[0][1] < FORMAL_MIN_HEIGHT
    ):
        fail("formal_quality resolution must be at least 1920x1080")
    return dimensions[0]


def validate_camera(
    manifest: dict[str, Any], trace: dict[str, Any]
) -> tuple[str, list[int]]:
    camera = require_object(manifest, "camera", "manifest")
    mode = require_string(camera, "mode", "manifest.camera")
    if mode not in {"authored_views", "moving_sequence"}:
        fail("manifest.camera.mode must be authored_views or moving_sequence")
    raw_indices = require_array(camera, "trace_frame_indices", "manifest.camera")
    indices: list[int] = []
    for index, value in enumerate(raw_indices):
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            fail(f"manifest.camera.trace_frame_indices[{index}] must be a non-negative integer")
        indices.append(value)
    if not indices:
        fail("manifest.camera.trace_frame_indices must not be empty")
    if mode == "authored_views" and indices != [0, 1]:
        fail("authored_views quality capture must cover trace frames 0 and 1")
    if mode == "moving_sequence" and indices != [0, 1, 0]:
        fail("moving_sequence quality capture must be exactly trace frames 0 -> 1 -> 0")
    trace_frames = require_array(trace, "frames", "authoritative camera trace")
    if len(trace_frames) < 2:
        fail("authoritative camera trace must contain frozen frames 0 and 1")
    return mode, indices


def validate_formal_benchmark_artifacts(
    raw_frame: dict[str, Any],
    presentations: dict[str, dict[str, Any]],
    image_receipts: dict[str, dict[str, Any]],
    authority: Authority,
    root: pathlib.Path,
    expected_dimensions: tuple[int, int],
    capture_index: int,
    trace_frame_index: int,
    camera_receipt: dict[str, Any],
    context: str,
) -> None:
    pair = require_object(raw_frame, "benchmark_artifacts", context)
    pair_context = f"{context}.benchmark_artifacts"
    pair_id = require_string(pair, "pair_id", pair_context)
    benchmark_validator = load_benchmark_validator()
    run_ids: list[str] = []
    build_receipts: list[dict[str, Any]] = []
    for lane in ("exact", "candidate"):
        lane_receipt = require_object(pair, lane, pair_context)
        lane_context = f"{pair_context}.{lane}"
        artifact_directory = resolve_artifact_directory(
            root,
            require_string(lane_receipt, "path", lane_context),
            f"{lane_context}.path",
        )
        declared_artifact_sha256 = require_sha256(
            lane_receipt, "sha256", lane_context
        )
        if artifact_directory_sha256(artifact_directory) != declared_artifact_sha256:
            fail(f"{lane_context} benchmark artifact SHA-256 mismatch")
        try:
            benchmark_validator.validate(artifact_directory)
        except benchmark_validator.ValidationError as error:
            fail(f"{lane_context} is not a canonical gsplat-benchmark/v1 artifact: {error}")

        benchmark_manifest = benchmark_validator.load_json(
            artifact_directory / "manifest.json"
        )
        run_id = require_string(lane_receipt, "run_id", lane_context)
        if benchmark_manifest.get("run_id") != run_id:
            fail(f"{lane_context}.run_id does not match canonical benchmark manifest")
        run_ids.append(run_id)
        unavailable = set(benchmark_manifest["unavailable_fields"])
        benchmark_renderer = benchmark_manifest["renderer"]
        benchmark_frames = benchmark_validator.load_frames(
            artifact_directory / "frames.jsonl",
            run_id,
            unavailable,
            count_semantics=benchmark_renderer.get("count_semantics"),
            source_count=benchmark_manifest["dataset"]["splat_count"],
        )
        frame_index = require_int(lane_receipt, "frame_index", lane_context)
        if frame_index >= len(benchmark_frames):
            fail(f"{lane_context}.frame_index is unavailable in benchmark artifact")
        benchmark_frame = benchmark_frames[frame_index]
        if frame_index != len(benchmark_frames) - 1:
            fail(f"{lane_context}.frame_index must identify the terminal benchmark frame")

        expected_dataset = {
            "id": authority.dataset_id,
            "sha256": authority.dataset_asset_sha256,
            "bytes": authority.dataset_asset_bytes,
            "splat_count": authority.source_splat_count,
            "sh_degree": authority.source_sh_degree,
        }
        for key, expected in expected_dataset.items():
            if benchmark_manifest["dataset"].get(key) != expected:
                fail(f"{lane_context} benchmark dataset.{key} does not match authority")
        expected_trace = {
            "id": authority.trace_id,
            "sha256": authority.trace_content_sha256,
        }
        for key, expected in expected_trace.items():
            if benchmark_manifest["trace"].get(key) != expected:
                fail(f"{lane_context} benchmark trace.{key} does not match authority")
        display = benchmark_manifest["display"]
        if (display.get("width"), display.get("height")) != expected_dimensions:
            fail(f"{lane_context} benchmark display does not match formal image resolution")
        build = benchmark_manifest["build"]
        if build.get("repository_commit") is None or build.get("dirty") is not False:
            fail(f"{lane_context} benchmark build must identify a clean repository commit")
        build_receipts.append(
            {
                "repository_commit": build.get("repository_commit"),
                "dirty": build.get("dirty"),
                "profile": build.get("profile"),
                "package_version": build.get("package_version"),
            }
        )

        if benchmark_frame.get("pair_id") != pair_id:
            fail(f"{lane_context} benchmark pair_id mismatch")
        if benchmark_frame.get("capture_index") != capture_index:
            fail(f"{lane_context} benchmark capture_index mismatch")
        if benchmark_frame.get("trace_frame_index") != trace_frame_index:
            fail(f"{lane_context} benchmark trace_frame_index mismatch")
        if benchmark_frame.get("camera") != camera_receipt:
            fail(f"{lane_context} benchmark camera receipt mismatch")
        if benchmark_frame.get("terminal_outcome") != "presented":
            fail(f"{lane_context} benchmark terminal_outcome must equal 'presented'")
        if benchmark_frame.get("presentation") != presentations[lane]:
            fail(f"{lane_context} benchmark presentation receipt mismatch")
        capture_depth_precision = require_object(
            image_receipts[lane], "depth_precision", f"{context}.{lane}"
        )
        if require_object(
            lane_receipt, "depth_precision", lane_context
        ) != capture_depth_precision:
            fail(f"{lane_context} artifact depth-precision receipt mismatch")
        if benchmark_frame.get("capture_depth_precision") != capture_depth_precision:
            fail(f"{lane_context} benchmark capture depth-precision receipt mismatch")
        if benchmark_frame.get("active_splats") != authority.source_splat_count:
            fail(f"{lane_context} benchmark active_splats must equal source membership")

        benchmark_image = require_object(
            benchmark_manifest, "image", f"{lane_context}.manifest"
        )
        if require_sha256(
            benchmark_image, "sha256", f"{lane_context}.manifest.image"
        ) != require_sha256(image_receipts[lane], "sha256", f"{context}.{lane}"):
            fail(f"{lane_context} benchmark image does not match formal image receipt")
        if (
            require_int(benchmark_image, "width", f"{lane_context}.manifest.image")
            != expected_dimensions[0]
            or require_int(benchmark_image, "height", f"{lane_context}.manifest.image")
            != expected_dimensions[1]
        ):
            fail(f"{lane_context} benchmark image dimensions mismatch")

    if run_ids[0] == run_ids[1]:
        fail(f"{pair_context} Exact/candidate benchmark run IDs must differ")
    if build_receipts[0] != build_receipts[1]:
        fail(f"{pair_context} Exact/candidate benchmark build/profile must match")


def validate_frames(
    manifest: dict[str, Any],
    root: pathlib.Path,
    expected_dimensions: tuple[int, int],
    expected_trace_indices: list[int],
    authority: Authority,
    evidence_class: str,
) -> list[FramePixels]:
    raw_frames = require_array(manifest, "frames", "manifest")
    if len(raw_frames) != len(expected_trace_indices):
        fail("manifest.frames must cover every declared camera capture exactly once")
    frames: list[FramePixels] = []
    previous_tickets = {"exact": 0, "candidate": 0}
    previous_presentation_generations = {"exact": 0, "candidate": 0}
    for capture_index, raw_frame in enumerate(raw_frames):
        context = f"manifest.frames[{capture_index}]"
        if not isinstance(raw_frame, dict):
            fail(f"{context} must be an object")
        if require_int(raw_frame, "capture_index", context) != capture_index:
            fail(f"{context}.capture_index must be contiguous and ordered")
        trace_frame_index = require_int(raw_frame, "trace_frame_index", context)
        if trace_frame_index != expected_trace_indices[capture_index]:
            fail(f"{context}.trace_frame_index does not match the camera receipt")
        if not require_bool(raw_frame, "presented", context):
            fail(f"{context}.presented must be true")

        camera_receipt = require_object(raw_frame, "camera", context)
        if require_string(camera_receipt, "trace_id", f"{context}.camera") != authority.trace_id:
            fail(f"{context}.camera.trace_id does not match authoritative trace")
        if require_sha256(
            camera_receipt, "trace_content_sha256", f"{context}.camera"
        ) != authority.trace_content_sha256:
            fail(f"{context}.camera.trace_content_sha256 mismatch")
        trace_frame = authority.trace["frames"][trace_frame_index]
        pose_intrinsics_sha256 = canonical_sha256(
            {"pose": trace_frame["pose"], "intrinsics": trace_frame["intrinsics"]}
        )
        if require_sha256(
            camera_receipt, "pose_intrinsics_sha256", f"{context}.camera"
        ) != pose_intrinsics_sha256:
            fail(f"{context}.camera pose/intrinsics receipt mismatch")

        presentation_pair = require_object(raw_frame, "presentation", context)
        presentations: dict[str, dict[str, Any]] = {}
        generation_receipts: dict[str, dict[str, int]] = {}
        for lane in ("exact", "candidate"):
            presentation = require_object(
                presentation_pair, lane, f"{context}.presentation"
            )
            presentations[lane] = presentation
            presentation_context = f"{context}.presentation.{lane}"
            if require_string(presentation, "outcome", presentation_context) != "presented":
                fail(f"{presentation_context}.outcome must equal 'presented'")
            ticket = require_int(
                presentation, "ticket", presentation_context, positive=True
            )
            if ticket <= previous_tickets[lane]:
                fail(f"{presentation_context}.ticket must be strictly increasing")
            previous_tickets[lane] = ticket
            generations = {
                key: require_int(presentation, key, presentation_context, positive=True)
                for key in LIFECYCLE_GENERATIONS
            }
            generation_receipts[lane] = generations
            if (
                generations["presentation_generation"]
                <= previous_presentation_generations[lane]
            ):
                fail(
                    f"{presentation_context}.presentation_generation must be "
                    "strictly increasing"
                )
            previous_presentation_generations[lane] = generations[
                "presentation_generation"
            ]
        for generation in MATCHED_LIFECYCLE_GENERATIONS:
            if generation_receipts["exact"][generation] != generation_receipts[
                "candidate"
            ][generation]:
                fail(
                    f"{context}.presentation Exact/candidate {generation} must match"
                )

        exact_receipt = require_object(raw_frame, "exact", context)
        candidate_receipt = require_object(raw_frame, "candidate", context)
        image_receipts = {"exact": exact_receipt, "candidate": candidate_receipt}
        for lane, expected_profile in DEPTH_PRECISION_PROFILES.items():
            receipt_context = f"{context}.{lane}.depth_precision"
            depth_precision = require_object(
                image_receipts[lane], "depth_precision", f"{context}.{lane}"
            )
            if require_string(depth_precision, "profile", receipt_context) != expected_profile:
                fail(
                    f"{receipt_context}.profile must equal {expected_profile!r}"
                )
            presentation_sequence = require_int(
                depth_precision,
                "presentation_sequence",
                receipt_context,
                positive=True,
            )
            if (
                presentation_sequence
                != generation_receipts[lane]["presentation_generation"]
            ):
                fail(
                    f"{receipt_context}.presentation_sequence must match the "
                    "successful presentation generation"
                )
        if evidence_class == "formal_quality":
            validate_formal_benchmark_artifacts(
                raw_frame,
                presentations,
                image_receipts,
                authority,
                root,
                expected_dimensions,
                capture_index,
                trace_frame_index,
                camera_receipt,
                context,
            )
        exact = load_image(
            root,
            exact_receipt,
            expected_dimensions,
            f"{context}.exact",
        )
        candidate = load_image(
            root,
            candidate_receipt,
            expected_dimensions,
            f"{context}.candidate",
        )
        assert exact.path is not None and candidate.path is not None
        try:
            same_file = exact.path.samefile(candidate.path)
        except OSError as error:
            fail(f"{context} cannot compare Exact/candidate file identity: {error}")
        if same_file:
            fail(f"{context} Exact and candidate images must be separate artifacts")
        actual_metrics = compute_frame_metrics(exact, candidate)
        metric_receipt = require_object(raw_frame, "metrics", context)
        for key, (limit_kind, limit) in FRAME_METRIC_LIMITS.items():
            actual = actual_metrics[key]
            verify_metric_receipt(metric_receipt, key, actual, f"{context}.metrics")
            if limit_kind == "minimum" and actual < limit:
                fail(f"{context}.{key} is below the Balanced v1 gate")
            if limit_kind == "maximum" and actual > limit:
                fail(f"{context}.{key} exceeds the Balanced v1 gate")
        frames.append(
            FramePixels(
                capture_index=capture_index,
                trace_frame_index=trace_frame_index,
                exact=exact,
                candidate=candidate,
            )
        )
    return frames


def validate_transitions(
    manifest: dict[str, Any], mode: str, frames: list[FramePixels]
) -> int:
    raw_transitions = require_array(manifest, "transitions", "manifest")
    expected_count = len(frames) - 1 if mode == "moving_sequence" else 0
    if len(raw_transitions) != expected_count:
        fail("manifest.transitions must contain every and only adjacent moving capture")
    for transition_index, raw_transition in enumerate(raw_transitions):
        context = f"manifest.transitions[{transition_index}]"
        if not isinstance(raw_transition, dict):
            fail(f"{context} must be an object")
        previous = frames[transition_index]
        current = frames[transition_index + 1]
        required_indices = {
            "from_capture_index": previous.capture_index,
            "to_capture_index": current.capture_index,
            "from_trace_frame_index": previous.trace_frame_index,
            "to_trace_frame_index": current.trace_frame_index,
        }
        for key, expected in required_indices.items():
            if require_int(raw_transition, key, context) != expected:
                fail(f"{context}.{key} does not identify the adjacent capture")
        actual = compute_temporal_metric(previous, current)
        metric_receipt = require_object(raw_transition, "metrics", context)
        verify_metric_receipt(metric_receipt, TEMPORAL_METRIC, actual, f"{context}.metrics")
        if actual > TEMPORAL_LIMIT:
            fail(f"{context}.{TEMPORAL_METRIC} exceeds the Balanced v1 gate")
    return expected_count


def validate(path: pathlib.Path) -> ValidationResult:
    manifest_path = path.resolve()
    manifest = load_json(manifest_path)
    if manifest.get("schema") != SCHEMA:
        fail(f"manifest.schema must equal {SCHEMA!r}")
    evidence_class = require_string(manifest, "evidence_class", "manifest")
    if evidence_class not in EVIDENCE_CLASSES:
        fail("manifest.evidence_class must be contract_fixture or formal_quality")
    authority = validate_authority(manifest, evidence_class)
    validate_exactness(manifest, authority)
    dimensions = validate_resolution(manifest, authority.trace, evidence_class)
    mode, trace_indices = validate_camera(manifest, authority.trace)
    frames = validate_frames(
        manifest,
        manifest_path.parent,
        dimensions,
        trace_indices,
        authority,
        evidence_class,
    )
    transition_count = validate_transitions(manifest, mode, frames)
    return ValidationResult(
        evidence_class=evidence_class,
        frame_count=len(frames),
        transition_count=transition_count,
        validator_sha256=sha256_file(pathlib.Path(__file__).resolve()),
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=pathlib.Path)
    args = parser.parse_args(argv)
    try:
        result = validate(args.manifest)
    except ValidationError as error:
        print(f"balanced image gate rejected: {error}", file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "schema": SCHEMA,
                "validator": {
                    "version": VALIDATOR_VERSION,
                    "sha256": result.validator_sha256,
                },
                "evidence_class": result.evidence_class,
                "frame_count": result.frame_count,
                "transition_count": result.transition_count,
                "pass": True,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
