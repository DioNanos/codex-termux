"""Prepare an inspected build output using the existing platform runtime policy.

Receipts describe the declared build inputs and inspection, not authenticity or
approval. The current build commit comes from Bazel's workspace status file.
"""

import argparse
import importlib
import json
from pathlib import Path
import re
import sys
import tarfile
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from runtime import digest
from sdk import export_sdk


def prepare_built(prefix, build_receipt, status, target, output, *, sdk_output=None):
    prefix = prefix.resolve(strict=True)
    if build_receipt.stat().st_size > 1024 * 1024 or status.stat().st_size > 65536:
        raise ValueError("native build metadata exceeds limits")
    build = json.loads(build_receipt.read_text())
    commits = [
        line.removeprefix("STABLE_GIT_COMMIT ")
        for line in status.read_text().splitlines()
        if line.startswith("STABLE_GIT_COMMIT ")
    ]
    manifest_hash = digest(Path(__file__).with_name("sources.json"))
    steps = build.get("steps", [])
    if (
        len(commits) != 1
        or not re.fullmatch(r"[0-9a-f]{40}", commits[0])
        or build.get("target") != target
        or build.get("manifest_sha256") != manifest_hash
        or not 1 <= len(steps) <= 128
        or any(step.get("exit_code") != 0 for step in steps)
        or steps[-1].get("name") != "gst-plugins-good-install"
    ):
        raise ValueError("native build receipt is incomplete or mismatched")
    suffix = target.partition("-")[2]
    modules = {
        "apple-darwin": "macos_runtime",
        "unknown-linux-gnu": "linux_runtime",
        "pc-windows-msvc": "windows_runtime",
    }
    if suffix not in modules or target.partition("-")[0] not in ("aarch64", "x86_64"):
        raise ValueError("unsupported native runtime target")
    platform = importlib.import_module(modules[suffix])
    records = []
    for path in sorted(prefix.rglob("*")):
        if not re.fullmatch(r".+\.(?:dylib|dll|so(?:\.[0-9]+)*)", path.name):
            continue
        if path.is_symlink():
            if not path.resolve(strict=True).is_relative_to(prefix):
                raise ValueError("native library link escapes its prefix")
            continue
        if not path.is_file():
            continue
        if len(records) >= 128:
            raise ValueError("native inventory exceeds limits")
        platform.inspect(path, target)
        records.append(
            {
                "path": path.relative_to(prefix).as_posix(),
                "target": target,
                "sha256": digest(path),
            }
        )
    if not records:
        raise ValueError("native prefix contains no libraries")
    with tempfile.TemporaryDirectory(prefix="voice-receipts-") as temporary:
        receipts = Path(temporary)
        (receipts / "inspection").mkdir()
        (receipts / "inspection/binaries.json").write_text(json.dumps(records))
        (receipts / "ci.json").write_text(
            json.dumps(
                {
                    "commit": commits[0],
                    "target": target,
                    "manifest_sha256": manifest_hash,
                    "build_complete": True,
                    "inspection_complete": True,
                }
            )
        )
        platform.project(prefix, receipts, target, output)
        if sdk_output is not None:
            export_sdk(prefix, receipts, target, sdk_output)


def prepare_archive(archive, build_receipt, status, target, output, *, sdk_output=None):
    # Archives preserve native aliases through Bazel's cache and sandbox links.
    with tempfile.TemporaryDirectory(prefix="voice-prefix-") as temporary:
        prefix = Path(temporary)
        with tarfile.open(archive) as source:
            source.extractall(prefix, filter="data")
        prepare_built(
            prefix, build_receipt, status, target, output, sdk_output=sdk_output
        )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("prefix", "build-receipt", "status", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--sdk-output", type=Path)
    args = parser.parse_args()
    # Executors may leave the TreeArtifact absent or create an empty directory.
    try:
        args.output.rmdir()
    except FileNotFoundError:
        pass
    if args.sdk_output is not None:
        try:
            args.sdk_output.rmdir()
        except FileNotFoundError:
            pass
    prepare_archive(
        args.prefix,
        args.build_receipt,
        args.status,
        args.target,
        args.output,
        sdk_output=args.sdk_output,
    )
