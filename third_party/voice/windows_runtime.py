"""Prepare verified Windows audio DLLs in one private development runtime directory."""

import argparse
from pathlib import Path
import re
import subprocess
import sys

# Import only this script's siblings, including under PYTHONSAFEPATH.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from runtime import Binary, PLUGINS, RuntimeFormat, prepare, required_library_paths

# VCRUNTIME140 is a development prerequisite, not a guaranteed Windows component.
EXTERNAL_IMPORTS = frozenset(
    {
        "advapi32.dll",
        "dnsapi.dll",
        "iphlpapi.dll",
        "kernel32.dll",
        "ole32.dll",
        "shell32.dll",
        "shlwapi.dll",
        "user32.dll",
        "ws2_32.dll",
        "vcruntime140.dll",
        *(
            f"api-ms-win-crt-{part}-l1-1-0.dll"
            for part in (
                "conio",
                "convert",
                "environment",
                "filesystem",
                "heap",
                "locale",
                "math",
                "process",
                "runtime",
                "stdio",
                "string",
                "time",
                "utility",
            )
        ),
    }
)


def inspect(path, target):
    machine = {"x86_64-pc-windows-msvc": "8664", "aarch64-pc-windows-msvc": "AA64"}[
        target
    ]
    if not 64 <= path.stat().st_size <= 64 * 1024 * 1024:
        raise ValueError("invalid PE file size")
    result = subprocess.run(
        ["dumpbin", "/nologo", "/headers", "/dependents", "/exports", str(path)],
        capture_output=True,
        timeout=30,
    )
    output = result.stdout.decode("ascii", errors="backslashreplace").replace(
        "\r\n", "\n"
    )
    if (
        result.returncode
        or result.stderr
        or re.search(r"(?:fatal error|warning) LNK[0-9]+", output)
    ):
        raise ValueError("dumpbin rejected the PE library")
    headers = output.split("SECTION HEADER #", 1)[0]
    if (
        "File Type: DLL\n" not in headers
        or not re.search(r"^ +" + machine + r" machine \(", headers, re.M)
        or not re.search(r"^ +20B magic # \(PE32\+\)$", headers, re.M)
    ):
        raise ValueError(f"expected a {target} PE32+ DLL")
    count = re.search(r"^ +([0-9A-F]+) number of sections$", headers, re.M)
    if not count or not 1 <= int(count[1], 16) <= 96:
        raise ValueError("invalid PE section table")
    directories = {
        name: (int(address, 16), int(size, 16))
        for address, size, name in re.findall(
            r"^ +([0-9A-F]+) \[ *([0-9A-F]+)\] RVA \[size\] of ([^\n]+ Directory)$",
            headers,
            re.M,
        )
    }
    if len(directories) != 16:
        raise ValueError("unsupported PE data directories")
    if any(
        directories.get(name) != (0, 0)
        for name in ("Delay Import Directory", "COM Descriptor Directory")
    ):
        raise ValueError("delay-load and managed DLL dependencies are unsupported")
    if "(forwarded to " in output:
        raise ValueError("forwarded DLL exports are unsupported loader dependencies")
    groups = re.findall(
        r"\n  Image has the following dependencies:\n\n(.*?)(?:\n\n|\Z)", output, re.S
    )
    if len(groups) > 1:
        raise ValueError("ambiguous dumpbin dependencies")
    imports = (
        tuple(line.strip().lower() for line in groups[0].splitlines()) if groups else ()
    )
    if any(not re.fullmatch(r"[a-z0-9_+.-]+\.dll", name) for name in imports):
        raise ValueError("PE dependencies must be plain DLL names")
    address, size = directories["Import Directory"]
    # A descriptor per DLL plus its null terminator; reject truncated/injected output.
    if (address, size) != (0, 0) and (
        not address or size != 20 * (len(imports) + 1) or len(imports) > 128
    ):
        raise ValueError("inconsistent PE import directory")
    return Binary(path.name.lower(), imports, ())


def finalize_copy(destination, metadata, dependency_paths):
    # DLL import names already resolve among siblings; do not alter signed bytes.
    return metadata


def project(prefix, receipts, target, output):
    if sys.platform != "win32" or target not in (
        "x86_64-pc-windows-msvc",
        "aarch64-pc-windows-msvc",
    ):
        raise ValueError(
            "runtime preparation requires Windows and an explicit MSVC target"
        )
    format = RuntimeFormat(
        tuple(Path(f"lib/gstreamer-1.0/gst{name}.dll") for name in sorted(PLUGINS)),
        EXTERNAL_IMPORTS,
        inspect,
        finalize_copy,
        library_dir="bin",
        plugin_dir="bin",
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
