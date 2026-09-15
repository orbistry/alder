# Explicit Cloudflare tooling and stock releases

## Agreed scope

Implement `alder cloudflare setup` (`--check`, `--remove`) and
`alder cloudflare login`; retain target-aware `alder deploy`. Users provide Node.
Third-party tooling belongs in a shared compatibility/platform-keyed user cache,
not CLI archives, global npm installations, or project dependency trees.
Keep Miniflare/workerd and Wrangler integration. No publishing, login to real
accounts, deployment, tag changes, or commits are authorized.

## Acceptance ledger

- [x] Pinned, integrity-checked explicit setup with actionable prerequisites.
- [x] Shared cache reuse, isolation, locking, atomic publication, corruption and
  interrupted-install recovery, safe narrowly scoped removal.
- [x] Thin interactive Wrangler login with normal credentials and exit status.
- [x] All platform consumers use the cache; no implicit downloads; standalone
  remains Node-independent; installed tooling supports local offline operations.
- [x] Remove custom support preparation, archive verification, installer and
  checksum rewriting, obsolete tests/config, and support self-update handling.
- [x] Regenerate stock cargo-dist workflow and audit all installer formats.
- [x] Preserve legacy installations without silently deleting their support.
- [x] Aggregate release notes without erasing generated installation/download
  content; idempotent composition that fails without modifying ambiguous bodies.
- [x] Regression coverage, live local setup/development/prerender checks, release
  packaging checks, workspace tests, formatting, strict Clippy.
- [x] Docs, SPEC, changesets, platform limitations, temporary-file cleanup.

## Initial evidence

The worktree was clean at start. `platform.rs` currently searches versioned
installer, archive, Homebrew, and source-checkout support directories. Deployment
and prerendering invoke Node directly. `download.rs` also extracts and validates
support trees. Custom release scripts patch installers and generated workflows.

Pinned cargo-dist 0.32.0 `announce.rs` emits the exact changelog section as
`## Release Notes` followed by the selected changelog, then generated install and
download sections. The aggregation workflow currently replaces the whole body.
Use the plan's exact original changelog as the initial replacement boundary and
explicit markers for subsequent runs, preserving all text outside that boundary.

## Progress

Release-note composition and workflow integration implemented. Five focused
Python tests pass, covering byte-preserved surrounding content (including shell
and PowerShell instructions/downloads), repeat updates, ambiguous or missing
boundaries, reserved markers, embedded headings, and existing section parsing.
No remote release was edited. Full CLI/end-to-end aggregation validation remains
part of the completion audit, along with the tooling and release work above.

Shared tooling implementation is now wired into dev, prerender, and deploy;
`cloudflare setup`, `--check`, `--remove`, and `login` parse and dispatch. Seven
focused Rust tests and strict alder-cli Clippy pass. A real setup installed 36
production packages, and subsequent check/setup validated/reused the same cache
without running npm again. Version-manager symlinks must retain their basename
(the local node/npm shims dispatch via argv[0]); resolving them to their canonical
binary broke prerequisite detection and was corrected.

Current task-created installation for subsequent integration verification:
`/Users/rvcas/Library/Caches/alder/cloudflare/cf8f5ae8c80f3a874eeb940267877fd4843ce90b25fed1c51ca00c1bfff102e5`.
No real login or deployment performed. Release-script removal, archive extraction
simplification, workflow regeneration, docs, broader tests, platform review, and
completion audit remain outstanding. Audit setup interruption/child lifetime,
platform prerequisite matching, npm configuration, and cache publication failure
paths before declaring the cache complete.

Removed five obsolete support preparation/installer-patching scripts and tests,
the custom build-setup hook, support archive inclusion, and `allow-dirty`.
Regenerated release CI with installed cargo-dist 0.32.0; `dist generate --mode
ci --check` passes without exceptions. The proxy now extracts only the binary;
seven archive tests pass, including preservation of existing legacy support and
ignoring old archive support payloads. CLI strict Clippy and the five retained
release-note tests pass. Current release/tooling/runtime/example docs updated;
historical milestone records still need an explicit superseding note. Actual
stock artifact inspection and remaining setup robustness/integration gates are
not yet complete.

`dist build --artifacts global --tag alder-cli-v0.5.1` completed locally:
stock shell/PowerShell/Homebrew/npm outputs generated in `target/distrib`, with
no custom support-installation markers. No remote publication occurred. Native
Apple Silicon archive generation was started next; inspect its live process
handle/results before restarting or claiming completion.

Native archive build completed successfully. `tar -tf` confirms that the Apple
Silicon archive contains only `alder`, README, CHANGELOG, and LICENSE under its
root directory. This artifact predates the following cache-hardening changes and
must be rebuilt for final executable validation.

Publication now preserves the previous installation through replacement and
rolls back a failed rename. A selected-key `.previous` directory allows explicit
setup/removal to recover after interruption between publication steps; a valid
replacement is retained and an incomplete replacement restores the backup.
Regression tests cover failed publication and both interrupted states. Node
metadata validation now checks platform/architecture against the cache target,
including all five supported release targets. Ten focused Cloudflare tests,
strict alder-cli Clippy, formatting, and `git diff --check` pass. Process-tree
cancellation, npm configuration isolation, end-to-end integration and workspace
gates remain required; these focused tests do not establish their completion.

Full `cargo test` and `cargo clippy --all-targets --all-features -- -D warnings`
passed on macOS ARM64. Seven Python tests now include subprocess-level
multi-crate aggregation/deduplication, highest-bump grouping, continuation lines,
Unicode, exact repeated output and fail-safe empty stdout without input writes.
The script explicitly reads/writes UTF-8 for Windows runner compatibility.
`dist generate --mode ci --check` and `git diff --check` also pass.

A source-only temporary copy of `web-full` built and prerendered all three
entries with PATH restricted to `/usr/bin:/bin` (Node unavailable, independently
confirmed by the actionable failure of `cloudflare setup --check` under that
PATH). Its standalone server ran and returned the expected `/health` JSON.
Switching only the temporary copy's target to Cloudflare built/prerendered the
same entries through the shared cache. Cloudflare dev served `/health` and
`/users/ada` successfully and shut down cleanly on Ctrl-C. These Cloudflare runs
used `npm_config_offline=true`, not an OS network sandbox; do not claim that all
network access was physically blocked. No npm installation or account operation
was invoked. Both task-owned servers stopped, port 3107 was checked free, and
`/tmp/alder-cloudflare-check.VTcT6M` was removed. Browser interaction was not
retested in this check. Setup/login process cancellation and npm configuration
isolation still require implementation/audit and regression coverage.

Setup now explicitly overrides npm's installation prefix, global/workspace
mode, dependency layout, platform/architecture, lifecycle scripts, and required
dependency inclusion. An opt-in real integration test installs and validates 36
locked packages with npm offline, conflicting user settings, and paths with
spaces, then removes its temporary cache. It passed twice. The first run exposed
npm's symlinked-prefix handling on macOS `/var`; normalizing Unix staging paths
fixed that failure without changing the lockfile. Windows retains ordinary
absolute paths for npm.cmd. Linux and Windows CI now explicitly set up tooling,
run this offline regression, and remove their selected cache. Those CI runs
have not been executed remotely by this task.

Setup/probes/login now supervise delegated processes on cancellation, preserving
the cache lease until shutdown. Unix installation process groups are signalled
and cleaned up, while login retains its foreground terminal streams. Tests pass
for actual parent-only SIGINT forwarding, descendant cleanup, delegated statuses,
and a local fake login launcher (no authentication). The executable preserves
Unix signal-derived statuses and full Windows exit codes rather than truncating
to u8. Windows console/tree cancellation code and its platform-specific exit
test require Windows CI; they are not claimed as locally exercised. Latest CLI
tests (32 library + 2 binary tests, one explicit integration test ignored in the
default run), full workspace tests and strict workspace Clippy passed.

Rebuilt the native Apple Silicon archive after process supervision changes.
Installed it through the unmodified stock shell installer from a localhost
artifact server into an unmanaged temporary prefix. The prefix contained only
`bin/alder`; that binary ran without Node for `--version` and validated the
existing shared tooling. SHA-256 matched the generated checksum (shasum reported
the stock file's extra blank line separately). The locally generated shell
installer had no embedded checksums, so the manual checksum check—not the
installer—provides this verification. The npm package tar contains installer
sources/metadata only, with no Cloudflare tree; Homebrew installs `alder` and
ordinary metadata. Shell syntax passes. PowerShell, npm installation execution,
and Homebrew execution remain platform/tool-specific limitations, not tested
installer executions. The temporary install `/tmp/alder-stock-installer.aHaJOl`
was removed and its localhost server stopped; port 3108 was checked free.

Before completion: perform the final requirement-by-requirement audit, review
process/error edge cases and release-note workflow safety, rerun any gates after
last edits, update acceptance checkboxes/SPEC with evidence, and remove the
task-created shared tooling installation if no further integration needs it.

## Final requirement audit

This section supersedes outstanding-work statements in the chronological notes
above. Verification is local macOS ARM64 unless explicitly stated otherwise.

| Requirement | Evidence |
| --- | --- |
| Explicit setup/check/remove/login; retain deploy | Clap definitions and dispatch in `cmd/cloudflare.rs`/`cmd/mod.rs`; no alternate setup/deploy alias. `deploy.rs` retains required exact name/account arguments and target validation. |
| User-managed Node/npm and pinned integrity | Embedded existing package/lockfile, npm `ci`, local native probe and file inventory. Real setup and offline isolated installation pass; actual missing-Node, missing-npm and missing-cache invocations give actionable errors. |
| Shared compatibility/platform cache | Key has support contract and target, not compiler version. Repeated setup reused the same installation; key-isolation tests cover support changes and all five target mappings. |
| Concurrency, interruption, corruption, removal | Shared/exclusive leases, explicit unlock on drop, preserved replacement backup and recovery. Regressions cover inherited descriptors, publication failure, interrupted states, corrupt bytes/files, symlinks, narrow removal and preservation of unrelated data. |
| Login terminal/credentials/status/cancellation | Cached Wrangler launcher with inherited terminal streams; fake login execution and status tests, actual Unix signal forwarding and process-group cleanup. No credential copying or real authentication. |
| Consistent consumers and standalone/offline behavior | Dev/prerender/deploy resolve the shared cache; setup is the only npm invocation. Full example standalone build/prerender/run without Node and Cloudflare build/prerender/dev checks passed. Real setup regression uses npm offline. |
| Stock releases and non-destructive migration | Five obsolete scripts/tests removed, archive include/build hooks/exceptions removed, pinned workflow regeneration check passes. Native archive and stock shell installation inspected/executed; npm tar and Homebrew/PowerShell sources audited. Seven binary extraction regressions cover historical archives and legacy support preservation. |
| Preserve release notes | Seven Python tests plus composition against actual cargo-dist 0.32.0 `announcement_github_body`; complete install/download suffix retained and repeat output identical. Workflow passes the plan and only edits after successful composition. |
| Gates/docs/changeset | Workspace tests, strict all-target/all-feature Clippy, formatter and diff checks pass. Docs, SPEC, historical milestone notes and CLI minor changeset updated. |
| Cleanup and authorization | Task-created shared tooling removed via `setup --remove`; missing-cache check confirmed no implicit reinstall. Temporary projects/install prefixes and servers removed. No commits, publication, tags, deployment, live release edits or real login. |

The last audit found and fixed two concrete issues: resolution probes must not
install persistent Tokio signal handlers in the calling build/dev/deploy
process, and lease release must explicitly unlock rather than depend on the
last close of a descriptor that a concurrent fork might inherit. Dedicated
regressions pass. Repair/removal also handles a regular file replacing the
expected cache directory. The final default CLI suite has 36 passing library
tests plus two binary tests; the opt-in real npm integration passes separately.

Platform limitations: Windows console cancellation and installer execution,
Linux native integration, x64 macOS, and ARM64 Linux were not executed locally.
CI now covers explicit setup/offline isolation on Linux and Windows, but no
remote run is claimed. PowerShell/npm/Homebrew installer execution is not
claimed; their stock generated paths were audited. No actual browser login was
performed, as required. Existing browser application behavior was not rewritten.
Normal `target/` build artifacts are retained; task temporary directories and
the shared test installation are not. Stable zero-byte lock files are retained
intentionally to avoid lock-inode replacement races.

Final native archive rebuild completed after the audit fixes. Its complete
entry list is `alder`, README, CHANGELOG and LICENSE under the platform root.
The packaged binary runs without Node for `--version`; after cleanup its tooling
check gives the expected explicit setup instruction instead of falling back to
checkout dependencies or downloading anything. The final opt-in offline npm
integration passed again and cleaned its temporary installation.
