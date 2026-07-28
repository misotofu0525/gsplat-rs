"""Build and validate the upstream Truck product-quality authority."""

from __future__ import annotations

import dataclasses
import datetime
import hashlib
import json
import os
import pathlib
import shutil
import struct
from fractions import Fraction
from typing import Any


SCHEMA = "gsplat-q1-product-quality-authority/v1"
AUTHORITY_CLASS = "upstream_source_camera_images"


class AuthorityError(ValueError):
    pass


@dataclasses.dataclass(frozen=True)
class FileSpec:
    name: str
    bytes: int
    sha256: str


@dataclasses.dataclass(frozen=True)
class AuthoritySpec:
    archive_url: str
    archive: FileSpec
    files: tuple[FileSpec, ...]
    image_names: tuple[str, ...]
    scene_sha256: str
    scene_splat_count: int
    scene_sh_degree: int


OFFICIAL_SPEC = AuthoritySpec(
    archive_url=(
        "https://repo-sam.inria.fr/fungraph/3d-gaussian-splatting/"
        "datasets/input/tandt_db.zip"
    ),
    archive=FileSpec(
        "tandt_db.zip",
        682_628_995,
        "816e62f22a161abbfe841d2a6b10cdf036e297c9fa289b3bfeee9c6ec526d7e1",
    ),
    files=(
        FileSpec(
            "000001.jpg",
            465_332,
            "38063d904ed164b1e32193ab9b087b939e0a873767582e52162672f5d25beb56",
        ),
        FileSpec(
            "000108.jpg",
            442_096,
            "65f3477f1bc53e4a3b75cba59119e89cba3da2d442d9d16a2cefdf36dfbc035d",
        ),
        FileSpec(
            "cameras.bin",
            64,
            "f183f5e1a0ddbca7bb4e1e12c74ee2939069ea69929710a5af0692d3f53e3ac5",
        ),
        FileSpec(
            "images.bin",
            62_106_537,
            "437cdb07de6e45ecb4bdf986f7faa0017148879437b66497145ecdb7ec31403e",
        ),
    ),
    image_names=("000001.jpg", "000108.jpg"),
    scene_sha256="65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c",
    scene_splat_count=2_541_226,
    scene_sh_degree=3,
)


def fail(message: str) -> None:
    raise AuthorityError(message)


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def checked_file(root: pathlib.Path, spec: FileSpec) -> pathlib.Path:
    path = root / spec.name
    if path.is_symlink() or not path.is_file():
        fail(f"{spec.name}: missing regular file")
    if path.stat().st_size != spec.bytes or sha256(path) != spec.sha256:
        fail(f"{spec.name}: byte identity mismatch")
    return path


def jpeg_dimensions(path: pathlib.Path) -> tuple[int, int]:
    data = path.read_bytes()
    if len(data) < 4 or data[:2] != b"\xff\xd8":
        fail(f"{path.name}: missing JPEG SOI")
    offset = 2
    while offset + 4 <= len(data):
        if data[offset] != 0xFF:
            fail(f"{path.name}: malformed JPEG marker")
        while offset < len(data) and data[offset] == 0xFF:
            offset += 1
        if offset >= len(data):
            break
        marker = data[offset]
        offset += 1
        if marker in {0xD8, 0xD9}:
            continue
        if marker == 0xDA:
            break
        if offset + 2 > len(data):
            break
        length = struct.unpack_from(">H", data, offset)[0]
        if length < 2 or offset + length > len(data):
            fail(f"{path.name}: malformed JPEG segment")
        if marker in {0xC0, 0xC1, 0xC2}:
            if length < 8:
                fail(f"{path.name}: malformed JPEG SOF")
            height, width = struct.unpack_from(">HH", data, offset + 3)
            if width <= 0 or height <= 0:
                fail(f"{path.name}: invalid JPEG dimensions")
            return width, height
        offset += length
    fail(f"{path.name}: no supported JPEG SOF")


def parse_pinhole_camera(path: pathlib.Path) -> dict[str, Any]:
    data = path.read_bytes()
    if len(data) < 8:
        fail("cameras.bin: truncated header")
    count = struct.unpack_from("<Q", data, 0)[0]
    offset = 8
    cameras: dict[int, dict[str, Any]] = {}
    for _ in range(count):
        if offset + 56 > len(data):
            fail("cameras.bin: truncated PINHOLE record")
        camera_id, model_id = struct.unpack_from("<ii", data, offset)
        width, height = struct.unpack_from("<QQ", data, offset + 8)
        params = struct.unpack_from("<4d", data, offset + 24)
        offset += 56
        if model_id != 1:
            fail("cameras.bin: product authority requires COLMAP PINHOLE")
        if camera_id in cameras or width <= 0 or height <= 0:
            fail("cameras.bin: invalid or duplicate camera")
        cameras[camera_id] = {
            "camera_id": camera_id,
            "model": "PINHOLE",
            "width": width,
            "height": height,
            "fx": params[0],
            "fy": params[1],
            "cx": params[2],
            "cy": params[3],
        }
    if offset != len(data) or not cameras:
        fail("cameras.bin: trailing bytes or empty camera set")
    return cameras


def parse_selected_images(
    path: pathlib.Path,
    cameras: dict[int, dict[str, Any]],
    selected: tuple[str, ...],
) -> dict[str, dict[str, Any]]:
    data = path.read_bytes()
    if len(data) < 8:
        fail("images.bin: truncated header")
    count = struct.unpack_from("<Q", data, 0)[0]
    offset = 8
    found: dict[str, dict[str, Any]] = {}
    seen_names: set[str] = set()
    for metadata_index in range(count):
        if offset + 64 > len(data):
            fail("images.bin: truncated image record")
        image_id = struct.unpack_from("<i", data, offset)[0]
        qvec = struct.unpack_from("<4d", data, offset + 4)
        tvec = struct.unpack_from("<3d", data, offset + 36)
        camera_id = struct.unpack_from("<i", data, offset + 60)[0]
        offset += 64
        try:
            name_end = data.index(0, offset)
        except ValueError:
            fail("images.bin: unterminated image name")
        try:
            name = data[offset:name_end].decode("utf-8")
        except UnicodeDecodeError:
            fail("images.bin: image name is not UTF-8")
        offset = name_end + 1
        if offset + 8 > len(data):
            fail("images.bin: missing point count")
        point_count = struct.unpack_from("<Q", data, offset)[0]
        offset += 8
        point_bytes = point_count * 24
        if point_bytes > len(data) - offset:
            fail("images.bin: truncated points2D")
        offset += point_bytes
        if name in seen_names:
            fail(f"images.bin: duplicate image name {name}")
        seen_names.add(name)
        if name in selected:
            camera = cameras.get(camera_id)
            if camera is None:
                fail(f"images.bin: {name} references missing camera")
            found[name] = {
                "metadata_index": metadata_index,
                "image_id": image_id,
                "name": name,
                "qvec_wxyz": list(qvec),
                "tvec": list(tvec),
                "camera": camera,
            }
    if offset != len(data):
        fail("images.bin: trailing bytes")
    if set(found) != set(selected):
        fail("images.bin: selected image set is incomplete")
    return found


def receipt_for_source(
    source: pathlib.Path,
    *,
    spec: AuthoritySpec = OFFICIAL_SPEC,
) -> dict[str, Any]:
    files = {item.name: checked_file(source, item) for item in spec.files}
    cameras = parse_pinhole_camera(files["cameras.bin"])
    images = parse_selected_images(files["images.bin"], cameras, spec.image_names)
    views = []
    for name in spec.image_names:
        image_path = files[name]
        image_width, image_height = jpeg_dimensions(image_path)
        record = images[name]
        camera = record["camera"]
        sx = Fraction(image_width, camera["width"])
        sy = Fraction(image_height, camera["height"])
        views.append(
            {
                "name": name,
                "jpeg": {
                    "path": f"source/{name}",
                    "bytes": image_path.stat().st_size,
                    "sha256": sha256(image_path),
                    "width": image_width,
                    "height": image_height,
                },
                "colmap": record,
                "intrinsic_scale": {
                    "sx": {"numerator": sx.numerator, "denominator": sx.denominator},
                    "sy": {"numerator": sy.numerator, "denominator": sy.denominator},
                    "fx": camera["fx"] * float(sx),
                    "fy": camera["fy"] * float(sy),
                    "cx": camera["cx"] * float(sx),
                    "cy": camera["cy"] * float(sy),
                },
            }
        )
    return {
        "schema": SCHEMA,
        "authority_class": AUTHORITY_CLASS,
        "source": {
            "url": spec.archive_url,
            "archive": {
                "bytes": spec.archive.bytes,
                "sha256": spec.archive.sha256,
            },
            "files": [
                {"path": f"source/{item.name}", "bytes": item.bytes, "sha256": item.sha256}
                for item in spec.files
            ],
        },
        "scene": {
            "id": "inria-3dgs-truck-iteration-30000",
            "sha256": spec.scene_sha256,
            "splat_count": spec.scene_splat_count,
            "sh_degree": spec.scene_sh_degree,
        },
        "views": views,
        "image_transform": "none_source_jpeg_pixels",
        "endpoint_scope": ["gsplat_rs", "playcanvas"],
    }


def receipt_for_inputs(
    archive: pathlib.Path,
    source: pathlib.Path,
    *,
    spec: AuthoritySpec = OFFICIAL_SPEC,
) -> dict[str, Any]:
    checked_file(
        archive.parent, dataclasses.replace(spec.archive, name=archive.name)
    )
    return receipt_for_source(source, spec=spec)


def validate_receipt(receipt: Any, *, spec: AuthoritySpec = OFFICIAL_SPEC) -> None:
    if not isinstance(receipt, dict):
        fail("authority receipt must be an object")
    if receipt.get("schema") != SCHEMA or receipt.get("authority_class") != AUTHORITY_CLASS:
        fail("authority schema or class mismatch")
    if receipt.get("endpoint_scope") != ["gsplat_rs", "playcanvas"]:
        fail("authority endpoint scope mismatch")
    if receipt.get("image_transform") != "none_source_jpeg_pixels":
        fail("authority image transform mismatch")
    scene = receipt.get("scene")
    if not isinstance(scene, dict) or (
        scene.get("id") != "inria-3dgs-truck-iteration-30000"
        or scene.get("sha256") != spec.scene_sha256
        or scene.get("splat_count") != spec.scene_splat_count
        or scene.get("sh_degree") != spec.scene_sh_degree
    ):
        fail("authority scene identity mismatch")
    views = receipt.get("views")
    if not isinstance(views, list) or [
        view.get("name") for view in views if isinstance(view, dict)
    ] != list(spec.image_names):
        fail("authority view set mismatch")
    source = receipt.get("source")
    expected_files = [
        {"path": f"source/{item.name}", "bytes": item.bytes, "sha256": item.sha256}
        for item in spec.files
    ]
    if not isinstance(source, dict) or (
        source.get("url") != spec.archive_url
        or source.get("archive")
        != {"bytes": spec.archive.bytes, "sha256": spec.archive.sha256}
        or source.get("files") != expected_files
    ):
        fail("authority source identity mismatch")


def build_authority(
    archive: pathlib.Path,
    source: pathlib.Path,
    output: pathlib.Path,
    *,
    spec: AuthoritySpec = OFFICIAL_SPEC,
) -> dict[str, Any]:
    if output.exists() or output.is_symlink():
        fail(f"output already exists: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = output.with_name(f".{output.name}.staging-{os.getpid()}")
    if staging.exists() or staging.is_symlink():
        fail(f"staging path already exists: {staging}")
    receipt = receipt_for_inputs(archive, source, spec=spec)
    validate_receipt(receipt, spec=spec)
    receipt["generated_at_utc"] = datetime.datetime.now(
        datetime.timezone.utc
    ).isoformat().replace("+00:00", "Z")
    try:
        (staging / "source").mkdir(parents=True)
        for item in spec.files:
            shutil.copyfile(source / item.name, staging / "source" / item.name)
        encoded = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
        with (staging / "authority.json").open("x", encoding="utf-8") as handle:
            handle.write(encoded)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(staging, output)
    except Exception:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    return receipt


def validate_authority(
    root: pathlib.Path, *, spec: AuthoritySpec = OFFICIAL_SPEC
) -> dict[str, Any]:
    if root.is_symlink() or not root.is_dir():
        fail("authority root must be a regular directory")
    expected = {"authority.json", "source"}
    if {path.name for path in root.iterdir()} != expected:
        fail("authority root file set mismatch")
    receipt_path = root / "authority.json"
    if receipt_path.is_symlink() or not receipt_path.is_file():
        fail("authority.json must be a regular file")
    try:
        receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read authority receipt: {error}")
    validate_receipt(receipt, spec=spec)
    generated = receipt.get("generated_at_utc")
    if not isinstance(generated, str) or not generated.endswith("Z"):
        fail("authority generated_at_utc must be UTC")
    try:
        parsed = datetime.datetime.fromisoformat(generated[:-1] + "+00:00")
    except ValueError:
        fail("authority generated_at_utc is invalid")
    if parsed.tzinfo != datetime.timezone.utc:
        fail("authority generated_at_utc must be UTC")
    source = root / "source"
    if source.is_symlink() or not source.is_dir():
        fail("authority source must be a regular directory")
    if {path.name for path in source.iterdir()} != {item.name for item in spec.files}:
        fail("authority source file set mismatch")
    for item in spec.files:
        checked_file(source, item)
    expected = receipt_for_source(source, spec=spec)
    observed = dict(receipt)
    observed.pop("generated_at_utc", None)
    if observed != expected:
        fail("authority receipt does not match retained source")
    return receipt
