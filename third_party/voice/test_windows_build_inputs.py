"""Exercise explicit Windows tool selection and rejection before native builds."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

from windows_build_inputs import build_environment


class WindowsInputsTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="voice selected tools ")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.target = "x86_64-pc-windows-msvc"
        self.selected = {}
        for directory, names in {
            "msvc": [
                "cl.exe",
                "link.exe",
                "lib.exe",
                "dumpbin.exe",
                "ml64.exe",
                "nmake.exe",
            ],
            "sdk": ["rc.exe", "mt.exe"],
            "cygwin": [
                "bash.exe",
                "cygpath.exe",
                "automake-1.18",
                "make.exe",
                "link.exe",
            ],
            "cmake": ["cmake.exe"],
            "pkgconf": ["pkgconf.exe"],
            "Windows/System32": ["cmd.exe"],
        }.items():
            parent = self.root / directory
            parent.mkdir(parents=True)
            for name in names:
                (parent / name).touch()
        self.tools = {
            "cc": self.root / "msvc/cl.exe",
            "cxx": self.root / "msvc/cl.exe",
            "link": self.root / "msvc/link.exe",
            "lib": self.root / "msvc/lib.exe",
            "dumpbin": self.root / "msvc/dumpbin.exe",
            "assembler": self.root / "msvc/ml64.exe",
            "bootstrap_make": self.root / "msvc/nmake.exe",
            "rc": self.root / "sdk/rc.exe",
            "mt": self.root / "sdk/mt.exe",
            "make": self.root / "cygwin/make.exe",
            "shell": self.root / "cygwin/bash.exe",
            "cygpath": self.root / "cygwin/cygpath.exe",
            "automake": self.root / "cygwin/automake-1.18",
            "cmake": self.root / "cmake/cmake.exe",
            "pkg_config": self.root / "pkgconf/pkgconf.exe",
            "python": Path(sys.executable),
        }
        self.document = {
            "schemaVersion": 1,
            "target": self.target,
            "tools": {key: str(value) for key, value in self.tools.items()},
            "systemRoot": str(self.root / "Windows"),
            "INCLUDE": [str(self.root / "sdk")],
            "LIB": [str(self.root / "msvc")],
        }
        self.path = self.root / "inputs.json"

    def environment(self):
        self.path.write_text(json.dumps(self.document), encoding="utf-8")
        return build_environment(self.path, self.target, self.selected)[0]

    def test_msvc_link_precedes_same_named_cygwin_tool(self):
        environment = self.environment()
        first = next(
            Path(directory) / "link.exe"
            for directory in environment["PATH"].split(os.pathsep)
            if (Path(directory) / "link.exe").exists()
        )
        self.assertEqual(first, self.tools["link"])
        self.assertEqual(
            {name: environment[name] for name in ("INCLUDE", "LIB", "COMSPEC")},
            {
                "INCLUDE": str(self.root / "sdk"),
                "LIB": str(self.root / "msvc"),
                "COMSPEC": str(self.root / "Windows/System32/cmd.exe"),
            },
        )

    @unittest.skipIf(os.name == "nt", "Portable shell subprocess selection probe")
    def test_subprocess_uses_selected_tool_without_ambient_path(self):
        # Execute the name upstream uses, with a conflicting Cygwin executable.
        for path, text in (
            (self.tools["link"], "msvc"),
            (self.root / "cygwin/link.exe", "cygwin"),
        ):
            path.write_text(f"#!/bin/sh\nprintf '{text}'\n")
            path.chmod(0o755)
        result = subprocess.run(
            ["link.exe"],
            env=self.environment(),
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.stdout, "msvc")

    def test_missing_or_wrong_architecture_assembler_is_rejected(self):
        self.tools["assembler"].unlink()
        with self.assertRaisesRegex(ValueError, "assembler"):
            self.environment()
        self.tools["assembler"].touch()
        self.target = "aarch64-pc-windows-msvc"
        self.document["target"] = self.target
        with self.assertRaisesRegex(ValueError, "assembler"):
            self.environment()
        arm = self.tools["assembler"].with_name("armasm64.exe")
        arm.touch()
        self.document["tools"]["assembler"] = str(arm)
        self.environment()

    def test_shadowed_sdk_tool_is_rejected(self):
        shutil.copyfile(self.tools["rc"], self.root / "msvc/rc.exe")
        with self.assertRaisesRegex(ValueError, "shadowed: rc"):
            self.environment()

    def test_cli_tool_must_match_recorded_selection(self):
        self.selected["cc"] = self.tools["cmake"]
        with self.assertRaisesRegex(ValueError, "disagrees with --cc"):
            self.environment()

    def test_missing_sdk_and_relative_paths_are_rejected(self):
        for value in ([], ["relative"], [str(self.root / "missing")]):
            with self.subTest(value=value):
                self.document["INCLUDE"] = value
                with self.assertRaises(ValueError):
                    self.environment()

    def test_target_mismatch_cannot_reuse_other_inputs(self):
        self.document["target"] = "aarch64-pc-windows-msvc"
        with self.assertRaisesRegex(ValueError, "target"):
            self.environment()
