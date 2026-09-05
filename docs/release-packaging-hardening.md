# Release packaging verification

Checkpoint: compiler-hardening after `32fe7de`, Rust/Cargo 1.92.0.

All 17 publishable workspace crates were packaged and verified with:

```sh
cargo package --workspace --exclude stub --offline --target-dir /tmp/alder-package-hardening.KwkjGq
```

That target directory was freshly allocated with `mktemp -d`. Cargo built the
extracted archives and resolved workspace dependencies through its temporary
registry; this was not `--no-verify` or a workspace-only build. The unpublished
`tasks/stub` package is deliberately excluded.

Archive inspection confirmed all 15 embedded stdlib files in `alder-can` and
`kernel/src/index.ts` plus its build script in `alder-kernel`. Verification
successfully compiled the include/build-script paths outside the workspace.

An initial run against the existing workspace target directory failed with
stale dependency API errors in codegen, despite the current packaged sources
containing those APIs. A fresh target directory passed without source changes.
Repeated pre-release packaging of unchanged version numbers must not rely on
old dependency artifacts. The CI check now verifies packages in a fresh runner
temporary target directory, separately from the cached workspace build.

The CLI produced by package verification executed `examples/hello` and the
`async`, `externs`, `traits`, `records`, and `control_flow` end-to-end fixtures
with `/tmp` as the working directory. This checks both packaged compiler/runtime
integration and physical source-relative extern resolution.

`cargo dist plan --tag alder-cli-v0.2.3` also succeeded with cargo-dist 0.32.0.
It planned the configured macOS/Linux ARM64 and x86-64 binaries, Windows x86-64
binary, checksums, and shell/PowerShell/npm/Homebrew installers. This command
did not create a tag, publish a release, or execute an installer.

Limits: this is local host verification, not proof of Linux/Windows builds,
cross-architecture builds, installer behavior, or Sampo publication. Existing
CI includes Linux checks and a Windows CLI build; cargo-dist builds the release
matrix. Those remote jobs have not been run for this checkpoint. Rerun package
verification against the final hardening commit and the eventual Sampo version
updates before treating the release gate as complete.
