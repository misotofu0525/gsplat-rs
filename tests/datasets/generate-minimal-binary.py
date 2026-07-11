#!/usr/bin/env python3
"""Generate the deterministic binary PLY equivalent of minimal_ascii.ply."""

from __future__ import annotations

import pathlib
import struct


ROOT = pathlib.Path(__file__).resolve().parent
SOURCE = ROOT / "minimal_ascii.ply"
OUTPUT = ROOT / "minimal_binary.ply"


def main() -> None:
    text = SOURCE.read_text(encoding="ascii")
    header_text, body_text = text.split("end_header\n", 1)
    header_lines = header_text.splitlines()
    properties = [line.split()[-1] for line in header_lines if line.startswith("property float ")]
    vertex_line = next(line for line in header_lines if line.startswith("element vertex "))
    vertex_count = int(vertex_line.split()[-1])
    rows = [
        [float(value) for value in line.split()]
        for line in body_text.splitlines()
        if line.strip()
    ]
    if len(rows) != vertex_count or any(len(row) != len(properties) for row in rows):
        raise ValueError("minimal ASCII fixture shape does not match its header")

    output_header = [
        "ply",
        "format binary_little_endian 1.0",
        "comment deterministic binary equivalent of minimal_ascii.ply",
        vertex_line,
        *[f"property float {name}" for name in properties],
        "end_header",
        "",
    ]
    payload = bytearray("\n".join(output_header).encode("ascii"))
    row_format = struct.Struct(f"<{len(properties)}f")
    for row in rows:
        payload.extend(row_format.pack(*row))
    OUTPUT.write_bytes(payload)
    print(OUTPUT)


if __name__ == "__main__":
    main()
