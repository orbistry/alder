# CLI reporting

Implement static Cargo-style CLI statuses while preserving diagnostics, source
links, runtime streams, exit behavior, and existing workspace/proxy resolution.

- [x] Add optional structured driver progress and silent compatibility entry points.
- [x] Add injected CLI rendering, global verbosity/color, and startup option scan.
- [x] Cover check/build/run/test/fmt and proxy/download statuses.
- [x] Test output, counts, ordering, streams, proxy forwarding, and colors.
- [x] Update tooling documentation and changesets; pass fmt, strict Clippy, tests.

The JavaScript test runner owns executed pass/fail counts. An opt-in runtime
`TestEvent` callback lets the CLI render runner-owned results on stderr and honor
quiet mode without parsing or changing user-program console output. Ordinary
runtime embedding and explicit JavaScript report callbacks retain the old text
fallback. Test failures have one result summary, not an additional parent failure
summary. Driver progress and test results are separate from source diagnostics.

`--quiet` takes precedence over `--verbose` in all positions. Proxy startup scans
only reporting options and stops at `--`; original OS arguments are forwarded.
Project-path and workspace compiler-version resolution are unchanged.

Validation: `cargo fmt --all -- --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, and the full
`cargo test --quiet` suite pass. CLI coverage includes all 11 existing
diagnostic/editor integration tests and 9 reporting integration tests. Renderer,
driver-callback, runtime-event, and fake archive/proxy tests avoid network
downloads and use fixed durations for timing assertions. Live release downloads
and Windows proxy execution were not exercised on this macOS host.
