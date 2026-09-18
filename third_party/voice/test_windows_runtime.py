"""Exercise native MSVC DLL preparation and restricted-search loading."""

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

from runtime import PLUGINS, digest
from windows_runtime import inspect, project


@unittest.skipUnless(sys.platform == "win32", "Windows native runtime preparation")
class RuntimeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="native voice ")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.prefix, self.receipts, self.output = (
            self.root / name for name in ("input", "receipts", "runtime")
        )
        self.target = (
            "aarch64"
            if platform.machine().lower() in ("arm64", "aarch64")
            else "x86_64"
        ) + "-pc-windows-msvc"
        (self.prefix / "lib/gstreamer-1.0").mkdir(parents=True)
        (self.prefix / "bin").mkdir()
        (self.receipts / "inspection").mkdir(parents=True)
        source = self.root / "fixture.c"
        source.write_text(
            "__declspec(dllexport) int voice_fixture(void) { return 42; }\n"
        )
        self.library = self.prefix / "bin/fixture.dll"
        import_library = self.root / "fixture.lib"
        subprocess.run(
            [
                "cl",
                "/nologo",
                "/LD",
                "/MD",
                str(source),
                "/link",
                f"/OUT:{self.library}",
                f"/IMPLIB:{import_library}",
            ],
            check=True,
            capture_output=True,
            cwd=self.root,
        )
        shutil.copy2(self.library, self.prefix / "bin/gio-2.0-0.dll")
        source = self.root / "plugin.c"
        source.write_text(
            "__declspec(dllimport) int voice_fixture(void); "
            "__declspec(dllexport) int voice_plugin(void) { return voice_fixture(); }\n"
        )
        for name in PLUGINS:
            plugin = self.prefix / f"lib/gstreamer-1.0/gst{name}.dll"
            subprocess.run(
                [
                    "cl",
                    "/nologo",
                    "/LD",
                    "/MD",
                    str(source),
                    str(import_library),
                    "/link",
                    f"/OUT:{plugin}",
                    f"/IMPLIB:{self.root / (name + '.lib')}",
                ],
                check=True,
                capture_output=True,
                cwd=self.root,
            )
        self.inventory_path = self.receipts / "inspection/binaries.json"
        self.inventory = [
            {
                "path": str(p.relative_to(self.prefix)),
                "sha256": digest(p),
                "target": self.target,
            }
            for p in sorted(self.prefix.rglob("*.dll"))
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

    def test_receipts_match_across_checkout_line_endings(self):
        source = Path(__file__).with_name("sources.json")
        checkout = self.root / "checkout"
        manifest = checkout / "third_party/voice/sources.json"
        manifest.parent.mkdir(parents=True)
        manifest.write_bytes(source.read_bytes().replace(b"\r\n", b"\n"))
        shutil.copy2(source.parents[2] / ".gitattributes", checkout)
        git = ["git", "-C", str(checkout), "-c", "core.autocrlf=false"]
        subprocess.run([*git, "init", "-q"], check=True, capture_output=True)
        subprocess.run([*git, "add", "."], check=True, capture_output=True)
        hashes = []
        for autocrlf in ("true", "false"):
            manifest.unlink()
            subprocess.run(
                [*git, "-c", f"core.autocrlf={autocrlf}", "checkout-index", "-a", "-f"],
                check=True,
                capture_output=True,
            )
            hashes.append(digest(manifest))
        self.assertEqual(hashes, [digest(source), digest(source)])
        receipt_path = self.receipts / "ci.json"
        receipt = json.loads(receipt_path.read_text())
        receipt["manifest_sha256"] = hashes[0]
        receipt_path.write_text(json.dumps(receipt))
        project(self.prefix, self.receipts, self.target, self.output)

    def test_moved_dlls_load_with_private_directory_and_system_search_only(self):
        subprocess.run(
            [
                sys.executable,
                str(Path(__file__).with_name("windows_runtime.py").resolve()),
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
        result = subprocess.run(
            [
                sys.executable,
                "-c",
                "import ctypes,json,pathlib,sys; root=pathlib.Path(sys.argv[1]); "
                "manifest=json.loads((root/'runtime.json').read_text()); "
                "print([ctypes.CDLL(str(root/p),winmode=0x100|0x800).voice_plugin() for p in manifest['plugins']])",
                str(moved),
            ],
            check=True,
            capture_output=True,
            text=True,
            cwd=self.root,
        )
        self.assertEqual(json.loads(result.stdout), [42] * len(PLUGINS))
        manifest = json.loads((moved / "runtime.json").read_text())
        self.assertEqual(
            {record["path"] for record in manifest["libraries"]},
            {
                "bin/fixture.dll",
                "bin/gio-2.0-0.dll",
                *(f"bin/gst{name}.dll" for name in PLUGINS),
            },
        )
        for record in manifest["libraries"]:
            self.assertEqual(digest(moved / record["path"]), record["sourceSha256"])

    def test_missing_dependency_and_digest_mismatch_leave_no_output(self):
        for records in (
            [r for r in self.inventory if "gio-2.0" not in r["path"]],
            [r for r in self.inventory if "fixture.dll" not in r["path"]],
            [{**r, "sha256": "b" * 64} for r in self.inventory],
        ):
            with self.subTest(records=records), self.assertRaises(ValueError):
                self.inventory_path.write_text(json.dumps(records))
                project(self.prefix, self.receipts, self.target, self.output)
            self.assertFalse(self.output.exists())

    def test_case_alias_dll_identities_are_rejected(self):
        duplicate = self.prefix / "lib/FIXTURE.dll"
        shutil.copy2(self.library, duplicate)
        records = self.inventory + [
            {
                "path": str(duplicate.relative_to(self.prefix)),
                "sha256": digest(duplicate),
                "target": self.target,
            }
        ]
        self.inventory_path.write_text(json.dumps(records))
        with self.assertRaisesRegex(ValueError, "duplicate"):
            project(self.prefix, self.receipts, self.target, self.output)
        self.assertFalse(self.output.exists())

    def test_malformed_headers_and_delayed_imports_are_rejected(self):
        original = self.library.read_bytes()
        pe = struct.unpack_from("<I", original, 60)[0]
        for offset, replacement in (
            (60, struct.pack("<I", len(original))),
            (pe + 4, b"\0\0"),
            (pe + 6, b"\xff\xff"),
            (pe + 24 + 112 + 13 * 8, struct.pack("<II", 1, 32)),
        ):
            data = bytearray(original)
            data[offset : offset + len(replacement)] = replacement
            self.library.write_bytes(data)
            with self.subTest(offset=offset), self.assertRaises(ValueError):
                inspect(self.library, self.target)

    def test_path_bearing_import_is_rejected(self):
        plugin = self.prefix / "lib/gstreamer-1.0/gstapp.dll"
        original = plugin.read_bytes()
        self.assertIn(b"fixture.dll\0", original)
        plugin.write_bytes(original.replace(b"fixture.dll\0", b"/ixture.dll\0"))
        with self.assertRaisesRegex(ValueError, "plain DLL names"):
            inspect(plugin, self.target)

    def test_forwarded_exports_are_rejected(self):
        definition = self.root / "forward.def"
        definition.write_text("EXPORTS\nforwarded=KERNEL32.Sleep\n")
        destination = self.root / "forward.dll"
        machine = "ARM64" if self.target.startswith("aarch64") else "X64"
        subprocess.run(
            [
                "link",
                "/NOLOGO",
                "/DLL",
                "/NOENTRY",
                f"/MACHINE:{machine}",
                f"/DEF:{definition}",
                f"/OUT:{destination}",
            ],
            check=True,
            capture_output=True,
            cwd=self.root,
        )
        with self.assertRaisesRegex(ValueError, "forwarded DLL exports"):
            inspect(destination, self.target)
