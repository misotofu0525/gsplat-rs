#!/usr/bin/env python3
"""Enforce render-core ownership, dependency and lifecycle guardrails.

Physical line counts are retained only as non-blocking legacy evidence. They do
not define a target, ceiling, review trigger or task completion condition.

The checker intentionally uses only the Python standard library. Run it from
the repository root; paths in the policy and diagnostics are repository-relative.
Physical LOC includes every physical line, including blanks, comments, and
embedded tests.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys
from dataclasses import dataclass
from typing import Any, Iterable


SCRIPT_PATH = pathlib.Path(__file__).resolve()
DEFAULT_ROOT = SCRIPT_PATH.parents[2]
DEFAULT_CONFIG = pathlib.Path("tests/architecture/source_architecture_policy.json")
STATE_ALIASES = {
    "active": "active",
    "accept": "accepted",
    "accepted": "accepted",
    "reject": "rejected",
    "rejected": "rejected",
    "defer": "deferred",
    "deferred": "deferred",
}
TERMINAL_STATES = {"accepted", "rejected", "deferred"}
RAW_STRING_START_RE = re.compile(r"(?:br|rb|r)(?P<hashes>#{0,255})\"")
TASK_STATE_BLOCK_BEGIN = "<!-- gsplat-program-task-states: begin -->"
TASK_STATE_BLOCK_END = "<!-- gsplat-program-task-states: end -->"
ACTIVE_LANE_BLOCK_BEGIN = "<!-- gsplat-program-active-lanes: begin -->"
ACTIVE_LANE_BLOCK_END = "<!-- gsplat-program-active-lanes: end -->"


@dataclass(frozen=True)
class Issue:
    severity: str
    code: str
    path: str
    message: str
    line: int | None = None

    def sort_key(self) -> tuple[str, int, str, str]:
        return (self.path, self.line or 0, self.code, self.message)

    def render(self) -> str:
        location = self.path
        if self.line is not None:
            location += f":{self.line}"
        return f"{self.severity.upper()} [{self.code}] {location}: {self.message}"


def error(code: str, path: str, message: str, line: int | None = None) -> Issue:
    return Issue("error", code, path, message, line)


def policy_integer(
    value: Any,
    *,
    name: str,
    minimum: int,
    fallback: int,
    issues: list[Issue],
) -> int:
    if isinstance(value, int) and not isinstance(value, bool) and value >= minimum:
        return value
    issues.append(
        error(
            "config.invalid_grandfather_ratchet",
            "<policy>",
            f"{name} must be an integer greater than or equal to {minimum}",
        )
    )
    return fallback


def physical_loc(path: pathlib.Path) -> int:
    """Count physical lines; splitlines counts a final unterminated line too."""

    return len(path.read_bytes().splitlines())


def path_matches(path: str, patterns: Iterable[str]) -> bool:
    candidate = pathlib.PurePosixPath(path)
    return any(candidate.match(pattern) for pattern in patterns)


def discover(root: pathlib.Path, source_set: dict[str, Any]) -> list[str]:
    includes = source_set.get("include", [])
    excludes = source_set.get("exclude", [])
    found: list[str] = []
    # Enumerate only declared source roots. A repository-wide rglob would walk
    # target/, node_modules/, external datasets, and ignored build products.
    for pattern in includes:
        for candidate in root.glob(pattern):
            if not candidate.is_file():
                continue
            relative = candidate.relative_to(root).as_posix()
            if not path_matches(relative, excludes):
                found.append(relative)
    return sorted(set(found))


def load_policy(path: pathlib.Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot load policy {path}: {error}") from error
    if value.get("schema") != "gsplat-source-architecture-policy/v1":
        raise ValueError("policy schema must be gsplat-source-architecture-policy/v1")
    return value


def line_number(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def sanitize_rust(source: str) -> str:
    """Blank Rust comments and literals while preserving offsets and newlines."""

    chars = list(source)

    def blank(start: int, end: int) -> None:
        for index in range(start, end):
            if chars[index] != "\n":
                chars[index] = " "

    index = 0
    length = len(source)
    while index < length:
        char = source[index]
        if char == "/" and source.startswith("//", index):
            end = source.find("\n", index + 2)
            if end < 0:
                end = length
            blank(index, end)
            index = end
            continue
        if char == "/" and source.startswith("/*", index):
            depth = 1
            end = index + 2
            while end < length and depth:
                if source.startswith("/*", end):
                    depth += 1
                    end += 2
                elif source.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
            blank(index, end)
            index = end
            continue

        raw = RAW_STRING_START_RE.match(source, index) if char in {"b", "r"} else None
        if raw:
            hashes = raw.group("hashes")
            delimiter = '"' + hashes
            content_start = raw.end()
            close = source.find(delimiter, content_start)
            end = length if close < 0 else close + len(delimiter)
            blank(index, end)
            index = end
            continue

        if char == '"':
            end = index + 1
            escaped = False
            while end < length:
                char = source[end]
                if char == '"' and not escaped:
                    end += 1
                    break
                if char == "\\" and not escaped:
                    escaped = True
                else:
                    escaped = False
                end += 1
            blank(index, end)
            index = end
            continue

        if char == "'":
            lifetime = re.match(r"'(?:r#)?[A-Za-z_]\w*", source[index:])
            if lifetime:
                lifetime_end = index + lifetime.end()
                if lifetime_end >= length or source[lifetime_end] != "'":
                    index += 1
                    continue
            end = index + 1
            escaped = False
            closing = -1
            while end < min(length, index + 12) and source[end] != "\n":
                char = source[end]
                if char == "'" and not escaped:
                    closing = end + 1
                    break
                if char == "\\" and not escaped:
                    escaped = True
                else:
                    escaped = False
                end += 1
            if closing >= 0:
                blank(index, closing)
                index = closing
                continue
        index += 1
    return "".join(chars)


def module_path(relative: str) -> list[str]:
    marker = "/src/"
    if marker not in relative:
        return []
    tail = relative.split(marker, 1)[1]
    parts = tail.removesuffix(".rs").split("/")
    if parts[-1] in {"lib", "main", "mod"}:
        parts.pop()
    return parts


RUST_PATH_RE = re.compile(
    r"\b(?P<prefix>crate|self|super)\s*::\s*"
    r"(?P<tail>(?:r#)?[A-Za-z_][A-Za-z0-9_]*"
    r"(?:\s*::\s*(?:r#)?[A-Za-z_][A-Za-z0-9_]*)*)"
)
ROOT_MODULE_ALIAS_RE = re.compile(
    r"\b(?:"
    r"(?:use\s+(?:crate|self|super(?:\s*::\s*super)*)|extern\s+crate\s+self)"
    r"\s+as\s+(?:r#)?[A-Za-z_]\w*\s*;"
    r"|use\s+[^;]*\b(?:crate|self|super(?:\s*::\s*super)*)\s*::\s*\{"
    r"[^;]*\bself\s+as\s+(?:r#)?[A-Za-z_]\w*[^;]*;"
    r")",
    re.DOTALL,
)


def resolve_rust_path(prefix: str, tail: str, current: list[str]) -> list[str]:
    parts = [part.removeprefix("r#") for part in re.findall(r"(?:r#)?[A-Za-z_]\w*", tail)]
    if prefix == "crate":
        base: list[str] = []
    elif prefix == "self":
        base = list(current)
    else:
        base = list(current[:-1])
    while parts and parts[0] == "super":
        if base:
            base.pop()
        parts.pop(0)
    return base + parts


def split_grouped_use(body: str) -> Iterable[tuple[str, int]]:
    depth = 0
    start = 0
    for index, char in enumerate(body + ","):
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
        elif char == "," and depth == 0:
            item = body[start:index].strip()
            if item:
                yield item, start
            start = index + 1


def grouped_use_leaves(
    body: str, prefix: list[str], base_offset: int
) -> Iterable[tuple[list[str], int]]:
    """Expand the path-bearing leaves of a Rust grouped `use` tree."""

    for item, item_offset in split_grouped_use(body):
        item_without_alias = re.split(r"\s+as\s+", item, maxsplit=1)[0].strip()
        nested = re.match(
            r"(?P<head>(?:(?:r#)?[A-Za-z_]\w*\s*::\s*)+)"
            r"\{(?P<body>.*)\}\s*$",
            item_without_alias,
            re.DOTALL,
        )
        if nested:
            head = [
                part.removeprefix("r#")
                for part in re.findall(r"(?:r#)?[A-Za-z_]\w*", nested.group("head"))
            ]
            yield from grouped_use_leaves(
                nested.group("body"),
                prefix + head,
                base_offset + item_offset + nested.start("body"),
            )
            continue
        parts = [
            part.removeprefix("r#")
            for part in re.findall(r"(?:r#)?[A-Za-z_]\w*", item_without_alias)
        ]
        if parts == ["self"]:
            parts = []
        if parts:
            yield prefix + parts, base_offset + item_offset


def grouped_rust_paths(
    code: str, current: list[str]
) -> Iterable[tuple[list[str], int]]:
    """Resolve `crate/self/super::{...}` imports, including nested groups."""

    pattern = re.compile(
        r"\buse\s+(?P<root>crate|self|super(?:\s*::\s*super)*)\s*::\s*"
        r"\{(?P<body>[^;]*)\}\s*;",
        re.DOTALL,
    )
    for match in pattern.finditer(code):
        root = re.sub(r"\s+", "", match.group("root"))
        root_parts = root.split("::")
        prefix = root_parts[0]
        tail_prefix = root_parts[1:]
        for leaf, offset in grouped_use_leaves(
            match.group("body"), tail_prefix, match.start("body")
        ):
            yield resolve_rust_path(prefix, "::".join(leaf), current), offset


def prefix_matches(path: list[str], prefixes: Iterable[list[str]]) -> bool:
    return any(path[: len(prefix)] == prefix for prefix in prefixes)


def task_state_observations(
    progress: str, path: str
) -> tuple[list[tuple[str, str, int]], list[Issue]]:
    """Read only the unique machine state block; prose is never task state."""

    lines = progress.splitlines()
    begins = [index for index, line in enumerate(lines) if line.strip() == TASK_STATE_BLOCK_BEGIN]
    ends = [index for index, line in enumerate(lines) if line.strip() == TASK_STATE_BLOCK_END]
    if len(begins) != 1 or len(ends) != 1 or begins[0] >= ends[0]:
        message = "package ledger requires exactly one ordered machine task-state block"
        return [], [error("program_state.invalid_block", path, message)]
    observations: list[tuple[str, str, int]] = []
    issues: list[Issue] = []
    for index in range(begins[0] + 1, ends[0]):
        line = lines[index].strip()
        if not line:
            continue
        record = re.fullmatch(
            r"(?P<task>[A-Za-z][A-Za-z0-9.-]*)\s*=\s*(?P<state>[A-Za-z]+)",
            line,
        )
        if not record:
            message = "machine task-state records must use `TASK = STATE`"
            issues.append(error("program_state.invalid_record", path, message, index + 1))
            continue
        observations.append((record.group("task"), record.group("state").lower(), index + 1))
    return observations, issues


def active_lane_observations(
    progress: str, path: str
) -> tuple[str | None, list[tuple[str, str, int]], list[Issue]]:
    """Read the optional exact writer-lane block used during parallel work."""

    lines = progress.splitlines()
    begins = [index for index, line in enumerate(lines) if line.strip() == ACTIVE_LANE_BLOCK_BEGIN]
    ends = [index for index, line in enumerate(lines) if line.strip() == ACTIVE_LANE_BLOCK_END]
    if not begins and not ends:
        return None, [], []
    if len(begins) != 1 or len(ends) != 1 or begins[0] >= ends[0]:
        message = "package ledger requires at most one ordered machine active-lane block"
        return None, [], [error("program_state.invalid_active_lane_block", path, message)]
    activation_commit: str | None = None
    observations: list[tuple[str, str, int]] = []
    issues: list[Issue] = []
    for index in range(begins[0] + 1, ends[0]):
        line = lines[index].strip()
        if not line:
            continue
        activation = re.fullmatch(r"activation_commit\s*=\s*(?P<commit>[0-9a-f]{40})", line)
        if activation:
            if activation_commit is not None:
                issues.append(
                    error(
                        "program_state.duplicate_activation_commit",
                        path,
                        "machine active-lane block repeats activation_commit",
                        index + 1,
                    )
                )
            else:
                activation_commit = activation.group("commit")
            continue
        record = re.fullmatch(
            r"(?P<task>[A-Za-z][A-Za-z0-9.-]*)\s*=\s*(?P<lane>[A-Za-z][A-Za-z0-9.-]*)",
            line,
        )
        if not record:
            message = "machine active-lane records must use `TASK = LANE`"
            issues.append(
                error("program_state.invalid_active_lane_record", path, message, index + 1)
            )
            continue
        observations.append((record.group("task"), record.group("lane"), index + 1))
    if activation_commit is None:
        issues.append(
            error(
                "program_state.missing_activation_commit",
                path,
                "machine active-lane block requires activation_commit",
            )
        )
    return activation_commit, observations, issues


def check_task_references(policy: dict[str, Any]) -> list[Issue]:
    state = policy.get("program_task_state", {})
    catalog = set(state.get("task_catalog", {}))
    external = set(state.get("external_owner_review_allowlist", []))
    issues: list[Issue] = []

    def require(task: Any, path: str, field: str) -> None:
        if task not in catalog:
            message = f"{field} {task!r} is not in the static task catalog"
            issues.append(error("config.untracked_task_reference", path, message))

    for entry in policy.get("grandfather", []):
        owner = entry.get("owner_task")
        review = entry.get("review_task")
        if owner not in catalog and (owner not in external or review not in catalog):
            require(owner, entry.get("path", "<grandfather>"), "owner_task")
        if review is not None:
            require(review, entry.get("path", "<grandfather>"), "review_task")
    for entry in policy.get("exceptions", []):
        require(entry.get("removal_task"), entry.get("path", "<exception>"), "removal_task")
    require(policy.get("top_level_orchestration", {}).get("activation_task"), "<policy>", "top-level activation_task")
    require(policy.get("dependency_rules", {}).get("plans", {}).get("activation_task"), "<policy>", "plans activation_task")
    return issues


def check_sizes(
    root: pathlib.Path,
    policy: dict[str, Any],
    sources: dict[str, list[str]],
    completed: set[str],
) -> list[Issue]:
    issues: list[Issue] = []
    all_sources = {path: kind for kind, paths in sources.items() for path in paths}
    grandfather: dict[str, dict[str, Any]] = {}
    for entry in policy.get("grandfather", []):
        path = entry.get("path", "")
        if path in grandfather:
            issues.append(Issue("error", "config.duplicate_grandfather", path, "duplicate grandfather entry"))
            continue
        grandfather[path] = entry
        for field in (
            "a0_physical_loc",
            "baseline_physical_loc",
            "owner_task",
            "exit_condition",
        ):
            if not entry.get(field):
                issues.append(Issue("error", "config.invalid_grandfather", path, f"missing {field}"))
        a0_loc = entry.get("a0_physical_loc")
        baseline_loc = entry.get("baseline_physical_loc")
        if not isinstance(a0_loc, int) or isinstance(a0_loc, bool) or a0_loc <= 0:
            issues.append(
                Issue(
                    "error",
                    "config.invalid_grandfather",
                    path,
                    "a0_physical_loc must be a positive integer",
                )
            )
        elif isinstance(baseline_loc, int) and baseline_loc > a0_loc:
            issues.append(
                Issue(
                    "error",
                    "config.invalid_grandfather",
                    path,
                    "legacy snapshot may not exceed immutable A0 physical LOC",
                )
            )
        if entry.get("review_task") and not entry.get("review_action"):
            issues.append(
                Issue(
                    "error",
                    "config.invalid_grandfather",
                    path,
                    "review_task requires review_action",
                )
            )
        if path not in all_sources:
            issues.append(Issue("error", "grandfather.missing", path, "allowlisted source no longer exists; remove the stale entry"))

    exceptions: dict[str, dict[str, Any]] = {}
    expired: set[str] = set()
    for entry in policy.get("exceptions", []):
        path = entry.get("path", "")
        if path in exceptions:
            issues.append(Issue("error", "config.duplicate_exception", path, "duplicate exception entry"))
            continue
        exceptions[path] = entry
        required = ("reason", "removal_task")
        for field in required:
            if field not in entry or entry[field] in (None, ""):
                issues.append(Issue("error", "config.invalid_exception", path, f"missing {field}"))
        if path not in all_sources:
            issues.append(Issue("error", "exception.missing", path, "exception source does not exist"))
        removal_task = entry.get("removal_task")
        if removal_task in completed:
            expired.add(path)
            issues.append(
                Issue(
                    "error",
                    "exception.expired",
                    path,
                    f"exception expired when {removal_task} reached a terminal closeout",
                )
            )

    for path in sorted(all_sources):
        loc = physical_loc(root / path)
        entry = grandfather.get(path)
        exception = exceptions.get(path)
        exception_active = exception is not None and path not in expired

        if entry:
            baseline = entry.get("baseline_physical_loc")
            if isinstance(baseline, int) and not isinstance(baseline, bool) and baseline > 0:
                if loc > baseline and not exception_active:
                    issues.append(
                        Issue(
                            "notice",
                            "size.grandfather_growth",
                            path,
                            f"{loc} physical LOC is {loc - baseline} lines above the legacy "
                            f"snapshot {baseline}; review responsibility cohesion, dependency "
                            "direction, test seams and maintenance risk",
                        )
                    )
            else:
                issues.append(Issue("error", "config.invalid_grandfather", path, "baseline_physical_loc must be positive"))
            owner = entry.get("owner_task")
            if owner in completed:
                issues.append(
                    Issue(
                        "error",
                        "grandfather.exit_due",
                        path,
                        f"owner task {owner} is closed but its legacy grandfather entry remains; "
                        "remove the entry so the file returns to its normal size profile",
                    )
                )
            review = entry.get("review_task")
            if review in completed:
                issues.append(
                    Issue(
                        "error",
                        "grandfather.review_due",
                        path,
                        f"review task {review} is closed; remove, reassign, or explicitly renew this external owner entry",
                    )
                )
            continue
    return issues


PLAN_GLOBAL_OPERATIONS = (
    ("dependency.plan.submit", re.compile(r"\.\s*submit\s*\("), "plans may not submit GPU work"),
    ("dependency.plan.present", re.compile(r"\.\s*present\s*\("), "plans may not present"),
    ("dependency.plan.poll", re.compile(r"\.\s*poll\s*\("), "plans may not poll a device or queue"),
    (
        "dependency.plan.map",
        re.compile(r"\.\s*(?:map_async|get_mapped_range(?:_mut)?|unmap)\s*\("),
        "plans may not map or read back buffers",
    ),
)

PLAN_FRAME_OPERATIONS = (
    (
        "dependency.plan.env",
        re.compile(r"\b(?:std\s*::\s*)?env\s*::\s*(?:var|var_os|vars|vars_os)\s*\("),
        "per-frame plan files may not parse environment selection",
    ),
    (
        "dependency.plan.pipeline",
        re.compile(r"\bcreate_(?:compute_pipeline|render_pipeline|pipeline_layout)\s*\("),
        "per-frame plan files may not create pipelines",
    ),
)


RUNTIME_PASS_VECTOR_RE = re.compile(
    r"\bVec\s*<\s*Box\s*<\s*dyn\s+"
    r"(?:(?:r#)?[A-Za-z_]\w*\s*::\s*)*"
    r"(?:Pass|RenderPlan|(?:r#)?[A-Za-z_]\w*Pass)\b"
)
PUBLIC_RENDER_PLAN_RE = re.compile(
    r"(?m)^\s*pub(?:\s*\([^)]*\))?\s+(?:unsafe\s+)?trait\s+RenderPlan\b"
)


def find_matching_brace(code: str, opening: int) -> int | None:
    depth = 0
    for index in range(opening, len(code)):
        if code[index] == "{":
            depth += 1
        elif code[index] == "}":
            depth -= 1
            if depth == 0:
                return index
    return None


def find_parameter_opening(code: str, start: int) -> int | None:
    angle_depth = 0
    for index in range(start, len(code)):
        char = code[index]
        if char == "<":
            angle_depth += 1
        elif char == ">" and angle_depth:
            angle_depth -= 1
        elif char == "(" and angle_depth == 0:
            return index
        elif char in ";{" and angle_depth == 0:
            return None
    return None


def find_matching_delimiter(
    code: str, opening: int, open_char: str, close_char: str
) -> int | None:
    depth = 0
    for index in range(opening, len(code)):
        if code[index] == open_char:
            depth += 1
        elif code[index] == close_char:
            depth -= 1
            if depth == 0:
                return index
    return None


def find_signature_body_opening(code: str, start: int) -> int | None:
    """Find a function body, ignoring delimiters and semicolons in its signature."""

    depths = {"(": 0, "[": 0, "<": 0}
    pairs = {")": "(", "]": "[", ">": "<"}
    for index in range(start, len(code)):
        char = code[index]
        if char in depths:
            depths[char] += 1
            continue
        if char in pairs:
            opening = pairs[char]
            if depths[opening]:
                depths[opening] -= 1
            continue
        if not any(depths.values()):
            if char == ";":
                return None
            if char == "{":
                return index
    return None


def find_function_spans(code: str, name: str) -> list[tuple[int, int]]:
    spans: list[tuple[int, int]] = []
    pattern = re.compile(rf"\bfn\s+(?:r#)?{re.escape(name)}\b")
    for match in pattern.finditer(code):
        parameters = find_parameter_opening(code, match.end())
        if parameters is None:
            continue
        parameter_end = find_matching_delimiter(code, parameters, "(", ")")
        if parameter_end is None:
            continue
        body_start = find_signature_body_opening(code, parameter_end + 1)
        if body_start is None:
            continue
        body_end = find_matching_brace(code, body_start)
        if body_end is not None:
            spans.append((match.start(), body_end + 1))
    return spans


def struct_fields(code: str) -> Iterable[tuple[str, str, int]]:
    for match in re.finditer(r"\bstruct\s+(?:r#)?[A-Za-z_]\w*[^;{]*\{", code):
        opening = code.find("{", match.start())
        closing = find_matching_brace(code, opening)
        if closing is None:
            continue
        body = code[opening + 1 : closing]
        for field in re.finditer(
            r"(?m)^\s*(?:pub(?:\s*\([^)]*\))?\s+)?"
            r"(?P<name>(?:r#)?[A-Za-z_]\w*)\s*:\s*(?P<type>[^,\n]+)",
            body,
        ):
            yield (
                field.group("name").removeprefix("r#"),
                field.group("type").strip(),
                opening + 1 + field.start(),
            )


def tuple_struct_fields(code: str) -> Iterable[tuple[str, str, int]]:
    pattern = re.compile(
        r"\bstruct\s+(?:r#)?[A-Za-z_]\w*(?:\s*<[^;{}>]*>)?\s*\("
    )
    for match in pattern.finditer(code):
        opening = code.find("(", match.start())
        closing = find_matching_delimiter(code, opening, "(", ")")
        if closing is not None:
            yield "", code[opening + 1 : closing], opening + 1


def check_dependencies(
    root: pathlib.Path,
    policy: dict[str, Any],
    rust_paths: list[str],
    completed: set[str],
    state_path: str,
) -> list[Issue]:
    issues: list[Issue] = []
    dependency = policy["dependency_rules"]
    gpu_paths = dependency["gpu"]["include"]
    plan_rule = dependency["plans"]
    plan_paths = plan_rule["include"]
    host_paths = dependency["platform_hosts"]["include"]
    forbidden_prefixes = dependency["gpu"]["forbidden_prefixes"]
    discovered_plans = {path for path in rust_paths if path_matches(path, plan_paths)}
    configured_boundaries: dict[str, list[str]] = {}
    for entry in plan_rule.get("per_frame_functions", []):
        configured_boundaries.setdefault(entry["path"], []).append(entry["name"])
    preparation_only = set(plan_rule.get("preparation_only_files", []))
    activation_task = plan_rule.get("activation_task")
    if activation_task in completed and not configured_boundaries:
        issues.append(
            Issue(
                "error",
                "dependency.plan.activation_due",
                state_path,
                f"{activation_task} is terminal but no plan file boundary is configured",
            )
        )
    for path in sorted(set(configured_boundaries) | preparation_only):
        if path not in discovered_plans:
            issues.append(
                Issue(
                    "error",
                    "config.stale_plan_boundary",
                    path,
                    "configured plan boundary does not name a discovered plan source",
                )
            )
        if path in configured_boundaries and path in preparation_only:
            issues.append(
                Issue(
                    "error",
                    "config.conflicting_plan_boundary",
                    path,
                    "plan source cannot be both per-frame and preparation-only",
                )
            )

    for path in rust_paths:
        source = (root / path).read_text(encoding="utf-8")
        code = sanitize_rust(source)
        if path_matches(path, gpu_paths):
            resolved: list[tuple[list[str], int]] = []
            current = module_path(path)
            root_alias = ROOT_MODULE_ALIAS_RE.search(code)
            if root_alias:
                issues.append(
                    Issue(
                        "error",
                        "dependency.gpu.root_module_alias",
                        path,
                        "gpu/ may not alias crate/self/super module roots; use explicit paths",
                        line_number(code, root_alias.start()),
                    )
                )
            for match in RUST_PATH_RE.finditer(code):
                resolved.append(
                    (
                        resolve_rust_path(match.group("prefix"), match.group("tail"), current),
                        match.start(),
                    )
                )
            resolved.extend(grouped_rust_paths(code, current))
            emitted: set[str] = set()
            for rust_path, offset in resolved:
                for category, prefixes in forbidden_prefixes.items():
                    if category in emitted:
                        continue
                    if prefix_matches(rust_path, prefixes):
                        emitted.add(category)
                        issues.append(
                            Issue(
                                "error",
                                f"dependency.gpu.{category}",
                                path,
                                f"gpu/ may not depend on {'::'.join(rust_path)}",
                                line_number(code, offset),
                            )
                        )

        if path_matches(path, plan_paths):
            for code_name, pattern, message in PLAN_GLOBAL_OPERATIONS:
                match = pattern.search(code)
                if match:
                    issues.append(Issue("error", code_name, path, message, line_number(code, match.start())))
            boundaries = configured_boundaries.get(path, [])
            if not boundaries and path not in preparation_only:
                issues.append(
                    Issue(
                        "error",
                        "dependency.plan.frame_boundary_missing",
                        path,
                        "declare exact per-frame functions or mark this file preparation-only before plan code is admitted",
                    )
                )
            for function_name in boundaries:
                spans = find_function_spans(code, function_name)
                if len(spans) != 1:
                    issues.append(
                        Issue(
                            "error",
                            "dependency.plan.frame_boundary_ambiguous",
                            path,
                            f"expected exactly one body for per-frame fn {function_name}, found {len(spans)}",
                        )
                    )
            if boundaries:
                for code_name, pattern, message in PLAN_FRAME_OPERATIONS:
                    match = pattern.search(code)
                    if match:
                        issues.append(
                            Issue(
                                "error",
                                code_name,
                                path,
                                message,
                                line_number(code, match.start()),
                            )
                        )

        if path_matches(path, host_paths):
            emitted: set[str] = set()
            members = [*struct_fields(code), *tuple_struct_fields(code)]
            for name, field_type, offset in members:
                lowered = name.lower()
                categories: list[tuple[str, bool, str]] = [
                    (
                        "plan_selection",
                        bool(
                            lowered in {"plan", "plan_id", "plan_set", "controller"}
                            or re.search(
                                r"(?:selected|active|current|requested)_plan(?:_id)?$",
                                lowered,
                            )
                            or re.search(r"^plan_(?:selection|selector|controller|set)$", lowered)
                            or re.search(
                                r"\b(?:PlanId|PlanSet|WholePlanController|Controller)\b",
                                field_type,
                            )
                        ),
                        "platform hosts may not own plan selection",
                    ),
                    (
                        "cache_generation",
                        lowered == "renderer_generation"
                        or (
                            "cache" in lowered
                            and ("generation" in lowered or lowered.endswith("_gen"))
                        )
                        or bool(
                            re.search(
                                r"\b(?:CacheGeneration|RendererGeneration|RendererCacheGeneration)\b",
                                field_type,
                            )
                        ),
                        "platform hosts may not own renderer cache generations",
                    ),
                    (
                        "adaptive_state",
                        "adaptive" in lowered
                        or bool(
                            re.search(
                                r"\b(?:AdaptiveState|WholePlanController|PlanSampler)\b",
                                field_type,
                            )
                        ),
                        "platform hosts may not own adaptive state",
                    ),
                    (
                        "renderer_state",
                        lowered == "frame_state"
                        or bool(re.search(r"\bFrameState\b", field_type)),
                        "platform hosts may not own renderer FrameState",
                    ),
                ]
                for category, matches, message in categories:
                    if matches and category not in emitted:
                        emitted.add(category)
                        issues.append(
                            Issue(
                                "error",
                                f"dependency.host.{category}",
                                path,
                                message,
                                line_number(code, offset),
                            )
                        )

        vector = RUNTIME_PASS_VECTOR_RE.search(code)
        if vector:
            issues.append(
                Issue(
                    "error",
                    "architecture.runtime_pass_vector",
                    path,
                    "production runtime may not use Vec<Box<dyn ...Pass>>",
                    line_number(code, vector.start()),
                )
            )
        public_trait = PUBLIC_RENDER_PLAN_RE.search(code)
        if public_trait:
            issues.append(
                Issue(
                    "error",
                    "architecture.public_render_plan_trait",
                    path,
                    "RenderPlan may not be a public trait",
                    line_number(code, public_trait.start()),
                )
            )
    return issues


def find_function_locs(source: str, name: str) -> list[int]:
    code = sanitize_rust(source)
    locs: list[int] = []
    for start, end in find_function_spans(code, name):
        start_line = line_number(code, start)
        end_line = line_number(code, end - 1)
        locs.append(end_line - start_line + 1)
    return locs


def check_orchestration(
    root: pathlib.Path,
    policy: dict[str, Any],
    completed: set[str],
    progress_path: str,
) -> list[Issue]:
    rule = policy["top_level_orchestration"]
    issues: list[Issue] = []
    if not rule["enabled"]:
        activation_paths = rule.get("activation_paths", [])
        existing = [path for path in activation_paths if (root / path).is_file()]
        if existing:
            issues.append(
                Issue(
                    "error",
                    "orchestration.activation_due",
                    existing[0],
                    "future orchestration path now exists; configure exact function boundaries and enable the orchestration review signal",
                )
            )
        task = rule.get("activation_task")
        if task in completed:
            issues.append(
                Issue(
                    "error",
                    "orchestration.activation_due",
                    progress_path,
                    f"activation task {task} is closed but the orchestration rule remains disabled",
                )
            )
        if not rule.get("reason") or not task:
            issues.append(
                Issue(
                    "error",
                    "config.invalid_orchestration",
                    "<policy>",
                    "disabled orchestration rule requires reason and activation_task",
                )
            )
        return issues

    functions = rule.get("functions", [])
    if not functions:
        return [Issue("error", "config.invalid_orchestration", "<policy>", "enabled rule requires explicit functions")]
    for entry in functions:
        path = entry["path"]
        name = entry["name"]
        source_path = root / path
        if not source_path.is_file():
            issues.append(Issue("error", "orchestration.file_missing", path, "configured function source is missing"))
            continue
        locs = find_function_locs(source_path.read_text(encoding="utf-8"), name)
        if len(locs) != 1:
            issues.append(
                Issue(
                    "error",
                    "orchestration.boundary_ambiguous",
                    path,
                    f"expected exactly one body for fn {name}, found {len(locs)}",
                )
            )
    return issues


def load_program_task_states(
    root: pathlib.Path, policy: dict[str, Any]
) -> tuple[set[str], str, list[Issue]]:
    """Merge task state from one static active/completed ledger pair per package."""

    config = policy.get("program_task_state", {})
    catalog = config.get("task_catalog", {})
    packages = config.get("package_ledgers", {})
    issues: list[Issue] = []
    if not isinstance(catalog, dict) or not catalog:
        issues.append(error("config.invalid_task_catalog", "<policy>", "task_catalog must be a non-empty task-to-package map"))
        return set(), "<program-task-state>", issues
    if not isinstance(packages, dict) or not packages:
        issues.append(error("config.invalid_package_ledgers", "<policy>", "package_ledgers must be a non-empty package map"))
        return set(), "<program-task-state>", issues

    expected_config_keys = {
        "external_owner_review_allowlist",
        "parallel_execution",
        "task_catalog",
        "package_ledgers",
    }
    if set(config) != expected_config_keys:
        issues.append(
            error(
                "config.invalid_program_task_state",
                "<policy>",
                f"program_task_state requires exactly {sorted(expected_config_keys)}",
            )
        )

    known_packages = set(packages)
    for task, package in catalog.items():
        if not isinstance(task, str) or not isinstance(package, str) or package not in known_packages:
            issues.append(error("config.invalid_task_catalog", "<policy>", f"invalid catalog entry {task!r}: {package!r}"))

    parallel_execution = config.get("parallel_execution")
    parallel_key: tuple[tuple[str, str], ...] | None = None
    if parallel_execution is not None:
        path = "<policy>.program_task_state.parallel_execution"
        expected_keys = {"activation_commit", "members", "integration_order", "reason"}
        if not isinstance(parallel_execution, dict) or set(parallel_execution) != expected_keys:
            issues.append(
                error(
                    "config.invalid_parallel_execution",
                    path,
                    f"parallel_execution requires exactly {sorted(expected_keys)}",
                )
            )
        else:
            activation_commit = parallel_execution["activation_commit"]
            members = parallel_execution["members"]
            integration_order = parallel_execution["integration_order"]
            reason = parallel_execution["reason"]
            valid = True
            if (
                not isinstance(activation_commit, str)
                or re.fullmatch(r"[0-9a-f]{40}", activation_commit) is None
            ):
                issues.append(
                    error(
                        "config.invalid_parallel_execution",
                        path,
                        "activation_commit must be a full lowercase commit SHA",
                    )
                )
                valid = False
            else:
                try:
                    exists = subprocess.run(
                        ["git", "-C", str(root), "cat-file", "-e", f"{activation_commit}^{{commit}}"],
                        check=False,
                        capture_output=True,
                    ).returncode == 0
                    ancestor = exists and subprocess.run(
                        ["git", "-C", str(root), "merge-base", "--is-ancestor", activation_commit, "HEAD"],
                        check=False,
                        capture_output=True,
                    ).returncode == 0
                except OSError:
                    exists = False
                    ancestor = False
                if not exists or not ancestor:
                    issues.append(
                        error(
                            "config.invalid_parallel_activation_commit",
                            path,
                            "activation_commit must exist and be an ancestor of current HEAD",
                        )
                    )
                    valid = False
            if not isinstance(reason, str) or not reason.strip():
                issues.append(error("config.invalid_parallel_execution", path, "reason must be non-empty"))
                valid = False
            member_pairs: list[tuple[str, str]] = []
            claimed_write_paths: dict[str, str] = {}
            if not isinstance(members, list) or len(members) < 2:
                issues.append(error("config.invalid_parallel_execution", path, "members must contain at least two writer lanes"))
                valid = False
            else:
                for index, member in enumerate(members):
                    member_path = f"{path}.members[{index}]"
                    member_keys = {"task", "lane", "write_allowlist"}
                    if not isinstance(member, dict) or set(member) != member_keys:
                        issues.append(error("config.invalid_parallel_execution", member_path, f"member requires exactly {sorted(member_keys)}"))
                        valid = False
                        continue
                    task = member["task"]
                    lane = member["lane"]
                    write_allowlist = member["write_allowlist"]
                    if (
                        not isinstance(task, str)
                        or task not in catalog
                        or not isinstance(lane, str)
                        or re.fullmatch(r"[A-Za-z][A-Za-z0-9.-]*", lane) is None
                    ):
                        issues.append(error("config.invalid_parallel_execution", member_path, "task must be cataloged and lane must be a stable identifier"))
                        valid = False
                        continue
                    if (
                        not isinstance(write_allowlist, list)
                        or not write_allowlist
                        or any(not isinstance(item, str) or not item for item in write_allowlist)
                        or len(set(write_allowlist)) != len(write_allowlist)
                    ):
                        issues.append(error("config.invalid_parallel_execution", member_path, "write_allowlist must contain unique non-empty paths"))
                        valid = False
                        continue
                    member_pairs.append((task, lane))
                    for item in write_allowlist:
                        pure = pathlib.PurePosixPath(item)
                        if (
                            pure.is_absolute()
                            or ".." in pure.parts
                            or pure.as_posix() != item
                            or any(character in item for character in "*?[]")
                            or (root / item).is_dir()
                        ):
                            issues.append(
                                error(
                                    "config.invalid_parallel_execution",
                                    member_path,
                                    f"write allowlist item must be one normalized exact file path: {item!r}",
                                )
                            )
                            valid = False
                            continue
                        for prior_path, prior_lane in claimed_write_paths.items():
                            prior_parts = pathlib.PurePosixPath(prior_path).parts
                            item_parts = pure.parts
                            common = min(len(prior_parts), len(item_parts))
                            if prior_parts[:common] == item_parts[:common]:
                                issues.append(
                                    error(
                                        "config.parallel_write_overlap",
                                        member_path,
                                        f"{item} overlaps {prior_path} owned by lane {prior_lane}",
                                    )
                                )
                                valid = False
                                break
                        else:
                            claimed_write_paths[item] = lane
            if len(set(member_pairs)) != len(member_pairs) or len({lane for _, lane in member_pairs}) != len(member_pairs):
                issues.append(error("config.invalid_parallel_execution", path, "member task/lane identifiers must be unique"))
                valid = False
            expected_order = [lane for _, lane in member_pairs]
            if (
                not isinstance(integration_order, list)
                or any(not isinstance(item, str) for item in integration_order)
                or len(integration_order) != len(expected_order)
                or set(integration_order) != set(expected_order)
            ):
                issues.append(error("config.invalid_parallel_execution", path, "integration_order must be a permutation of member lanes"))
                valid = False
            if valid:
                parallel_key = tuple(sorted(member_pairs))

    selected: list[tuple[str, str, bool]] = []
    selected_paths: dict[str, str] = {}
    present_packages: set[str] = set()
    claimed_paths: dict[str, str] = {}
    for package, pair in packages.items():
        required_keys = {"active", "completed", "required_after"}
        if not isinstance(pair, dict) or set(pair) != required_keys:
            issues.append(error("config.invalid_package_ledgers", "<policy>", f"package {package} requires {sorted(required_keys)}"))
            continue
        required_after = pair["required_after"]
        if required_after is not None and (
            not isinstance(required_after, str) or required_after not in catalog
        ):
            issues.append(error("config.untracked_task_reference", "<policy>", f"package {package} required_after {required_after!r} is not cataloged"))
        candidates = [pair["active"], pair["completed"]]
        if (
            any(not isinstance(path, str) or not path for path in candidates)
            or candidates[0] == candidates[1]
        ):
            issues.append(error("config.invalid_package_ledgers", "<policy>", f"package {package} paths must be distinct non-empty strings"))
            continue
        for path in candidates:
            prior_package = claimed_paths.get(path)
            if prior_package:
                issues.append(error("config.invalid_package_ledgers", "<policy>", f"{path} is shared by {prior_package} and {package}"))
            else:
                claimed_paths[path] = package
        existing = [path for path in candidates if (root / path).is_file()]
        if existing:
            present_packages.add(package)
        if len(existing) > 1:
            issues.append(error("program_state.ambiguous_package_ledger", package, f"active and completed ledgers both exist: {', '.join(existing)}"))
        elif existing:
            selected.append((package, existing[0], existing[0] == pair["completed"]))
            selected_paths[package] = existing[0]

    states: dict[str, tuple[str, str, int]] = {}
    active: list[str] = []
    lane_records: list[tuple[str, str, str, int, bool]] = []
    lane_activation_commits: dict[str, str] = {}
    for package, path, completed_ledger in selected:
        progress = (root / path).read_text(encoding="utf-8")
        observations, block_issues = task_state_observations(progress, path)
        issues.extend(block_issues)
        lane_activation_commit, lane_observations, lane_issues = active_lane_observations(
            progress, path
        )
        issues.extend(lane_issues)
        if lane_activation_commit is not None:
            lane_activation_commits[package] = lane_activation_commit
        lane_records.extend(
            (package, task, lane, line, completed_ledger)
            for task, lane, line in lane_observations
        )
        for task, raw_state, line in observations:
            state = STATE_ALIASES.get(raw_state)
            if task not in catalog:
                issues.append(error("program_state.unknown_task", path, f"task {task} is not cataloged", line))
                continue
            if catalog[task] != package:
                issues.append(error("program_state.wrong_package", path, f"task {task} belongs to {catalog[task]}, not {package}", line))
                continue
            if state is None:
                issues.append(error("program_state.unknown_state", path, f"task {task} has unsupported state {raw_state!r}", line))
                continue
            prior = states.get(task)
            if prior:
                code = (
                    "program_state.duplicate_task"
                    if prior[0] == state
                    else "program_state.conflict"
                )
                message = f"task {task} was {prior[0]} at {prior[1]}:{prior[2]}; repeated as {state}"
                issues.append(error(code, path, message, line))
                continue
            states[task] = (state, path, line)
            if state == "active":
                active.append(task)
                if completed_ledger:
                    issues.append(error("program_state.active_in_completed_ledger", path, f"completed package ledger leaves {task} Active", line))
    active_lanes: dict[str, str] = {}
    for package, task, lane, line, completed_ledger in lane_records:
        path = selected_paths.get(package, package)
        if completed_ledger:
            issues.append(error("program_state.stale_active_lane", path, f"completed ledger retains active lane {task} = {lane}", line))
        if task not in catalog:
            issues.append(error("program_state.unknown_task", path, f"lane task {task} is not cataloged", line))
            continue
        if catalog[task] != package:
            issues.append(error("program_state.wrong_package", path, f"lane task {task} belongs to {catalog[task]}, not {package}", line))
            continue
        if task in active_lanes:
            issues.append(error("program_state.duplicate_active_lane", path, f"task {task} has more than one active lane", line))
            continue
        active_lanes[task] = lane
        if states.get(task, (None, "", 0))[0] != "active":
            issues.append(error("program_state.stale_active_lane", path, f"lane {task} = {lane} does not name an Active task", line))

    if len(active) > 1:
        missing_lanes = [task for task in active if task not in active_lanes]
        key = tuple(sorted((task, active_lanes[task]) for task in active if task in active_lanes))
        if missing_lanes or parallel_key != key:
            detail = f"active tasks {', '.join(sorted(active))} do not match the exact parallel execution"
            if missing_lanes:
                detail += f"; missing lane records for {', '.join(sorted(missing_lanes))}"
            issues.append(error("program_state.multiple_active", "<program-task-state>", detail))
        elif isinstance(parallel_execution, dict):
            expected_commit = parallel_execution.get("activation_commit")
            active_packages = {catalog[task] for task in active}
            recorded_commits = {
                lane_activation_commits.get(package) for package in active_packages
            }
            if recorded_commits != {expected_commit}:
                issues.append(
                    error(
                        "program_state.parallel_activation_mismatch",
                        "<program-task-state>",
                        "every active package lane block must repeat parallel_execution activation_commit",
                    )
                )
    elif parallel_execution is not None:
        issues.append(error("program_state.stale_parallel_execution", "<policy>", "parallel_execution must be removed unless its exact multi-task set is Active"))
    if len(active) <= 1 and active_lanes:
        issues.append(error("program_state.stale_active_lane", "<program-task-state>", "active-lane records are only valid for an exact parallel execution"))

    closed = {task for task, (state, _, _) in states.items() if state in TERMINAL_STATES}
    for package, pair in packages.items():
        if not isinstance(pair, dict) or "required_after" not in pair:
            continue
        trigger = pair["required_after"]
        if trigger is not None and trigger not in closed and package in present_packages:
            issues.append(error("program_state.package_started_before_dependency", package, f"package ledger exists before {trigger} became terminal"))
        elif (trigger is None or trigger in closed) and package not in present_packages:
            reason = "always" if trigger is None else f"after {trigger} became terminal"
            issues.append(error("program_state.required_package_missing", package, f"package {package} ledger is required {reason}"))
    return closed, ", ".join(path for _, path, _ in selected), issues


def check_repository(root: pathlib.Path, policy: dict[str, Any]) -> tuple[list[Issue], dict[str, int]]:
    root = root.resolve()
    source_sets = policy["source_sets"]
    sources = {kind: discover(root, source_set) for kind, source_set in source_sets.items()}
    completed, progress_path, issues = load_program_task_states(root, policy)
    issues.extend(check_task_references(policy))
    issues.extend(check_sizes(root, policy, sources, completed))
    issues.extend(
        check_dependencies(
            root,
            policy,
            sources.get("rust", []),
            completed,
            progress_path,
        )
    )
    issues.extend(check_orchestration(root, policy, completed, progress_path))
    return sorted(issues, key=Issue.sort_key), {kind: len(paths) for kind, paths in sources.items()}


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=pathlib.Path, default=DEFAULT_ROOT)
    parser.add_argument("--config", type=pathlib.Path, default=DEFAULT_CONFIG)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    root = args.root.resolve()
    config_path = args.config if args.config.is_absolute() else root / args.config
    try:
        policy = load_policy(config_path)
        issues, counts = check_repository(root, policy)
    except (KeyError, OSError, UnicodeError, ValueError) as error:
        print(f"ERROR [checker.config] {error}", file=sys.stderr)
        return 2

    for issue in issues:
        stream = sys.stderr if issue.severity == "error" else sys.stdout
        print(issue.render(), file=stream)
    errors = sum(issue.severity == "error" for issue in issues)
    if errors:
        print(f"source architecture policy: FAIL ({errors} errors)", file=sys.stderr)
        return 1
    print(
        "source architecture policy: PASS "
        f"({counts.get('rust', 0)} production Rust, {counts.get('wgsl', 0)} WGSL, "
        f"{len(policy.get('grandfather', []))} grandfathered)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
