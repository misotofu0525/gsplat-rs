"""Frozen Q0-derived schedule and normalized terminal receipt contract."""

from __future__ import annotations

import math
import pathlib
from datetime import datetime
from typing import Any

from .common import (
    array,
    canonical_sha256,
    fail,
    file_sha256,
    inside,
    integer,
    load_json,
    number,
    obj,
    sha256,
    string,
    utc,
)


SCHEMA = "gsplat-q1-truck-paired-comparison/v1"
RESULT_SCHEMA = "gsplat-q1-truck-paired-result/v1"
TERMINAL_SCHEMA = "gsplat-q1-webgpu-terminal-window/v1"
IMAGE_SCHEMA = "gsplat-q1-reference-image-comparison/v1"
FORMAL_INPUTS_SCHEMA = "gsplat-q1-truck-formal-inputs/v1"
FORMAL_LOCK_SCHEMA = "gsplat-q1-truck-formal-execution-lock/v1"
POST_RUN_SCHEMA = "gsplat-q1-truck-post-run-verification/v1"
COMMANDS_SCHEMA = "gsplat-q1-truck-paired-command-receipt/v1"
SAFE_HOST_ENVIRONMENT = frozenset({"PATH", "HOME", "TMPDIR", "LANG", "LC_ALL", "LC_CTYPE"})
POSTPROCESS_ENVIRONMENT = SAFE_HOST_ENVIRONMENT | {"CHROME_PATH"}
PROCESS_TIMEOUTS_SECONDS = {
    "git_helper": 120,
    "producer": 1800,
    "canonical_validator": 300,
    "image_comparison": 600,
    "final_validator": 600,
    "process_group_term_grace": 5,
    "process_group_kill_grace": 5,
}
BROWSER_OWNERSHIP_ENVIRONMENT = frozenset({
    "GSPLAT_Q1_BROWSER_OWNER_MARKER",
    "GSPLAT_Q1_BROWSER_USER_DATA_DIR",
    "GSPLAT_Q1_BROWSER_HANDSHAKE_PATH",
})
PUPPETEER_GRAPH_SCHEMA = "gsplat-q1-puppeteer-production-modules/v1"
PLAYCANVAS_COMMAND_ENVIRONMENT = BROWSER_OWNERSHIP_ENVIRONMENT | frozenset({
    "CHROME_PATH",
    "HEADLESS",
    "PHASE_E_QUALIFICATION",
    "PLAYCANVAS_CAMERA_MODE",
    "PLAYCANVAS_WARMUP_FRAMES",
    "PLAYCANVAS_MEASURED_FRAMES",
    "PLAYCANVAS_VIEWPORT_WIDTH",
    "PLAYCANVAS_VIEWPORT_HEIGHT",
    "PLAYCANVAS_Q1_SERIES_ROOT",
    "PLAYCANVAS_Q1_PRODUCER_REQUEST",
    "PLAYCANVAS_ARTIFACT_DIR",
})
GSPLAT_COMMAND_ENVIRONMENT = BROWSER_OWNERSHIP_ENVIRONMENT | frozenset({
    "CHROME_PATH",
    "HEADLESS",
    "GSPLAT_PHASE_E_QUALIFICATION",
    "GSPLAT_TRUCK_QUALIFICATION_STAGE",
    "GSPLAT_DATASET",
    "GSPLAT_ARTIFACT_DIR",
    "GSPLAT_GEOMETRY_PATH",
    "GSPLAT_ORDER_BACKEND",
    "GSPLAT_PROJECTED_POLICY",
    "GSPLAT_SORT_INTERVAL",
    "GSPLAT_BENCHMARK_WARMUP_FRAMES",
    "GSPLAT_BENCHMARK_FRAMES",
    "GSPLAT_CAMERA_TRACE_URL",
    "GSPLAT_CAMERA_TRACE_SEQUENCE",
    "GSPLAT_CAMERA_TRACE_LOOPS",
    "GSPLAT_CAMERA_FRAME_INDICES",
    "GSPLAT_Q1_ARTIFACT_ROLE",
    "GSPLAT_Q1_PROTOCOL_SHA256",
    "GSPLAT_Q1_WASM_PACKAGE_DIR",
    "GSPLAT_Q1_RUN_CONTEXT",
    "GSPLAT_Q1_COLLECTION_SESSION_ID",
    "GSPLAT_Q1_SERIES_ROOT",
    "GSPLAT_HTTP_PORT",
    "GSPLAT_ORDER_COMPLETION_PROTOCOL",
    "GSPLAT_BENCHMARK_WINDOW_MODE",
})
LOCKED_REPOSITORY_FILES = frozenset({
    "tests/perf/collect-q1-truck-paired-series.py",
    "tests/perf/browser-process-ownership.mjs",
    "tests/perf/validate-q1-truck-paired-comparison.py",
    "tests/perf/validate-benchmark-artifacts.py",
    "tests/perf/validate-balanced-image-gate.py",
    "tests/perf/compare-image-ssim.mjs",
    "tests/perf/trace/fixtures/quality/candidate-truck-quality-1920x1080-v1.json",
    "tests/datasets/external/inria_3dgs/truck/point_cloud.ply",
    "tests/competitive/playcanvas/package.json",
    "tests/competitive/playcanvas/package-lock.json",
    "tests/competitive/playcanvas/expected-engine.json",
})
LOCKED_REPOSITORY_TREES = frozenset({
    "tests/perf/q1_pair_admission",
    "tests/competitive/playcanvas/scripts",
    "tests/competitive/playcanvas/public",
    "tests/competitive/playcanvas/node_modules/playcanvas/build/playcanvas",
    "tests/competitive/playcanvas/node_modules/puppeteer-core",
    "examples/web/scripts",
    "examples/web/src",
})
TRUCK = {
    "id": "inria-3dgs-truck-iteration-30000",
    "sha256": "65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c",
    "bytes": 630_225_580,
    "splat_count": 2_541_226,
    "sh_degree": 3,
}
TRACE = {
    "id": "candidate-truck-quality-2view-1920x1080-v1",
    "sha256": "34d47dbddf73d915bfd55431b33da9430882767a40d9d74c636c508f7d7a5ab3",
    "frame_indices": [0, 1],
}
TRACE_FRAME_POSE_INTRINSICS_SHA256 = {
    0: "a008beb20adfdc24af484b25112503edb636e03010028aaeae812c3454527b46",
    1: "4b1d63381a662226712fd58cf5b3ea120fe5378beb73c228a509f55ec393f265",
}


def _load_playcanvas_camera_authority() -> dict[int, dict[str, Any]]:
    module_directory = pathlib.Path(__file__).parent
    fixture = load_json(
        module_directory / "fixtures/playcanvas-truck-camera-receipts-v1.json",
        "PlayCanvas camera authority fixture",
    )
    if fixture.get("schema") != "gsplat-playcanvas-camera-authority-fixture/v1":
        fail("PlayCanvas camera authority fixture schema mismatch")
    if fixture.get("source_trace") != {
        "id": TRACE["id"],
        "content_sha256": TRACE["sha256"],
    }:
        fail("PlayCanvas camera authority fixture does not bind the frozen Truck trace")
    authority = obj(fixture, "authority", "PlayCanvas camera authority fixture")
    authority_path = "tests/competitive/playcanvas/public/trace-camera.js"
    if authority != {
        "path": authority_path,
        "sha256": file_sha256(module_directory.parents[2] / authority_path),
        "oracle_export": "canonicalPlayCanvasCameraOracle",
        "receipt_export": "createPlayCanvasCameraReceipt",
    }:
        fail("PlayCanvas camera authority fixture is stale; run its generator")
    receipts = obj(fixture, "receipts", "PlayCanvas camera authority fixture")
    if set(receipts) != {str(index) for index in TRACE["frame_indices"]}:
        fail("PlayCanvas camera authority fixture does not cover the frozen Truck trace")
    result: dict[int, dict[str, Any]] = {}
    for index in TRACE["frame_indices"]:
        receipt = receipts[str(index)]
        if not isinstance(receipt, dict):
            fail(f"PlayCanvas camera authority fixture receipt {index} must be an object")
        result[index] = receipt
    return result


PLAYCANVAS_CAMERA_AUTHORITY = _load_playcanvas_camera_authority()
WIDTH = 1920
HEIGHT = 1080
WARMUP = 20
MEASURED = 80
PLAYCANVAS = {
    "version": "2.21.0-beta.14",
    "revision": "d5fe88878e338936fe763bbce1a58bc315e89cbe",
    "runtime_revision": "d5fe888",
    "integrity": "sha512-qYN8vp9CBRBU8qs9eJudUM3fFkPbXz8qN5IqjFicULJoD0CNnEtlFsGoHVAOQZ5b2+LWlUxUFhXMSzNL86moWg==",
}
COMMON_ENVIRONMENT_FIELDS = (
    "os",
    "device",
    "browser",
    "browser_executable_sha256",
    "browser_launch_args_sha256",
    "adapter",
    "adapter_identity_status",
    "driver",
    "canonical_adapter_supported_limits_sha256",
    "power_source",
    "collection_session_id",
)
WEBGPU_ENVIRONMENT_SCHEMA = "gsplat-q1-webgpu-selected-device-environment/v1"
CANONICAL_ADAPTER_SCHEMA = "gsplat-q1-webgpu-canonical-adapter/v1"
CANONICAL_SUPPORTED_LIMITS_SCHEMA = (
    "wgpu-28-browser-webgpu-direct-supported-limits/v1"
)
ADAPTER_SELECTION_CLASS = "browser_webgpu_renderer_actual_selected_adapter"
ADAPTER_IDENTITY_STATUS = (
    "hardware_name_unavailable_cross_endpoint_wgpu28_browser_backend"
)
CANONICAL_SUPPORTED_LIMIT_NAMES = (
    "maxBindGroups",
    "maxBindingsPerBindGroup",
    "maxBufferSize",
    "maxColorAttachmentBytesPerSample",
    "maxColorAttachments",
    "maxComputeInvocationsPerWorkgroup",
    "maxComputeWorkgroupSizeX",
    "maxComputeWorkgroupSizeY",
    "maxComputeWorkgroupSizeZ",
    "maxComputeWorkgroupStorageSize",
    "maxComputeWorkgroupsPerDimension",
    "maxDynamicStorageBuffersPerPipelineLayout",
    "maxDynamicUniformBuffersPerPipelineLayout",
    "maxSampledTexturesPerShaderStage",
    "maxSamplersPerShaderStage",
    "maxStorageBufferBindingSize",
    "maxStorageBuffersPerShaderStage",
    "maxStorageTexturesPerShaderStage",
    "maxTextureArrayLayers",
    "maxTextureDimension1D",
    "maxTextureDimension2D",
    "maxTextureDimension3D",
    "maxUniformBufferBindingSize",
    "maxUniformBuffersPerShaderStage",
    "maxVertexAttributes",
    "maxVertexBufferArrayStride",
    "maxVertexBuffers",
    "minStorageBufferOffsetAlignment",
    "minUniformBufferOffsetAlignment",
)


def validate_protocol(document: dict[str, Any]) -> tuple[str, float]:
    protocol = obj(document, "protocol", "schedule")
    if obj(protocol, "dataset", "schedule.protocol") != TRUCK:
        fail("schedule.protocol.dataset is not the frozen complete Truck SH3 source")
    if obj(protocol, "trace", "schedule.protocol") != TRACE:
        fail("schedule.protocol.trace is not the frozen two-view Truck trace")
    if obj(protocol, "display", "schedule.protocol") != {"width": WIDTH, "height": HEIGHT, "dpr": 1}:
        fail("schedule.protocol.display must be exact 1920x1080 DPR-1")
    expected = {
        "camera_mode": "trace_sequence",
        "camera_mutation_point": "before_update_order_project_render",
        "warmup_frames": WARMUP,
        "measured_frames": MEASURED,
        "terminal_boundary": "first_measured_camera_input_to_final_gpu_queue_completion",
        "claim_scope": "near-contract",
    }
    for key, value in expected.items():
        if protocol.get(key) != value:
            fail(f"schedule.protocol.{key} must equal {value!r}")
    quality = obj(protocol, "quality_gate", "schedule.protocol")
    if quality.get("metric") != "ssim-luma-srgb-window8":
        fail("schedule.protocol.quality_gate.metric mismatch")
    minimum = number(quality, "minimum_ssim", "schedule.protocol.quality_gate")
    if not 0 <= minimum <= 1:
        fail("schedule.protocol.quality_gate.minimum_ssim must be in [0,1]")
    return canonical_sha256(protocol), minimum


def validate_schedule(document: dict[str, Any]) -> tuple[list[dict[str, Any]], str, datetime]:
    schedule = obj(document, "schedule", "schedule")
    integer(schedule, "seed", "schedule.schedule")
    predeclared = utc(schedule.get("predeclared_at_utc"), "schedule.schedule.predeclared_at_utc")
    pairs = array(schedule, "pairs", "schedule.schedule")
    references = array(schedule, "reference_images", "schedule.schedule")
    if len(references) != 2:
        fail("schedule.schedule.reference_images must cover both views")
    if len(pairs) != 5:
        fail("schedule.schedule.pairs must contain exactly five pairs")
    seen: set[str] = set()
    orders: list[str] = []
    for index, pair in enumerate(pairs):
        if not isinstance(pair, dict):
            fail(f"schedule.schedule.pairs[{index}] must be an object")
        pair_id = string(pair, "pair_id", f"schedule.schedule.pairs[{index}]")
        if pair_id in seen:
            fail(f"duplicate predeclared pair id: {pair_id}")
        seen.add(pair_id)
        order = string(pair, "run_order", f"schedule.schedule.pairs[{index}]")
        if order not in {"playcanvas-first", "gsplat-rs-first"}:
            fail(f"{pair_id}: invalid run_order")
        orders.append(order)
    if set(orders) != {"playcanvas-first", "gsplat-rs-first"} or abs(
        orders.count("playcanvas-first") - orders.count("gsplat-rs-first")
    ) > 1:
        fail("predeclared order is not counterbalanced AB/BA")
    return pairs, canonical_sha256(schedule), predeclared


def _hashed_entry(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        fail(f"{context} must be an object")
    string(value, "path", context)
    sha256(value.get("sha256"), f"{context}.sha256")
    integer(value, "bytes", context)
    return value


def _validate_command_environments(
    commands: dict[str, Any], browser_path: str, root: pathlib.Path
) -> None:
    root = root.resolve()
    postprocess = obj(commands, "postprocess", "schedule orchestration command receipt")
    post_environment = obj(postprocess, "environment", "schedule orchestration postprocess")
    if (
        not {"PATH", "HOME"}.issubset(post_environment)
        or set(post_environment) != POSTPROCESS_ENVIRONMENT
        or post_environment.get("CHROME_PATH") != browser_path
    ):
        fail("schedule orchestration postprocess environment is not the strict host allowlist")
    timeouts = obj(postprocess, "timeout_seconds", "schedule orchestration postprocess")
    if timeouts != {
        key: PROCESS_TIMEOUTS_SECONDS[key]
        for key in ("canonical_validator", "image_comparison", "final_validator")
    }:
        fail("schedule orchestration postprocess timeouts are not the frozen safety bounds")
    invocations = array(commands, "invocations", "schedule orchestration command receipt")
    for index, invocation in enumerate(invocations):
        if not isinstance(invocation, dict):
            fail(f"schedule orchestration command {index} must be an object")
        endpoint = invocation.get("endpoint")
        role = invocation.get("artifact_role")
        environment = obj(invocation, "environment", f"schedule orchestration command {index}")
        expected = (
            PLAYCANVAS_COMMAND_ENVIRONMENT
            if endpoint == "playcanvas"
            else GSPLAT_COMMAND_ENVIRONMENT if endpoint == "gsplat_rs" else frozenset()
        )
        expected = set(expected) | set(post_environment)
        if role == "control":
            expected.add(
                "PLAYCANVAS_CAPTURE_TRACE_FRAME"
                if endpoint == "playcanvas"
                else "GSPLAT_Q1_CAPTURE_TRACE_FRAME"
            )
        elif role == "throughput" and endpoint == "gsplat_rs":
            expected.update({
                "GSPLAT_CURRENT_STATS_CONTROL_ARTIFACT",
                "GSPLAT_Q1_CONTROL_ARTIFACT_0",
                "GSPLAT_Q1_CONTROL_ARTIFACT_1",
            })
        elif role != "throughput":
            fail(f"schedule orchestration command {index} has an invalid role")
        if set(environment) != expected:
            fail(f"schedule orchestration command {index} environment is not exact")
        if environment.get("CHROME_PATH") != browser_path or environment.get("HEADLESS") != "0":
            fail(f"schedule orchestration command {index} does not use the locked headful Chrome")
        if any(not isinstance(value, str) or not value for value in environment.values()):
            fail(f"schedule orchestration command {index} environment contains an empty value")
        ownership = obj(invocation, "browser_ownership", f"schedule orchestration command {index}")
        marker = ownership.get("marker")
        invocation_id = string(
            invocation, "invocation_id", f"schedule orchestration command {index}"
        )
        if not isinstance(marker, str) or not marker.startswith("gsplat-q1-"):
            fail(f"schedule orchestration command {index} has an invalid browser marker")
        expected_profile = root / "process-home" / "browser-profiles" / invocation_id
        expected_handshake = root / "browser-handshakes" / f"{invocation_id}.json"
        expected_ownership = {
            "marker": marker,
            "marker_argument": f"--user-data-dir={expected_profile}",
            "user_data_dir": str(expected_profile),
            "handshake_path": str(expected_handshake),
        }
        if ownership != expected_ownership:
            fail(f"schedule orchestration command {index} browser ownership paths are not predeclared")
        if (
            environment.get("GSPLAT_Q1_BROWSER_OWNER_MARKER") != marker
            or environment.get("GSPLAT_Q1_BROWSER_USER_DATA_DIR") != str(expected_profile)
            or environment.get("GSPLAT_Q1_BROWSER_HANDSHAKE_PATH") != str(expected_handshake)
        ):
            fail(f"schedule orchestration command {index} browser ownership environment drifted")
        if invocation.get("timeout_seconds") != PROCESS_TIMEOUTS_SECONDS["producer"]:
            fail(f"schedule orchestration command {index} producer timeout is not frozen")


def _authority_identity(
    value: Any,
    context: str,
    *,
    expected_root: str | None = None,
) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != {
        "root_path",
        "receipt_path",
        "receipt_sha256",
        "repository_commit",
        "release_binary_sha256",
        "generated_at_utc",
        "tree",
        "views",
    }:
        fail(f"{context} fields are not frozen")
    root_path = string(value, "root_path", context)
    if expected_root is not None and root_path != expected_root:
        fail(f"{context}.root_path mismatch")
    if value.get("receipt_path") != "reference.json":
        fail(f"{context}.receipt_path mismatch")
    sha256(value.get("receipt_sha256"), f"{context}.receipt_sha256")
    commit = string(value, "repository_commit", context)
    if len(commit) != 40 or any(character not in "0123456789abcdef" for character in commit):
        fail(f"{context}.repository_commit must be a full lowercase Git SHA")
    sha256(value.get("release_binary_sha256"), f"{context}.release_binary_sha256")
    utc(value.get("generated_at_utc"), f"{context}.generated_at_utc")
    tree = obj(value, "tree", context)
    if set(tree) != {"file_count", "bytes", "sha256", "files"}:
        fail(f"{context}.tree fields are not frozen")
    files = array(tree, "files", f"{context}.tree")
    paths: set[str] = set()
    for index, entry in enumerate(files):
        item_context = f"{context}.tree.files[{index}]"
        if not isinstance(entry, dict) or set(entry) != {"path", "bytes", "sha256"}:
            fail(f"{item_context} fields are not frozen")
        path = string(entry, "path", item_context)
        relative = pathlib.PurePosixPath(path)
        if relative.is_absolute() or ".." in relative.parts or path in paths:
            fail(f"{item_context}.path is unsafe or repeated")
        paths.add(path)
        if integer(entry, "bytes", item_context) < 0:
            fail(f"{item_context}.bytes must be non-negative")
        sha256(entry.get("sha256"), f"{item_context}.sha256")
    if (
        integer(tree, "file_count", f"{context}.tree") != len(files)
        or integer(tree, "bytes", f"{context}.tree")
        != sum(entry["bytes"] for entry in files)
        or sha256(tree.get("sha256"), f"{context}.tree.sha256")
        != canonical_sha256(files)
        or "reference.json" not in paths
    ):
        fail(f"{context}.tree identity mismatch")
    views = array(value, "views", context)
    seen: set[int] = set()
    for index, view in enumerate(views):
        view_context = f"{context}.views[{index}]"
        if not isinstance(view, dict) or set(view) != {
            "trace_frame_index",
            "path",
            "sha256",
            "decoded_rgba8_sha256",
            "pose_intrinsics_sha256",
        }:
            fail(f"{view_context} fields are not frozen")
        trace = integer(view, "trace_frame_index", view_context)
        path = string(view, "path", view_context)
        if trace not in {0, 1} or trace in seen or path not in paths:
            fail(f"{view_context} trace/path identity mismatch")
        seen.add(trace)
        sha256(view.get("sha256"), f"{view_context}.sha256")
        sha256(view.get("decoded_rgba8_sha256"), f"{view_context}.decoded_rgba8_sha256")
        sha256(view.get("pose_intrinsics_sha256"), f"{view_context}.pose_intrinsics_sha256")
    if seen != {0, 1}:
        fail(f"{context}.views must bind both frozen traces")
    return value


def _retained_authority_identity(root: pathlib.Path) -> dict[str, Any]:
    # Import lazily because artifacts owns the semantic authority parser and
    # imports this module's frozen schedule constants.
    from .artifacts import reference_authority

    authority_root = root / "reference-authority"
    authority = reference_authority(authority_root)
    return {
        "root_path": "reference-authority",
        "receipt_path": "reference.json",
        "receipt_sha256": authority["receipt_sha256"],
        "repository_commit": authority["repository_commit"],
        "release_binary_sha256": authority["release_binary_sha256"],
        "generated_at_utc": authority["generated_at_utc"],
        "tree": authority["tree"],
        "views": [
            {
                "trace_frame_index": trace,
                "path": pathlib.Path(authority["views"][trace]["path"])
                .relative_to(authority_root)
                .as_posix(),
                "sha256": authority["views"][trace]["sha256"],
                "decoded_rgba8_sha256": authority["views"][trace][
                    "decoded_rgba8_sha256"
                ],
                "pose_intrinsics_sha256": authority["views"][trace][
                    "pose_intrinsics_sha256"
                ],
            }
            for trace in (0, 1)
        ],
    }


def validate_orchestration(
    document: dict[str, Any],
    root: pathlib.Path,
    *,
    series_id: str,
    schedule_sha: str,
    protocol_sha: str,
) -> dict[str, Any] | None:
    """Validate the optional one-shot formal lock and return it for endpoint joins."""

    orchestration = document.get("orchestration")
    if orchestration is None:
        return None
    if not isinstance(orchestration, dict):
        fail("schedule.orchestration must be an object")
    locked = obj(orchestration, "formal_execution_lock", "schedule.orchestration")
    post = obj(orchestration, "post_run_verification", "schedule.orchestration")
    if locked.get("schema") != FORMAL_LOCK_SCHEMA:
        fail("schedule orchestration formal lock schema mismatch")
    reviewed = string(locked, "reviewed_commit", "schedule.orchestration.formal_execution_lock")
    if len(reviewed) != 40 or any(character not in "0123456789abcdef" for character in reviewed):
        fail("schedule orchestration reviewed_commit must be a full lowercase Git SHA")
    expected_identity = {
        "series_id": series_id,
        "schedule_sha256": schedule_sha,
        "protocol_sha256": protocol_sha,
    }
    for key, expected in expected_identity.items():
        if locked.get(key) != expected:
            fail(f"schedule orchestration formal lock {key} mismatch")
    formal = obj(locked, "formal_inputs", "schedule.orchestration.formal_execution_lock")
    if formal.get("schema") != FORMAL_INPUTS_SCHEMA or formal.get("reviewed_commit") != reviewed:
        fail("schedule orchestration formal input identity mismatch")
    if sha256(locked.get("formal_inputs_sha256"), "formal_inputs_sha256") != canonical_sha256(formal):
        fail("schedule orchestration formal input digest mismatch")
    git = obj(formal, "git", "schedule.orchestration.formal_inputs")
    if git != {"head": reviewed, "clean": True}:
        fail("schedule orchestration did not lock one clean reviewed commit")
    browser = _hashed_entry(formal.get("browser"), "schedule.orchestration.browser")
    toolchains = array(formal, "toolchains", "schedule.orchestration.formal_inputs")
    if {value.get("name") for value in toolchains if isinstance(value, dict)} != {
        "node",
        "python",
    }:
        fail("schedule orchestration does not lock Node and Python executables")
    for index, value in enumerate(toolchains):
        _hashed_entry(value, f"schedule.orchestration.toolchains[{index}]")
    wasm = obj(formal, "wasm_package", "schedule.orchestration.formal_inputs")
    string(wasm, "path", "schedule.orchestration.wasm_package")
    wasm_files = array(wasm, "files", "schedule.orchestration.wasm_package")
    expected_wasm = {"gsplat_web.js", "gsplat_web_bg.wasm", "gsplat_web_build_receipt.json"}
    if {value.get("name") for value in wasm_files if isinstance(value, dict)} != expected_wasm:
        fail("schedule orchestration WASM lock does not cover JS, WASM, and build receipt")
    for index, value in enumerate(wasm_files):
        _hashed_entry(value, f"schedule.orchestration.wasm_package.files[{index}]")
    repository_files = array(formal, "repository_files", "schedule.orchestration.formal_inputs")
    repository_trees = array(formal, "repository_trees", "schedule.orchestration.formal_inputs")
    puppeteer_modules = obj(
        formal,
        "puppeteer_production_modules",
        "schedule.orchestration.formal_inputs",
    )
    for index, value in enumerate(repository_files):
        _hashed_entry(value, f"schedule.orchestration.repository_files[{index}]")
    for index, value in enumerate(repository_trees):
        entry = _hashed_entry(value, f"schedule.orchestration.repository_trees[{index}]")
        integer(entry, "file_count", f"schedule.orchestration.repository_trees[{index}]")
    locked_file_paths = {value["path"] for value in repository_files}
    locked_tree_paths = {value["path"] for value in repository_trees}
    if locked_file_paths != LOCKED_REPOSITORY_FILES or locked_tree_paths != LOCKED_REPOSITORY_TREES:
        fail("schedule orchestration omits a producer/validator/dataset input")
    module_packages = array(
        puppeteer_modules,
        "packages",
        "schedule.orchestration.puppeteer_production_modules",
    )
    if (
        puppeteer_modules.get("schema") != PUPPETEER_GRAPH_SCHEMA
        or puppeteer_modules.get("root") != "node_modules/puppeteer-core"
        or puppeteer_modules.get("package_count") != len(module_packages)
        or not module_packages
        or puppeteer_modules.get("package_lock_sha256")
        != next(value["sha256"] for value in repository_files if value["path"].endswith("package-lock.json"))
    ):
        fail("schedule orchestration Puppeteer production module graph is incomplete")
    for index, value in enumerate(module_packages):
        entry = _hashed_entry(
            value,
            f"schedule.orchestration.puppeteer_production_modules.packages[{index}]",
        )
        string(entry, "lock_path", "Puppeteer production module")
        integer(entry, "file_count", "Puppeteer production module")
        dependencies = array(
            entry,
            "runtime_dependencies",
            f"schedule.orchestration.puppeteer_production_modules.packages[{index}]",
        )
        for dependency_index, dependency in enumerate(dependencies):
            if not isinstance(dependency, dict):
                fail(f"Puppeteer runtime dependency {dependency_index} must be an object")
            string(dependency, "name", "Puppeteer runtime dependency")
            string(dependency, "lock_path", "Puppeteer runtime dependency")
            if not isinstance(dependency.get("required"), bool):
                fail("Puppeteer runtime dependency required flag must be boolean")
    package_paths = {value["lock_path"] for value in module_packages}
    if "node_modules/puppeteer-core" not in package_paths or any(
        dependency["lock_path"] not in package_paths
        for value in module_packages
        for dependency in value["runtime_dependencies"]
    ):
        fail("schedule orchestration Puppeteer production module graph is not closed")
    if puppeteer_modules.get("sha256") != canonical_sha256(module_packages):
        fail("schedule orchestration Puppeteer production module digest mismatch")
    formal_authority = _authority_identity(
        formal.get("reference_authority"),
        "schedule.orchestration.formal_inputs.reference_authority",
    )
    if formal_authority["repository_commit"] != reviewed:
        fail("schedule orchestration reference authority commit mismatch")
    references = array(formal, "references", "schedule.orchestration.formal_inputs")
    scheduled_references = {
        value.get("trace_frame_index"): value
        for value in array(obj(document, "schedule", "schedule"), "reference_images", "schedule.schedule")
        if isinstance(value, dict)
    }
    if len(references) != 2:
        fail("schedule orchestration must lock two decoded references")
    for index, value in enumerate(references):
        if not isinstance(value, dict):
            fail(f"schedule orchestration reference {index} must be an object")
        trace = integer(value, "trace_frame_index", f"schedule.orchestration.references[{index}]")
        scheduled = scheduled_references.get(trace)
        if trace not in {0, 1} or scheduled is None or value.get("series_path") != scheduled.get("path"):
            fail("schedule orchestration reference path/trace mismatch")
        if sha256(value.get("sha256"), "reference sha256") != scheduled.get("sha256"):
            fail("schedule orchestration reference digest mismatch")
        authority_view = next(
            item for item in formal_authority["views"]
            if item["trace_frame_index"] == trace
        )
        expected_series_path = f"reference-authority/{authority_view['path']}"
        expected_receipt_path = "reference-authority/reference.json"
        if (
            value.get("sha256") != authority_view["sha256"]
            or value.get("rgba8_sha256") != authority_view["decoded_rgba8_sha256"]
            or value.get("pose_intrinsics_sha256") != authority_view["pose_intrinsics_sha256"]
            or value.get("series_path") != expected_series_path
            or scheduled.get("path") != expected_series_path
            or value.get("authority_receipt_path") != expected_receipt_path
            or scheduled.get("authority_receipt_path") != expected_receipt_path
            or value.get("authority_receipt_sha256")
            != formal_authority["receipt_sha256"]
            or scheduled.get("decoded_rgba8_sha256")
            != authority_view["decoded_rgba8_sha256"]
            or scheduled.get("pose_intrinsics_sha256")
            != authority_view["pose_intrinsics_sha256"]
            or scheduled.get("authority_receipt_sha256")
            != formal_authority["receipt_sha256"]
        ):
            fail("schedule orchestration reference authority join mismatch")
        if (value.get("width"), value.get("height"), value.get("pixel_format")) != (
            WIDTH,
            HEIGHT,
            "rgba8unorm-srgb",
        ):
            fail("schedule orchestration reference was not decoded as frozen RGBA8 1080p")
    command = obj(locked, "command_receipt", "schedule.orchestration.formal_execution_lock")
    command_path = inside(root, command.get("path"), "schedule orchestration command receipt")
    command_sha = sha256(command.get("sha256"), "schedule orchestration command receipt sha256")
    if file_sha256(command_path) != command_sha or command.get("invocation_count") != 30:
        fail("schedule orchestration command receipt identity mismatch")
    commands = load_json(command_path, "schedule orchestration command receipt")
    expected_commands_authority = {
        "source": formal_authority,
        "series_root": "reference-authority",
        "series_receipt_path": "reference-authority/reference.json",
        "series_files": [
            {
                **entry,
                "destination": f"reference-authority/{entry['path']}",
            }
            for entry in formal_authority["tree"]["files"]
        ],
    }
    if (
        commands.get("schema") != COMMANDS_SCHEMA
        or commands.get("reviewed_commit") != reviewed
        or commands.get("formal_inputs_sha256") != locked["formal_inputs_sha256"]
        or commands.get("schedule_sha256") != schedule_sha
        or commands.get("protocol_sha256") != protocol_sha
        or commands.get("invocation_count") != 30
        or len(commands.get("invocations", [])) != 30
        or commands.get("reference_authority") != expected_commands_authority
    ):
        fail("schedule orchestration command receipt is not frozen to this series")
    _validate_command_environments(commands, browser["path"], root)
    if locked.get("timeouts_seconds") != PROCESS_TIMEOUTS_SECONDS:
        fail("schedule orchestration execution lock timeouts are not frozen")
    authority_lock = obj(
        locked, "reference_authority", "schedule.orchestration.formal_execution_lock"
    )
    claimed_pre = _authority_identity(
        authority_lock.get("claimed_pre"),
        "schedule.orchestration.formal_execution_lock.reference_authority.claimed_pre",
        expected_root="reference-authority",
    )
    expected_claimed = {**formal_authority, "root_path": "reference-authority"}
    if (
        set(authority_lock)
        != {"source_pre_sha256", "claimed_pre", "claimed_pre_sha256"}
        or authority_lock.get("source_pre_sha256") != canonical_sha256(formal_authority)
        or claimed_pre != expected_claimed
        or authority_lock.get("claimed_pre_sha256") != canonical_sha256(claimed_pre)
    ):
        fail("schedule orchestration formal authority lock mismatch")
    if claimed_pre != _retained_authority_identity(root):
        fail("schedule orchestration retained authority does not match its formal lock")
    lock_path = root / "formal-execution-lock.json"
    if load_json(lock_path, "formal execution lock file") != locked:
        fail("schedule orchestration embedded/file lock mismatch")
    if post.get("schema") != POST_RUN_SCHEMA:
        fail("schedule orchestration post-run schema mismatch")
    post_git = obj(post, "git", "schedule.orchestration.post_run_verification")
    if (
        post.get("reviewed_commit") != reviewed
        or post_git != {"head": reviewed, "clean": True}
        or post.get("formal_inputs_sha256") != locked["formal_inputs_sha256"]
        or post.get("command_receipt_sha256") != command_sha
        or post.get("formal_lock_sha256") != file_sha256(lock_path)
    ):
        fail("schedule orchestration post-run verification does not rebind the formal lock")
    post_authority = obj(
        post, "reference_authority", "schedule.orchestration.post_run_verification"
    )
    if post_authority != {
        "source_pre_sha256": authority_lock["source_pre_sha256"],
        "source_post_sha256": authority_lock["source_pre_sha256"],
        "claimed_pre_sha256": authority_lock["claimed_pre_sha256"],
        "claimed_post_sha256": authority_lock["claimed_pre_sha256"],
        "receipt_sha256": formal_authority["receipt_sha256"],
        "tree_sha256": formal_authority["tree"]["sha256"],
    }:
        fail("schedule orchestration post-run authority join mismatch")
    utc(post.get("verified_at_utc"), "schedule.orchestration.post_run_verification.verified_at_utc")
    return {
        "reviewed_commit": reviewed,
        "formal_inputs_sha256": locked["formal_inputs_sha256"],
        "command_receipt_sha256": command_sha,
        "browser": browser,
        "wasm_files": wasm_files,
        "repository_files": repository_files,
        "repository_trees": repository_trees,
        "puppeteer_production_modules": puppeteer_modules,
        "reference_authority": formal_authority,
    }


def validate_terminal(q1: dict[str, Any], summary: dict[str, Any], context: str) -> float:
    terminal = obj(q1, "terminal_window", context)
    expected = {
        "schema": TERMINAL_SCHEMA,
        "clock": "performance_now_monotonic",
        "start_boundary": "first_measured_camera_input_accepted",
        "end_boundary": "final_measured_gpu_queue_completion",
        "completion_primitive": "gpu_queue_on_submitted_work_done",
        "frame_loop_policy": "controlled_presented_raf",
        "camera_mutation_point": "before_update_order_project_render",
        "warmup_queue_drained": True,
        "continuous_submissions": True,
        "per_frame_observer_reads": 0,
        "extra_submissions_during_terminal_drain": 0,
        "measured_camera_input_count": MEASURED,
        "measured_submission_count": MEASURED,
        "dropped_frame_count": 0,
        "submission_counter_stable_during_drain": True,
    }
    for key, value in expected.items():
        if terminal.get(key) != value:
            fail(f"{context}.terminal_window.{key} must equal {value!r}")
    before = integer(terminal, "submission_counter_before_first", f"{context}.terminal_window")
    after = integer(terminal, "submission_counter_after_last", f"{context}.terminal_window")
    if after - before != MEASURED:
        fail(f"{context}.terminal_window does not prove exactly 80 submissions")
    start = number(terminal, "started_at_monotonic_ms", f"{context}.terminal_window")
    end = number(terminal, "completed_at_monotonic_ms", f"{context}.terminal_window")
    duration = number(terminal, "duration_ms", f"{context}.terminal_window")
    if not start < end or not math.isclose(duration, end - start, abs_tol=1e-6):
        fail(f"{context}.terminal_window has inconsistent boundaries")
    sustained = obj(summary, "sustained_throughput", context.replace("manifest", "summary"))
    expected_metrics = {
        "measured_frame_count": MEASURED,
        "terminal_window_ms": duration,
        "mean_frame_ms": duration / MEASURED,
        "mean_fps": 1000 * MEASURED / duration,
    }
    for key, value in expected_metrics.items():
        observed = sustained.get(key)
        if not isinstance(observed, (int, float)) or isinstance(observed, bool) or not math.isclose(
            float(observed), float(value), rel_tol=1e-9, abs_tol=1e-9
        ):
            fail(f"{context} summary.sustained_throughput.{key} mismatch")
    return duration
