"""Prepare verified GNU Linux audio libraries with package-relative loader paths."""

import argparse
import os
from pathlib import Path
import re
import struct
import sys

# Import only this script's siblings, including under PYTHONSAFEPATH.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from runtime import Binary, PLUGINS, RuntimeFormat, prepare, required_library_paths

SYSTEM_IMPORTS = frozenset(
    {
        "libc.so.6",
        "libm.so.6",
        "libdl.so.2",
        "libpthread.so.0",
        "librt.so.1",
        "libresolv.so.2",
        "ld-linux-x86-64.so.2",
        "ld-linux-aarch64.so.1",
    }
)


def inspect(path, target):
    machine = {"x86_64-unknown-linux-gnu": 62, "aarch64-unknown-linux-gnu": 183}[target]
    with path.open("rb") as source:
        data = source.read(64 * 1024 * 1024 + 1)
    if not 64 <= len(data) <= 64 * 1024 * 1024:
        raise ValueError("invalid ELF file size")
    header = struct.unpack_from("<16sHHIQQQIHHHHHH", data)
    if header[0][:7] != b"\x7fELF\x02\x01\x01" or header[0][7] not in (0, 3):
        raise ValueError("expected a little-endian ELF64 library")
    if header[1:4] != (3, machine, 1) or header[8:10] != (64, 56):
        raise ValueError(f"expected a {target} shared library")
    phoff, count = header[5], header[10]
    if not 1 <= count <= 128 or phoff < 64 or phoff + count * 56 > len(data):
        raise ValueError("invalid ELF program headers")
    loads, dynamic = [], []
    for index in range(count):
        kind, _, offset, address, _, size, memory_size, _ = struct.unpack_from(
            "<IIQQQQQQ", data, phoff + index * 56
        )
        if size > memory_size or offset + size > len(data):
            raise ValueError("invalid ELF segment bounds")
        if kind == 1:
            loads.append((address, offset, size))
        elif kind == 2:
            dynamic.append((offset, size, address))
        elif kind == 3:
            raise ValueError("an ELF runtime library must not name an interpreter")
    if len(dynamic) != 1 or not 16 <= dynamic[0][1] <= 65536 or dynamic[0][1] % 16:
        raise ValueError("invalid ELF dynamic table")
    tags = {}
    offset, size, address = dynamic[0]
    mappings = [
        file_offset + address - start
        for start, file_offset, length in loads
        if start <= address and address + size <= start + length
    ]
    if mappings != [offset]:
        raise ValueError("ELF dynamic table must have one matching file-backed mapping")
    for cursor in range(offset, offset + size, 16):
        tag, value = struct.unpack_from("<qQ", data, cursor)
        if tag == 0:
            break
        # Auxiliary/filter libraries and audit modules are additional loader inputs.
        if tag in (0x7FFFFFFD, 0x7FFFFFFF, 0x6FFFFEFB, 0x6FFFFEFC):
            raise ValueError("unsupported ELF loader dependency")
        tags.setdefault(tag, []).append(value)
    else:
        raise ValueError("unterminated ELF dynamic table")
    if any(len(tags.get(tag, [])) != 1 for tag in (5, 10)):
        raise ValueError("invalid ELF dynamic string table")
    address, size = tags[5][0], tags[10][0]
    offsets = [
        offset + address - start
        for start, offset, length in loads
        if start <= address and address + size <= start + length
    ]
    if len(offsets) != 1 or not 1 <= size <= 1024 * 1024:
        raise ValueError("invalid ELF string table bounds")
    strings = data[offsets[0] : offsets[0] + size]
    values = {}
    for tag in (1, 14, 15, 29):
        values[tag] = []
        if tag != 1 and len(tags.get(tag, [])) > 1:
            raise ValueError("duplicate ELF loader metadata")
        for offset in tags.get(tag, []):
            end = strings.find(b"\0", offset)
            if not 0 <= offset < len(strings) or end < 0:
                raise ValueError("invalid ELF loader string")
            value = (
                os.fsdecode(strings[offset:end])
                if tag in (15, 29)
                else strings[offset:end].decode("ascii")
            )
            if tag in (1, 14) and not re.fullmatch(
                r"[A-Za-z0-9_+.-]+\.so(?:\.[0-9]+)*", value
            ):
                raise ValueError("ELF dependencies must be plain library names")
            values[tag].append(value)
    identity = values[14][0] if values[14] else path.name
    return Binary(
        identity,
        tuple(values[1]),
        tuple(f"{tag}={value}" for tag in (15, 29) for value in values[tag]),
    )


def finalize_copy(destination, metadata, dependency_paths):
    allowed = {"29=$ORIGIN", "29=$ORIGIN:$ORIGIN/.."}
    needs_parent = destination.parent.name == "gstreamer-1.0"
    required = "29=$ORIGIN:$ORIGIN/.." if needs_parent else "29=$ORIGIN"
    if any(path not in allowed for path in metadata.rpaths) or (
        any(name not in SYSTEM_IMPORTS for name in metadata.imports)
        and required not in metadata.rpaths
        and "29=$ORIGIN:$ORIGIN/.." not in metadata.rpaths
    ):
        raise ValueError("rebuild native libraries with package-relative runtime paths")
    return metadata


def project(prefix, receipts, target, output):
    if sys.platform != "linux" or target not in (
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
    ):
        raise ValueError(
            "runtime preparation requires GNU Linux and an explicit GNU target"
        )
    format = RuntimeFormat(
        tuple(Path(f"lib/gstreamer-1.0/libgst{name}.so") for name in sorted(PLUGINS)),
        SYSTEM_IMPORTS,
        inspect,
        finalize_copy,
        plugin_dir="lib/gstreamer-1.0",
        required_libraries=tuple(
            Path(path).name for path in required_library_paths(target)
        ),
    )
    prepare(prefix, receipts, target, output, format)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prefix", type=Path, required=True)
    parser.add_argument("--receipts", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    project(args.prefix, args.receipts, args.target, args.output)
