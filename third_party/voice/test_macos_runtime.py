"""Check real Mach-O relocation and fail-closed native input handling."""

import ctypes
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

from macos_runtime import PLUGINS, digest, inspect, project


@unittest.skipUnless(sys.platform == "darwin", "macOS native runtime projection")
class RuntimeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="native voice ")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.prefix = self.root / "input"
        self.receipts = self.root / "receipts"
        self.output = self.root / "runtime"
        self.target = (
            "aarch64" if platform.machine() == "arm64" else "x86_64"
        ) + "-apple-darwin"
        (self.prefix / "lib/gstreamer-1.0").mkdir(parents=True)
        (self.receipts / "inspection").mkdir(parents=True)
        source = self.root / "fixture.c"
        source.write_text("int voice_fixture(void) { return 42; }\n")
        self.library = self.prefix / "lib/libfixture.1.2.dylib"
        subprocess.run(
            [
                "/usr/bin/cc",
                "-dynamiclib",
                str(source),
                "-o",
                str(self.library),
                "-Wl,-install_name,@rpath/libfixture.1.dylib",
                "-Wl,-headerpad_max_install_names",
            ],
            check=True,
            capture_output=True,
        )
        (self.prefix / "lib/libfixture.1.dylib").symlink_to(self.library.name)
        gio = self.prefix / "lib/libgio-2.0.0.dylib"
        shutil.copy2(self.library, gio)
        subprocess.run(
            ["/usr/bin/install_name_tool", "-id", "@rpath/" + gio.name, str(gio)],
            check=True,
            capture_output=True,
        )
        source.write_text(
            "extern int voice_fixture(void); int voice_plugin(void) { return voice_fixture(); }\n"
        )
        prototype = self.prefix / f"lib/gstreamer-1.0/libgst{PLUGINS[0]}.dylib"
        subprocess.run(
            [
                "/usr/bin/cc",
                "-dynamiclib",
                str(source),
                str(self.library),
                "-o",
                str(prototype),
                f"-Wl,-install_name,{prototype}",
                f"-Wl,-rpath,{self.prefix / 'lib'}",
                "-Wl,-headerpad_max_install_names",
            ],
            check=True,
            capture_output=True,
        )
        for name in PLUGINS[1:]:
            path = self.prefix / f"lib/gstreamer-1.0/libgst{name}.dylib"
            shutil.copy2(prototype, path)
            subprocess.run(
                ["/usr/bin/install_name_tool", "-id", str(path), str(path)],
                check=True,
                capture_output=True,
            )
        self.ci = {
            "commit": "a" * 40,
            "target": self.target,
            "build_complete": True,
            "inspection_complete": True,
            "manifest_sha256": digest(Path(__file__).with_name("sources.json")),
        }
        (self.receipts / "ci.json").write_text(json.dumps(self.ci))
        self.inventory = [
            {
                "path": p.relative_to(self.prefix).as_posix(),
                "target": self.target,
                "sha256": digest(p),
            }
            for p in sorted(self.prefix.rglob("*.dylib"))
            if not p.is_symlink()
        ]
        self.inventory_path = self.receipts / "inspection/binaries.json"
        self.inventory_path.write_text(json.dumps(self.inventory))

    def test_relocated_payload_loads_after_original_prefix_is_removed(self):
        subprocess.run(
            [
                sys.executable,
                str(Path(__file__).with_name("macos_runtime.py").resolve()),
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
        manifest = json.loads((self.output / "runtime.json").read_text())
        before = {r["path"]: digest(self.prefix / r["path"]) for r in self.inventory}
        self.assertEqual(before, {r["path"]: r["sha256"] for r in self.inventory})
        self.assertEqual(
            {r["path"] for r in manifest["libraries"]},
            {
                "lib/libfixture.1.dylib",
                "lib/libgio-2.0.0.dylib",
                *(f"plugins/libgst{name}.dylib" for name in PLUGINS),
            },
        )
        moved = self.root / "relocated package"
        self.output.rename(moved)
        shutil.rmtree(self.prefix)
        for relative in manifest["plugins"]:
            plugin = ctypes.CDLL(str(moved / relative))
            self.assertEqual(plugin.voice_plugin(), 42)
        for record in manifest["libraries"]:
            metadata = inspect(moved / record["path"], self.target)
            self.assertEqual(metadata.rpaths, ())
            self.assertEqual(metadata.imports, tuple(record["imports"]))
            self.assertEqual(digest(moved / record["path"]), record["sha256"])

    def test_input_digest_target_and_receipt_must_match(self):
        for key, value in [
            ("build_complete", False),
            ("manifest_sha256", "b" * 64),
            ("target", "wrong"),
        ]:
            with self.subTest(key=key), self.assertRaises(ValueError):
                (self.receipts / "ci.json").write_text(
                    json.dumps({**self.ci, key: value})
                )
                project(self.prefix, self.receipts, self.target, self.output)
            self.assertFalse(self.output.exists())
        (self.receipts / "ci.json").write_text(json.dumps(self.ci))
        self.library.write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "digest mismatch"):
            project(self.prefix, self.receipts, self.target, self.output)
        self.assertFalse(self.output.exists())

    def test_case_alias_cannot_place_output_inside_inputs(self):
        for source in (self.prefix, self.receipts):
            alias = source.with_name(source.name.upper())
            if not alias.exists():
                self.skipTest("requires a case-insensitive filesystem")
            with (
                self.subTest(source=source),
                self.assertRaisesRegex(ValueError, "outside the inputs"),
            ):
                project(self.prefix, self.receipts, self.target, alias / "runtime")
            self.assertFalse((source / "runtime").exists())

    def test_case_colliding_library_destinations_are_rejected(self):
        duplicate = self.prefix / "lib/another.dylib"
        shutil.copy2(self.library, duplicate)
        subprocess.run(
            [
                "/usr/bin/install_name_tool",
                "-id",
                "@rpath/LIBFIXTURE.1.dylib",
                str(duplicate),
            ],
            check=True,
            capture_output=True,
        )
        self.inventory.append(
            {
                "path": duplicate.relative_to(self.prefix).as_posix(),
                "target": self.target,
                "sha256": digest(duplicate),
            }
        )
        self.inventory_path.write_text(json.dumps(self.inventory))
        with self.assertRaisesRegex(ValueError, "colliding runtime library filenames"):
            project(self.prefix, self.receipts, self.target, self.output)
        self.assertFalse(self.output.exists())

    def test_changed_source_at_copy_is_rejected(self):
        copy = shutil.copy2

        def change_then_copy(source, destination):
            source.write_bytes(source.read_bytes() + b"changed after verification")
            return copy(source, destination)

        with patch("runtime.shutil.copy2", side_effect=change_then_copy):
            with self.assertRaisesRegex(ValueError, "copied native input changed"):
                project(self.prefix, self.receipts, self.target, self.output)
        self.assertFalse(self.output.exists())

    def test_missing_dependency_and_duplicate_identity_fail_before_output_creation(
        self,
    ):
        self.inventory_path.write_text(
            json.dumps([r for r in self.inventory if "libgio" not in r["path"]])
        )
        with self.assertRaisesRegex(ValueError, "missing required native library"):
            project(self.prefix, self.receipts, self.target, self.output)
        self.assertFalse(self.output.exists())
        self.inventory_path.write_text(
            json.dumps([r for r in self.inventory if "libfixture" not in r["path"]])
        )
        with self.assertRaisesRegex(ValueError, "undeclared native dependency"):
            project(self.prefix, self.receipts, self.target, self.output)
        self.inventory_path.write_text(json.dumps([*self.inventory, self.inventory[0]]))
        with self.assertRaisesRegex(ValueError, "duplicate"):
            project(self.prefix, self.receipts, self.target, self.output)
        self.assertFalse(self.output.exists())

    def test_inventory_cannot_escape_prefix(self):
        for relative in (
            "../fixture.c",
            str(self.library),
            "lib/../lib/libfixture.1.2.dylib",
        ):
            self.inventory_path.write_text(
                json.dumps([{**self.inventory[0], "path": relative}])
            )
            with self.subTest(path=relative), self.assertRaises(ValueError):
                project(self.prefix, self.receipts, self.target, self.output)
            self.assertFalse(self.output.exists())

    def test_multiline_import_cannot_impersonate_a_system_library(self):
        external = self.root / "outside\n         name /usr/lib/libSystem.B.dylib"
        external.parent.mkdir(parents=True)
        shutil.copy2(self.library, external)
        plugin = self.prefix / f"lib/gstreamer-1.0/libgst{PLUGINS[0]}.dylib"
        subprocess.run(
            [
                "/usr/bin/install_name_tool",
                "-change",
                "@rpath/libfixture.1.dylib",
                str(external),
                str(plugin),
            ],
            check=True,
            capture_output=True,
        )
        for record in self.inventory:
            record["sha256"] = digest(self.prefix / record["path"])
        self.inventory_path.write_text(json.dumps(self.inventory))
        with self.assertRaisesRegex(ValueError, "undeclared native dependency"):
            project(self.prefix, self.receipts, self.target, self.output)
        self.assertFalse(self.output.exists())

    def test_malformed_load_command_bounds_and_strings_are_rejected(self):
        command = struct.pack("<6I", 0xD, 32, 24, 0, 0, 0) + b"test\0\0\0\0"
        for count, size, payload in (
            (1, 1024 * 1024 + 1, command),
            (2, 32, command),
            (1, 32, command[:-1]),
            (1, 32, struct.pack("<3I", 0xD, 40, 24) + command[12:]),
            (1, 32, struct.pack("<3I", 0xD, 32, 8) + command[12:]),
            (1, 32, command[:24] + b"no null!"),
        ):
            with self.subTest(count=count, size=size, payload=payload):
                cpu = 0x100000C if self.target.startswith("aarch64") else 0x1000007
                self.library.write_bytes(
                    struct.pack("<8I", 0xFEEDFACF, cpu, 0, 6, count, size, 0, 0)
                    + payload
                )
                with self.assertRaises(ValueError):
                    inspect(self.library, self.target)

    def test_tool_cannot_hide_other_architectures_or_loader_commands(self):
        cpu = 0x100000C if self.target.startswith("aarch64") else 0x1000007
        name = b"@rpath/libtest.dylib\0"
        command = (
            struct.pack("<6I", 0xD, 48, 24, 0, 0, 0) + name + bytes(24 - len(name))
        )
        thin = struct.pack("<8I", 0xFEEDFACF, cpu, 0, 6, 1, 48, 0, 0) + command
        for tag in (0x2B, 0x3A):
            self.library.write_bytes(
                struct.pack("<8I", 0xFEEDFACF, cpu, 0, 6, 2, 64, 0, 0)
                + command
                + struct.pack("<4I", tag, 16, 0, 0)
            )
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                inspect(self.library, self.target)
        fat = struct.pack(">7I", 0xCAFEBABE, 1, cpu, 0, 4096, len(thin), 12)
        self.library.write_bytes(fat + bytes(4096 - len(fat)) + thin)
        with self.assertRaises(ValueError):
            inspect(self.library, self.target)
        fake_cpu = "X86_64" if cpu == 0x100000C else "ARM64"
        fake_target = (
            "x86_64-apple-darwin" if cpu == 0x100000C else "aarch64-apple-darwin"
        )
        path = (
            self.root
            / f"fake\nMH_MAGIC_64 {fake_cpu} ALL 0x00 DYLIB 1 48 0x00000000\n\n.dylib"
        )
        path.write_bytes(thin)
        with self.assertRaises(ValueError):
            inspect(path, fake_target)

    def test_existing_outputs_and_failed_transform_preserve_inputs(self):
        for output in (self.prefix, self.prefix / "nested", self.receipts / "nested"):
            with self.subTest(output=output), self.assertRaises(ValueError):
                project(self.prefix, self.receipts, self.target, output)
        import macos_runtime

        original_run = macos_runtime.run

        def fail_transform(command):
            if command[0].endswith("install_name_tool"):
                raise subprocess.CalledProcessError(1, command)
            return original_run(command)

        with patch("macos_runtime.run", side_effect=fail_transform):
            with self.assertRaises(subprocess.CalledProcessError):
                project(self.prefix, self.receipts, self.target, self.output)
        self.assertFalse(self.output.exists())
        self.assertEqual(
            {r["path"]: digest(self.prefix / r["path"]) for r in self.inventory},
            {r["path"]: r["sha256"] for r in self.inventory},
        )


if __name__ == "__main__":
    unittest.main()
