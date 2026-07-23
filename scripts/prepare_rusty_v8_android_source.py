#!/usr/bin/env python3

from __future__ import annotations

import argparse
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_REMOTE = "https://github.com/denoland/rusty_v8.git"
DEFAULT_SUBMODULES = ["build", "v8", "tools/clang", "buildtools"]


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


def run(command: list[str], cwd: Path | None = None) -> None:
    subprocess.run(command, cwd=cwd, check=True)


def ensure_android_toolchain_symlink(checkout_dir: Path) -> None:
    android_toolchain_dir = checkout_dir / "third_party" / "android_toolchain"
    android_toolchain_dir.mkdir(parents=True, exist_ok=True)

    ndk_link = android_toolchain_dir / "ndk"
    expected_target = Path("..") / "android_ndk"

    if ndk_link.is_symlink() and ndk_link.readlink() == expected_target:
        return

    if ndk_link.exists() or ndk_link.is_symlink():
        ndk_link.unlink()

    ndk_link.symlink_to(expected_target)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Prepare a full rusty_v8 checkout for Android artifact builds."
    )
    parser.add_argument(
        "--checkout-dir",
        default=str(ROOT / ".artifacts" / "rusty_v8-src"),
        help="Base directory where the rusty_v8 checkout will be created.",
    )
    parser.add_argument(
        "--remote",
        default=DEFAULT_REMOTE,
        help=f"Git remote used for the checkout (default: {DEFAULT_REMOTE})",
    )
    parser.add_argument(
        "--tag",
        help="Optional rusty_v8 tag. Defaults to v<resolved_v8_crate_version>.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    version = resolved_v8_crate_version()
    tag = args.tag or f"v{version}"

    checkout_root = Path(args.checkout_dir).resolve()
    checkout_dir = checkout_root / tag

    if not checkout_dir.exists():
        checkout_root.mkdir(parents=True, exist_ok=True)
        run(
            [
                "git",
                "clone",
                "--depth",
                "1",
                "--branch",
                tag,
                args.remote,
                str(checkout_dir),
            ]
        )

    run(
        [
            "git",
            "submodule",
            "update",
            "--init",
            "--depth",
            "1",
            "--recursive",
            *DEFAULT_SUBMODULES,
        ],
        cwd=checkout_dir,
    )
    ensure_android_toolchain_symlink(checkout_dir)

    print(f"resolved v8 crate version: {version}")
    print(f"checkout tag: {tag}")
    print(f"checkout path: {checkout_dir}")
    print("initialized submodules:")
    for submodule in DEFAULT_SUBMODULES:
        print(f"- {submodule}")
    print("- android_toolchain/ndk -> ../android_ndk symlink bootstrapped")
    print()
    print("next step:")
    print('  export GN="$HOME/tmp/gn-bootstrap/out/gn"  # or another recent gn binary')
    print("  export V8_FROM_SOURCE=1")
    print("  python3 build/linux/sysroot_scripts/install-sysroot.py --arch=amd64")
    print(
        f"  cd {checkout_dir} && cargo build --target aarch64-linux-android --release"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
