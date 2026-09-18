"""Project a verified macOS prefix into a private, relocatable development runtime."""

import argparse
import os
from pathlib import Path
import re
import subprocess
import sys

# Import only this script's siblings, including under PYTHONSAFEPATH.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from runtime import Binary as MachO
from runtime import PLUGINS, RuntimeFormat, digest, prepare, required_library_paths

SYSTEM_IMPORTS = frozenset(
    {
        "/usr/lib/libSystem.B.dylib",
        "/usr/lib/libc++.1.dylib",
        "/usr/lib/libobjc.A.dylib",
        "/usr/lib/libiconv.2.dylib",
        "/usr/lib/libresolv.9.dylib",
        "/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit",
        "/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation",
        "/System/Library/Frameworks/CoreServices.framework/Versions/A/CoreServices",
        "/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation",
    }
)


def run(command):
    return subprocess.check_output(
        command, text=True, stderr=subprocess.STDOUT, timeout=30
    )


def inspect(path, target):
    cpu = {"aarch64-apple-darwin": "ARM64", "x86_64-apple-darwin": "X86_64"}[target]
    try:
        output = run(
            [
                "/usr/bin/xcrun",
                "llvm-objdump",
                "--macho",
                "--universal-headers",
                "--private-headers",
                str(path),
            ]
        )
    except subprocess.CalledProcessError as error:
        raise ValueError("LLVM rejected the Mach-O library") from error
    header, *commands = re.split(r"(?m)^Load command [0-9]+\n", output)
    match = re.fullmatch(
        r"Mach header\n[^\n]+\nMH_MAGIC_64 +"
        + cpu
        + r" +\S+ +\S+ +DYLIB +(\d+) +(\d+) +[^\n]+\n",
        header.removeprefix(str(path) + ":\n"),
    )
    if not match or len(commands) != int(match[1]) or int(match[2]) > 1024 * 1024:
        raise ValueError(f"expected a thin {target} dylib with bounded load commands")
    identities, imports, rpaths = [], [], []
    for block in commands:
        match = re.match(r" +cmd (\S+)\n +cmdsize ([0-9]+)\n", block)
        if not match:
            raise ValueError("unrecognized LLVM load-command output")
        command = match[1]
        if command in {
            "LC_PREBOUND_DYLIB",
            "LC_REEXPORT_DYLIB",
            "LC_LAZY_LOAD_DYLIB",
            "LC_LOAD_UPWARD_DYLIB",
            "LC_DYLD_ENVIRONMENT",
            "LC_DYLIB_CODE_SIGN_DRS",
            "LC_LAZY_LOAD_DYLIB_INFO",
            "?(0x0000003a)",
        }:
            raise ValueError(f"unsupported loader command: {command}")
        if command not in {
            "LC_ID_DYLIB",
            "LC_LOAD_DYLIB",
            "LC_LOAD_WEAK_DYLIB",
            "LC_RPATH",
        }:
            continue
        # Match the entire command to reject multiline names masquerading as metadata.
        pattern = r" +path ([^\r\n]+) \(offset [0-9]+\)\n"
        if command != "LC_RPATH":
            pattern = (
                r" +name ([^\r\n]+) \(offset [0-9]+\)\n"
                r" +time stamp [^\n]+\n +current version [^\n]+\ncompatibility version [^\n]+\n"
            )
        value = re.fullmatch(pattern, block[match.end() :])
        if not value:
            raise ValueError(
                "undeclared native dependency or unrecognized LLVM loader output"
            )
        if command == "LC_ID_DYLIB":
            identities.append(value[1])
        elif command == "LC_RPATH":
            rpaths.append(value[1])
        else:
            imports.append(value[1])
    if len(identities) != 1 or not re.fullmatch(
        r"[A-Za-z0-9_+.-]+\.dylib", Path(identities[0]).name
    ):
        raise ValueError("expected one valid native library identity")
    return MachO(identities[0], tuple(imports), tuple(rpaths))


def finalize_copy(destination, metadata, dependency_paths):
    identity = f"@rpath/{destination.name}"
    command = ["/usr/bin/install_name_tool", "-id", identity]
    imports = []
    for dependency in metadata.imports:
        rewritten = dependency
        if dependency not in SYSTEM_IMPORTS:
            rewritten = "@loader_path/" + os.path.relpath(
                dependency_paths[dependency], destination.parent
            )
            command.extend(["-change", dependency, rewritten])
        imports.append(rewritten)
    for rpath in metadata.rpaths:
        command.extend(["-delete_rpath", rpath])
    run([*command, str(destination)])
    # Development code signatures only; no identity, entitlement or trust-policy changes.
    run(
        [
            "/usr/bin/codesign",
            "--force",
            "--sign",
            "-",
            "--timestamp=none",
            str(destination),
        ]
    )
    run(["/usr/bin/codesign", "--verify", "--strict", str(destination)])
    return MachO(identity, tuple(imports), ())


def project(prefix, receipts, target, output):
    if sys.platform != "darwin" or target not in (
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ):
        raise ValueError(
            "runtime projection requires macOS and an explicit macOS target"
        )
    format = RuntimeFormat(
        tuple(
            Path(f"lib/gstreamer-1.0/libgst{name}.dylib") for name in sorted(PLUGINS)
        ),
        SYSTEM_IMPORTS,
        inspect,
        finalize_copy,
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
