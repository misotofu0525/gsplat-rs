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


class ValidationError(ValueError):
    pass


@dataclass(frozen=True)
class DecodedImage:
    width: int
    height: int
    rgba: bytes


@dataclass(frozen=True)
class FramePixels:
    capture_index: int
    trace_frame_index: int
    exact: DecodedImage
    candidate: DecodedImage


@dataclass(frozen=True)
class ValidationResult:
    frame_count: int
    transition_count: int
    validator_sha256: str


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
        filtered = decompressor.decompress(bytes(idat), expected_bytes + 1)
        filtered += decompressor.flush()
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
        source = filtered[offset + 1 : offset + 1 + row_bytes]
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
    return decode_rgba8_png(data, context, (width, height))


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


def validate_exactness(manifest: dict[str, Any]) -> None:
    exactness = require_object(manifest, "exactness", "manifest")
    counts = [
        require_int(exactness, key, "manifest.exactness", positive=True)
        for key in EXACT_COUNT_FIELDS
    ]
    if len(set(counts)) != 1:
        fail("manifest.exactness requires source=decoded=encoded=resident=addressable")
    source_sh_degree = require_int(exactness, "source_sh_degree", "manifest.exactness")
    resident_sh_degree = require_int(exactness, "resident_sh_degree", "manifest.exactness")
    if resident_sh_degree != source_sh_degree:
        fail("manifest.exactness resident SH degree must equal source SH degree")
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


def validate_resolution(manifest: dict[str, Any]) -> tuple[int, int]:
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
    return dimensions[0]


def validate_camera(manifest: dict[str, Any]) -> tuple[str, list[int]]:
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
    return mode, indices


def validate_frames(
    manifest: dict[str, Any],
    root: pathlib.Path,
    expected_dimensions: tuple[int, int],
    expected_trace_indices: list[int],
) -> list[FramePixels]:
    raw_frames = require_array(manifest, "frames", "manifest")
    if len(raw_frames) != len(expected_trace_indices):
        fail("manifest.frames must cover every declared camera capture exactly once")
    frames: list[FramePixels] = []
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
        exact_receipt = require_object(raw_frame, "exact", context)
        candidate_receipt = require_object(raw_frame, "candidate", context)
        if exact_receipt.get("path") == candidate_receipt.get("path"):
            fail(f"{context} Exact and candidate images must be separate artifacts")
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
    validate_exactness(manifest)
    dimensions = validate_resolution(manifest)
    mode, trace_indices = validate_camera(manifest)
    frames = validate_frames(manifest, manifest_path.parent, dimensions, trace_indices)
    transition_count = validate_transitions(manifest, mode, frames)
    return ValidationResult(
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
