#!/usr/bin/env python3
"""Run the two Q1 view-000001 quality producers once, then evaluate them.

This is a quality-only transaction.  It never retries a producer, never emits
performance evidence, and publishes the requested destination only after the
native producer, PlayCanvas producer, and offline gate have all succeeded.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import pathlib
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import uuid
from collections.abc import Callable, Sequence
from typing import Any


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
PERF_ROOT = REPO_ROOT / "tests/perf"
PLAYCANVAS_ROOT = REPO_ROOT / "tests/competitive/playcanvas"
NATIVE_PRODUCER = PERF_ROOT / "collect-q1-product-quality-native.py"
OFFLINE_GATE = PERF_ROOT / "validate-q1-product-quality-smoke.py"
QUALITY_PROTOCOL = PERF_ROOT / "q1-product-quality-smoke-v1.md"
DEFAULT_FORMAL_TRACE_AUTHORITY = (
    REPO_ROOT / "target/qualification/q1-formal-truck-product-quality-trace-v1"
)
DEFAULT_EVALUATION_AUTHORITY = (
    REPO_ROOT / "target/qualification/q1-product-quality-evaluation-authority-v1"
)
DEFAULT_TRUCK = REPO_ROOT / "tests/datasets/external/inria_3dgs/truck/point_cloud.ply"

SCHEMA = "gsplat-q1-product-quality-view000001-collection/v1"
REQUEST_SCHEMA = "gsplat-q1-playcanvas-quality-only-request/v1"
TRUCK_BYTES = 630_225_580
TRUCK_SHA256 = "65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c"
SHA_PATTERN = re.compile(r"[0-9a-f]{40}")


class CollectionError(ValueError):
    """The one-shot transaction cannot publish a valid quality collection."""


def fail(message: str) -> None:
    raise CollectionError(message)


def load_module(name: str, path: pathlib.Path) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        fail(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


Q1 = load_module("q1_view000001_transaction_gate", PERF_ROOT / "q1_product_quality_smoke.py")
SHARED = load_module(
    "q1_view000001_transaction_shared",
    PERF_ROOT / "collect-desktop-surface-evidence.py",
)


def canonical_json(value: Any) -> str:
    return json.dumps(
        value,
        allow_nan=False,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    )


def file_sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path: pathlib.Path, value: Any) -> None:
    path.write_text(canonical_json(value) + "\n", encoding="utf-8")


def resolve_existing(path: pathlib.Path, context: str, *, directory: bool) -> pathlib.Path:
    raw = pathlib.Path(os.path.abspath(path))
    if raw.is_symlink():
        fail(f"{context} must not be a symlink")
    try:
        resolved = raw.resolve(strict=True)
    except OSError as error:
        fail(f"{context} is unavailable: {error}")
    if directory and not resolved.is_dir():
        fail(f"{context} must be a directory")
    if not directory and not resolved.is_file():
        fail(f"{context} must be a file")
    return resolved


def git_receipt(repo: pathlib.Path) -> dict[str, Any]:
    head = subprocess.run(
        ("git", "-C", str(repo), "rev-parse", "HEAD"),
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    status = subprocess.run(
        (
            "git",
            "-C",
            str(repo),
            "status",
            "--porcelain",
            "--untracked-files=normal",
        ),
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if head.returncode != 0 or status.returncode != 0:
        fail("cannot prove repository identity")
    return {"commit": head.stdout.strip(), "clean": not status.stdout.strip()}


def require_clean_exact(repo: pathlib.Path, expected_commit: str) -> None:
    if SHA_PATTERN.fullmatch(expected_commit) is None:
        fail("expected commit must be a full lowercase SHA")
    receipt = git_receipt(repo)
    if receipt != {"commit": expected_commit, "clean": True}:
        fail(
            "quality collection requires the clean exact reviewed commit: "
            f"head={receipt['commit']} clean={receipt['clean']} expected={expected_commit}"
        )


def require_disjoint_output(output: pathlib.Path, inputs: Sequence[pathlib.Path]) -> pathlib.Path:
    requested = pathlib.Path(os.path.abspath(output))
    # Check the caller's literal path before resolve(strict=False): resolving a
    # dangling leaf symlink would otherwise turn it into an apparently fresh
    # target.  Reject every symlink ancestor as well so an output cannot escape
    # through an aliased parent between admission and publication.
    if os.path.lexists(requested):
        fail(f"output already exists: {requested}")
    for ancestor in requested.parents:
        if ancestor.is_symlink():
            fail(f"output path has a symlink ancestor: {ancestor}")
    if not requested.parent.is_dir():
        fail("output parent must be a real existing directory")
    output = requested.resolve(strict=False)
    if os.path.lexists(output):
        fail(f"resolved output already exists: {output}")
    for source in inputs:
        resolved = source.resolve(strict=True)
        if output == resolved or output in resolved.parents or resolved in output.parents:
            fail(f"output overlaps immutable input: {source}")
    SHARED.validate_ignored_output(REPO_ROOT, output)
    return output


def immutable_input_binding(
    formal: pathlib.Path,
    evaluation: pathlib.Path,
    truck: pathlib.Path,
) -> dict[str, Any]:
    formal = resolve_existing(formal, "formal trace authority", directory=True)
    evaluation = resolve_existing(evaluation, "evaluation authority", directory=True)
    truck = resolve_existing(truck, "complete Truck", directory=False)
    truck_bytes = truck.stat().st_size
    truck_sha256 = file_sha256(truck)
    if truck_bytes != TRUCK_BYTES or truck_sha256 != TRUCK_SHA256:
        fail("complete Truck identity mismatch")
    formal_receipt = Q1._formal_trace(formal, evaluation)
    _, ground_truth = Q1._ground_truth(evaluation, formal_receipt)
    return {
        "formal_trace": {
            "content_sha256": formal_receipt["trace"]["content_sha256"],
            "file_sha256": formal_receipt["trace_file_sha256"],
            "receipt_sha256": formal_receipt["trace_receipt_sha256"],
        },
        "evaluation_authority": {
            "receipt_sha256": formal_receipt["evaluation_receipt_sha256"],
            "ground_truth": ground_truth,
        },
        "complete_truck": {
            "bytes": truck_bytes,
            "sha256": truck_sha256,
        },
    }


def revalidate_immutable_inputs(inputs: dict[str, Any]) -> dict[str, Any]:
    observed = immutable_input_binding(
        inputs["formal"], inputs["evaluation"], inputs["truck"]
    )
    if observed != inputs["immutable_binding"]:
        fail("immutable input binding drifted after endpoint collection")
    return observed


def preflight_inputs(args: argparse.Namespace) -> dict[str, Any]:
    require_clean_exact(REPO_ROOT, args.expected_commit)
    formal = resolve_existing(args.formal_trace_authority, "formal trace authority", directory=True)
    evaluation = resolve_existing(args.evaluation_authority, "evaluation authority", directory=True)
    truck = resolve_existing(args.dataset, "complete Truck", directory=False)
    output = require_disjoint_output(args.output, (formal, evaluation, truck))
    protocol = resolve_existing(QUALITY_PROTOCOL, "Q1 quality protocol", directory=False)

    # Validate every immutable authority before either expensive producer is
    # allowed to launch.  This also decodes the official GT once.
    immutable_binding = immutable_input_binding(formal, evaluation, truck)
    return {
        "formal": formal,
        "evaluation": evaluation,
        "truck": truck,
        "output": output,
        "protocol_sha256": file_sha256(protocol),
        "immutable_binding": immutable_binding,
    }


Invoker = Callable[[Sequence[str], pathlib.Path, dict[str, str]], subprocess.CompletedProcess[str]]


def default_invoke(
    argv: Sequence[str], cwd: pathlib.Path, env: dict[str, str]
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        tuple(argv),
        cwd=cwd,
        env=env,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def command_receipt(argv: Sequence[str], returncode: int) -> dict[str, Any]:
    return {
        "argv": list(argv),
        "returncode": returncode,
        "automatic_retry": False,
    }


def run_once(
    name: str,
    argv: Sequence[str],
    cwd: pathlib.Path,
    env: dict[str, str],
    invoke: Invoker,
) -> dict[str, Any]:
    try:
        completed = invoke(tuple(argv), cwd, env)
    except Exception as error:
        raise CollectionError(f"{name} raised before completion: {error}") from error
    if completed.returncode != 0:
        raise CollectionError(f"{name} exited with {completed.returncode}")
    return command_receipt(argv, completed.returncode)


def tree_receipt(root: pathlib.Path) -> dict[str, Any]:
    files = []
    total = 0
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            fail(f"published transaction contains a symlink: {path}")
        if path.is_file():
            size = path.stat().st_size
            total += size
            files.append(
                {
                    "path": path.relative_to(root).as_posix(),
                    "bytes": size,
                    "sha256": file_sha256(path),
                }
            )
    return {
        "file_count": len(files),
        "bytes": total,
        "files_sha256": hashlib.sha256(canonical_json(files).encode()).hexdigest(),
    }


def failed_destination(output: pathlib.Path, session_id: str) -> pathlib.Path:
    return output.with_name(f"{output.name}.failed-{session_id}")


def publish_failure(
    stage: pathlib.Path,
    output: pathlib.Path,
    session_id: str,
    step: str,
    error: Exception,
) -> pathlib.Path:
    destination = failed_destination(output, session_id)
    # A failed final no-replace publication happens after the staging root was
    # frozen. Restore write permission only on that unpublished root so the
    # failure ledger can be added; immutable producer children stay untouched.
    os.chmod(stage, stage.stat().st_mode | stat.S_IWUSR)
    formal_receipt = stage / "receipt.json"
    candidate_receipt_removed = False
    if os.path.lexists(formal_receipt):
        if formal_receipt.is_symlink() or not formal_receipt.is_file():
            fail("unpublished formal receipt is not a regular file")
        formal_receipt.unlink()
        candidate_receipt_removed = True
    write_json(
        stage / "blocker.json",
        {
            "schema": SCHEMA,
            "status": "failed_attempt",
            "failed_step": step,
            "reason": str(error),
            "automatic_retry": False,
            "formal_output_published": False,
            "unpublished_candidate_receipt_removed": candidate_receipt_removed,
            "product_quality": "Deferred",
            "performance_authorized": False,
        },
    )
    SHARED.fsync_tree(stage)
    SHARED.make_tree_immutable(stage)
    Q1._publish_directory_noreplace(stage, destination)
    return destination


def discard_unpublished_stage(stage: pathlib.Path) -> None:
    """Remove a hidden unpublished stage even if its directories were frozen."""

    directories = [stage]
    directories.extend(
        path for path in stage.rglob("*") if path.is_dir() and not path.is_symlink()
    )
    for directory in directories:
        os.chmod(directory, directory.stat().st_mode | stat.S_IWUSR | stat.S_IXUSR)
    shutil.rmtree(stage)


def collect(
    args: argparse.Namespace,
    *,
    invoke: Invoker = default_invoke,
    preflight: Callable[[argparse.Namespace], dict[str, Any]] = preflight_inputs,
    revalidate: Callable[[dict[str, Any]], dict[str, Any]] = revalidate_immutable_inputs,
) -> pathlib.Path:
    inputs = preflight(args)
    output: pathlib.Path = inputs["output"]
    session_id = f"q1-view000001-{args.expected_commit[:12]}-{uuid.uuid4().hex[:12]}"
    stage = pathlib.Path(
        tempfile.mkdtemp(prefix=f".{output.name}.staging-", dir=output.parent)
    )
    step = "prepare"
    published = False
    try:
        native_output = stage / "native-view000001"
        playcanvas_output = stage / "playcanvas-view000001"
        result_output = stage / "quality-result-view000001"
        request_root = stage / "requests"
        request_root.mkdir()
        request = request_root / "playcanvas-view000001.json"
        write_json(
            request,
            {
                "schema": REQUEST_SCHEMA,
                "artifact_role": "quality_only",
                "formal_view_id": "000001",
                "trace_frame_index": 0,
                "protocol_sha256": inputs["protocol_sha256"],
                "collection_session_id": session_id,
                "product_quality_state": "Deferred",
                "performance_authorized": False,
            },
        )

        environment = os.environ.copy()
        step = "native_quality_only"
        native_argv = (
            sys.executable,
            str(NATIVE_PRODUCER),
            "--expected-commit",
            args.expected_commit,
            "--output",
            str(native_output),
            "--formal-trace-authority",
            str(inputs["formal"]),
            "--dataset",
            str(inputs["truck"]),
        )
        steps = [run_once(step, native_argv, REPO_ROOT, environment, invoke)]
        require_clean_exact(REPO_ROOT, args.expected_commit)

        step = "playcanvas_quality_only"
        playcanvas_argv = (
            "npm",
            "run",
            "quality:truck-view000001",
            "--prefix",
            "tests/competitive/playcanvas",
        )
        playcanvas_env = dict(environment)
        playcanvas_env.update(
            {
                "PLAYCANVAS_Q1_SERIES_ROOT": str(stage),
                "PLAYCANVAS_Q1_PRODUCER_REQUEST": str(request),
                "PLAYCANVAS_ARTIFACT_DIR": str(playcanvas_output),
                "HEADLESS": "0",
            }
        )
        steps.append(
            run_once(step, playcanvas_argv, REPO_ROOT, playcanvas_env, invoke)
        )
        require_clean_exact(REPO_ROOT, args.expected_commit)

        step = "offline_quality_gate"
        gate_argv = (
            sys.executable,
            str(OFFLINE_GATE),
            "--formal-trace-authority",
            str(inputs["formal"]),
            "--evaluation-authority",
            str(inputs["evaluation"]),
            "--gsplat-capture",
            str(native_output),
            "--playcanvas-capture",
            str(playcanvas_output),
            "--output",
            str(result_output),
        )
        steps.append(run_once(step, gate_argv, REPO_ROOT, environment, invoke))
        require_clean_exact(REPO_ROOT, args.expected_commit)

        result = Q1._load_json(result_output / "result.json", "one-view quality result")
        Q1.validate_result(result)
        step = "final_input_revalidation"
        final_binding = revalidate(inputs)
        require_clean_exact(REPO_ROOT, args.expected_commit)
        step = "final_publication"
        receipt = {
            "schema": SCHEMA,
            "status": "complete",
            "view_id": "000001",
            "expected_commit": args.expected_commit,
            "collection_session_id": session_id,
            "formal_inputs": {
                **final_binding,
                "protocol_sha256": inputs["protocol_sha256"],
            },
            "steps": steps,
            "artifacts": {
                "native": tree_receipt(native_output),
                "playcanvas": tree_receipt(playcanvas_output),
                "quality_result": tree_receipt(result_output),
            },
            "product_quality": "Deferred",
            "one_view_smoke": result["status"],
            "performance_authorized": False,
            "automatic_retry": False,
        }
        write_json(stage / "receipt.json", receipt)
        SHARED.fsync_tree(stage)
        SHARED.make_tree_immutable(stage)
        step = "final_frozen_input_revalidation"
        terminal_binding = revalidate(inputs)
        if terminal_binding != final_binding:
            fail("final immutable input binding differs from staged receipt binding")
        require_clean_exact(REPO_ROOT, args.expected_commit)
        step = "final_publication"
        Q1._publish_directory_noreplace(stage, output)
        published = True
        return output
    except Exception as error:
        if stage.exists():
            try:
                failure = publish_failure(stage, output, session_id, step, error)
            except Exception as publication_error:
                raise CollectionError(
                    f"{error}; failure staging could not be published: {publication_error}"
                ) from error
            raise CollectionError(f"{error}; retained failure: {failure}") from error
        raise
    finally:
        if not published and stage.exists():
            discard_unpublished_stage(stage)


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected-commit", required=True)
    parser.add_argument("--formal-trace-authority", type=pathlib.Path, required=True)
    parser.add_argument("--evaluation-authority", type=pathlib.Path, required=True)
    parser.add_argument("--dataset", type=pathlib.Path, default=DEFAULT_TRUCK)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    try:
        output = collect(parse_args(argv))
    except (CollectionError, OSError, subprocess.SubprocessError, Q1.OneViewQualityError) as error:
        print(f"Q1 view 000001 collection rejected: {error}", file=sys.stderr)
        return 1
    print(
        canonical_json(
            {
                "status": "complete",
                "output": str(output),
                "view_id": "000001",
                "product_quality": "Deferred",
                "performance_authorized": False,
            }
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
