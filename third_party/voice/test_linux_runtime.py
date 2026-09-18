"""Exercise real ELF relocation and rejection of unsafe native build inputs."""

import json
import os
from pathlib import Path
import platform
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from linux_runtime import inspect, project
from runtime import PLUGINS, digest


@unittest.skipUnless(sys.platform == "linux", "GNU Linux native runtime preparation")
class RuntimeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="native voice ")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.prefix, self.receipts, self.output = (
            self.root / name for name in ("input é", "receipts", "runtime")
        )
        self.target = (
            "aarch64" if platform.machine() == "aarch64" else "x86_64"
        ) + "-unknown-linux-gnu"
        (self.prefix / "lib/gstreamer-1.0").mkdir(parents=True)
        (self.receipts / "inspection").mkdir(parents=True)
        source = self.root / "fixture.c"
        source.write_text("int voice_fixture(void) { return 42; }\n")
        self.library = self.prefix / "lib/libfixture.so.1.2"
        subprocess.run(
            [
                "cc",
                "-shared",
                "-fPIC",
                str(source),
                "-o",
                str(self.library),
                "-Wl,-soname,libfixture.so.1",
            ],
            check=True,
            capture_output=True,
        )
        source.write_text("int voice_gio_dependency(void) { return 84; }\n")
        gio_dependency = self.prefix / "lib/libgiofixture.so.1.2"
        subprocess.run(
            [
                "cc",
                "-shared",
                "-fPIC",
                str(source),
                "-o",
                str(gio_dependency),
                "-Wl,-soname,libgiofixture.so.1",
            ],
            check=True,
            capture_output=True,
        )
        source.write_text(
            "extern int voice_gio_dependency(void); "
            "int voice_gio_fixture(void) { return voice_gio_dependency(); }\n"
        )
        subprocess.run(
            [
                "cc",
                "-shared",
                "-fPIC",
                str(source),
                str(gio_dependency),
                "-Wl,-rpath,$ORIGIN",
                "-o",
                str(self.prefix / "lib/libgio-2.0.so.0.8800.3"),
                "-Wl,-soname,libgio-2.0.so.0",
            ],
            check=True,
            capture_output=True,
        )
        source.write_text(
            "extern int voice_fixture(void); int voice_plugin(void) { return voice_fixture(); }\n"
        )
        for name in PLUGINS:
            plugin = self.prefix / f"lib/gstreamer-1.0/libgst{name}.so"
            subprocess.run(
                [
                    "cc",
                    "-shared",
                    "-fPIC",
                    str(source),
                    str(self.library),
                    "-o",
                    str(plugin),
                    f"-Wl,-soname,{plugin.name}",
                    "-Wl,-rpath,$ORIGIN:$ORIGIN/..",
                ],
                check=True,
                capture_output=True,
            )
        self.inventory_path = self.receipts / "inspection/binaries.json"
        self.inventory = [
            {
                "path": p.relative_to(self.prefix).as_posix(),
                "sha256": digest(p),
                "target": self.target,
            }
            for p in sorted(self.prefix.rglob("*.so*"))
            if p.is_file()
        ]
        self.inventory_path.write_text(json.dumps(self.inventory))
        (self.receipts / "ci.json").write_text(
            json.dumps(
                {
                    "commit": "a" * 40,
                    "target": self.target,
                    "build_complete": True,
                    "inspection_complete": True,
                    "manifest_sha256": digest(Path(__file__).with_name("sources.json")),
                }
            )
        )

    def test_relocated_libraries_load_without_original_prefix(self):
        subprocess.run(
            [
                sys.executable,
                str(Path(__file__).with_name("linux_runtime.py").resolve()),
                "--prefix",
                str(self.prefix),
                "--receipts",
                str(self.receipts),
                "--target",
                self.target,
                "--output",
                str(self.output),
            ],
            check=True,
            capture_output=True,
            cwd=self.root,
            env={**os.environ, "PYTHONSAFEPATH": "1"},
        )
        self.assertEqual(
            {r["path"]: digest(self.prefix / r["path"]) for r in self.inventory},
            {r["path"]: r["sha256"] for r in self.inventory},
        )
        moved = self.root / "moved runtime"
        self.output.rename(moved)
        shutil.rmtree(self.prefix)
        environment = {k: v for k, v in os.environ.items() if not k.startswith("LD_")}
        result = subprocess.run(
            [
                sys.executable,
                "-c",
                "import ctypes,json,pathlib,sys; root=pathlib.Path(sys.argv[1]); "
                "manifest=json.loads((root/'runtime.json').read_text()); "
                "print([ctypes.CDLL(str(root/p)).voice_plugin() for p in manifest['plugins']] + "
                "[ctypes.CDLL(str(root/'lib/libgio-2.0.so.0')).voice_gio_fixture()])",
                str(moved),
            ],
            check=True,
            capture_output=True,
            text=True,
            env=environment,
            cwd=self.root,
        )
        self.assertEqual(json.loads(result.stdout), [42] * len(PLUGINS) + [84])
        manifest = json.loads((moved / "runtime.json").read_text())
        self.assertEqual(
            {record["path"] for record in manifest["libraries"]},
            {
                "lib/libfixture.so.1",
                "lib/libgio-2.0.so.0",
                "lib/libgiofixture.so.1",
                *(f"lib/gstreamer-1.0/libgst{name}.so" for name in PLUGINS),
            },
        )
        for record in manifest["libraries"]:
            path = moved / record["path"]
            expected = (
                ("29=$ORIGIN:$ORIGIN/..",)
                if path.parent.name == "gstreamer-1.0"
                else ()
            )
            if path.name == "libgio-2.0.so.0":
                expected = ("29=$ORIGIN",)
            self.assertEqual(inspect(path, self.target).rpaths, expected)
            self.assertEqual(digest(path), record["sourceSha256"])
            self.assertEqual(record["sha256"], record["sourceSha256"])

    def test_absolute_build_paths_require_a_new_native_build(self):
        plugin = self.prefix / "lib/gstreamer-1.0/libgstapp.so"
        subprocess.run(
            ["patchelf", "--set-rpath", str(self.prefix / "lib"), str(plugin)],
            check=True,
            capture_output=True,
        )
        for record in self.inventory:
            record["sha256"] = digest(self.prefix / record["path"])
        self.inventory_path.write_text(json.dumps(self.inventory))
        with self.assertRaisesRegex(ValueError, "rebuild native libraries"):
            project(self.prefix, self.receipts, self.target, self.output)
        self.assertFalse(self.output.exists())

    def test_missing_dependency_and_digest_mismatch_leave_no_output(self):
        for records in (
            [r for r in self.inventory if "libgio" not in r["path"]],
            [r for r in self.inventory if "libfixture" not in r["path"]],
            [r for r in self.inventory if "libgiofixture" not in r["path"]],
            [{**r, "sha256": "b" * 64} for r in self.inventory],
        ):
            with self.subTest(records=records), self.assertRaises(ValueError):
                self.inventory_path.write_text(json.dumps(records))
                project(self.prefix, self.receipts, self.target, self.output)
            self.assertFalse(self.output.exists())

    def test_path_import_is_not_treated_as_a_system_library(self):
        plugin = self.prefix / "lib/gstreamer-1.0/libgstapp.so"
        subprocess.run(
            [
                "patchelf",
                "--replace-needed",
                "libfixture.so.1",
                "outside\nlibc.so.6",
                str(plugin),
            ],
            check=True,
            capture_output=True,
        )
        with self.assertRaisesRegex(ValueError, "plain library names"):
            inspect(plugin, self.target)

    def test_malformed_header_and_segment_bounds_are_rejected(self):
        original = self.library.read_bytes()
        for offset, replacement in (
            (18, b"\x00\x00"),
            (32, struct.pack("<Q", len(original))),
            (56, b"\xff\xff"),
        ):
            data = bytearray(original)
            data[offset : offset + len(replacement)] = replacement
            self.library.write_bytes(data)
            with self.subTest(offset=offset), self.assertRaises(ValueError):
                inspect(self.library, self.target)

    def test_additional_loader_dependencies_are_rejected(self):
        data = bytearray(self.library.read_bytes())
        header = struct.unpack_from("<16sHHIQQQIHHHHHH", data)
        for index in range(header[10]):
            segment = struct.unpack_from("<IIQQQQQQ", data, header[5] + index * 56)
            if segment[0] == 2:
                struct.pack_into("<qQ", data, segment[2], 0x7FFFFFFF, 0)
                break
        self.library.write_bytes(data)
        with self.assertRaisesRegex(ValueError, "unsupported ELF loader"):
            inspect(self.library, self.target)

    def test_dynamic_table_mapping_is_checked_before_creating_output(self):
        # An existing intended runpath can make patchelf leave this malformed table untouched.
        subprocess.run(
            ["patchelf", "--set-rpath", "$ORIGIN", str(self.library)],
            check=True,
            capture_output=True,
        )
        original = self.library.read_bytes()
        header = struct.unpack_from("<16sHHIQQQIHHHHHH", original)
        dynamic_offset = next(
            header[5] + index * 56
            for index in range(header[10])
            if struct.unpack_from("<I", original, header[5] + index * 56)[0] == 2
        )
        address = struct.unpack_from("<Q", original, dynamic_offset + 16)[0]
        for invalid_address in (address + 8, 0x77770000):
            data = bytearray(original)
            struct.pack_into("<Q", data, dynamic_offset + 16, invalid_address)
            self.library.write_bytes(data)
            self.inventory_path.write_text(
                json.dumps(
                    [
                        {**record, "sha256": digest(self.prefix / record["path"])}
                        for record in self.inventory
                    ]
                )
            )
            with (
                self.subTest(address=invalid_address),
                self.assertRaisesRegex(ValueError, "dynamic table.*mapping"),
            ):
                project(self.prefix, self.receipts, self.target, self.output)
            self.assertFalse(self.output.exists())

    def test_failed_copy_removes_only_fresh_output(self):
        before = {r["path"]: digest(self.prefix / r["path"]) for r in self.inventory}
        with patch("runtime.shutil.copy2", side_effect=RuntimeError("copy failed")):
            with self.assertRaisesRegex(RuntimeError, "copy failed"):
                project(self.prefix, self.receipts, self.target, self.output)
        self.assertFalse(self.output.exists())
        self.assertEqual(
            before, {r["path"]: digest(self.prefix / r["path"]) for r in self.inventory}
        )
