#!/usr/bin/env python3
"""Build deterministic point-count tiers from a fixed-record binary PLY scene.

The generated PLYs retain the source header byte-for-byte except for the
``element vertex`` count. Every property and every non-vertex element is copied
unchanged. Vertex records are selected with integer-only midpoint stratification
so the same source bytes always produce the same tier on every platform.
"""

from __future__ import annotations

import argparse
import dataclasses
import decimal
import hashlib
import json
import os
import pathlib
import re
import tempfile
from collections.abc import Iterable
from typing import Any, BinaryIO


ROOT = pathlib.Path(__file__).resolve().parents[2]
HEADER_LIMIT = 1024 * 1024
COPY_CHUNK_BYTES = 8 * 1024 * 1024
LADDER_SCHEMA = "gsplat-ply-ladder/v1"
SELECTION_ALGORITHM = "stratified-midpoint-integer/v1"

SCALAR_BYTES = {
    "char": 1,
    "int8": 1,
    "uchar": 1,
    "uint8": 1,
    "short": 2,
    "int16": 2,
    "ushort": 2,
    "uint16": 2,
    "int": 4,
    "int32": 4,
    "uint": 4,
    "uint32": 4,
    "float": 4,
    "float32": 4,
    "double": 8,
    "float64": 8,
}


class PlyLadderError(ValueError):
    """Raised when an input cannot be transformed without changing its layout."""


@dataclasses.dataclass(frozen=True)
class PlyElement:
    name: str
    count: int
    record_bytes: int
    property_names: tuple[str, ...]


@dataclasses.dataclass(frozen=True)
class PlyLayout:
    header: bytes
    data_offset: int
    format: str
    elements: tuple[PlyElement, ...]
    vertex_element_index: int

    @property
    def vertex(self) -> PlyElement:
        return self.elements[self.vertex_element_index]

    @property
    def expected_file_bytes(self) -> int:
        return self.data_offset + sum(
            element.count * element.record_bytes for element in self.elements
        )

    @property
    def vertex_data_offset(self) -> int:
        return self.data_offset + sum(
            element.count * element.record_bytes
            for element in self.elements[: self.vertex_element_index]
        )

    @property
    def bytes_after_vertex(self) -> int:
        return sum(
            element.count * element.record_bytes
            for element in self.elements[self.vertex_element_index + 1 :]
        )


@dataclasses.dataclass
class _TierWriter:
    count: int
    path: pathlib.Path
    temporary_path: pathlib.Path
    handle: BinaryIO
    digest: Any
    next_output_index: int = 0

    def write(self, data: bytes | bytearray | memoryview) -> None:
        self.handle.write(data)
        self.digest.update(data)


def _read_header(handle: BinaryIO) -> bytes:
    data = bytearray()
    terminator = re.compile(rb"(?:^|\n)end_header\r?\n")
    while len(data) <= HEADER_LIMIT:
        chunk = handle.read(min(4096, HEADER_LIMIT + 1 - len(data)))
        if not chunk:
            break
        data.extend(chunk)
        match = terminator.search(data)
        if match is not None:
            end = match.end()
            handle.seek(end)
            return bytes(data[:end])
    raise PlyLadderError(f"PLY header exceeds {HEADER_LIMIT} bytes or lacks end_header")


def inspect_binary_ply(path: pathlib.Path) -> PlyLayout:
    """Inspect a scalar-only binary PLY without decoding any property values."""

    with path.open("rb") as handle:
        header = _read_header(handle)
    try:
        text = header.decode("ascii")
    except UnicodeDecodeError as error:
        raise PlyLadderError("PLY header must be ASCII") from error

    lines = text.splitlines()
    if not lines or lines[0] != "ply":
        raise PlyLadderError("missing PLY magic")

    format_name: str | None = None
    mutable_elements: list[dict[str, Any]] = []
    current: dict[str, Any] | None = None
    for line in lines[1:]:
        fields = line.split()
        if not fields or fields[0] in {"comment", "obj_info"}:
            continue
        if fields[0] == "format":
            if len(fields) != 3 or fields[2] != "1.0":
                raise PlyLadderError(f"unsupported PLY format declaration: {line}")
            format_name = fields[1]
        elif fields[0] == "element":
            if len(fields) != 3:
                raise PlyLadderError(f"malformed element declaration: {line}")
            try:
                count = int(fields[2])
            except ValueError as error:
                raise PlyLadderError(f"invalid element count: {line}") from error
            if count < 0:
                raise PlyLadderError(f"negative element count: {line}")
            current = {
                "name": fields[1],
                "count": count,
                "record_bytes": 0,
                "property_names": [],
            }
            mutable_elements.append(current)
        elif fields[0] == "property":
            if current is None:
                raise PlyLadderError("property declared before an element")
            if len(fields) >= 2 and fields[1] == "list":
                raise PlyLadderError(
                    "list properties have variable row sizes and cannot be subset safely"
                )
            if len(fields) != 3 or fields[1] not in SCALAR_BYTES:
                raise PlyLadderError(f"unsupported scalar property declaration: {line}")
            current["record_bytes"] += SCALAR_BYTES[fields[1]]
            current["property_names"].append(fields[2])
        elif fields[0] == "end_header":
            break

    if format_name not in {"binary_little_endian", "binary_big_endian"}:
        raise PlyLadderError(
            "input must use binary_little_endian 1.0 or binary_big_endian 1.0"
        )
    elements = tuple(
        PlyElement(
            name=value["name"],
            count=value["count"],
            record_bytes=value["record_bytes"],
            property_names=tuple(value["property_names"]),
        )
        for value in mutable_elements
    )
    vertex_indices = [index for index, value in enumerate(elements) if value.name == "vertex"]
    if len(vertex_indices) != 1:
        raise PlyLadderError("PLY must contain exactly one vertex element")
    vertex = elements[vertex_indices[0]]
    if vertex.record_bytes == 0:
        raise PlyLadderError("vertex element has no fixed-width properties")
    for element in elements:
        if element.count and element.record_bytes == 0:
            raise PlyLadderError(f"element {element.name!r} has no properties")

    layout = PlyLayout(
        header=header,
        data_offset=len(header),
        format=format_name,
        elements=elements,
        vertex_element_index=vertex_indices[0],
    )
    actual_bytes = path.stat().st_size
    if actual_bytes != layout.expected_file_bytes:
        raise PlyLadderError(
            "PLY payload size does not match its fixed-width header: "
            f"expected={layout.expected_file_bytes} actual={actual_bytes}"
        )
    return layout


def midpoint_index(output_index: int, output_count: int, source_count: int) -> int:
    """Return the center record of an integer partition, without float rounding."""

    if not 0 <= output_index < output_count <= source_count:
        raise ValueError("expected 0 <= output_index < output_count <= source_count")
    return ((2 * output_index + 1) * source_count) // (2 * output_count)


def _header_with_vertex_count(layout: PlyLayout, count: int) -> bytes:
    pattern = re.compile(rb"(?m)^(element[ \t]+vertex[ \t]+)([0-9]+)([ \t]*\r?)$")
    replacement = lambda match: match.group(1) + str(count).encode("ascii") + match.group(3)
    header, replacements = pattern.subn(replacement, layout.header)
    if replacements != 1:
        raise PlyLadderError("could not rewrite exactly one vertex count in PLY header")
    return header


def _parse_human_count(value: str) -> int:
    match = re.fullmatch(r"([0-9]+(?:\.[0-9]+)?)([kKmM]?)", value.strip())
    if match is None:
        raise argparse.ArgumentTypeError(f"invalid point count: {value!r}")
    multiplier = {"": 1, "k": 1_000, "m": 1_000_000}[match.group(2).lower()]
    number = decimal.Decimal(match.group(1)) * multiplier
    if number != number.to_integral_value() or number <= 0:
        raise argparse.ArgumentTypeError(f"point count must be a positive integer: {value!r}")
    return int(number)


def _portable_path(path: pathlib.Path) -> str:
    resolved = path.resolve()
    try:
        return resolved.relative_to(ROOT).as_posix()
    except ValueError:
        return resolved.as_posix()


def _read_source_manifest(path: pathlib.Path | None, source: pathlib.Path) -> dict[str, Any] | None:
    if path is None:
        return None
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise PlyLadderError(f"cannot read source manifest {path}: {error}") from error
    if not isinstance(value, dict):
        raise PlyLadderError("source manifest must contain a JSON object")
    local_path = value.get("local_path")
    if isinstance(local_path, str):
        candidate = pathlib.Path(local_path)
        if not candidate.is_absolute():
            candidate = ROOT / candidate
        if candidate.resolve() != source.resolve():
            raise PlyLadderError(
                f"source manifest local_path resolves to {candidate}, not {source}"
            )
    return value


def _copy_exact(
    source: BinaryIO,
    byte_count: int,
    source_digest: Any,
    outputs: Iterable[_TierWriter],
) -> None:
    remaining = byte_count
    while remaining:
        chunk = source.read(min(remaining, COPY_CHUNK_BYTES))
        if not chunk:
            raise PlyLadderError("unexpected EOF while copying PLY payload")
        source_digest.update(chunk)
        for output in outputs:
            output.write(chunk)
        remaining -= len(chunk)


def _metadata_projection(value: dict[str, Any]) -> dict[str, Any]:
    keys = (
        "schema",
        "id",
        "identity_status",
        "source_url",
        "asset_url",
        "archive_entry",
        "source_repository",
        "license",
        "license_url",
        "license_context",
        "source_repository_license_url",
        "attribution",
        "redistribution",
        "allowed_use",
        "upstream_dataset",
        "upstream_dataset_url",
        "download_helper_source",
        "sha256",
        "bytes",
        "archive_crc32",
        "splat_count",
        "sh_degree",
    )
    return {key: value[key] for key in keys if key in value}


def generate_ladder(
    source: pathlib.Path,
    counts: Iterable[int],
    output_dir: pathlib.Path,
    *,
    source_manifest_path: pathlib.Path | None = None,
    output_manifest_path: pathlib.Path | None = None,
    overwrite: bool = False,
) -> dict[str, Any]:
    """Generate tiers and return the committed provenance document."""

    source = source.resolve()
    output_dir = output_dir.resolve()
    layout = inspect_binary_ply(source)
    source_count = layout.vertex.count
    normalized_counts = sorted(set(counts))
    if not normalized_counts:
        raise PlyLadderError("at least one output count is required")
    if normalized_counts[0] <= 0 or normalized_counts[-1] > source_count:
        raise PlyLadderError(
            f"tier counts must be in 1..{source_count}, got {normalized_counts}"
        )
    source_manifest = _read_source_manifest(source_manifest_path, source)

    output_dir.mkdir(parents=True, exist_ok=True)
    output_manifest_path = (
        output_manifest_path.resolve()
        if output_manifest_path is not None
        else output_dir / "ladder.json"
    )
    final_paths = [output_dir / f"{source.stem}-n{count}.ply" for count in normalized_counts]
    collisions = [path for path in [*final_paths, output_manifest_path] if path.exists()]
    if collisions and not overwrite:
        joined = ", ".join(str(path) for path in collisions)
        raise PlyLadderError(f"refusing to overwrite existing output(s): {joined}")

    writers: list[_TierWriter] = []
    temporary_paths: list[pathlib.Path] = []
    source_digest = hashlib.sha256()
    try:
        for count, final_path in zip(normalized_counts, final_paths, strict=True):
            temp = tempfile.NamedTemporaryFile(
                mode="w+b",
                prefix=f".{final_path.name}.",
                suffix=".tmp",
                dir=output_dir,
                delete=False,
            )
            temporary_path = pathlib.Path(temp.name)
            temporary_paths.append(temporary_path)
            writer = _TierWriter(
                count=count,
                path=final_path,
                temporary_path=temporary_path,
                handle=temp,
                digest=hashlib.sha256(),
            )
            writer.write(_header_with_vertex_count(layout, count))
            writers.append(writer)

        with source.open("rb") as handle:
            header = handle.read(layout.data_offset)
            if header != layout.header:
                raise PlyLadderError("source PLY changed after header inspection")
            source_digest.update(header)

            prefix_bytes = layout.vertex_data_offset - layout.data_offset
            _copy_exact(handle, prefix_bytes, source_digest, writers)

            row_bytes = layout.vertex.record_bytes
            records_per_chunk = max(1, COPY_CHUNK_BYTES // row_bytes)
            source_index = 0
            while source_index < source_count:
                record_count = min(records_per_chunk, source_count - source_index)
                chunk = handle.read(record_count * row_bytes)
                if len(chunk) != record_count * row_bytes:
                    raise PlyLadderError("unexpected EOF in vertex payload")
                source_digest.update(chunk)
                chunk_end = source_index + record_count
                view = memoryview(chunk)
                for writer in writers:
                    selected = bytearray()
                    while writer.next_output_index < writer.count:
                        selected_index = midpoint_index(
                            writer.next_output_index, writer.count, source_count
                        )
                        if selected_index >= chunk_end:
                            break
                        if selected_index < source_index:
                            raise AssertionError("selection cursor moved backwards")
                        offset = (selected_index - source_index) * row_bytes
                        selected.extend(view[offset : offset + row_bytes])
                        writer.next_output_index += 1
                    if selected:
                        writer.write(selected)
                source_index = chunk_end

            _copy_exact(handle, layout.bytes_after_vertex, source_digest, writers)
            if handle.read(1):
                raise PlyLadderError("unexpected trailing bytes after PLY payload")

        source_hash = source_digest.hexdigest()
        source_bytes = source.stat().st_size
        if source_manifest is not None:
            expected_hash = source_manifest.get("sha256")
            expected_bytes = source_manifest.get("bytes")
            expected_count = source_manifest.get("splat_count")
            mismatches = []
            if expected_hash is not None and expected_hash != source_hash:
                mismatches.append(f"sha256 expected={expected_hash} actual={source_hash}")
            if expected_bytes is not None and expected_bytes != source_bytes:
                mismatches.append(f"bytes expected={expected_bytes} actual={source_bytes}")
            if expected_count is not None and expected_count != source_count:
                mismatches.append(f"splat_count expected={expected_count} actual={source_count}")
            if mismatches:
                raise PlyLadderError("source manifest mismatch: " + "; ".join(mismatches))

        tiers = []
        for writer in writers:
            if writer.next_output_index != writer.count:
                raise AssertionError(
                    f"wrote {writer.next_output_index} of {writer.count} selected records"
                )
            writer.handle.flush()
            os.fsync(writer.handle.fileno())
            output_bytes = writer.handle.tell()
            writer.handle.close()
            tiers.append(
                {
                    "splat_count": writer.count,
                    "local_path": _portable_path(writer.path),
                    "sha256": writer.digest.hexdigest(),
                    "bytes": output_bytes,
                    "selection_first_index": midpoint_index(0, writer.count, source_count),
                    "selection_last_index": midpoint_index(
                        writer.count - 1, writer.count, source_count
                    ),
                }
            )

        manifest: dict[str, Any] = {
            "schema": LADDER_SCHEMA,
            "source": {
                "local_path": _portable_path(source),
                "sha256": source_hash,
                "bytes": source_bytes,
                "format": layout.format,
                "splat_count": source_count,
                "vertex_record_bytes": layout.vertex.record_bytes,
                "vertex_properties": list(layout.vertex.property_names),
                "header_sha256": hashlib.sha256(layout.header).hexdigest(),
            },
            "selection": {
                "algorithm": SELECTION_ALGORITHM,
                "formula": "floor(((2 * output_index + 1) * source_count) / (2 * output_count))",
                "ordering": "ascending source record index",
                "nesting": "not guaranteed; pair backends on the exact same tier file",
                "purpose": (
                    "performance scaling only; not a replacement for full-scene quality runs"
                ),
            },
            "tiers": tiers,
        }
        if source_manifest is not None:
            manifest["source"]["provenance_manifest"] = _portable_path(
                source_manifest_path or pathlib.Path()
            )
            manifest["source"]["provenance"] = _metadata_projection(source_manifest)

        for writer in writers:
            os.replace(writer.temporary_path, writer.path)
            temporary_paths.remove(writer.temporary_path)

        output_manifest_path.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            prefix=f".{output_manifest_path.name}.",
            suffix=".tmp",
            dir=output_manifest_path.parent,
            delete=False,
        ) as handle:
            json.dump(manifest, handle, indent=2, sort_keys=False)
            handle.write("\n")
            manifest_temp = pathlib.Path(handle.name)
        temporary_paths.append(manifest_temp)
        os.replace(manifest_temp, output_manifest_path)
        temporary_paths.remove(manifest_temp)
        return manifest
    finally:
        for writer in writers:
            if not writer.handle.closed:
                writer.handle.close()
        for path in temporary_paths:
            path.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=pathlib.Path, help="fixed-record binary PLY")
    parser.add_argument(
        "--counts",
        type=_parse_human_count,
        nargs="+",
        required=True,
        help="tier sizes; integer, k, and m suffixes are accepted (for example 50k 1.5m)",
    )
    parser.add_argument(
        "--output-dir",
        type=pathlib.Path,
        help="default: a ladder directory next to the source PLY",
    )
    parser.add_argument(
        "--source-manifest",
        type=pathlib.Path,
        help="optional source provenance JSON; identity fields are verified and copied",
    )
    parser.add_argument(
        "--manifest",
        type=pathlib.Path,
        help="output provenance JSON path (default: <output-dir>/ladder.json)",
    )
    parser.add_argument("--overwrite", action="store_true")
    args = parser.parse_args()

    output_dir = args.output_dir or args.source.resolve().parent / "ladder"
    try:
        manifest = generate_ladder(
            args.source,
            args.counts,
            output_dir,
            source_manifest_path=args.source_manifest,
            output_manifest_path=args.manifest,
            overwrite=args.overwrite,
        )
    except (OSError, PlyLadderError) as error:
        parser.exit(1, f"PLY ladder generation failed: {error}\n")
    for tier in manifest["tiers"]:
        print(
            f"ply={tier['local_path']} splats={tier['splat_count']} "
            f"sha256={tier['sha256']}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
