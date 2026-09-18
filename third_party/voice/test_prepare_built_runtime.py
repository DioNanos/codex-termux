"""Exercise the build receipt boundary and real runtime preparation adapter."""

import json
from pathlib import Path
import sys
import subprocess
import shutil
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import macos_runtime
from prepare_built_runtime import prepare_archive, prepare_built
from runtime import digest
import test_macos_runtime
import test_sdk


class BuiltRuntimeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.prefix = self.root / "prefix"
        self.prefix.mkdir()
        self.library = self.prefix / "libfixture.dylib"
        self.library.write_bytes(b"synthetic inventory fixture")
        self.target = "aarch64-apple-darwin"
        self.status = self.root / "status"
        self.status.write_text("STABLE_GIT_COMMIT " + "a" * 40 + "\n")
        self.receipt = self.root / "built.json"
        self.build = {
            "target": self.target,
            "manifest_sha256": digest(Path(__file__).with_name("sources.json")),
            "steps": [{"name": "gst-plugins-good-install", "exit_code": 0}],
        }
        self.receipt.write_text(json.dumps(self.build))

    def test_inspection_precedes_truthful_receipts_and_projection(self):
        def project(prefix, receipts, target, output):
            inspect.assert_called_once_with(self.library.resolve(), target)
            self.assertEqual(
                json.loads((receipts / "ci.json").read_text()),
                {
                    "commit": "a" * 40,
                    "target": self.target,
                    "manifest_sha256": self.build["manifest_sha256"],
                    "build_complete": True,
                    "inspection_complete": True,
                },
            )
            self.assertEqual(
                json.loads((receipts / "inspection/binaries.json").read_text()),
                [
                    {
                        "path": self.library.name,
                        "target": self.target,
                        "sha256": digest(self.library),
                    }
                ],
            )

        with (
            patch.object(macos_runtime, "inspect") as inspect,
            patch.object(macos_runtime, "project", side_effect=project),
        ):
            prepare_built(
                self.prefix, self.receipt, self.status, self.target, self.root / "out"
            )

    def test_rejects_failed_incomplete_and_mismatched_builds_before_inspection(self):
        for change in (
            {"target": "x86_64-apple-darwin"},
            {"manifest_sha256": "0" * 64},
            {"steps": []},
            {"steps": [{"name": "glib-install", "exit_code": 0}]},
            {"steps": [{"name": "gst-plugins-good-install", "exit_code": 1}]},
        ):
            with (
                self.subTest(change=change),
                patch.object(macos_runtime, "inspect") as inspect,
            ):
                self.receipt.write_text(json.dumps({**self.build, **change}))
                with self.assertRaises(ValueError):
                    prepare_built(
                        self.prefix,
                        self.receipt,
                        self.status,
                        self.target,
                        self.root / "out",
                    )
                inspect.assert_not_called()

    def test_rejects_unknown_or_ambiguous_build_commit(self):
        for text in ("STABLE_GIT_COMMIT unknown\n", self.status.read_text() * 2):
            self.status.write_text(text)
            with self.assertRaises(ValueError):
                prepare_built(
                    self.prefix,
                    self.receipt,
                    self.status,
                    self.target,
                    self.root / "out",
                )

    def test_inspection_failure_never_reaches_projection(self):
        with (
            patch.object(
                macos_runtime, "inspect", side_effect=ValueError("bad library")
            ),
            patch.object(macos_runtime, "project") as project,
        ):
            with self.assertRaisesRegex(ValueError, "bad library"):
                prepare_built(
                    self.prefix,
                    self.receipt,
                    self.status,
                    self.target,
                    self.root / "out",
                )
            project.assert_not_called()

    @unittest.skipUnless(sys.platform == "darwin", "Uses real Mach-O fixture libraries")
    def test_projects_archive_through_sandbox_link_with_native_aliases(self):
        fixture = test_macos_runtime.RuntimeTests("runTest")
        self.addCleanup(fixture.doCleanups)
        fixture.setUp()
        sdk_fixture = test_sdk.SdkTests("runTest")
        self.addCleanup(sdk_fixture.doCleanups)
        sdk_fixture.setUp()
        for relative in ("include", "lib/glib-2.0", "lib/pkgconfig"):
            shutil.copytree(sdk_fixture.prefix / relative, fixture.prefix / relative)
        self.receipt.write_text(json.dumps({**self.build, "target": fixture.target}))
        library = next((fixture.prefix / "lib").glob("*.dylib"))
        (library.parent / "development-alias.dylib").symlink_to(library.name)
        archive = self.root / "prefix.tar"
        with tarfile.open(archive, "w") as source:
            source.add(fixture.prefix, arcname=".")
        sandbox_input = self.root / "sandbox-input.tar"
        sandbox_input.symlink_to(archive)
        for state in ("absent", "empty"):
            with self.subTest(output_state=state):
                output = self.root / f"runtime-{state}"
                if state == "empty":
                    output.mkdir()
                sdk_output = self.root / f"sdk-{state}"
                if state == "empty":
                    sdk_output.mkdir()
                subprocess.run(
                    [
                        sys.executable,
                        str(Path(__file__).with_name("prepare_built_runtime.py")),
                        "--prefix",
                        str(sandbox_input),
                        "--build-receipt",
                        str(self.receipt),
                        "--status",
                        str(self.status),
                        "--target",
                        fixture.target,
                        "--output",
                        str(output),
                        "--sdk-output",
                        str(sdk_output),
                    ],
                    check=True,
                )
                sdk_manifest = json.loads((sdk_output / "sdk.json").read_text())
                self.assertEqual(
                    {
                        record["path"]: record["sha256"]
                        for record in sdk_manifest["files"]
                    },
                    {
                        path.relative_to(sdk_output).as_posix(): digest(path)
                        for path in sdk_output.rglob("*")
                        if path.is_file() and path.name != "sdk.json"
                    },
                )
                manifest = json.loads((output / "runtime.json").read_text())
                self.assertEqual(
                    (sdk_manifest["sourceCommit"], sdk_manifest["target"]),
                    (manifest["sourceCommit"], manifest["target"]),
                )
                self.assertEqual(manifest["sourceCommit"], "a" * 40)
                self.assertEqual(manifest["target"], fixture.target)
                self.assertTrue(
                    all(
                        not macos_runtime.inspect(
                            output / record["path"], fixture.target
                        ).rpaths
                        for record in manifest["libraries"]
                    )
                )

    def test_archive_rejects_escaping_native_alias_before_inspection(self):
        archive = self.root / "prefix.tar"
        with tarfile.open(archive, "w") as source:
            link = tarfile.TarInfo("lib/escape.dylib")
            link.type = tarfile.SYMTYPE
            link.linkname = "../../outside.dylib"
            source.addfile(link)
        with patch.object(macos_runtime, "inspect") as inspect:
            with self.assertRaises(tarfile.LinkOutsideDestinationError):
                prepare_archive(
                    archive, self.receipt, self.status, self.target, self.root / "out"
                )
            inspect.assert_not_called()
