#!/usr/bin/env python3
# SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES.
# SPDX-License-Identifier: Apache-2.0
"""Range-extract selected official INRIA 3DGS paper scenes.

The Zip64 central-directory approach is adapted from gsplat's Apache-2.0
``examples/download_3dgs_paper_scenes.py``. This version streams extraction to
disk, validates ZIP CRC/size, pins the selected PLY identities, and emits a
provenance manifest next to every ignored local asset.
"""

from __future__ import annotations

import argparse
import binascii
import dataclasses
import hashlib
import json
import os
import pathlib
import re
import struct
import tempfile
import urllib.error
import urllib.request
import zlib
from typing import Any, BinaryIO

from ply_ladder import COPY_CHUNK_BYTES, ROOT, inspect_binary_ply


MODELS_ZIP_URL = (
    "https://repo-sam.inria.fr/fungraph/3d-gaussian-splatting/"
    "datasets/pretrained/models.zip"
)
SOURCE_REPOSITORY = "https://github.com/graphdeco-inria/gaussian-splatting"
SOURCE_LICENSE_URL = f"{SOURCE_REPOSITORY}/blob/main/LICENSE.md"
DOWNLOAD_HELPER_SOURCE = (
    "https://github.com/nerfstudio-project/gsplat/blob/main/"
    "examples/download_3dgs_paper_scenes.py"
)
MANIFEST_SCHEMA = "gsplat-external-scene/v1"
USER_AGENT = "gsplat-rs-dataset-fetch/1"


# These archive-entry identities were read from the official Zip64 central
# directory. They make a changed upstream object fail closed; extraction also
# records the full PLY SHA-256 in the ignored local source.json evidence.
SCENES: dict[str, dict[str, Any]] = {
    "bonsai": {
        "family": "Mip-NeRF 360 indoor",
        "upstream_dataset_url": "https://jonbarron.info/mipnerf360/",
        "sha256": "a16af6d8815498ffbf9eb5d5ee93f5bcc9dca34c4e3eb6f7a796ef9e97c0d273",
        "bytes": 308_716_644,
        "compressed_bytes": 260_603_022,
        "archive_crc32": "3088ffa4",
        "splat_count": 1_244_819,
    },
    "truck": {
        "family": "Tanks and Temples",
        "upstream_dataset_url": (
            "https://repo-sam.inria.fr/fungraph/3d-gaussian-splatting/"
            "datasets/input/tandt_db.zip"
        ),
        "sha256": "65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c",
        "bytes": 630_225_580,
        "compressed_bytes": 550_481_900,
        "archive_crc32": "44027887",
        "splat_count": 2_541_226,
    },
    "garden": {
        "family": "Mip-NeRF 360 outdoor",
        "upstream_dataset_url": "https://jonbarron.info/mipnerf360/",
        "sha256": "16701d5e0630dfaca74f8794ed7ce2aa23fa922f87dc09a7e37484e8d3f82d5a",
        "bytes": 1_447_027_964,
        "compressed_bytes": 1_290_779_747,
        "archive_crc32": "2d0a09ee",
        "splat_count": 5_834_784,
    },
    "bicycle": {
        "family": "Mip-NeRF 360 outdoor",
        "upstream_dataset_url": "https://jonbarron.info/mipnerf360/",
        "sha256": "64d357cb25bd85f710f8551a18d830f8497277fbd8c5805adfd72ffe9ca78227",
        "bytes": 1_520_726_124,
        "compressed_bytes": 1_353_363_151,
        "archive_crc32": "ebf2474a",
        "splat_count": 6_131_954,
    },
}


class DownloadError(RuntimeError):
    pass


@dataclasses.dataclass(frozen=True)
class ZipEntry:
    name: str
    local_header_offset: int
    compressed_bytes: int
    uncompressed_bytes: int
    compression: int
    flags: int
    crc32: int


def _request(
    url: str,
    *,
    method: str = "GET",
    start: int | None = None,
    length: int | None = None,
):
    request = urllib.request.Request(
        url, method=method, headers={"User-Agent": USER_AGENT}
    )
    if start is not None:
        if length is None or length <= 0:
            raise ValueError("a positive length is required for a range request")
        request.add_header("Range", f"bytes={start}-{start + length - 1}")
    try:
        return urllib.request.urlopen(request, timeout=60)
    except urllib.error.URLError as error:
        raise DownloadError(f"request failed for {url}: {error}") from error


def _remote_size(url: str) -> int:
    with _request(url, method="HEAD") as response:
        length = response.headers.get("Content-Length")
        if length is None:
            raise DownloadError("remote archive HEAD response lacks Content-Length")
        try:
            size = int(length)
        except ValueError as error:
            raise DownloadError(f"invalid Content-Length: {length!r}") from error
    if size <= 0:
        raise DownloadError(f"invalid remote archive size: {size}")
    return size


def _open_range(url: str, start: int, length: int):
    response = _request(url, start=start, length=length)
    status = getattr(response, "status", None)
    if status != 206:
        response.close()
        raise DownloadError(
            f"server ignored range request (status={status}); refusing a full archive download"
        )
    content_range = response.headers.get("Content-Range", "")
    match = re.fullmatch(r"bytes ([0-9]+)-([0-9]+)/([0-9]+|\*)", content_range)
    expected_end = start + length - 1
    if match is None or int(match.group(1)) != start or int(match.group(2)) != expected_end:
        response.close()
        raise DownloadError(
            f"unexpected Content-Range {content_range!r}; expected bytes {start}-{expected_end}"
        )
    return response


def _read_range(url: str, start: int, length: int) -> bytes:
    with _open_range(url, start, length) as response:
        data = response.read(length + 1)
    if len(data) != length:
        raise DownloadError(
            f"short range response at {start}: expected={length} actual={len(data)}"
        )
    return data


def _zip64_values(
    extra: bytes,
    uncompressed: int,
    compressed: int,
    offset: int,
    disk: int,
) -> tuple[int, int, int]:
    position = 0
    while position + 4 <= len(extra):
        tag, size = struct.unpack_from("<HH", extra, position)
        payload = extra[position + 4 : position + 4 + size]
        if len(payload) != size:
            raise DownloadError("truncated ZIP extra field")
        if tag == 0x0001:
            cursor = 0

            def take_u64(label: str) -> int:
                nonlocal cursor
                if cursor + 8 > len(payload):
                    raise DownloadError(f"missing Zip64 {label}")
                value = struct.unpack_from("<Q", payload, cursor)[0]
                cursor += 8
                return value

            if uncompressed == 0xFFFFFFFF:
                uncompressed = take_u64("uncompressed size")
            if compressed == 0xFFFFFFFF:
                compressed = take_u64("compressed size")
            if offset == 0xFFFFFFFF:
                offset = take_u64("local header offset")
            if disk == 0xFFFF:
                if cursor + 4 > len(payload):
                    raise DownloadError("missing Zip64 disk number")
            return uncompressed, compressed, offset
        position += 4 + size
    if 0xFFFFFFFF in {uncompressed, compressed, offset}:
        raise DownloadError("Zip64 sentinel present without a Zip64 extra field")
    return uncompressed, compressed, offset


def _parse_central_directory(data: bytes) -> list[ZipEntry]:
    entries: list[ZipEntry] = []
    position = 0
    while position < len(data):
        if data[position : position + 4] != b"PK\x01\x02":
            raise DownloadError(f"invalid central-directory signature at byte {position}")
        if position + 46 > len(data):
            raise DownloadError("truncated central-directory entry")
        flags = struct.unpack_from("<H", data, position + 8)[0]
        compression = struct.unpack_from("<H", data, position + 10)[0]
        crc32 = struct.unpack_from("<I", data, position + 16)[0]
        compressed = struct.unpack_from("<I", data, position + 20)[0]
        uncompressed = struct.unpack_from("<I", data, position + 24)[0]
        name_length = struct.unpack_from("<H", data, position + 28)[0]
        extra_length = struct.unpack_from("<H", data, position + 30)[0]
        comment_length = struct.unpack_from("<H", data, position + 32)[0]
        disk = struct.unpack_from("<H", data, position + 34)[0]
        offset = struct.unpack_from("<I", data, position + 42)[0]
        entry_end = position + 46 + name_length + extra_length + comment_length
        if entry_end > len(data):
            raise DownloadError("central-directory entry exceeds declared directory size")
        name_start = position + 46
        name_bytes = data[name_start : name_start + name_length]
        try:
            name = name_bytes.decode("utf-8" if flags & (1 << 11) else "cp437")
        except UnicodeDecodeError as error:
            raise DownloadError("cannot decode ZIP entry name") from error
        extra_start = name_start + name_length
        extra = data[extra_start : extra_start + extra_length]
        uncompressed, compressed, offset = _zip64_values(
            extra, uncompressed, compressed, offset, disk
        )
        entries.append(
            ZipEntry(
                name=name,
                local_header_offset=offset,
                compressed_bytes=compressed,
                uncompressed_bytes=uncompressed,
                compression=compression,
                flags=flags,
                crc32=crc32,
            )
        )
        position = entry_end
    return entries


def read_zip_entries(url: str) -> tuple[int, list[ZipEntry]]:
    archive_bytes = _remote_size(url)
    tail_bytes = min(131_072, archive_bytes)
    tail_start = archive_bytes - tail_bytes
    tail = _read_range(url, tail_start, tail_bytes)

    locator_position = tail.rfind(b"PK\x06\x07")
    if locator_position >= 0:
        if locator_position + 20 > len(tail):
            raise DownloadError("truncated Zip64 locator")
        zip64_offset = struct.unpack_from("<Q", tail, locator_position + 8)[0]
        zip64_eocd = _read_range(url, zip64_offset, 56)
        if zip64_eocd[:4] != b"PK\x06\x06":
            raise DownloadError("invalid Zip64 end-of-central-directory signature")
        central_bytes = struct.unpack_from("<Q", zip64_eocd, 40)[0]
        central_offset = struct.unpack_from("<Q", zip64_eocd, 48)[0]
    else:
        eocd_position = tail.rfind(b"PK\x05\x06")
        if eocd_position < 0 or eocd_position + 22 > len(tail):
            raise DownloadError("cannot locate ZIP end-of-central-directory record")
        central_bytes = struct.unpack_from("<I", tail, eocd_position + 12)[0]
        central_offset = struct.unpack_from("<I", tail, eocd_position + 16)[0]
        if central_bytes == 0xFFFFFFFF or central_offset == 0xFFFFFFFF:
            raise DownloadError("Zip64 archive lacks a readable Zip64 locator")

    if central_offset + central_bytes > archive_bytes:
        raise DownloadError("central directory lies outside the remote archive")
    central = _read_range(url, central_offset, central_bytes)
    return archive_bytes, _parse_central_directory(central)


def _scene_entry(scene: str, entries: list[ZipEntry]) -> ZipEntry:
    matches = []
    for entry in entries:
        parts = entry.name.strip("/").split("/")
        if (
            scene in parts
            and parts[-1] == "point_cloud.ply"
            and "iteration_30000" in parts
        ):
            matches.append(entry)
    if not matches:
        raise DownloadError(f"cannot find iteration_30000 point_cloud.ply for {scene!r}")
    if len(matches) > 1:
        matches.sort(key=lambda value: len(value.name), reverse=True)
    return matches[0]


def _copy_inflated(
    source: BinaryIO,
    destination: BinaryIO,
    compressed_bytes: int,
    compression: int,
) -> tuple[str, int, int]:
    digest = hashlib.sha256()
    crc32 = 0
    output_bytes = 0
    inflater = zlib.decompressobj(-15) if compression == 8 else None
    remaining = compressed_bytes
    while remaining:
        chunk = source.read(min(COPY_CHUNK_BYTES, remaining))
        if not chunk:
            raise DownloadError("short compressed payload range")
        remaining -= len(chunk)
        if compression == 0:
            decoded = chunk
        elif inflater is not None:
            try:
                decoded = inflater.decompress(chunk)
            except zlib.error as error:
                raise DownloadError(f"deflate stream failed: {error}") from error
        else:
            raise DownloadError(f"unsupported ZIP compression method: {compression}")
        if decoded:
            destination.write(decoded)
            digest.update(decoded)
            crc32 = binascii.crc32(decoded, crc32)
            output_bytes += len(decoded)
    if inflater is not None:
        decoded = inflater.flush()
        if decoded:
            destination.write(decoded)
            digest.update(decoded)
            crc32 = binascii.crc32(decoded, crc32)
            output_bytes += len(decoded)
        if not inflater.eof:
            raise DownloadError("truncated deflate stream")
        if inflater.unused_data:
            raise DownloadError("unexpected data after deflate stream")
    return digest.hexdigest(), output_bytes, crc32 & 0xFFFFFFFF


def _portable_path(path: pathlib.Path) -> str:
    resolved = path.resolve()
    try:
        return resolved.relative_to(ROOT).as_posix()
    except ValueError:
        return resolved.as_posix()


def _sh_degree(property_names: tuple[str, ...]) -> int:
    rest_count = sum(name.startswith("f_rest_") for name in property_names)
    try:
        return {0: 0, 9: 1, 24: 2, 45: 3}[rest_count]
    except KeyError as error:
        raise DownloadError(f"unsupported f_rest property count: {rest_count}") from error


def _validate_expected(
    scene: str, expected: dict[str, Any], sha256: str, byte_count: int, splat_count: int
) -> str:
    expected_hash = expected.get("sha256")
    if expected.get("bytes") != byte_count or expected.get("splat_count") != splat_count:
        raise DownloadError(
            f"{scene}: identity mismatch expected_bytes={expected.get('bytes')} "
            f"actual_bytes={byte_count} expected_splats={expected.get('splat_count')} "
            f"actual_splats={splat_count}"
        )
    if expected_hash is not None and expected_hash != sha256:
        raise DownloadError(
            f"{scene}: SHA-256 mismatch expected={expected_hash} actual={sha256}"
        )
    return "verified-pinned" if expected_hash is not None else "verified-archive-metadata"


def _validate_entry(scene: str, entry: ZipEntry, expected: dict[str, Any]) -> None:
    expected_crc32 = int(expected["archive_crc32"], 16)
    actual = (entry.compressed_bytes, entry.uncompressed_bytes, entry.crc32)
    pinned = (expected["compressed_bytes"], expected["bytes"], expected_crc32)
    if actual != pinned:
        raise DownloadError(
            f"{scene}: archive entry identity mismatch expected={pinned} actual={actual}"
        )


def _write_json(path: pathlib.Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        prefix=f".{path.name}.",
        suffix=".tmp",
        dir=path.parent,
        delete=False,
    ) as handle:
        json.dump(value, handle, indent=2, sort_keys=False)
        handle.write("\n")
        temporary = pathlib.Path(handle.name)
    os.replace(temporary, path)


def _hash_and_crc(path: pathlib.Path) -> tuple[str, int, int]:
    digest = hashlib.sha256()
    crc32 = 0
    byte_count = 0
    with path.open("rb") as handle:
        while chunk := handle.read(COPY_CHUNK_BYTES):
            digest.update(chunk)
            crc32 = binascii.crc32(chunk, crc32)
            byte_count += len(chunk)
    return digest.hexdigest(), byte_count, crc32 & 0xFFFFFFFF


def _scene_manifest(
    url: str,
    scene: str,
    entry: ZipEntry,
    destination: pathlib.Path,
    expected: dict[str, Any],
    digest: str,
    output_bytes: int,
    crc32: int,
    splat_count: int,
    sh_degree: int,
    ply_format: str,
    vertex_record_bytes: int,
    identity_status: str,
) -> dict[str, Any]:
    return {
        "schema": MANIFEST_SCHEMA,
        "id": f"inria-3dgs-{scene}-iteration-30000",
        "identity_status": identity_status,
        "local_path": _portable_path(destination),
        "source_url": url,
        "archive_entry": entry.name,
        "source_repository": SOURCE_REPOSITORY,
        "license": "NOASSERTION",
        "license_context": (
            "the source repository publishes a research/evaluation software license, "
            "but the pretrained archive does not state an asset-specific model license"
        ),
        "source_repository_license_url": SOURCE_LICENSE_URL,
        "attribution": (
            "Official pretrained 3D Gaussian Splatting model by Kerbl et al., "
            "Inria GRAPHDECO and MPII"
        ),
        "upstream_dataset": expected["family"],
        "upstream_dataset_url": expected["upstream_dataset_url"],
        "redistribution": "prohibited unless model and upstream dataset rights are clarified",
        "allowed_use": "local research/evaluation only",
        "download_helper_source": DOWNLOAD_HELPER_SOURCE,
        "sha256": digest,
        "bytes": output_bytes,
        "archive_crc32": f"{crc32:08x}",
        "splat_count": splat_count,
        "sh_degree": sh_degree,
        "format": ply_format,
        "vertex_record_bytes": vertex_record_bytes,
    }


def download_scene(
    url: str,
    scene: str,
    entry: ZipEntry,
    output_dir: pathlib.Path,
    expected: dict[str, Any],
    *,
    overwrite: bool = False,
) -> dict[str, Any]:
    scene_dir = output_dir.resolve() / scene
    scene_dir.mkdir(parents=True, exist_ok=True)
    destination = scene_dir / "point_cloud.ply"
    manifest_path = scene_dir / "source.json"
    _validate_entry(scene, entry, expected)
    if destination.exists() and not overwrite:
        digest, output_bytes, crc32 = _hash_and_crc(destination)
        if output_bytes != entry.uncompressed_bytes or crc32 != entry.crc32:
            raise DownloadError(
                f"{scene}: existing PLY does not match the pinned archive entry; "
                "pass --overwrite to fetch it again"
            )
        layout = inspect_binary_ply(destination)
        splat_count = layout.vertex.count
        identity_status = _validate_expected(
            scene, expected, digest, output_bytes, splat_count
        )
        if manifest_path.exists():
            try:
                prior = json.loads(manifest_path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError) as error:
                raise DownloadError(f"{scene}: cannot read existing source.json") from error
            if prior.get("sha256") != digest:
                raise DownloadError(f"{scene}: existing PLY and source.json disagree")
        manifest = _scene_manifest(
            url,
            scene,
            entry,
            destination,
            expected,
            digest,
            output_bytes,
            crc32,
            splat_count,
            _sh_degree(layout.vertex.property_names),
            layout.format,
            layout.vertex.record_bytes,
            identity_status,
        )
        _write_json(manifest_path, manifest)
        return manifest
    if manifest_path.exists() and not destination.exists() and not overwrite:
        raise DownloadError(
            f"{scene}: source.json exists without its PLY; pass --overwrite to repair it"
        )
    if entry.flags & 1:
        raise DownloadError(f"{scene}: encrypted ZIP entries are unsupported")
    if entry.compression not in {0, 8}:
        raise DownloadError(f"{scene}: unsupported compression method {entry.compression}")

    local_header = _read_range(url, entry.local_header_offset, 30)
    if local_header[:4] != b"PK\x03\x04":
        raise DownloadError(f"{scene}: invalid local ZIP header")
    local_flags = struct.unpack_from("<H", local_header, 6)[0]
    local_compression = struct.unpack_from("<H", local_header, 8)[0]
    name_length = struct.unpack_from("<H", local_header, 26)[0]
    extra_length = struct.unpack_from("<H", local_header, 28)[0]
    if local_flags != entry.flags or local_compression != entry.compression:
        raise DownloadError(f"{scene}: local and central ZIP headers disagree")
    data_offset = entry.local_header_offset + 30 + name_length + extra_length

    temporary: pathlib.Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w+b",
            prefix=f".{destination.name}.",
            suffix=".tmp",
            dir=scene_dir,
            delete=False,
        ) as handle:
            temporary = pathlib.Path(handle.name)
            with _open_range(url, data_offset, entry.compressed_bytes) as response:
                digest, output_bytes, crc32 = _copy_inflated(
                    response, handle, entry.compressed_bytes, entry.compression
                )
            handle.flush()
            os.fsync(handle.fileno())
    except BaseException:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
        raise
    if temporary is None:
        raise AssertionError("temporary PLY path was not initialized")

    try:
        if output_bytes != entry.uncompressed_bytes:
            raise DownloadError(
                f"{scene}: uncompressed size mismatch expected={entry.uncompressed_bytes} "
                f"actual={output_bytes}"
            )
        if crc32 != entry.crc32:
            raise DownloadError(
                f"{scene}: CRC32 mismatch expected={entry.crc32:08x} actual={crc32:08x}"
            )
        layout = inspect_binary_ply(temporary)
        splat_count = layout.vertex.count
        identity_status = _validate_expected(
            scene, expected, digest, output_bytes, splat_count
        )
        os.replace(temporary, destination)
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise

    manifest = _scene_manifest(
        url,
        scene,
        entry,
        destination,
        expected,
        digest,
        output_bytes,
        crc32,
        splat_count,
        _sh_degree(layout.vertex.property_names),
        layout.format,
        layout.vertex.record_bytes,
        identity_status,
    )
    _write_json(manifest_path, manifest)
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list", action="store_true", help="list the bounded benchmark scenes")
    parser.add_argument("--scenes", nargs="+", choices=sorted(SCENES))
    parser.add_argument(
        "--output-dir",
        type=pathlib.Path,
        default=ROOT / "tests/datasets/external/inria_3dgs",
    )
    parser.add_argument(
        "--acknowledge-local-use-only",
        action="store_true",
        help="acknowledge NOASSERTION rights and local research/evaluation-only use",
    )
    parser.add_argument("--overwrite", action="store_true")
    args = parser.parse_args()

    if args.list:
        for name, value in SCENES.items():
            pin = value["splat_count"] if value["splat_count"] is not None else "unrecorded"
            print(f"{name}\t{value['family']}\tsplats={pin}")
        return 0
    if not args.scenes:
        parser.error("--scenes is required unless --list is used")
    if not args.acknowledge_local_use_only:
        parser.error(
            "pass --acknowledge-local-use-only after reviewing the source and upstream terms"
        )

    print(f"archive={MODELS_ZIP_URL}")
    print("license=NOASSERTION")
    print(f"source_repository_license={SOURCE_LICENSE_URL}")
    print("use=local research/evaluation only; redistribution is not authorized")
    try:
        archive_bytes, entries = read_zip_entries(MODELS_ZIP_URL)
        print(f"archive_bytes={archive_bytes} entries={len(entries)}")
        for scene in args.scenes:
            entry = _scene_entry(scene, entries)
            _validate_entry(scene, entry, SCENES[scene])
            destination = args.output_dir.resolve() / scene / "point_cloud.ply"
            action = "verifying" if destination.exists() and not args.overwrite else "extracting"
            print(
                f"{action}={scene} compressed_bytes={entry.compressed_bytes} "
                f"archive_entry={entry.name}",
                flush=True,
            )
            manifest = download_scene(
                MODELS_ZIP_URL,
                scene,
                entry,
                args.output_dir,
                SCENES[scene],
                overwrite=args.overwrite,
            )
            print(
                f"ply={manifest['local_path']} splats={manifest['splat_count']} "
                f"sha256={manifest['sha256']}",
                flush=True,
            )
    except (DownloadError, OSError, ValueError) as error:
        parser.exit(1, f"INRIA scene fetch failed: {error}\n")
    except KeyboardInterrupt:
        parser.exit(130, "INRIA scene fetch cancelled; incomplete temporary file removed\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
