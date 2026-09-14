#!/usr/bin/env python3
"""Complete cargo-dist 0.32.0 installers with versioned compiler support.

This is deliberately pinned and fails closed if an upstream template changes.
It runs after global artifact generation, before checksums/manifests are uploaded.
No compiler release is downloaded or published by this script.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import tarfile
import zipfile

DIST_VERSION = "0.32.0"
MARKER = "ALDER_VENDORED_SUPPORT_V1"
SHELL_ANCHOR = '    local _arch="$5"\n    for _bin_name in $_bins; do\n'
POWERSHELL_ANCHOR = '  # Just copy the binaries from the temp location to the install dir\n'
WORKFLOW_ANCHOR = '          dist build ${{ needs.plan.outputs.tag-flag }} --output-format=json "--artifacts=global" > dist-manifest.json\n'
WORKFLOW_STEP = '          python3 .github/scripts/package-alder-installers.py dist-manifest.json\n'
LOCAL_ANCHOR = '          dist build ${{ needs.plan.outputs.tag-flag }} --print=linkage --output-format=json ${{ matrix.dist_args }} > dist-manifest.json\n'
LOCAL_STEP = '          python .github/scripts/package-alder-installers.py --verify-local dist-manifest.json\n'

SHELL_SUPPORT = '''    # ALDER_VENDORED_SUPPORT_V1: publish support before replacing the binary.
    local _support_parent="$_install_dir/.alder-support/$APP_VERSION"
    local _support_dest="$_support_parent/$_arch"
    [ -f "$_src_dir/support/release-support.json" ] || err "Archive is missing vendored Alder support"
    [ -f "$_src_dir/support/node_modules/miniflare/package.json" ] || err "Archive is missing Miniflare"
    ensure mkdir -p "$_support_parent"
    if [ -e "$_support_dest" ]; then
        [ -f "$_support_dest/release-support.json" ] || err "Incomplete Alder support at $_support_dest; move it aside and retry"
    else
        ensure mv "$_src_dir/support" "$_install_temp/support"
        ensure mv "$_install_temp/support" "$_support_dest"
    fi
    say "  compiler support (Node.js >=22 required for Workers commands)"
'''

POWERSHELL_SUPPORT = '''  # ALDER_VENDORED_SUPPORT_V1: publish support before replacing the binary.
  $support_source = Join-Path (Split-Path -Parent $artifacts["bin_paths"][0]) "support"
  $support_parent = Join-Path $dest_dir ".alder-support\\$app_version"
  if (-not (Test-Path -LiteralPath (Join-Path $support_source "release-support.json") -PathType Leaf)) {
    throw "Archive is missing vendored Alder support"
  }
  if (-not (Test-Path -LiteralPath (Join-Path $support_source "node_modules/miniflare/package.json") -PathType Leaf)) {
    throw "Archive is missing Miniflare"
  }
  # Get-TargetTriple may be an ARM64 emulation alias for the x64 archive.
  # Use the archive's target, which is what the compiled binary reports.
  $support_metadata = Get-Content -LiteralPath (Join-Path $support_source "release-support.json") -Raw | ConvertFrom-Json
  if ($support_metadata.target -ne "x86_64-pc-windows-msvc") {
    throw "Unexpected Windows compiler support target"
  }
  $support_dest = Join-Path $support_parent $support_metadata.target
  $null = New-Item -ItemType Directory -Force -Path $support_parent
  if (Test-Path -LiteralPath $support_dest) {
    if (-not (Test-Path -LiteralPath (Join-Path $support_dest "release-support.json") -PathType Leaf)) {
      throw "Incomplete Alder support at $support_dest; move it aside and retry"
    }
  } else {
    $support_staging = Join-Path $support_parent (".install-" + [System.Guid]::NewGuid().ToString())
    try {
      Copy-Item -LiteralPath $support_source -Destination $support_staging -Recurse -ErrorAction Stop
      Move-Item -LiteralPath $support_staging -Destination $support_dest -ErrorAction Stop
    } finally {
      if (Test-Path -LiteralPath $support_staging) {
        Remove-Item -LiteralPath $support_staging -Recurse -Force -ErrorAction Stop
      }
    }
  }
  Write-Information "  compiler support (Node.js >=22 required for Workers commands)"
'''


def replace_once(text: str, anchor: str, replacement: str) -> str:
    if MARKER in text or text.count(anchor) != 1:
        raise ValueError("cargo-dist installer template changed or was already patched; review the pinned adapter")
    return text.replace(anchor, replacement, 1)


def patch_shell(text: str) -> str:
    if 'APP_NAME="alder-cli"' not in text:
        raise ValueError("Not an Alder shell installer")
    return replace_once(text, SHELL_ANCHOR, '    local _arch="$5"\n' + SHELL_SUPPORT + '    for _bin_name in $_bins; do\n')


def patch_powershell(text: str) -> str:
    if "$app_name = 'alder-cli'" not in text:
        raise ValueError("Not an Alder PowerShell installer")
    return replace_once(text, POWERSHELL_ANCHOR, POWERSHELL_SUPPORT + POWERSHELL_ANCHOR)


def digest(data: bytes, algorithm: str) -> str:
    if algorithm not in {"sha256", "sha512"}:
        raise ValueError(f"Unsupported release checksum algorithm {algorithm}; update the adapter explicitly")
    return hashlib.new(algorithm, data).hexdigest()


def prepare(manifest: dict) -> dict[Path, bytes]:
    if manifest.get("dist_version") != DIST_VERSION:
        raise ValueError(f"Installer adapter requires cargo-dist {DIST_VERSION}")
    artifacts = manifest["artifacts"]
    changed: dict[Path, bytes] = {}
    for name, patch in [("alder-cli-installer.sh", patch_shell), ("alder-cli-installer.ps1", patch_powershell)]:
        artifact = artifacts[name]
        path = Path(artifact["path"])
        if artifact.get("kind") != "installer" or path.name != name:
            raise ValueError(f"Unexpected installer artifact {name}")
        changed[path] = patch(path.read_text()).encode()

    # Preserve checksum entries for native archives. Update any existing
    # script entries, and add both installers to the aggregate SHA-256 list.
    by_name = {path.name: data for path, data in changed.items()}
    checksum_paths = []
    for name, artifact in artifacts.items():
        if artifact.get("kind") not in {"checksum", "unified-checksum"} or "path" not in artifact:
            continue
        path = Path(artifact["path"])
        if not path.is_file() and name not in {"sha256.sum", "sha512.sum"}:
            if any(item.get("checksum") == name for item_name, item in artifacts.items() if item_name in by_name):
                raise ValueError(f"Missing checksum file for a modified installer: {path}")
            # Native artifact paths may belong to a different matrix runner;
            # no bytes from those archives were changed by this global step.
            continue
        if name == "sha256.sum" or name.endswith(".sha256"):
            algorithm = "sha256"
        elif name == "sha512.sum" or name.endswith(".sha512"):
            algorithm = "sha512"
        else:
            raise ValueError(f"Unknown checksum artifact {name}")
        lines = []
        names = set()
        for line in path.read_text().splitlines():
            if not line.strip():
                continue
            match = re.fullmatch(r"([0-9a-f]+) [ *](.+)", line)
            if not match or len(match[1]) != len(digest(b"", algorithm)):
                raise ValueError(f"Unexpected checksum format in {path}")
            filename = match[2]
            if filename in names:
                raise ValueError(f"Duplicate checksum entry {filename}")
            names.add(filename)
            value = digest(by_name[filename], algorithm) if filename in by_name else match[1]
            lines.append(f"{value} *{filename}")
        if name == "sha256.sum":
            for filename in sorted(by_name.keys() - names):
                lines.append(f"{digest(by_name[filename], algorithm)} *{filename}")
        updated = ("\n".join(lines) + "\n").encode()
        if updated != path.read_bytes():
            changed[path] = updated
        checksum_paths.append(name)
    if "sha256.sum" not in checksum_paths:
        raise ValueError("Expected cargo-dist's aggregate sha256.sum artifact")

    # All validation precedes writes. Each modified artifact's embedded hashes
    # must describe the final bytes, including the modified aggregate list.
    for artifact in artifacts.values():
        path = Path(artifact["path"]) if "path" in artifact else None
        if path not in changed:
            continue
        algorithms = set(artifact.get("checksums", {})) | {"sha256"}
        artifact["checksums"] = {algorithm: digest(changed[path], algorithm) for algorithm in sorted(algorithms)}
    return changed


def verify_archive(path: Path, target: str) -> None:
    binary = "alder.exe" if "windows" in target else "alder"
    platform = {
        "aarch64-apple-darwin": "darwin-arm64",
        "x86_64-apple-darwin": "darwin-64",
        "aarch64-unknown-linux-gnu": "linux-arm64",
        "x86_64-unknown-linux-gnu": "linux-64",
        "x86_64-pc-windows-msvc": "windows-64",
    }[target]
    workerd = "workerd.exe" if "windows" in target else "workerd"
    required = {
        binary,
        "support/package.json",
        "support/cloudflare-dev.mjs",
        "support/release-support.json",
        "support/node_modules/miniflare/package.json",
        "support/node_modules/wrangler/bin/wrangler.js",
        "support/node_modules/workerd/bin/workerd",
        f"support/node_modules/@cloudflare/workerd-{platform}/bin/{workerd}",
    }
    found = set()
    metadata = None
    root = None

    def visit(name, regular, directory, read):
        nonlocal root, metadata
        parts = PurePosixPath(name).parts
        if ".." in parts or "\\" in name or ":" in name or name.startswith("/"):
            raise ValueError(f"Unsafe archive path {name}")
        if not parts:
            return
        if not (regular or directory):
            raise ValueError(f"Archive must contain regular support files, not links: {name}")
        index = 0 if parts[0] in {binary, "support"} else 1
        if len(parts) <= index or parts[index] not in {binary, "support"}:
            return
        prefix = parts[:index]
        if root is not None and root != prefix:
            raise ValueError("Compiler and support must share one archive root")
        root = prefix
        if directory:
            return
        relative = "/".join(parts[index:])
        if relative in found:
            raise ValueError(f"Duplicate support archive entry {relative}")
        found.add(relative)
        if relative == "support/release-support.json":
            metadata = json.loads(read())

    if path.suffix == ".zip":
        with zipfile.ZipFile(path) as archive:
            for info in archive.infolist():
                mode = info.external_attr >> 16
                regular = mode & 0o170000 in {0, 0o100000}
                visit(info.filename, regular, info.is_dir(), lambda info=info: archive.read(info))
    else:
        with tarfile.open(path, "r|xz") as archive:
            for info in archive:
                visit(info.name, info.isfile(), info.isdir(), lambda info=info: archive.extractfile(info).read())
    if required - found:
        raise ValueError(f"Release archive {path.name} lacks vendored files: {sorted(required - found)}")
    if not metadata or metadata.get("target") != target or metadata.get("node") != ">=22":
        raise ValueError(f"Release archive {path.name} contains support for a different platform/Node contract")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path, nargs="?")
    parser.add_argument("--workflow", type=Path, help="Reapply the checked global step after dist generate --mode ci")
    parser.add_argument("--verify-local", type=Path, help="Verify each native compiler archive before upload")
    args = parser.parse_args()
    if args.workflow:
        text = args.workflow.read_text()
        for anchor, step in [(WORKFLOW_ANCHOR, WORKFLOW_STEP), (LOCAL_ANCHOR, LOCAL_STEP)]:
            if text.count(anchor) != 1 or text.count(step) > 1:
                raise ValueError("Generated release workflow changed; review the build adapter")
            if step not in text:
                text = text.replace(anchor, anchor + step)
        args.workflow.write_text(text)
        return
    if args.verify_local:
        manifest = json.loads(args.verify_local.read_text())
        count = 0
        for artifact in manifest["artifacts"].values():
            if artifact.get("kind") == "executable-zip" and artifact.get("name", "").startswith("alder-cli-"):
                targets = artifact["target_triples"]
                if len(targets) != 1:
                    raise ValueError("Native support requires one target per archive")
                verify_archive(Path(artifact["path"]), targets[0])
                count += 1
        if not count:
            raise ValueError("No native Alder archive was verified")
        return
    if not args.manifest:
        parser.error("a dist manifest or --workflow is required")
    manifest = json.loads(args.manifest.read_text())
    changed = prepare(manifest)
    for path, data in changed.items():
        path.write_bytes(data)
    args.manifest.write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
