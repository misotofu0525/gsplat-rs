#!/usr/bin/env python3
"""Produce one Q1 formal-view native Metal quality-only capture.

The existing desktop Surface host remains the sole renderer and presentation
owner.  This collector builds that host at one reviewed clean commit, invokes
it exactly once, and reduces its renderer-owned terminal receipts to the
minimal artifact consumed by ``q1_product_quality_smoke._native_capture``.
It deliberately publishes no performance measurements.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
from typing import Any, Callable, Sequence


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
PERF_DIR = REPO_ROOT / "tests/perf"
SHARED_COLLECTOR = PERF_DIR / "collect-desktop-surface-evidence.py"
Q1_SMOKE_PATH = PERF_DIR / "q1_product_quality_smoke.py"
FORMAL_TRACE_ROOT = (
    PERF_DIR / "trace/fixtures/quality/formal-truck-product-quality-979x546-v1"
)
TRUCK_PATH = REPO_ROOT / "tests/datasets/external/inria_3dgs/truck/point_cloud.ply"

SCHEMA = "gsplat-benchmark/v1"
WIDTH = 979
HEIGHT = 546
TRACE_FRAME_INDEX = 0
TRUCK_ID = "inria-3dgs-truck-iteration-30000"
TRUCK_SHA256 = "65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c"
TRUCK_BYTES = 630_225_580
TRUCK_SPLAT_COUNT = 2_541_226
TRUCK_SH_DEGREE = 3
RUN_TIMEOUT_SECONDS = 30 * 60
FEATURE = "diagnostic-surface-capture-receipt"
PLAN_REQUESTED = "cpu_post_sort"
PLAN_CLI = "cpu-post-sort"
PLAN_RECEIPT = "CpuPostSort"

PREFIXES = {
    "begin": "SURFACE_EXACT_EVIDENCE_BEGIN ",
    "frame": "SURFACE_EXACT_EVIDENCE_FRAME ",
    "capture": "SURFACE_EXACT_EVIDENCE_CAPTURE ",
    "diagnostic": "SURFACE_DIAGNOSTIC_CAPTURE_RECEIPT ",
    "camera": "SURFACE_DIAGNOSTIC_CAPTURE_CAMERA_RECEIPT ",
    "summary": "SURFACE_EXACT_EVIDENCE_SUMMARY ",
}

FORBIDDEN_PERFORMANCE_KEYS = frozenset(
    {
        "call_ms",
        "cpu_preprocess_ms",
        "cpu_render_submit_ms",
        "cpu_sort_ms",
        "elapsed_ns",
        "fps",
        "frame_budget_ms",
        "frame_wall_ms",
        "gpu_complete_ms",
        "gpu_wait_ms",
        "pairing",
        "speed_ratio",
        "throughput",
        "timing",
        "winner",
    }
)


def load_module(name: str, path: pathlib.Path) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


SHARED = load_module("q1_native_quality_shared", SHARED_COLLECTOR)
Q1 = load_module("q1_native_quality_gate", Q1_SMOKE_PATH)
ValidationError = SHARED.ValidationError
require = SHARED.require
parse_payload = SHARED.parse_payload
parse_uint = SHARED.parse_uint
parse_bool = SHARED.parse_bool
sha256_file = SHARED.sha256_file
git_receipt = SHARED.git_receipt
validate_ignored_output = SHARED.validate_ignored_output
write_json = SHARED.write_json
write_jsonl = SHARED.write_jsonl
fsync_tree = SHARED.fsync_tree
make_tree_immutable = SHARED.make_tree_immutable


def canonical_json(value: Any) -> str:
    return json.dumps(
        value,
        allow_nan=False,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    )


def canonical_sha256(value: Any) -> str:
    return hashlib.sha256(canonical_json(value).encode()).hexdigest()


def finite(value: str, context: str) -> float:
    try:
        parsed = float(value)
    except ValueError as error:
        raise ValidationError(f"{context} must be a number") from error
    require(math.isfinite(parsed), f"{context} must be finite")
    return parsed


def one(records: dict[str, list[dict[str, str]]], name: str) -> dict[str, str]:
    values = records[name]
    require(len(values) == 1, f"host must emit exactly one {name} record")
    return values[0]


def parse_records(stdout: str, stderr: str) -> dict[str, list[dict[str, str]]]:
    records = {name: [] for name in PREFIXES}
    for stream_name, stream in (("stdout", stdout), ("stderr", stderr)):
        for line_number, line in enumerate(stream.splitlines(), 1):
            for name, prefix in PREFIXES.items():
                if line.startswith(prefix):
                    value = parse_payload(
                        line[len(prefix) :], f"{stream_name}:{line_number}:{name}"
                    )
                    value["__stream"] = stream_name
                    records[name].append(value)
                    break
    return records


def exact_fields(
    value: dict[str, str], expected: dict[str, str], context: str
) -> None:
    for key, wanted in expected.items():
        require(value.get(key) == wanted, f"{context}.{key} mismatch")


def load_formal_trace(root: pathlib.Path) -> dict[str, Any]:
    root = pathlib.Path(os.path.abspath(root))
    require(not root.is_symlink() and root.is_dir(), "formal trace root must be a real directory")
    root = root.resolve(strict=True)
    require(
        {entry.name for entry in root.iterdir()} == {"camera-trace.json", "receipt.json"},
        "formal trace root file set mismatch",
    )
    fixture_trace = Q1._load_json(
        FORMAL_TRACE_ROOT / "camera-trace.json", "committed formal trace"
    )
    fixture_receipt = Q1._load_json(
        FORMAL_TRACE_ROOT / "receipt.json", "committed formal trace receipt"
    )
    trace_path = root / "camera-trace.json"
    receipt_path = root / "receipt.json"
    trace = Q1._load_json(trace_path, "formal trace")
    receipt = Q1._load_json(receipt_path, "formal trace receipt")
    require(canonical_json(trace) == canonical_json(fixture_trace), "formal trace drifted")
    require(
        canonical_json(receipt) == canonical_json(fixture_receipt),
        "formal trace receipt drifted",
    )
    try:
        Q1.validate_trace_v1(trace)
    except Exception as error:
        raise ValidationError(f"formal trace validator rejected input: {error}") from error
    frames = trace.get("frames")
    require(isinstance(frames, list) and len(frames) == 2, "formal trace frame set mismatch")
    frame = frames[TRACE_FRAME_INDEX]
    require(isinstance(frame, dict), "formal view 000001 is unavailable")
    require(
        trace.get("trace_id") == "formal-truck-product-quality-000001-000009-979x546-v1"
        and trace.get("content_sha256")
        == "46819f71d5025bb61f6583392448d977051a0b4c67a0c05db860232033c0676c"
        and trace.get("display") == {"width": WIDTH, "height": HEIGHT},
        "formal trace identity mismatch",
    )
    return {
        "root": root,
        "path": trace_path,
        "file_sha256": sha256_file(trace_path),
        "receipt_sha256": sha256_file(receipt_path),
        "value": trace,
        "frame": frame,
        "pose_intrinsics_sha256": canonical_sha256(
            {"pose": frame["pose"], "intrinsics": frame["intrinsics"]}
        ),
    }


def load_truck(path: pathlib.Path) -> pathlib.Path:
    path = pathlib.Path(os.path.abspath(path))
    require(not path.is_symlink() and path.is_file(), f"formal Truck is unavailable: {path}")
    path = path.resolve(strict=True)
    require(path.stat().st_size == TRUCK_BYTES, "formal Truck byte count mismatch")
    require(sha256_file(path) == TRUCK_SHA256, "formal Truck SHA-256 mismatch")
    return path


def make_command(
    binary: pathlib.Path, dataset: pathlib.Path, trace: pathlib.Path
) -> list[str]:
    return [
        str(binary),
        str(dataset),
        "--geometry-path",
        "packed",
        "--interactive",
        "--camera-trace",
        str(trace),
        "--camera-frame",
        "0",
        "--camera-warmup-frames",
        "0",
        "--camera-measured-frames",
        "1",
        "--surface-benchmark-mode",
        "isolated",
        "--surface-sort-policy",
        "every-frame",
        "--surface-evidence-plan",
        PLAN_CLI,
        "--surface-diagnostic-capture-receipt",
        "--png",
        "capture.png",
    ]


def validate_host(
    stdout: str,
    stderr: str,
    *,
    trace: dict[str, Any],
    capture_path: pathlib.Path,
) -> tuple[dict[str, Any], dict[str, Any]]:
    records = parse_records(stdout, stderr)
    begin = one(records, "begin")
    frame = one(records, "frame")
    capture = one(records, "capture")
    diagnostic = one(records, "diagnostic")
    camera_line = one(records, "camera")
    summary = one(records, "summary")
    require(
        all(value.get("__stream") == "stdout" for value in (begin, frame, capture, diagnostic, camera_line, summary)),
        "quality receipts must be emitted on stdout",
    )
    exact_fields(
        begin,
        {
            "trace_id": trace["value"]["trace_id"],
            "trace_sha256": trace["value"]["content_sha256"],
            "exact_plan_requested": PLAN_REQUESTED,
            "geometry_path": "packed_atlas",
            "raster_execution_plan": "projected_quads_exact",
            "blend_mode": "sorted_alpha",
            "source_membership": "all",
            "sampling": "disabled",
            "lod": "disabled",
            "adapter_backend": "metal",
            "source_count": str(TRUCK_SPLAT_COUNT),
            "decoded_count": str(TRUCK_SPLAT_COUNT),
            "encoded_count": str(TRUCK_SPLAT_COUNT),
            "resident_count": str(TRUCK_SPLAT_COUNT),
            "addressable_count": str(TRUCK_SPLAT_COUNT),
            "sh_degree": str(TRUCK_SH_DEGREE),
            "requested_width": str(WIDTH),
            "requested_height": str(HEIGHT),
            "surface_width": str(WIDTH),
            "surface_height": str(HEIGHT),
            "internal_render_width": str(WIDTH),
            "internal_render_height": str(HEIGHT),
            "dynamic_resolution": "disabled",
            "upscaling": "disabled",
            "full_resolution": "true",
            "trace_frames": "1",
        },
        "begin",
    )
    require(begin.get("adapter_name") not in (None, "", "unavailable"), "Metal adapter is unavailable")
    exact_fields(
        frame,
        {
            "trace_frame": "0",
            "phase": "measure",
            "measured_sample": "0",
            "exact_plan_requested": PLAN_REQUESTED,
            "exact_plan_actual": PLAN_REQUESTED,
            "count_semantics": "direct_draw_equals_visible",
            "source_count": str(TRUCK_SPLAT_COUNT),
            "exact_contributor_compaction": "false",
            "actual_backend": "cpu",
            "requested_width": str(WIDTH),
            "requested_height": str(HEIGHT),
            "presented_width": str(WIDTH),
            "presented_height": str(HEIGHT),
            "frame_presented": "true",
            "terminal_receipt": "ready",
        },
        "frame",
    )
    exact_fields(
        capture,
        {
            "status": "ok",
            "path": "capture.png",
            "trace_frame": "0",
            "exact_plan_requested": PLAN_REQUESTED,
            "exact_plan_actual": PLAN_REQUESTED,
            "count_semantics": "direct_draw_equals_visible",
            "source_count": str(TRUCK_SPLAT_COUNT),
            "exact_contributor_compaction": "false",
            "actual_backend": "cpu",
            "requested_width": str(WIDTH),
            "requested_height": str(HEIGHT),
            "captured_width": str(WIDTH),
            "captured_height": str(HEIGHT),
            "frame_presented": "true",
            "terminal_receipt": "ready",
        },
        "capture",
    )
    exact_fields(
        diagnostic,
        {
            "depth_precision_profile": "ExactFull32",
            "projected_cache_precision_profile": "ExactAxes32",
            "resident_sh_codec_profile": "ExactSigned11BandScale5",
            "resident_sh_source_count": str(TRUCK_SPLAT_COUNT),
            "resident_sh_encoded_count": str(TRUCK_SPLAT_COUNT),
            "resident_sh_resident_count": str(TRUCK_SPLAT_COUNT),
            "resident_sh_addressable_count": str(TRUCK_SPLAT_COUNT),
            "resident_sh_source_degree": "3",
            "resident_sh_resident_degree": "3",
            "plan_id": PLAN_RECEIPT,
            "width": str(WIDTH),
            "height": str(HEIGHT),
        },
        "diagnostic capture",
    )
    for field in (
        "scene_generation",
        "camera_revision",
        "viewport_generation",
        "contract_generation",
        "plan_set_generation",
        "order_generation",
        "presentation_sequence",
    ):
        require(
            diagnostic.get(field) == capture.get(field),
            f"diagnostic capture.{field} is not the same presentation terminal",
        )
    for context, value in (("frame", frame), ("capture", capture)):
        source = parse_uint(value.get("source_count", ""), f"{context}.source_count")
        visible = parse_uint(value.get("visible_count", ""), f"{context}.visible_count")
        contributor = parse_uint(
            value.get("contributor_count", ""), f"{context}.contributor_count"
        )
        drawn = parse_uint(value.get("drawn_count", ""), f"{context}.drawn_count")
        require(
            source == TRUCK_SPLAT_COUNT and contributor <= visible <= source and drawn == visible,
            f"{context} violates complete-source C<=V=S/D semantics",
        )
    exact_fields(
        camera_line,
        {
            "trace_frame": "0",
            "camera_revision": diagnostic.get("camera_revision", ""),
            "presentation_sequence": diagnostic.get("presentation_sequence", ""),
        },
        "camera receipt",
    )
    exact_fields(
        summary,
        {
            "status": "ok",
            "exact_plan_requested": PLAN_REQUESTED,
            "actual_plan_set": PLAN_REQUESTED,
            "trace_frames": "1",
            "measured_frames": "1",
            "terminal_receipts": "2",
            "final_capture": "available",
        },
        "summary",
    )
    require(not capture_path.is_symlink() and capture_path.is_file(), "capture PNG is missing")
    rgba = Q1._decode_png(capture_path, "native quality-only capture")
    rgba_sha = hashlib.sha256(rgba).hexdigest()
    require(rgba_sha == diagnostic.get("rgba8_sha256"), "capture RGBA digest mismatch")

    def positive(field: str) -> int:
        return parse_uint(diagnostic.get(field, ""), f"diagnostic.{field}", positive=True)

    camera_revision = positive("camera_revision")
    presentation_sequence = positive("presentation_sequence")
    native_camera = {
        "position": [
            finite(camera_line.get(f"position_{axis}", ""), f"camera.position_{axis}")
            for axis in "xyz"
        ],
        "rotationXyzw": [
            finite(camera_line.get(f"rotation_{axis}", ""), f"camera.rotation_{axis}")
            for axis in "xyzw"
        ],
        "intrinsics": {
            "verticalFovRadians": finite(
                camera_line.get("vertical_fov_radians", ""), "camera.vertical_fov_radians"
            ),
            "nearPlane": finite(camera_line.get("near_plane", ""), "camera.near_plane"),
            "farPlane": finite(camera_line.get("far_plane", ""), "camera.far_plane"),
            "focalLengthXOverY": finite(
                camera_line.get("focal_length_x_over_y", ""),
                "camera.focal_length_x_over_y",
            ),
        },
    }
    Q1._native_runtime_camera(native_camera, trace_for_gate(trace), "native host camera")
    return native_camera, {
        "scene_generation": positive("scene_generation"),
        "camera_revision": camera_revision,
        "viewport_generation": parse_uint(
            diagnostic.get("viewport_generation", ""), "diagnostic.viewport_generation"
        ),
        "contract_generation": positive("contract_generation"),
        "plan_set_generation": positive("plan_set_generation"),
        "plan_id": diagnostic["plan_id"],
        "order_generation": positive("order_generation"),
        "presentation_sequence": presentation_sequence,
        "width": WIDTH,
        "height": HEIGHT,
        "rgba8_sha256": rgba_sha,
        "profile": diagnostic["depth_precision_profile"],
    }


def trace_for_gate(trace: dict[str, Any]) -> dict[str, Any]:
    return {
        "trace": trace["value"],
        "pose_intrinsics_sha256": trace["pose_intrinsics_sha256"],
        "focal_length_x_over_y": trace["frame"]["intrinsics"]["focal_length_x_over_y"],
    }


def reject_performance_fields(value: Any, context: str = "artifact") -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            require(key not in FORBIDDEN_PERFORMANCE_KEYS, f"{context} contains forbidden {key}")
            reject_performance_fields(child, f"{context}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            reject_performance_fields(child, f"{context}[{index}]")


def require_disjoint_output(output: pathlib.Path, inputs: Sequence[pathlib.Path]) -> None:
    output = output.resolve(strict=False)
    for value in inputs:
        source = value.resolve(strict=True)
        overlaps = False
        try:
            output.relative_to(source)
            overlaps = True
        except ValueError:
            pass
        try:
            source.relative_to(output)
            overlaps = True
        except ValueError:
            pass
        require(not overlaps, f"output overlaps immutable input: {source}")


def materialize_artifact(
    directory: pathlib.Path,
    *,
    trace: dict[str, Any],
    native_camera: dict[str, Any],
    capture: dict[str, Any],
    capture_path: pathlib.Path,
    commit: str,
    binary_sha256: str,
) -> tuple[dict[str, Any], dict[str, Any]]:
    directory.mkdir()
    final_png = directory / "final-frame.png"
    shutil.copyfile(capture_path, final_png)
    frame = {
        "trace_frame_index": TRACE_FRAME_INDEX,
        "camera_revision": capture["camera_revision"],
        "presentation_sequence": capture["presentation_sequence"],
        "capture_depth_precision": capture,
    }
    runtime_camera_sha = canonical_sha256(native_camera)
    dimensions = {
        **{f"{stage}_width": WIDTH for stage in ("requested", "surface", "internal_render", "presented")},
        **{f"{stage}_height": HEIGHT for stage in ("requested", "surface", "internal_render", "presented")},
    }
    manifest = {
        "schema": SCHEMA,
        "build": {
            "repository_commit": commit,
            "dirty": False,
            "profile": "release",
            "executable_sha256": binary_sha256,
        },
        "dataset": {
            "id": TRUCK_ID,
            "sha256": TRUCK_SHA256,
            "bytes": TRUCK_BYTES,
            "splat_count": TRUCK_SPLAT_COUNT,
            "sh_degree": TRUCK_SH_DEGREE,
        },
        "exactness": {
            "source_splat_count": TRUCK_SPLAT_COUNT,
            "decoded_splat_count": TRUCK_SPLAT_COUNT,
            "encoded_splat_count": TRUCK_SPLAT_COUNT,
            "resident_splat_count": TRUCK_SPLAT_COUNT,
            "addressable_splat_count": TRUCK_SPLAT_COUNT,
            "source_sh_degree": TRUCK_SH_DEGREE,
            "resident_sh_degree": TRUCK_SH_DEGREE,
            "source_membership": "all",
            "sampling": "disabled",
            "lod": "disabled",
            "partial_scene_published": False,
            "full_quality": True,
        },
        "resolution": {
            **dimensions,
            "dynamic_resolution": "disabled",
            "upscaling": "disabled",
            "full_resolution": True,
        },
        "trace": {
            "id": trace["value"]["trace_id"],
            "sha256": trace["value"]["content_sha256"],
            "file_sha256": trace["file_sha256"],
            "receipt_sha256": trace["receipt_sha256"],
            "capture_frame_index": TRACE_FRAME_INDEX,
        },
        "camera_receipt": native_camera,
        "q1_comparison": {
            "artifact_role": "control",
            "performance_evidence": False,
            "product_quality": "Deferred",
            "performance_eligible": False,
            "presentation_identity": {
                "trace_frame_index": TRACE_FRAME_INDEX,
                "camera": {
                    "trace_id": trace["value"]["trace_id"],
                    "trace_content_sha256": trace["value"]["content_sha256"],
                    "trace_frame_index": TRACE_FRAME_INDEX,
                    "pose_intrinsics_sha256": trace["pose_intrinsics_sha256"],
                    "camera_revision": capture["camera_revision"],
                    "runtime_camera_receipt_sha256": runtime_camera_sha,
                },
                "terminal_identity": {
                    "frame_index": 0,
                    "frame_sha256": canonical_sha256(frame),
                    "presentation_sequence": capture["presentation_sequence"],
                },
                "successful_present": True,
                "queue_terminal_complete": True,
                "captured_after_terminal": True,
                "dimensions": dimensions,
            },
        },
    }
    reject_performance_fields(manifest)
    reject_performance_fields(frame)
    write_json(directory / "manifest.json", manifest)
    write_jsonl(directory / "frames.jsonl", [frame])
    return manifest, frame


HostInvoker = Callable[[Sequence[str], pathlib.Path], subprocess.CompletedProcess[str]]


def default_host_invoker(
    command: Sequence[str], cwd: pathlib.Path
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(command),
        cwd=cwd,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=RUN_TIMEOUT_SECONDS,
    )


def collect(
    args: argparse.Namespace,
    *,
    repo: pathlib.Path = REPO_ROOT,
    invoke: HostInvoker = default_host_invoker,
) -> pathlib.Path:
    output = pathlib.Path(os.path.abspath(args.output))
    require(not output.exists() and not output.is_symlink(), f"output already exists: {output}")
    require(output.parent.is_dir() and not output.parent.is_symlink(), "output parent must be real")
    validate_ignored_output(repo, output)
    initial_git = git_receipt(repo)
    require(not initial_git["dirty"], "quality producer requires a clean repository")
    require(
        re.fullmatch(r"[0-9a-f]{40}", args.expected_commit) is not None,
        "expected commit must be a full lowercase SHA",
    )
    require(initial_git["commit"] == args.expected_commit, "reviewed commit does not match HEAD")
    trace = load_formal_trace(args.formal_trace_authority)
    dataset = load_truck(args.dataset)
    require_disjoint_output(output, (trace["root"], dataset))

    stage = pathlib.Path(tempfile.mkdtemp(prefix=f".{output.name}.q1-native-", dir=output.parent))
    try:
        build = SHARED.build_locked_desktop_binary(
            repo, stage, initial_git, feature=FEATURE, build_jobs=1
        )
        binary = pathlib.Path(build["path"])
        host = stage / "host"
        host.mkdir()
        command = make_command(binary, dataset, trace["path"])
        write_json(host / "command.json", {"argv": command, "cwd": "host"})
        completed = invoke(command, host)
        (host / "stdout.log").write_text(completed.stdout, encoding="utf-8")
        (host / "stderr.log").write_text(completed.stderr, encoding="utf-8")
        require(completed.returncode == 0, f"native quality host exited with {completed.returncode}")
        require(sha256_file(binary) == build["sha256"], "locked native binary changed")
        require(sha256_file(dataset) == TRUCK_SHA256, "formal Truck changed during capture")
        require(sha256_file(trace["path"]) == trace["file_sha256"], "formal trace changed during capture")
        require(git_receipt(repo) == initial_git, "repository changed during capture")
        capture_path = host / "capture.png"
        camera, capture = validate_host(
            completed.stdout, completed.stderr, trace=trace, capture_path=capture_path
        )
        artifact = stage / "artifact"
        materialize_artifact(
            artifact,
            trace=trace,
            native_camera=camera,
            capture=capture,
            capture_path=capture_path,
            commit=initial_git["commit"],
            binary_sha256=build["sha256"],
        )
        require(
            {path.name for path in artifact.iterdir()}
            == {"manifest.json", "frames.jsonl", "final-frame.png"},
            "quality artifact contains an unexpected file",
        )
        Q1._native_capture(artifact, trace_for_gate(trace))
        fsync_tree(artifact)
        make_tree_immutable(artifact)
        Q1._publish_directory_noreplace(artifact, output)
        return output
    finally:
        if stage.exists():
            shutil.rmtree(stage, ignore_errors=True)


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected-commit", required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument(
        "--formal-trace-authority", type=pathlib.Path, default=FORMAL_TRACE_ROOT
    )
    parser.add_argument("--dataset", type=pathlib.Path, default=TRUCK_PATH)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    try:
        output = collect(parse_args(argv))
    except (OSError, subprocess.SubprocessError, ValidationError, Q1.OneViewQualityError) as error:
        print(f"Q1 native quality producer rejected: {error}", file=sys.stderr)
        return 1
    print(canonical_json({"status": "valid_quality_only_capture", "output": str(output)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
