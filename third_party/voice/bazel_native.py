#!/usr/bin/env python3
"""Run the existing native recipe with Bazel-declared inputs; export a raw prefix."""

import json
import os
from pathlib import Path
import shutil
import sys
import tarfile
import tempfile
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))
from build_native import NativeBuild


def main():
    config = json.loads(Path(sys.argv[1]).read_text())
    root = str(Path.cwd())
    # Toolchain flags may name execroot-relative SDK files even though upstream
    # build systems change directory. Substitute a literal marker, never a shell.
    config = json.loads(json.dumps(config).replace("@VOICE_EXECROOT@", root))
    with tempfile.TemporaryDirectory(prefix="voice-native-") as temporary:
        temporary = Path(temporary)
        archives = temporary / "archives"
        archives.mkdir()
        for archive in config.pop("archives"):
            source = Path(archive)
            shutil.copyfile(source, archives / source.name)
        prefix = Path(config.pop("prefix")).absolute()
        receipt = Path(config.pop("receipt")).absolute()
        ld = Path(config.pop("ld")).absolute()
        for name in (
            "cc",
            "cxx",
            "ar",
            "ranlib",
            "cmake",
            "make",
            "pkg_config",
            "shell",
        ):
            config[name] = Path(config[name]).absolute()
        args = SimpleNamespace(
            **config,
            archives=archives,
            output=temporary / "build",
            bootstrap_make=None,
        )
        builder = NativeBuild(args, os.environ)
        # Libtool probes the raw linker separately from compiler link flags.
        builder.environment["LD"] = str(ld)
        try:
            builder.build()
        except Exception:
            # Preserve the failing upstream diagnostic in the Bazel action log.
            if builder.record["steps"]:
                log = builder.output / (builder.record["steps"][-1]["name"] + ".log")
                if log.is_file():
                    print(log.read_text(errors="replace"), file=sys.stderr)
            raise
        with tarfile.open(prefix, "w") as archive:
            archive.add(builder.prefix, arcname=".")
        shutil.copyfile(builder.output / "built.json", receipt)


if __name__ == "__main__":
    main()
