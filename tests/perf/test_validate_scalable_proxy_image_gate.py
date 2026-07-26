#!/usr/bin/env python3

from __future__ import annotations

import binascii
import copy
import hashlib
import importlib.util
import io
import json
import pathlib
import shutil
import struct
import sys
import tempfile
import unittest
import zlib
from contextlib import redirect_stdout


ROOT = pathlib.Path(__file__).resolve().parents[2]
VALIDATOR_PATH = ROOT / "tests/perf/validate-scalable-proxy-image-gate.py"
TRACE_V1_PATH = ROOT / "tests/perf/trace/trace_v1.py"


def load_module(name: str, path: pathlib.Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    try:
        spec.loader.exec_module(module)
    finally:
        sys.modules.pop(name, None)
    return module


VALIDATOR = load_module("scalable_proxy_validator_test", VALIDATOR_PATH)
TRACE_V1 = load_module("scalable_proxy_trace_v1_test", TRACE_V1_PATH)
BALANCED = VALIDATOR.load_module(
    "scalable_proxy_balanced_test_dependency", VALIDATOR.BALANCED_VALIDATOR_PATH
)


def write_json(path: pathlib.Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: pathlib.Path) -> str:
    return sha256_bytes(path.read_bytes())


def canonical_sha256(value: object) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return sha256_bytes(encoded)


def png_chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", binascii.crc32(kind + payload) & 0xFFFFFFFF)
    )


def rgba_png(width: int, height: int, rgba: bytes) -> bytes:
    rows = b"".join(
        b"\x00" + rgba[offset : offset + width * 4]
        for offset in range(0, len(rgba), width * 4)
    )
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + png_chunk(b"IDAT", zlib.compress(rows))
        + png_chunk(b"IEND", b"")
    )


def make_trace(width: int, height: int, trace_id: str) -> dict:
    frames = []
    for index, x in enumerate((0.0, 0.25)):
        position = [x, 0.0, -3.0]
        rotation = [0.0, 0.0, 0.0, 1.0]
        fov = 1.0
        near = 0.01
        far = 100.0
        view = TRACE_V1.view_matrix(position, rotation)
        projection = TRACE_V1.projection_matrix(fov, near, far, width / height)
        frames.append(
            {
                "frame_index": index,
                "timestamp_ns": index * 1_000_000,
                "pose": {"position": position, "rotation_xyzw": rotation},
                "intrinsics": {
                    "vertical_fov_radians": fov,
                    "near_plane": near,
                    "far_plane": far,
                },
                "view_matrix": view,
                "projection_matrix": projection,
                "view_projection_matrix": TRACE_V1.mat4_multiply(projection, view),
            }
        )
    return TRACE_V1.with_content_hash(
        {
            "schema": "gsplat-camera-trace/v1",
            "trace_id": trace_id,
            "coordinate_system": {
                "handedness": "right",
                "axes": "RUF",
                "camera_forward": "+Z",
            },
            "matrix_convention": {
                "storage_order": "row-major",
                "vector_convention": "column",
                "composition": "projection * view * world_position",
                "ndc_xy": "[-1,1]",
                "ndc_z": "[0,1]",
                "clip_w": "camera_z",
            },
            "display": {"width": width, "height": height},
            "frames": frames,
        }
    )


def image_receipt(root: pathlib.Path, relative: str, data: bytes, width: int, height: int) -> dict:
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    return {
        "path": relative,
        "sha256": sha256_bytes(data),
        "width": width,
        "height": height,
    }


def benchmark_distribution(value: float, count: int) -> dict:
    return {
        "count": count,
        "mean": value,
        "p50": value,
        "p90": value,
        "p95": value,
        "p99": value,
        "max": value,
    }


def write_benchmark_artifact(
    root: pathlib.Path,
    *,
    unique: str,
    lane: str,
    authority: dict,
    endpoint: dict,
    comparison: dict,
    presentation: dict,
    image: dict,
    active: int,
) -> dict:
    run_id = f"fixture-{lane}-{unique}"
    artifact = root / "benchmark" / run_id
    artifact.mkdir(parents=True)
    image_data = (root / image["path"]).read_bytes()
    (artifact / "final-frame.png").write_bytes(image_data)
    manifest = {
        "schema": "gsplat-benchmark/v1",
        "record_type": "manifest",
        "run_id": run_id,
        "identity": {
            "series_id": f"proxy-fixture-{unique}",
            "started_at_utc": "2026-07-27T00:00:00Z",
            "ended_at_utc": "2026-07-27T00:00:01Z",
            "measurement_started_at_utc": "2026-07-27T00:00:00Z",
            "measurement_ended_at_utc": "2026-07-27T00:00:01Z",
        },
        "build": {
            "repository_commit": "a" * 40,
            "dirty": False,
            "profile": "release",
            "package_version": "0.1.3",
        },
        "dataset": {
            "id": authority["dataset_id"],
            "sha256": authority["source_sha256"],
            "bytes": authority["source_bytes"],
            "splat_count": authority["source_splats"],
            "sh_degree": 3,
        },
        "trace": {
            "id": endpoint["trace_id"],
            "sha256": endpoint["trace_content_sha256"],
        },
        "renderer": {
            "implementation": f"s1-fixture-{lane}",
            "path": "sorted_alpha_fixture",
            "backend": endpoint["backend"],
            "sort_policy": f"forced_{comparison['order_backend']}",
            "exact_plan_actual": "post_sort",
            "count_semantics": "candidate_visible_contributor_issued_v1",
        },
        "display": {
            "width": endpoint["width"],
            "height": endpoint["height"],
            "dpr": 1.0,
            "refresh_hz": 60.0,
            "frame_budget_ms": 16.6666667,
            "refresh_hz_source": "contract fixture",
            "frame_budget_source": "contract fixture",
        },
        "environment": {
            "platform": "fixture",
            "os": "fixture",
            "device": endpoint["id"],
            "browser": None,
            "adapter": endpoint["backend"],
            "driver": None,
        },
        "image": {
            "path": "final-frame.png",
            "sha256": image["sha256"],
            "width": endpoint["width"],
            "height": endpoint["height"],
        },
        "unavailable_fields": [
            "environment.browser",
            "environment.driver",
            "frames[*].preprocess_ms",
            "frames[*].sort_ms",
            "frames[*].geometry_submit_ms",
            "frames[*].gpu_wait_ms",
            "frames[*].gpu_complete_ms",
        ],
    }
    write_json(artifact / "manifest.json", manifest)
    frame = {
        "schema": "gsplat-benchmark/v1",
        "record_type": "frame",
        "run_id": run_id,
        "frame_index": 0,
        "elapsed_ns": 1_000_000,
        "call_ms": 1.0,
        "frame_wall_ms": 1.0,
        "preprocess_ms": None,
        "sort_ms": None,
        "geometry_submit_ms": None,
        "gpu_wait_ms": None,
        "gpu_complete_ms": None,
        "visible": active,
        "contributor": active,
        "drawn": active,
        "exact_contributor_compaction": False,
        "active_splats": active,
        "sort_refreshed": True,
        "pair_id": comparison["pair_id"],
        "endpoint_id": comparison["endpoint_id"],
        "order_backend": comparison["order_backend"],
        "cut_name": comparison["cut_name"],
        "sequence": comparison["sequence"],
        "capture_index": comparison["capture_index"],
        "trace_frame_index": comparison["trace_frame_index"],
        "camera": copy.deepcopy(comparison["camera"]),
        "terminal_outcome": "presented",
        "presentation": copy.deepcopy(presentation),
        "global_plan": "post_sort",
    }
    (artifact / "frames.jsonl").write_text(
        json.dumps(frame, sort_keys=True) + "\n", encoding="utf-8"
    )
    metrics = (
        "call_ms",
        "frame_wall_ms",
        "preprocess_ms",
        "sort_ms",
        "geometry_submit_ms",
        "gpu_wait_ms",
        "gpu_complete_ms",
    )
    summary = {
        "schema": "gsplat-benchmark/v1",
        "record_type": "summary",
        "run_id": run_id,
        "sample_count": 1,
        "warmup_count": 0,
        "frame_budget_ms": 16.6666667,
        "missed_frame_count": 0,
        "distributions": {
            key: benchmark_distribution(1.0, 1)
            if key in {"call_ms", "frame_wall_ms"}
            else None
            for key in metrics
        },
    }
    write_json(artifact / "summary.json", summary)
    return {
        "path": artifact.relative_to(root).as_posix(),
        "sha256": BALANCED.artifact_directory_sha256(artifact),
        "run_id": run_id,
        "frame_index": 0,
    }


def coverage_receipt(
    name: str,
    source_sha256: str,
    hierarchy_sha256: str,
    source_splats: int,
    active: int,
) -> dict:
    node_ids = [f"{name}-node-0", f"{name}-node-1"]
    page_hashes = [sha256_bytes(f"{name}-page-0".encode())]
    return {
        "source_sha256": source_sha256,
        "hierarchy_manifest_sha256": hierarchy_sha256,
        "source_splat_count": source_splats,
        "represented_source_leaves": source_splats,
        "active_proxy_splats": active,
        "missing_leaves": 0,
        "overlap_count": 0,
        "missing_page_count": 0,
        "antichain_valid": True,
        "parent_descendant_overlap": False,
        "source_sh_degree": 3,
        "sh_representation": "source_sh3",
        "sampling": "disabled",
        "partial_child_publication": "disabled",
        "ordered_node_ids": node_ids,
        "ordered_node_list_sha256": canonical_sha256(node_ids),
        "page_sha256": page_hashes,
        "page_list_sha256": canonical_sha256(page_hashes),
        "replacement_count": 2 if name == "mixed_depth_two_replacements" else 0,
        "depth_count": 2 if name == "mixed_depth_two_replacements" else 1,
        "payload_bit_exact_to_source": name == "complete_leaf_exact",
    }


def build_fixture(root: pathlib.Path) -> dict:
    width = height = 8
    source_sha256 = "1" * 64
    dataset = {
        "schema": "gsplat-dataset/v1",
        "id": "s1-contract-fixture",
        "local_path": "contract-fixture.ply",
        "sha256": source_sha256,
        "bytes": 1024,
        "splat_count": 8,
        "sh_degree": 3,
    }
    dataset_path = root / "authority/dataset.json"
    write_json(dataset_path, dataset)
    hierarchy_path = root / "authority/hierarchy.bin"
    hierarchy_path.write_bytes(b"s1 hierarchy contract fixture")
    hierarchy_sha256 = sha256_file(hierarchy_path)
    authority_values = {
        "dataset_id": dataset["id"],
        "source_sha256": source_sha256,
        "source_bytes": dataset["bytes"],
        "source_splats": dataset["splat_count"],
    }

    endpoint_docs = []
    endpoint_values = {}
    for endpoint_id, backend in (
        ("apple_m4_metal", "metal"),
        ("nothing_a065_vulkan", "vulkan"),
    ):
        trace = make_trace(width, height, f"{endpoint_id}-fixture-trace")
        trace_path = root / f"authority/{endpoint_id}-trace.json"
        write_json(trace_path, trace)
        trace_receipt = {
            "scope": "artifact",
            "path": trace_path.relative_to(root).as_posix(),
            "sha256": sha256_file(trace_path),
            "trace_id": trace["trace_id"],
            "content_sha256": trace["content_sha256"],
        }
        endpoint_docs.append(
            {
                "id": endpoint_id,
                "status": "complete",
                "backend": backend,
                "resolution": {
                    **{
                        f"{stage}_{axis}": value
                        for stage in VALIDATOR.RESOLUTION_STAGES
                        for axis, value in (("width", width), ("height", height))
                    },
                    "dynamic_resolution": "disabled",
                    "upscaling": "disabled",
                    "full_resolution": True,
                },
                "surface_probe": {"status": "observed", "width": width, "height": height},
                "trace": trace_receipt,
            }
        )
        endpoint_values[endpoint_id] = {
            "id": endpoint_id,
            "backend": backend,
            "width": width,
            "height": height,
            "trace_id": trace["trace_id"],
            "trace_content_sha256": trace["content_sha256"],
            "trace": trace,
        }

    active_counts = {
        "complete_leaf_exact": 8,
        "bootstrap_roots": 2,
        "mixed_depth_two_replacements": 4,
    }
    cuts = []
    coverage_by_cut = {}
    for name in VALIDATOR.REQUIRED_CUTS:
        coverage = coverage_receipt(
            name, source_sha256, hierarchy_sha256, dataset["splat_count"], active_counts[name]
        )
        coverage_hash = canonical_sha256(coverage)
        coverage_by_cut[name] = coverage_hash
        cuts.append({"name": name, "coverage": coverage, "coverage_sha256": coverage_hash})

    manifest = {
        "schema": VALIDATOR.SCHEMA,
        "evidence_class": "contract_fixture",
        "contract": {
            "name": VALIDATOR.SCHEMA,
            "aggregation": "logical_all",
            "required_cuts": list(VALIDATOR.REQUIRED_CUTS),
            "required_order_backends": list(VALIDATOR.REQUIRED_ORDERS),
            "frame_metric_limits": VALIDATOR.FRAME_METRIC_LIMITS,
            "temporal_metric": {
                "name": VALIDATOR.TEMPORAL_METRIC,
                "maximum": VALIDATOR.TEMPORAL_LIMIT,
            },
        },
        "validator_dependencies": {
            "balanced_image_gate_sha256": VALIDATOR.BALANCED_VALIDATOR_SHA256,
            "benchmark_artifact_validator_sha256": VALIDATOR.BENCHMARK_VALIDATOR_SHA256,
        },
        "authority": {
            "dataset_manifest": {
                "scope": "artifact",
                "path": dataset_path.relative_to(root).as_posix(),
                "sha256": sha256_file(dataset_path),
            },
            "camera": {
                "path": "external/fixture-cameras.json",
                "sha256": "2" * 64,
                "bytes": 256,
                "entry_count": 2,
                "selected_camera_ids": [0, 146],
                "review": {},
                "review_sha256": "0" * 64,
            },
            "hierarchy_manifest": {
                "scope": "artifact",
                "path": hierarchy_path.relative_to(root).as_posix(),
                "sha256": hierarchy_sha256,
            },
            "builder": {
                "repository_commit": "b" * 40,
                "configuration_sha256": "3" * 64,
            },
        },
        "exact_reference": {},
        "cuts": cuts,
        "endpoints": endpoint_docs,
        "comparisons": [],
        "transitions": [],
    }
    camera_review = {
                    "status": "approved",
                    "source_sha256": source_sha256,
                    "camera_metadata_sha256": "2" * 64,
                    "selected_camera_ids": [0, 146],
        "trace_receipts": [
            {
                "endpoint_id": endpoint["id"],
                "trace_file_sha256": endpoint["trace"]["sha256"],
                "trace_content_sha256": endpoint["trace"]["content_sha256"],
            }
            for endpoint in endpoint_docs
        ],
    }
    manifest["authority"]["camera"]["review"] = camera_review
    manifest["authority"]["camera"]["review_sha256"] = canonical_sha256(camera_review)
    exactness = {
        "source_splat_count": dataset["splat_count"],
        "decoded_splat_count": dataset["splat_count"],
        "encoded_splat_count": dataset["splat_count"],
        "resident_splat_count": dataset["splat_count"],
        "addressable_splat_count": dataset["splat_count"],
        "source_sh_degree": 3,
        "resident_sh_degree": 3,
        "source_membership": "all",
        "sampling": "disabled",
        "lod": "disabled",
        "sh_degree_policy": "source",
        "render_mode": "sorted_alpha",
        "partial_scene_published": False,
        "full_quality": True,
    }
    exact_reference_sha256 = canonical_sha256(exactness)
    manifest["exact_reference"] = {
        "exactness": exactness,
        "exactness_sha256": exact_reference_sha256,
    }

    exact_data = rgba_png(width, height, bytes([64, 96, 128, 255]) * (width * height))
    comparison_counter = 0
    replacement_cuts = (
        "bootstrap_roots",
        "mixed_depth_two_replacements",
        "complete_leaf_exact",
    )
    for endpoint_id in VALIDATOR.REQUIRED_ENDPOINTS:
        endpoint = endpoint_values[endpoint_id]
        for order in VALIDATOR.REQUIRED_ORDERS:
            specs = []
            for cut_name in VALIDATOR.REQUIRED_CUTS:
                specs.extend((cut_name, "authored_views", index, trace_index) for index, trace_index in enumerate((0, 1)))
                specs.extend((cut_name, "moving_sequence", index, trace_index) for index, trace_index in enumerate((0, 1, 0)))
            specs.extend((cut_name, "replacement_sequence", index, 0) for index, cut_name in enumerate(replacement_cuts))
            for cut_name, sequence, capture_index, trace_index in specs:
                unique = f"{comparison_counter:03d}"
                comparison_counter += 1
                trace_frame = endpoint["trace"]["frames"][trace_index]
                camera = {
                    "trace_id": endpoint["trace_id"],
                    "trace_content_sha256": endpoint["trace_content_sha256"],
                    "pose_intrinsics_sha256": canonical_sha256(
                        {"pose": trace_frame["pose"], "intrinsics": trace_frame["intrinsics"]}
                    ),
                }
                pair_id = f"pair-{unique}"
                presentation = {
                    lane: {
                        "outcome": "presented",
                        "primitive_presented": True,
                        "ticket": comparison_counter * 2 + lane_index,
                        "scene_generation": 1,
                        "camera_generation": capture_index + 1,
                        "viewport_generation": 1,
                        "contract_generation": 1,
                        "plan_generation": 1,
                        "presentation_generation": comparison_counter,
                    }
                    for lane_index, lane in enumerate(("exact", "proxy"), 1)
                }
                exact_image = image_receipt(
                    root, f"images/{unique}-exact.png", exact_data, width, height
                )
                proxy_image = image_receipt(
                    root, f"images/{unique}-proxy.png", exact_data, width, height
                )
                comparison = {
                    "pair_id": pair_id,
                    "endpoint_id": endpoint_id,
                    "order_backend": order,
                    "cut_name": cut_name,
                    "sequence": sequence,
                    "capture_index": capture_index,
                    "trace_frame_index": trace_index,
                    "exact_reference_sha256": exact_reference_sha256,
                    "camera": camera,
                    "presentation": presentation,
                    "presented_cut": {
                        "source_sha256": source_sha256,
                        "hierarchy_manifest_sha256": hierarchy_sha256,
                        "cut_name": cut_name,
                        "coverage_sha256": coverage_by_cut[cut_name],
                        "order_backend": order,
                        "outcome": "presented",
                        "presentation": copy.deepcopy(presentation["proxy"]),
                        "coverage_generation": comparison_counter,
                        "source_splat_count": dataset["splat_count"],
                        "represented_source_leaves": dataset["splat_count"],
                        "active_proxy_splats": active_counts[cut_name],
                        "visible": active_counts[cut_name],
                        "contributor": active_counts[cut_name],
                        "drawn": active_counts[cut_name],
                        "exact_contributor_compaction": False,
                        "global_plan": "post_sort",
                    },
                    "images": {"exact": exact_image, "proxy": proxy_image},
                    "metrics": {
                        "ssim_luma_srgb_window8": 1.0,
                        "rgb_mae_normalized": 0.0,
                        "rgb_bad_pixel_fraction_over_3": 0.0,
                        "alpha_mae_normalized": 0.0,
                        "alpha_bad_pixel_fraction_over_1": 0.0,
                    },
                }
                comparison["benchmark_artifacts"] = {
                    "pair_id": pair_id,
                    "exact": write_benchmark_artifact(
                        root,
                        unique=unique,
                        lane="exact",
                        authority=authority_values,
                        endpoint=endpoint,
                        comparison=comparison,
                        presentation=presentation["exact"],
                        image=exact_image,
                        active=dataset["splat_count"],
                    ),
                    "proxy": write_benchmark_artifact(
                        root,
                        unique=unique,
                        lane="proxy",
                        authority=authority_values,
                        endpoint=endpoint,
                        comparison=comparison,
                        presentation=presentation["proxy"],
                        image=proxy_image,
                        active=active_counts[cut_name],
                    ),
                }
                manifest["comparisons"].append(comparison)

    for endpoint_id in VALIDATOR.REQUIRED_ENDPOINTS:
        for order in VALIDATOR.REQUIRED_ORDERS:
            for cut_name in VALIDATOR.REQUIRED_CUTS:
                for from_index, to_index in ((0, 1), (1, 2)):
                    manifest["transitions"].append(
                        {
                            "endpoint_id": endpoint_id,
                            "order_backend": order,
                            "cut_name": cut_name,
                            "sequence": "moving_sequence",
                            "from_capture_index": from_index,
                            "to_capture_index": to_index,
                            "metrics": {VALIDATOR.TEMPORAL_METRIC: 0.0},
                        }
                    )
            for from_index, to_index in ((0, 1), (1, 2)):
                manifest["transitions"].append(
                    {
                        "endpoint_id": endpoint_id,
                        "order_backend": order,
                        "cut_name": replacement_cuts[from_index],
                        "sequence": "replacement_sequence",
                        "from_capture_index": from_index,
                        "to_capture_index": to_index,
                        "metrics": {VALIDATOR.TEMPORAL_METRIC: 0.0},
                    }
                )
    return manifest


class ScalableProxyImageGateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.temporary = tempfile.TemporaryDirectory()
        cls.root = pathlib.Path(cls.temporary.name)
        cls.manifest = build_fixture(cls.root)

    @classmethod
    def tearDownClass(cls) -> None:
        cls.temporary.cleanup()

    def validate_manifest(self, manifest: dict, name: str = "gate.json"):
        path = self.root / name
        write_json(path, manifest)
        return VALIDATOR.validate(path)

    def replace_proxy_image(
        self, manifest: dict, comparison: dict, rgba: bytes, slug: str
    ) -> None:
        width = comparison["images"]["proxy"]["width"]
        height = comparison["images"]["proxy"]["height"]
        data = rgba_png(width, height, rgba)
        replacement = image_receipt(
            self.root, f"mutations/{slug}.png", data, width, height
        )
        comparison["images"]["proxy"] = replacement
        original_artifact = self.root / comparison["benchmark_artifacts"]["proxy"]["path"]
        mutated_artifact = self.root / f"mutations/{slug}-benchmark"
        if mutated_artifact.exists():
            shutil.rmtree(mutated_artifact)
        shutil.copytree(original_artifact, mutated_artifact)
        (mutated_artifact / "final-frame.png").write_bytes(data)
        benchmark_manifest = json.loads((mutated_artifact / "manifest.json").read_text())
        benchmark_manifest["image"]["sha256"] = replacement["sha256"]
        write_json(mutated_artifact / "manifest.json", benchmark_manifest)
        comparison["benchmark_artifacts"]["proxy"].update(
            {
                "path": mutated_artifact.relative_to(self.root).as_posix(),
                "sha256": BALANCED.artifact_directory_sha256(mutated_artifact),
                "run_id": benchmark_manifest["run_id"],
            }
        )
        exact = BALANCED.load_image(
            self.root,
            comparison["images"]["exact"],
            (width, height),
            "test.exact",
        )
        proxy = BALANCED.load_image(
            self.root, replacement, (width, height), "test.proxy"
        )
        comparison["metrics"] = BALANCED.compute_frame_metrics(exact, proxy)

    def test_complete_contract_fixture_validates_without_promoting_s1(self) -> None:
        result = self.validate_manifest(copy.deepcopy(self.manifest))
        self.assertEqual(result.evidence_class, "contract_fixture")
        self.assertEqual(result.endpoint_count, 2)
        self.assertEqual(result.comparison_count, 72)
        self.assertEqual(result.transition_count, 32)

    def test_cli_emits_validated_fixture_not_accepted(self) -> None:
        path = self.root / "gate-cli.json"
        write_json(path, self.manifest)
        output = io.StringIO()
        with redirect_stdout(output):
            self.assertEqual(VALIDATOR.main([str(path)]), 0)
        receipt = json.loads(output.getvalue())
        self.assertEqual(receipt["decision"], "ValidatedFixture")
        self.assertIs(receipt["pass"], True)
        self.assertEqual(receipt["validator"]["balanced_image_gate_sha256"], VALIDATOR.BALANCED_VALIDATOR_SHA256)

    def test_cli_emits_explicit_rejected_and_deferred_decisions(self) -> None:
        rejected = copy.deepcopy(self.manifest)
        rejected["cuts"][0]["coverage_sha256"] = "0" * 64
        rejected_path = self.root / "cli-rejected.json"
        write_json(rejected_path, rejected)
        rejected_output = io.StringIO()
        with redirect_stdout(rejected_output):
            self.assertEqual(VALIDATOR.main([str(rejected_path)]), 1)
        self.assertEqual(json.loads(rejected_output.getvalue())["decision"], "Rejected")

        deferred = copy.deepcopy(self.manifest)
        deferred["endpoints"].pop()
        deferred_path = self.root / "cli-deferred.json"
        write_json(deferred_path, deferred)
        deferred_output = io.StringIO()
        with redirect_stdout(deferred_output):
            self.assertEqual(VALIDATOR.main([str(deferred_path)]), 2)
        self.assertEqual(json.loads(deferred_output.getvalue())["decision"], "Deferred")

    def test_contract_fixture_cannot_be_relabelled_as_formal_quality(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["evidence_class"] = "formal_quality"
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "frozen Bonsai authority"):
            self.validate_manifest(manifest, "fixture-relabeled-formal.json")

    def test_missing_required_endpoint_is_deferred(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["endpoints"].pop()
        with self.assertRaisesRegex(VALIDATOR.DeferredEvidence, "nothing_a065"):
            self.validate_manifest(manifest, "missing-endpoint.json")

    def test_unavailable_camera_review_is_deferred(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        review = {
            "status": "unavailable",
            "reason": "manual authored-view review receipt is absent",
        }
        manifest["authority"]["camera"]["review"] = review
        manifest["authority"]["camera"]["review_sha256"] = canonical_sha256(review)
        with self.assertRaisesRegex(VALIDATOR.DeferredEvidence, "manual authored-view"):
            self.validate_manifest(manifest, "deferred-review.json")

    def test_coverage_hash_and_page_hashes_are_recomputed(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["cuts"][1]["coverage"]["active_proxy_splats"] = 3
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "coverage_sha256 mismatch"):
            self.validate_manifest(manifest, "bad-coverage-hash.json")

        manifest = copy.deepcopy(self.manifest)
        coverage = manifest["cuts"][1]["coverage"]
        coverage["page_sha256"][0] = "9" * 64
        manifest["cuts"][1]["coverage_sha256"] = canonical_sha256(coverage)
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "page_list_sha256 mismatch"):
            self.validate_manifest(manifest, "bad-page-hash.json")

    def test_unavailable_or_invalid_vcd_is_rejected(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["comparisons"][0]["presented_cut"]["visible"] = None
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "available non-negative integer"):
            self.validate_manifest(manifest, "unavailable-vcd.json")

        manifest = copy.deepcopy(self.manifest)
        manifest["comparisons"][0]["presented_cut"]["drawn"] -= 1
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "D=V"):
            self.validate_manifest(manifest, "invalid-vcd.json")

    def test_camera_resolution_and_canonical_hash_mismatches_reject(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["comparisons"][0]["camera"]["pose_intrinsics_sha256"] = "0" * 64
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "pose/intrinsics"):
            self.validate_manifest(manifest, "bad-camera.json")

        manifest = copy.deepcopy(self.manifest)
        manifest["endpoints"][0]["resolution"]["presented_width"] = 7
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "dimensions must match"):
            self.validate_manifest(manifest, "bad-resolution.json")

        manifest = copy.deepcopy(self.manifest)
        manifest["comparisons"][0]["benchmark_artifacts"]["proxy"]["sha256"] = "0" * 64
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "benchmark artifact SHA-256 mismatch"):
            self.validate_manifest(manifest, "bad-canonical-hash.json")

    def test_missing_or_misjoined_canonical_lane_rejects(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        del manifest["comparisons"][0]["benchmark_artifacts"]["proxy"]
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "proxy must be an object"):
            self.validate_manifest(manifest, "missing-canonical-lane.json")

        manifest = copy.deepcopy(self.manifest)
        manifest["comparisons"][0]["benchmark_artifacts"]["pair_id"] = "wrong-pair"
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "pair_id mismatch"):
            self.validate_manifest(manifest, "misjoined-canonical-pair.json")

    def test_unknown_cut_and_missing_forced_order_reject(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["cuts"].pop()
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "exactly the three frozen"):
            self.validate_manifest(manifest, "missing-frozen-cut.json")

        manifest = copy.deepcopy(self.manifest)
        manifest["comparisons"][0]["cut_name"] = "tuned_after_images"
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "unknown endpoint, order backend, or cut"):
            self.validate_manifest(manifest, "unknown-cut.json")

        manifest = copy.deepcopy(self.manifest)
        manifest["comparisons"] = [
            comparison
            for comparison in manifest["comparisons"]
            if comparison["order_backend"] != "gpu"
        ]
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "capture matrix is incomplete"):
            self.validate_manifest(manifest, "missing-gpu-order.json")

    def test_incomplete_capture_or_transition_matrix_rejects(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        manifest["comparisons"].pop()
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "capture matrix is incomplete"):
            self.validate_manifest(manifest, "missing-comparison.json")

        manifest = copy.deepcopy(self.manifest)
        manifest["transitions"].pop()
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "transition matrix is incomplete"):
            self.validate_manifest(manifest, "missing-transition.json")

    def test_image_threshold_is_recomputed_per_comparison(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        comparison = manifest["comparisons"][0]
        width = comparison["images"]["proxy"]["width"]
        height = comparison["images"]["proxy"]["height"]
        self.replace_proxy_image(
            manifest,
            comparison,
            bytes([72, 104, 136, 255]) * (width * height),
            "bad-proxy",
        )
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "S1 v1 image gate"):
            self.validate_manifest(manifest, "bad-image-threshold.json")

        manifest = copy.deepcopy(self.manifest)
        manifest["comparisons"][0]["metrics"]["rgb_mae_normalized"] = 0.001
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "recomputed RGBA8 bytes"):
            self.validate_manifest(manifest, "bad-image-metric-receipt.json")

    def test_temporal_threshold_is_recomputed_and_applied(self) -> None:
        manifest = copy.deepcopy(self.manifest)
        transition = manifest["transitions"][0]
        transition["metrics"][VALIDATOR.TEMPORAL_METRIC] = 0.01
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "does not match retained"):
            self.validate_manifest(manifest, "bad-temporal-receipt.json")

        manifest = copy.deepcopy(self.manifest)
        moving = [
            comparison
            for comparison in manifest["comparisons"]
            if comparison["endpoint_id"] == "apple_m4_metal"
            and comparison["order_backend"] == "cpu"
            and comparison["cut_name"] == "complete_leaf_exact"
            and comparison["sequence"] == "moving_sequence"
        ]
        moving.sort(key=lambda comparison: comparison["capture_index"])
        width = moving[0]["images"]["proxy"]["width"]
        height = moving[0]["images"]["proxy"]["height"]
        self.replace_proxy_image(
            manifest,
            moving[0],
            bytes([65, 97, 129, 255]) * (width * height),
            "temporal-plus-one",
        )
        self.replace_proxy_image(
            manifest,
            moving[1],
            bytes([63, 95, 127, 255]) * (width * height),
            "temporal-minus-one",
        )
        exact_previous = BALANCED.load_image(
            self.root, moving[0]["images"]["exact"], (width, height), "temporal.exact0"
        )
        proxy_previous = BALANCED.load_image(
            self.root, moving[0]["images"]["proxy"], (width, height), "temporal.proxy0"
        )
        exact_current = BALANCED.load_image(
            self.root, moving[1]["images"]["exact"], (width, height), "temporal.exact1"
        )
        proxy_current = BALANCED.load_image(
            self.root, moving[1]["images"]["proxy"], (width, height), "temporal.proxy1"
        )
        actual = BALANCED.compute_temporal_metric(
            BALANCED.FramePixels(0, 0, exact_previous, proxy_previous),
            BALANCED.FramePixels(1, 1, exact_current, proxy_current),
        )
        self.assertGreater(actual, VALIDATOR.TEMPORAL_LIMIT)
        transition = next(
            item
            for item in manifest["transitions"]
            if item["endpoint_id"] == "apple_m4_metal"
            and item["order_backend"] == "cpu"
            and item["cut_name"] == "complete_leaf_exact"
            and item["sequence"] == "moving_sequence"
            and item["from_capture_index"] == 0
        )
        transition["metrics"][VALIDATOR.TEMPORAL_METRIC] = actual
        with self.assertRaisesRegex(VALIDATOR.ValidationError, "S1 v1 temporal gate"):
            self.validate_manifest(manifest, "bad-temporal-threshold.json")


if __name__ == "__main__":
    unittest.main()
