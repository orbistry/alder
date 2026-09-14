import copy
import hashlib
import importlib.util
import io
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile
import tarfile
import unittest
import zipfile

SCRIPT = Path(__file__).with_name("package-alder-installers.py")
spec = importlib.util.spec_from_file_location("package_alder_installers", SCRIPT)
adapter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(adapter)


class InstallerTests(unittest.TestCase):
    def shell(self):
        return '#!/bin/sh\nAPP_NAME="alder-cli"\n' + adapter.SHELL_ANCHOR

    def powershell(self):
        return "$app_name = 'alder-cli'\n" + adapter.POWERSHELL_ANCHOR

    def test_patches_are_unique_and_fail_closed_on_template_changes(self):
        for source, patch in [(self.shell(), adapter.patch_shell), (self.powershell(), adapter.patch_powershell)]:
            changed = patch(source)
            self.assertEqual(changed.count(adapter.MARKER), 1)
            with self.assertRaises(ValueError):
                patch(changed)
            with self.assertRaises(ValueError):
                patch(source + source)
            with self.assertRaises(ValueError):
                patch("an incompatible upstream template")

    def test_metadata_and_all_published_hashes_match_final_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            artifacts = {}
            originals = {
                "alder-cli-installer.sh": self.shell().encode(),
                "alder-cli-installer.ps1": self.powershell().encode(),
                "native.tar.xz": b"unchanged native archive",
            }
            for name, data in originals.items():
                path = root / name
                path.write_bytes(data)
                artifacts[name] = {"name": name, "path": str(path), "kind": "installer" if name.startswith("alder-cli-installer") else "executable-zip", "checksums": {"sha256": hashlib.sha256(data).hexdigest()}}
            aggregate = root / "sha256.sum"
            aggregate.write_text(f'{hashlib.sha256(originals["native.tar.xz"]).hexdigest()} *native.tar.xz\n\n')
            individual = root / "alder-cli-installer.sh.sha256"
            individual.write_text(f'{hashlib.sha256(originals["alder-cli-installer.sh"]).hexdigest()} *alder-cli-installer.sh\n')
            artifacts[aggregate.name] = {"path": str(aggregate), "kind": "unified-checksum"}
            artifacts[individual.name] = {"path": str(individual), "kind": "checksum"}
            artifacts["alder-cli-installer.sh"]["checksum"] = individual.name
            manifest = {"dist_version": adapter.DIST_VERSION, "artifacts": artifacts}
            changed = adapter.prepare(manifest)
            self.assertEqual((root / "alder-cli-installer.sh").read_bytes(), originals["alder-cli-installer.sh"], "prepare must validate without writing")
            for path, data in changed.items():
                self.assertEqual(artifacts[path.name]["checksums"]["sha256"], hashlib.sha256(data).hexdigest())
            for path in [aggregate, individual]:
                for line in changed[path].decode().splitlines():
                    checksum, filename = line.split(" *")
                    data = changed.get(root / filename, originals.get(filename))
                    self.assertEqual(checksum, hashlib.sha256(data).hexdigest())
            invalid = copy.deepcopy(manifest)
            invalid["dist_version"] = "0.33.0"
            with self.assertRaises(ValueError):
                adapter.prepare(invalid)

    @unittest.skipUnless(os.name != "nt" and shutil.which("sh"), "POSIX shell required")
    def test_shell_installs_versioned_support_without_overwriting_other_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            install = root / "bin"
            staging = install / "staging"
            (source / "support/node_modules/miniflare").mkdir(parents=True)
            (source / "support/node_modules/miniflare/package.json").write_text("{}")
            (source / "support/release-support.json").write_text("{}")
            (install / "support").mkdir(parents=True)
            (install / "support/unrelated").write_text("keep")
            staging.mkdir()
            script = '''ensure() { "$@" || exit 1; }
err() { echo "$*" >&2; exit 1; }
say() { :; }
run() {
    local _src_dir="$1" _install_dir="$2" _install_temp="$3" _arch=native APP_VERSION=1.2.3
''' + adapter.SHELL_SUPPORT + '\n}\nrun "$1" "$2" "$3"\n'
            subprocess.run(["sh", "-c", script, "support-test", str(source), str(install), str(staging)], check=True)
            self.assertTrue((install / ".alder-support/1.2.3/native/node_modules/miniflare/package.json").is_file())
            self.assertEqual((install / "support/unrelated").read_text(), "keep")

    @unittest.skipUnless(shutil.which("pwsh"), "PowerShell required")
    def test_powershell_installs_versioned_support(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            install = root / "bin"
            (source / "support/node_modules/miniflare").mkdir(parents=True)
            (source / "support/node_modules/miniflare/package.json").write_text("{}")
            (source / "support/release-support.json").write_text('{"target":"x86_64-pc-windows-msvc"}')
            install.mkdir()
            script = '''$ErrorActionPreference = "Stop"
$artifacts = @{bin_paths = @((Join-Path $env:ALDER_TEST_SOURCE "alder.exe"))}
$dest_dir = $env:ALDER_TEST_INSTALL
$arch = "aarch64-pc-windows-msvc"
$app_version = "1.2.3"
''' + adapter.POWERSHELL_SUPPORT
            env = os.environ | {"ALDER_TEST_SOURCE": str(source), "ALDER_TEST_INSTALL": str(install)}
            subprocess.run(["pwsh", "-NoProfile", "-NonInteractive", "-Command", script], env=env, check=True)
            self.assertTrue((install / ".alder-support/1.2.3/x86_64-pc-windows-msvc/node_modules/miniflare/package.json").is_file())

    def test_release_workflow_keeps_checked_postprocessing_and_pinned_dist(self):
        repository = SCRIPT.resolve().parents[2]
        config = (repository / "dist-workspace.toml").read_text()
        self.assertEqual(re.search(r'^cargo-dist-version = "([^"]+)"', config, re.MULTILINE)[1], adapter.DIST_VERSION)
        workflow = (repository / ".github/workflows/alder-cli-release.yml").read_text()
        self.assertEqual(workflow.count(adapter.WORKFLOW_ANCHOR + adapter.WORKFLOW_STEP), 1)
        self.assertEqual(workflow.count(adapter.LOCAL_ANCHOR + adapter.LOCAL_STEP), 1)
        self.assertIn("npm ci --prefix crates/alder-cli/support --omit=dev --bin-links=false", workflow)

    def test_native_archives_must_ship_the_correct_platform_tree(self):
        with tempfile.TemporaryDirectory() as temporary:
            for target, platform, executable in [("x86_64-unknown-linux-gnu", "linux-64", "workerd"), ("x86_64-pc-windows-msvc", "windows-64", "workerd.exe")]:
                files = {
                    "alder.exe" if "windows" in target else "alder": b"compiler",
                    "support/package.json": b"{}",
                    "support/cloudflare-dev.mjs": b"bridge",
                    "support/release-support.json": json.dumps({"target": target, "node": ">=22"}).encode(),
                    "support/node_modules/miniflare/package.json": b"{}",
                    "support/node_modules/wrangler/bin/wrangler.js": b"wrangler",
                    "support/node_modules/workerd/bin/workerd": b"launcher",
                    f"support/node_modules/@cloudflare/workerd-{platform}/bin/{executable}": b"native runtime",
                }
                zip_path = Path(temporary) / "compiler.zip"
                with zipfile.ZipFile(zip_path, "w") as archive:
                    for name, data in files.items():
                        archive.writestr(name, data)
                adapter.verify_archive(zip_path, target)
                tar_path = Path(temporary) / "compiler.tar.xz"
                with tarfile.open(tar_path, "w:xz") as archive:
                    for name, data in files.items():
                        info = tarfile.TarInfo("release/" + name)
                        info.size = len(data)
                        archive.addfile(info, io.BytesIO(data))
                adapter.verify_archive(tar_path, target)
                with self.assertRaises(ValueError):
                    adapter.verify_archive(zip_path, "aarch64-apple-darwin")

    def test_incomplete_archives_fail_before_upload(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "compiler.zip"
            with zipfile.ZipFile(path, "w") as archive:
                archive.writestr("alder", b"compiler")
            with self.assertRaises(ValueError):
                adapter.verify_archive(path, "x86_64-unknown-linux-gnu")

    @unittest.skipUnless(os.environ.get("ALDER_GENERATED_INSTALLERS"), "optional live cargo-dist output not supplied")
    def test_pinned_real_generated_installer_templates(self):
        root = Path(os.environ["ALDER_GENERATED_INSTALLERS"])
        shell = adapter.patch_shell((root / "alder-cli-installer.sh").read_text())
        powershell = adapter.patch_powershell((root / "alder-cli-installer.ps1").read_text())
        self.assertEqual(shell.count(adapter.MARKER), 1)
        self.assertEqual(powershell.count(adapter.MARKER), 1)
        artifacts = {name: {"path": str(root / name), "kind": "installer"} for name in ["alder-cli-installer.sh", "alder-cli-installer.ps1"]}
        for path in [root / "sha256.sum", *root.glob("*.sha256")]:
            artifacts[path.name] = {"path": str(path), "kind": "unified-checksum" if path.name == "sha256.sum" else "checksum"}
        self.assertIn(root / "sha256.sum", adapter.prepare({"dist_version": adapter.DIST_VERSION, "artifacts": artifacts}))
        if os.name != "nt" and shutil.which("sh"):
            subprocess.run(["sh", "-n"], input=shell.encode(), check=True)


if __name__ == "__main__":
    unittest.main()
