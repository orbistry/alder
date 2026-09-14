# Compiler support in binary releases

The compiler's Workers tooling is a versioned part of the compiler distribution,
not an npm dependency of each Alder project. Official platform archives contain
`alder` (or `alder.exe`) and a sibling `support/` directory with the bridge,
locked production npm dependencies, Miniflare, Wrangler, and native workerd.
`node_modules` is generated during release builds and is never checked in.

Node.js 22 or newer must be available on `PATH` for Workers development,
prerendering, and deployment. Node is not bundled. Ordinary compiler-only
operations do not need Node. Source checkouts can populate their compiler-owned
support directory with `npm ci --prefix crates/alder-cli/support`; users of
binary releases should not need this command.

## Build and validation

`dist-workspace.toml` includes `crates/alder-cli/support` in platform archives.
The checked `github-build-setup` steps install Node 22, verify the release target
matches the runner's actual OS/architecture, run the lockfile-driven
`npm ci --omit=dev --bin-links=false`, and validate the resulting tree. Optional
native packages must be installed on the matching target, so merged or
cross-architecture support builds fail. `workerd --version` and loading Miniflare
are checked before packaging. `release-support.json` records the target and
pinned dependency versions.

After each native archive is built, a second check inspects the actual tar.xz or
zip output. It requires the bridge, Miniflare, Wrangler, launcher, native workerd,
and matching support metadata. Missing files, mixed roots, links, duplicate
files, or wrong-platform support stop the release before upload.

The npm package is a platform-neutral launcher; its installer keeps the complete
downloaded native archive. Homebrew installs extra files in
`share/alder/support`, which the CLI recognizes. The shell and PowerShell
installers place support at
`<binary-directory>/.alder-support/<compiler-version>/<target>/` before replacing
the binary. They do not delete unrelated sibling `support` directories or older
versioned support trees.

Cargo-dist 0.32.0 does not install arbitrary archive directories through its
shell/PowerShell templates. `.github/scripts/package-alder-installers.py`
therefore applies a deliberately version-pinned, fail-closed adapter to those
two generated installers. It asserts unique upstream template anchors,
recomputes modified installer hashes, updates checksum files and the aggregate
`sha256.sum`, and updates the dist manifest before upload. The npm/Homebrew
templates are not patched. CI runs adapter tests on Linux and Windows, including
actual shell/PowerShell support-directory installation and hash consistency.

When regenerating release CI, run both commands:

```sh
dist generate --mode ci
python3 .github/scripts/package-alder-installers.py --workflow .github/workflows/alder-cli-release.yml
```

The dist `allow-dirty = ["ci"]` setting permits these two checked post-build
steps. Tests require that both remain in the workflow and that the dist version
still matches the adapter. Upgrading cargo-dist requires reviewing its
installer templates and updating the adapter; it must not silently skip
support installation.

## Version proxy

The version proxy downloads the existing platform archive names. It extracts
only the compiler and its sibling support tree into a fresh staging directory,
checks support completeness and platform identity, then atomically publishes
the entire version directory. Permissions on native executables are retained.
Traversal, absolute paths, archive links, duplicate files, mixed roots, and
excessive extraction sizes are rejected. Failed extraction cannot leave a
cached binary that appears ready while its support is incomplete.

Older binary-only compiler releases remain installable. An existing nonempty
incomplete cache directory is preserved and reported for manual inspection,
not overwritten. `ALDER_SUPPORT_DIR` remains an explicit override for custom
compiler distributions.

Primary packaging references: [cargo-dist configuration](https://axodotdev.github.io/cargo-dist/book/reference/config.html),
[the pinned installer templates](https://github.com/axodotdev/cargo-dist/tree/v0.32.0/cargo-dist/templates/installer),
and [GitHub native runner labels](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
