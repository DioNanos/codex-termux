#!/usr/bin/env python3

import argparse
import hashlib
import shlex
import shutil
import sys
import tomllib
import urllib.error
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_TARGET = "aarch64-linux-android"
MANIFEST_PATH = ROOT / "third_party" / "v8" / "android-artifacts.toml"


def resolved_v8_crate_version() -> str:
    cargo_lock = tomllib.loads((ROOT / "codex-rs" / "Cargo.lock").read_text())
    versions = sorted(
        {
            package["version"]
            for package in cargo_lock["package"]
            if package["name"] == "v8"
        }
    )
    if len(versions) != 1:
        raise SystemExit(f"expected exactly one resolved v8 version, found: {versions}")
    return versions[0]


def download(url: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    with (
        urllib.request.urlopen(url, timeout=120) as response,
        destination.open("wb") as output,
    ):
        shutil.copyfileobj(response, output)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Fetch fork-owned rusty_v8 Android artifacts for Cargo builds."
    )
    parser.add_argument(
        "--target",
        default=DEFAULT_TARGET,
        help=f"Rust target triple to fetch (default: {DEFAULT_TARGET})",
    )
    parser.add_argument(
        "--release-tag",
        help="Optional release-tag override. Defaults to the audited manifest entry.",
    )
    parser.add_argument(
        "--output-dir",
        default=str(ROOT / ".artifacts" / "rusty_v8"),
        help="Directory where the archive and binding will be stored.",
    )
    return parser.parse_args()


def load_manifest() -> dict[str, object]:
    if not MANIFEST_PATH.exists():
        return {}
    return tomllib.loads(MANIFEST_PATH.read_text())


def manifest_entry(version: str, target: str) -> dict[str, str] | None:
    manifest = load_manifest()
    versions = manifest.get("versions")
    if not isinstance(versions, dict):
        return None
    version_entry = versions.get(version)
    if not isinstance(version_entry, dict):
        return None
    targets = version_entry.get("targets")
    if not isinstance(targets, dict):
        return None
    target_entry = targets.get(target)
    if not isinstance(target_entry, dict):
        return None
    return {key: value for key, value in target_entry.items() if isinstance(value, str)}


def main() -> int:
    args = parse_args()
    version = resolved_v8_crate_version()
    manifest = manifest_entry(version, args.target)
    if manifest is None:
        raise SystemExit(
            "missing pinned rusty_v8 Android manifest entry for "
            f"version {version}, target {args.target}; generate and audit the "
            "artifacts before building a release"
        )

    required_fields = (
        "repository",
        "release_tag",
        "archive_sha256",
        "binding_sha256",
    )
    missing_fields = [field for field in required_fields if not manifest.get(field)]
    if missing_fields:
        raise SystemExit(
            "incomplete rusty_v8 Android manifest entry; missing: "
            + ", ".join(missing_fields)
        )
    for digest_field in ("archive_sha256", "binding_sha256"):
        digest = manifest[digest_field]
        if len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest):
            raise SystemExit(
                f"invalid lowercase SHA-256 in manifest field {digest_field}: {digest}"
            )

    release_tag = args.release_tag or manifest["release_tag"]
    repository = manifest["repository"]
    output_dir = Path(args.output_dir).resolve()

    archive_name = f"librusty_v8_release_{args.target}.a.gz"
    binding_name = f"src_binding_release_{args.target}.rs"

    base_url = f"https://github.com/{repository}/releases/download/{release_tag}"
    archive_url = f"{base_url}/{archive_name}"
    binding_url = f"{base_url}/{binding_name}"

    archive_path = output_dir / release_tag / archive_name
    binding_path = output_dir / release_tag / binding_name

    try:
        download(archive_url, archive_path)
        download(binding_url, binding_path)
    except urllib.error.HTTPError as exc:
        raise SystemExit(
            "failed to download fork-owned rusty_v8 Android artifacts; "
            f"missing asset or tag: {exc.url} ({exc.code})"
        ) from exc
    except urllib.error.URLError as exc:
        raise SystemExit(
            f"failed to download rusty_v8 Android artifacts: {exc}"
        ) from exc

    expected_archive_sha = manifest["archive_sha256"]
    actual_archive_sha = sha256(archive_path)
    if actual_archive_sha != expected_archive_sha:
        raise SystemExit(
            f"archive checksum mismatch for {archive_path}; "
            f"expected {expected_archive_sha}, got {actual_archive_sha}"
        )
    expected_binding_sha = manifest["binding_sha256"]
    actual_binding_sha = sha256(binding_path)
    if actual_binding_sha != expected_binding_sha:
        raise SystemExit(
            f"binding checksum mismatch for {binding_path}; "
            f"expected {expected_binding_sha}, got {actual_binding_sha}"
        )

    print(f"resolved v8 crate version: {version}")
    print(f"release tag: {release_tag}")
    print(f"repository: {repository}")
    print(f"archive: {archive_path}")
    print(f"archive sha256: {sha256(archive_path)}")
    print(f"binding: {binding_path}")
    print(f"binding sha256: {sha256(binding_path)}")
    print()
    print(f"export RUSTY_V8_ARCHIVE={shlex.quote(str(archive_path))}")
    print(f"export RUSTY_V8_SRC_BINDING_PATH={shlex.quote(str(binding_path))}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
