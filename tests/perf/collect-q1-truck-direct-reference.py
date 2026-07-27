#!/usr/bin/env python3
"""Build two independent native Direct-f32 Truck reference images.

This producer never calls a browser endpoint and never contains rendering
logic. It invokes the existing release desktop offscreen renderer once for
each frozen trace frame, verifies the renderer-owned diagnostic receipt, and
atomically publishes immutable image identities plus their decoded RGBA8
hashes.
"""

from __future__ import annotations

import argparse
import binascii
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
from typing import Any
import uuid
import zlib


ROOT = Path(__file__).resolve().parents[2]
TRUCK = ROOT / "tests/datasets/external/inria_3dgs/truck/point_cloud.ply"
TRACE = ROOT / "tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json"
LOCK = ROOT / "Cargo.lock"
RUST_TOOLCHAIN = ROOT / "rust-toolchain.toml"
MATRIX = ROOT / "tests/perf/full-quality-matrix-plan-v1.json"
DATASET_ID = "truck-full"
EXPECTED_TRUCK_SHA256 = "65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c"
EXPECTED_TRUCK_BYTES = 630_225_580
EXPECTED_SPLATS = 2_541_226
EXPECTED_SH_DEGREE = 3
EXPECTED_SIZE = (1920, 1080)
EXPECTED_TRACE_FILE_SHA256 = "13081183bf2d1c6b6ec165324f53185cc30af2f304db3fefa7aa3d9044ac7c5a"
EXPECTED_TRACE_SEMANTIC_SHA256 = "34d47dbddf73d915bfd55431b33da9430882767a40d9d74c636c508f7d7a5ab3"
EXPECTED_RUST_TOOLCHAIN_SHA256 = "c4456f46c276e18ed729c3261a8ab245f49de776e5a4e4c3e3530352bdaffb12"
RECEIPT_PREFIX = "OFFSCREEN_REFERENCE_RECEIPT "
RECEIPT_SCHEMA = "gsplat-direct-f32-offscreen-receipt/v1"
SIDECAR_SCHEMA = "gsplat-q1-direct-f32-reference/v1"
SOURCE_PATHS = (
    Path("tests/perf/collect-q1-truck-direct-reference.py"),
    Path("tests/perf/full-quality-matrix-plan-v1.json"),
    Path("examples/desktop/src/main.rs"),
    Path("examples/desktop/src/cli.rs"),
    Path("examples/desktop/src/offscreen.rs"),
    Path("examples/desktop/src/scene.rs"),
    Path("examples/desktop/src/trace.rs"),
    Path("examples/desktop/src/image_output.rs"),
    Path("crates/gsplat-render-wgpu/src/renderer/facade.rs"),
    Path("crates/gsplat-render-wgpu/src/renderer/offscreen_host.rs"),
    Path("crates/gsplat-render-wgpu/src/direct_scene_gpu.rs"),
)


class ReferenceError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ReferenceError(message)


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def file_identity(
    path: Path, *, relative: bool = True, allow_symlink: bool = False
) -> dict[str, Any]:
    require(path.is_file(), f"input must be a regular file: {path}")
    require(allow_symlink or not path.is_symlink(), f"input must not be a symlink: {path}")
    data = path.read_bytes()
    receipt = {
        "path": path.relative_to(ROOT).as_posix() if relative else str(path.resolve()),
        "bytes": len(data),
        "sha256": sha256_bytes(data),
    }
    return receipt


def canonical_sha256(value: Any) -> str:
    return sha256_bytes(
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    )


def command_text(argv: list[str], timeout: int = 60) -> str:
    completed = subprocess.run(
        argv,
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        timeout=timeout,
    )
    require(completed.returncode == 0, f"command failed ({completed.returncode}): {' '.join(argv)}\n{completed.stderr}")
    return completed.stdout.strip()


def git_identity(expected_commit: str) -> dict[str, Any]:
    head = command_text(["git", "rev-parse", "HEAD"])
    require(head == expected_commit, f"HEAD {head} does not match expected commit {expected_commit}")
    require(len(expected_commit) == 40 and all(c in "0123456789abcdef" for c in expected_commit), "expected commit must be a full lowercase SHA")
    require(command_text(["git", "status", "--porcelain=v1", "--untracked-files=all"]) == "", "reference producer requires a clean worktree")
    return {"commit": head, "clean": True}


def source_identities() -> list[dict[str, Any]]:
    return [file_identity(ROOT / relative) for relative in SOURCE_PATHS]


def parse_receipt(stdout: str) -> dict[str, str]:
    lines = [line for line in stdout.splitlines() if line.startswith(RECEIPT_PREFIX)]
    require(len(lines) == 1, "desktop run must emit exactly one offscreen reference receipt")
    values: dict[str, str] = {}
    for field in lines[0][len(RECEIPT_PREFIX) :].split():
        require("=" in field, f"malformed offscreen receipt field: {field}")
        key, value = field.split("=", 1)
        require(key not in values and key, f"duplicate or empty offscreen receipt field: {key}")
        values[key] = value
    expected = {
        "schema": RECEIPT_SCHEMA,
        "geometry_path": "sorted_index_direct",
        "representation": "wide_f32",
        "render_mode": "sorted_alpha",
        "order_backend": "cpu",
        "depth_key_precision": "exact_full32",
        "stable_source_id_order": "true",
        "raster_execution_plan": "wgpu_direct_global_quads",
        "gpu_rasterizer": "true",
        "source_count": str(EXPECTED_SPLATS),
        "decoded_count": str(EXPECTED_SPLATS),
        "encoded_count": str(EXPECTED_SPLATS),
        "resident_count": str(EXPECTED_SPLATS),
        "addressable_count": str(EXPECTED_SPLATS),
        "source_sh_degree": str(EXPECTED_SH_DEGREE),
        "resident_sh_degree": str(EXPECTED_SH_DEGREE),
        "requested_width": str(EXPECTED_SIZE[0]),
        "requested_height": str(EXPECTED_SIZE[1]),
        "internal_render_width": str(EXPECTED_SIZE[0]),
        "internal_render_height": str(EXPECTED_SIZE[1]),
        "readback_width": str(EXPECTED_SIZE[0]),
        "readback_height": str(EXPECTED_SIZE[1]),
        "readback_format": "rgba8_unorm",
        "readback_row_origin": "top_left",
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "partial_scene_published": "false",
        "dynamic_resolution": "disabled",
        "upscaling": "disabled",
    }
    for key, value in expected.items():
        require(values.get(key) == value, f"offscreen receipt {key} must equal {value!r}")
    for key in ("adapter_backend", "adapter_device_type"):
        require(values.get(key), f"offscreen receipt {key} is unavailable")
    parse_uint(values.get("adapter_vendor"), "adapter_vendor")
    parse_uint(values.get("adapter_device"), "adapter_device")
    visible = parse_uint(values.get("visible_count"), "visible_count")
    drawn = parse_uint(values.get("drawn_count"), "drawn_count")
    require(visible == drawn <= EXPECTED_SPLATS, "Direct reference must prove drawn=visible<=source")
    return values


def parse_uint(value: str | None, label: str) -> int:
    require(value is not None and value.isascii() and value.isdigit(), f"{label} must be an unsigned integer")
    return int(value)


def decode_rgba8_png(data: bytes) -> tuple[int, int, bytes]:
    require(data.startswith(b"\x89PNG\r\n\x1a\n"), "capture is not a PNG")
    offset = 8
    ihdr: bytes | None = None
    compressed = bytearray()
    saw_iend = False
    while offset < len(data):
        require(len(data) - offset >= 12, "PNG has a truncated chunk")
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        end = offset + 12 + length
        require(end <= len(data), "PNG chunk exceeds file")
        kind = data[offset + 4 : offset + 8]
        payload = data[offset + 8 : offset + 8 + length]
        expected_crc = struct.unpack(">I", data[offset + 8 + length : end])[0]
        require((binascii.crc32(kind + payload) & 0xFFFFFFFF) == expected_crc, "PNG CRC mismatch")
        if kind == b"IHDR":
            require(ihdr is None and len(payload) == 13, "PNG has invalid IHDR")
            ihdr = payload
        elif kind == b"IDAT":
            compressed.extend(payload)
        elif kind == b"IEND":
            require(not payload and end == len(data), "PNG has invalid IEND")
            saw_iend = True
        elif kind[:1].isupper():
            raise ReferenceError(f"PNG uses unsupported critical chunk {kind!r}")
        offset = end
        if saw_iend:
            break
    require(ihdr is not None and compressed and saw_iend, "PNG is incomplete")
    width, height, depth, color, compression, filtering, interlace = struct.unpack(">IIBBBBB", ihdr)
    require((width, height) == EXPECTED_SIZE, "PNG is not the frozen 1920x1080 size")
    require((depth, color, compression, filtering, interlace) == (8, 6, 0, 0, 0), "PNG must be non-interlaced RGBA8")
    raw = zlib.decompress(bytes(compressed))
    stride = width * 4
    require(len(raw) == height * (stride + 1), "PNG decoded byte length is invalid")
    rows: list[bytes] = []
    previous = bytes(stride)
    for row_index in range(height):
        start = row_index * (stride + 1)
        filter_kind = raw[start]
        source = raw[start + 1 : start + stride + 1]
        row = bytearray(stride)
        for index, byte in enumerate(source):
            left = row[index - 4] if index >= 4 else 0
            above = previous[index]
            upper_left = previous[index - 4] if index >= 4 else 0
            if filter_kind == 0:
                predictor = 0
            elif filter_kind == 1:
                predictor = left
            elif filter_kind == 2:
                predictor = above
            elif filter_kind == 3:
                predictor = (left + above) // 2
            elif filter_kind == 4:
                estimate = left + above - upper_left
                distances = (abs(estimate - left), abs(estimate - above), abs(estimate - upper_left))
                predictor = (left, above, upper_left)[distances.index(min(distances))]
            else:
                raise ReferenceError(f"unsupported PNG filter {filter_kind}")
            row[index] = (byte + predictor) & 0xFF
        previous = bytes(row)
        rows.append(previous)
    return width, height, b"".join(rows)


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def fsync_tree(root: Path) -> None:
    for path in sorted(root.rglob("*"), key=lambda item: len(item.parts), reverse=True):
        require(not path.is_symlink(), f"artifact contains symlink: {path}")
        if path.is_file():
            with path.open("rb") as handle:
                os.fsync(handle.fileno())
        elif path.is_dir():
            descriptor = os.open(path, os.O_RDONLY)
            try:
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
    descriptor = os.open(root, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


class OutputTransaction:
    def __init__(self, output: Path):
        self.output = output.resolve()
        require(not self.output.exists(), f"output already exists: {self.output}")
        self.output.parent.mkdir(parents=True, exist_ok=True)
        self.stage = self.output.parent / f".{self.output.name}.staging-{uuid.uuid4().hex}"
        self.stage.mkdir(mode=0o700)
        self.published = False

    def publish(self) -> None:
        fsync_tree(self.stage)
        self.stage.rename(self.output)
        self.published = True
        descriptor = os.open(self.output.parent, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)

    def reject(self, error: Exception) -> None:
        if self.published:
            return
        sidecar = self.stage / "reference.json"
        if sidecar.exists():
            sidecar.unlink()
        write_json(
            self.stage / "blocker.json",
            {"schema": "gsplat-q1-direct-f32-reference-blocker/v1", "status": "rejected", "error": str(error)},
        )
        self.publish()


def frozen_trace() -> tuple[dict[str, Any], dict[str, Any]]:
    identity = file_identity(TRACE)
    require(
        identity["sha256"] == EXPECTED_TRACE_FILE_SHA256,
        "frozen trace file SHA-256 mismatch",
    )
    trace = json.loads(TRACE.read_text(encoding="utf-8"))
    require(trace.get("schema") == "gsplat-camera-trace/v1", "trace schema mismatch")
    semantic_sha256 = canonical_sha256(
        {key: value for key, value in trace.items() if key != "content_sha256"}
    )
    require(
        trace.get("content_sha256") == semantic_sha256 == EXPECTED_TRACE_SEMANTIC_SHA256,
        "trace semantic SHA-256 mismatch",
    )
    require(trace.get("display") == {"width": 1920, "height": 1080}, "trace display mismatch")
    frames = trace.get("frames")
    require(isinstance(frames, list) and [frame.get("frame_index") for frame in frames] == [0, 1], "trace must contain frozen frames 0 and 1")
    frame_receipts = []
    for frame in frames:
        semantic = {"pose": frame.get("pose"), "intrinsics": frame.get("intrinsics")}
        frame_receipts.append(
            {
                "frame_index": frame["frame_index"],
                "pose": frame["pose"],
                "intrinsics": frame["intrinsics"],
                "pose_intrinsics_sha256": canonical_sha256(semantic),
            }
        )
    identity.update(
        {
            "trace_id": trace.get("trace_id"),
            "semantic_sha256": semantic_sha256,
            "frames": frame_receipts,
        }
    )
    require(isinstance(identity["semantic_sha256"], str) and len(identity["semantic_sha256"]) == 64, "trace semantic SHA is unavailable")
    return trace, identity


def validate_matrix_authority(truck: dict[str, Any], trace: dict[str, Any]) -> None:
    matrix = json.loads(MATRIX.read_text(encoding="utf-8"))
    require(
        matrix.get("schema") == "gsplat-full-quality-experiment/v1",
        "full-quality matrix schema mismatch",
    )
    datasets = matrix.get("datasets")
    traces = matrix.get("traces")
    require(isinstance(datasets, list) and isinstance(traces, list), "matrix inputs unavailable")
    dataset_entries = [entry for entry in datasets if entry.get("id") == DATASET_ID]
    trace_entries = [entry for entry in traces if entry.get("id") == trace.get("trace_id")]
    require(len(dataset_entries) == len(trace_entries) == 1, "matrix Truck authority is ambiguous")
    dataset = dataset_entries[0]
    expected_dataset = {
        "role": "full_scene",
        "local_path": TRUCK.relative_to(ROOT).as_posix(),
        "sha256": truck["sha256"],
        "bytes": truck["bytes"],
        "splat_count": EXPECTED_SPLATS,
        "sh_degree": EXPECTED_SH_DEGREE,
    }
    for key, value in expected_dataset.items():
        require(dataset.get(key) == value, f"matrix Truck dataset {key} mismatch")
    trace_entry = trace_entries[0]
    expected_trace = {
        "sha256": EXPECTED_TRACE_SEMANTIC_SHA256,
        "width": EXPECTED_SIZE[0],
        "height": EXPECTED_SIZE[1],
        "frame_count": 2,
        "local_path": TRACE.relative_to(ROOT).as_posix(),
        "evidence_class": "formal_full_quality",
        "dataset_id": DATASET_ID,
    }
    for key, value in expected_trace.items():
        require(trace_entry.get(key) == value, f"matrix Truck trace {key} mismatch")


def validate_sidecar(stage: Path, sidecar: dict[str, Any]) -> None:
    require(sidecar.get("schema") == SIDECAR_SCHEMA, "reference sidecar schema mismatch")
    require(sidecar.get("status") == "accepted", "reference sidecar status mismatch")
    generated_at = sidecar.get("generated_at_utc")
    require(
        isinstance(generated_at, str) and generated_at.endswith("Z"),
        "reference generation timestamp is unavailable",
    )
    release_binary = sidecar.get("release_binary")
    require(isinstance(release_binary, dict), "release binary receipt is unavailable")
    retained = release_binary.get("retained")
    require(isinstance(retained, dict), "retained release binary receipt is unavailable")
    retained_path = stage / str(retained.get("path", ""))
    require(
        retained_path.is_file()
        and not retained_path.is_symlink()
        and retained_path.stat().st_size == retained.get("bytes")
        and sha256_bytes(retained_path.read_bytes()) == retained.get("sha256"),
        "retained release binary identity mismatch",
    )
    exactness = sidecar.get("exactness")
    require(isinstance(exactness, dict), "reference exactness receipt is unavailable")
    for key in (
        "source_count",
        "decoded_count",
        "encoded_count",
        "resident_count",
        "addressable_count",
    ):
        require(exactness.get(key) == EXPECTED_SPLATS, f"reference exactness {key} mismatch")
    captures = sidecar.get("captures")
    require(
        isinstance(captures, list)
        and [capture.get("frame_index") for capture in captures] == [0, 1],
        "reference sidecar must contain trace frames 0 and 1",
    )
    for capture in captures:
        image = capture.get("image")
        require(isinstance(image, dict), "reference image receipt is unavailable")
        name = image.get("path")
        require(
            isinstance(name, str) and name == f"reference-trace-{capture['frame_index']}.png",
            "reference image path mismatch",
        )
        path = stage / name
        require(path.is_file() and not path.is_symlink(), "reference image is unavailable")
        data = path.read_bytes()
        width, height, rgba = decode_rgba8_png(data)
        require(
            image
            == {
                "path": name,
                "bytes": len(data),
                "sha256": sha256_bytes(data),
                "width": width,
                "height": height,
                "format": "rgba8",
                "decoded_rgba8_sha256": sha256_bytes(rgba),
            },
            "reference image identity mismatch",
        )
        receipt = capture.get("renderer_receipt")
        require(isinstance(receipt, dict), "renderer receipt is unavailable")
        receipt_line = RECEIPT_PREFIX + " ".join(
            f"{key}={value}" for key, value in receipt.items()
        )
        parsed = parse_receipt(receipt_line)
        require(parsed == receipt, "renderer receipt changed during sidecar validation")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--expected-commit", required=True)
    args = parser.parse_args()
    transaction = OutputTransaction(args.output)
    try:
        initial_git = git_identity(args.expected_commit)
        initial_sources = source_identities()
        truck = file_identity(TRUCK, allow_symlink=True)
        require((truck["sha256"], truck["bytes"]) == (EXPECTED_TRUCK_SHA256, EXPECTED_TRUCK_BYTES), "Truck identity mismatch")
        trace, trace_identity = frozen_trace()
        validate_matrix_authority(truck, trace)
        lock_identity = file_identity(LOCK)
        rust_toolchain_identity = file_identity(RUST_TOOLCHAIN)
        require(
            rust_toolchain_identity["sha256"] == EXPECTED_RUST_TOOLCHAIN_SHA256,
            "rust-toolchain.toml identity mismatch",
        )
        rust_toolchain_value = {
            "channel": "1.93.0",
            "profile": "default",
            "components": ["rustfmt", "clippy"],
        }
        require(
            'channel = "1.93.0"' in RUST_TOOLCHAIN.read_text(encoding="utf-8"),
            "rust-toolchain.toml channel must be 1.93.0",
        )
        rustc = shutil.which("rustc")
        cargo = shutil.which("cargo")
        require(rustc is not None and cargo is not None, "rustc and cargo must be available")
        forbidden_build_environment = sorted(
            key
            for key in os.environ
            if key
            in {
                "RUSTFLAGS",
                "CARGO_ENCODED_RUSTFLAGS",
                "RUSTC_WRAPPER",
                "RUSTC_WORKSPACE_WRAPPER",
                "CARGO_BUILD_TARGET",
                "CARGO_INCREMENTAL",
            }
            or key.startswith("CARGO_PROFILE_RELEASE_")
        )
        require(
            not forbidden_build_environment,
            "reference build forbids compiler/profile overrides: "
            + ", ".join(forbidden_build_environment),
        )
        toolchain = {
            "rustc": {"path": str(Path(rustc).resolve()), "version": command_text([rustc, "--version", "--verbose"])},
            "cargo": {"path": str(Path(cargo).resolve()), "version": command_text([cargo, "--version", "--verbose"])},
        }
        build_target = ROOT / "target/q1-direct-f32-reference-build" / args.expected_commit
        build_environment = os.environ.copy()
        build_environment["CARGO_TARGET_DIR"] = str(build_target)
        build = subprocess.run(
            [cargo, "build", "--locked", "--release", "-p", "desktop-example"],
            cwd=ROOT,
            env=build_environment,
            check=False,
            capture_output=True,
            text=True,
            timeout=1800,
        )
        (transaction.stage / "build.stdout.log").write_text(build.stdout, encoding="utf-8")
        (transaction.stage / "build.stderr.log").write_text(build.stderr, encoding="utf-8")
        require(build.returncode == 0, f"release desktop build failed with {build.returncode}")
        binary = build_target / "release/desktop-example"
        binary_before = file_identity(binary, relative=False)
        retained_binary = transaction.stage / "producer/desktop-example"
        retained_binary.parent.mkdir()
        shutil.copyfile(binary, retained_binary, follow_symlinks=False)
        os.chmod(retained_binary, 0o555)
        retained_binary_data = retained_binary.read_bytes()
        retained_binary_identity = {
            "path": "producer/desktop-example",
            "bytes": len(retained_binary_data),
            "sha256": sha256_bytes(retained_binary_data),
        }
        require(
            retained_binary_identity["bytes"] == binary_before["bytes"]
            and retained_binary_identity["sha256"] == binary_before["sha256"],
            "retained release binary identity mismatch",
        )
        captures: list[dict[str, Any]] = []
        for frame_index in (0, 1):
            png_name = f"reference-trace-{frame_index}.png"
            png_path = transaction.stage / png_name
            command = [
                str(binary), str(TRUCK), "--geometry-path", "direct", "--order-backend", "cpu",
                "--camera-trace", str(TRACE), "--camera-frame", str(frame_index), "--frames", "1",
                "--png", str(png_path), "--offscreen-reference-receipt",
            ]
            run = subprocess.run(command, cwd=ROOT, check=False, capture_output=True, text=True, timeout=1800)
            (transaction.stage / f"trace-{frame_index}.stdout.log").write_text(run.stdout, encoding="utf-8")
            (transaction.stage / f"trace-{frame_index}.stderr.log").write_text(run.stderr, encoding="utf-8")
            require(run.returncode == 0, f"trace frame {frame_index} renderer failed with {run.returncode}")
            receipt = parse_receipt(run.stdout)
            png_data = png_path.read_bytes()
            width, height, rgba = decode_rgba8_png(png_data)
            captures.append(
                {
                    "frame_index": frame_index,
                    "pose_intrinsics_sha256": trace_identity["frames"][frame_index]["pose_intrinsics_sha256"],
                    "renderer_receipt": receipt,
                    "visible_count": int(receipt["visible_count"]),
                    "drawn_count": int(receipt["drawn_count"]),
                    "image": {"path": png_name, "bytes": len(png_data), "sha256": sha256_bytes(png_data), "width": width, "height": height, "format": "rgba8", "decoded_rgba8_sha256": sha256_bytes(rgba)},
                }
            )
        binary_after = file_identity(binary, relative=False)
        require(binary_after == binary_before, "release binary changed while producing references")
        adapter_fields = (
            "adapter_backend",
            "adapter_device_type",
            "adapter_vendor",
            "adapter_device",
        )
        require(
            all(
                captures[0]["renderer_receipt"][key]
                == captures[1]["renderer_receipt"][key]
                for key in adapter_fields
            ),
            "trace frames selected different GPU adapter identities",
        )
        require(
            file_identity(TRUCK, allow_symlink=True) == truck
            and file_identity(TRACE)
            == {key: trace_identity[key] for key in ("path", "bytes", "sha256")},
            "input identity changed while producing references",
        )
        require(file_identity(LOCK) == lock_identity, "Cargo.lock changed while producing references")
        require(
            file_identity(RUST_TOOLCHAIN) == rust_toolchain_identity,
            "rust-toolchain.toml changed while producing references",
        )
        final_sources = source_identities()
        final_git = git_identity(args.expected_commit)
        require(final_sources == initial_sources, "producer source changed while producing references")
        require(final_git == initial_git, "repository identity changed while producing references")
        sidecar = {
            "schema": SIDECAR_SCHEMA,
            "status": "accepted",
            "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
            "repository": initial_git,
            "cargo_lock": lock_identity,
            "rust_toolchain": {
                **rust_toolchain_identity,
                **rust_toolchain_value,
            },
            "toolchain": toolchain,
            "release_binary": {
                **binary_before,
                "retained": retained_binary_identity,
            },
            "producer_sources": initial_sources,
            "dataset": {**truck, "id": DATASET_ID, "splat_count": EXPECTED_SPLATS, "sh_degree": EXPECTED_SH_DEGREE},
            "trace": trace_identity,
            "exactness": {"source_count": EXPECTED_SPLATS, "decoded_count": EXPECTED_SPLATS, "encoded_count": EXPECTED_SPLATS, "resident_count": EXPECTED_SPLATS, "addressable_count": EXPECTED_SPLATS, "source_sh_degree": EXPECTED_SH_DEGREE, "resident_sh_degree": EXPECTED_SH_DEGREE, "source_membership": "all", "sampling": "disabled", "lod": "disabled", "partial_scene_published": False},
            "execution": {"geometry_path": "sorted_index_direct", "representation": "wide_f32", "order_backend": "cpu", "depth_key_precision": "exact_full32", "stable_source_id_order": True, "render_mode": "sorted_alpha", "raster_execution_plan": "wgpu_direct_global_quads", "gpu_rasterizer": True, "dynamic_resolution": "disabled", "upscaling": "disabled", "requested": {"width": 1920, "height": 1080}, "internal_render": {"width": 1920, "height": 1080, "format": "rgba8_unorm"}, "readback": {"width": 1920, "height": 1080, "format": "rgba8", "row_origin": "top_left"}},
            "environment": {
                key: captures[0]["renderer_receipt"][key]
                for key in adapter_fields
            },
            "captures": captures,
            "integrity": {
                "repository_pre": initial_git,
                "repository_post": final_git,
                "binary_pre": binary_before,
                "binary_post": binary_after,
                "producer_sources_pre_sha256": canonical_sha256(initial_sources),
                "producer_sources_post_sha256": canonical_sha256(final_sources),
                "inputs_rechecked_after_render": True,
            },
        }
        validate_sidecar(transaction.stage, sidecar)
        write_json(transaction.stage / "reference.json", sidecar)
        transaction.publish()
        print(json.dumps({"status": "accepted", "output": str(transaction.output), "schema": SIDECAR_SCHEMA}, sort_keys=True))
        return 0
    except (OSError, subprocess.SubprocessError, ValueError, ReferenceError) as error:
        transaction.reject(error)
        print(f"reference producer rejected: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
