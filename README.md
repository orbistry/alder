# alder

A programming language that compiles to JavaScript, with a special focus on
Cloudflare. Forked from the Elm compiler and ported to Rust.

Design lives in [`docs/`](docs/). The draft grammar and the roadmap live in
[`SPEC.md`](SPEC.md). All of it is current direction, not final.

## Development

Run the source-only web example with SSR, hydration, routing, and HMR:

```sh
cargo run -p alder-cli -- dev examples/web-full
cargo run -p alder-cli -- build examples/web-full
cargo run -p alder-cli -- run examples/web-full/dist/server.mjs -- --port 3000
```

See [web development](docs/web-development.md) for the smallest two-file app,
Cloudflare setup, production artifacts, browser checks, and deployment. The
[M6 acceptance ledger](plans/m6-acceptance.md) separates implemented/local
verification from the explicitly authorized live-preview deployment gate.

Runnable examples live in [`tests/e2e/`](tests/e2e/) alongside their assertions.
For example:

```sh
cargo run --bin alder -- run tests/e2e/hello
cargo run --bin alder -- run tests/e2e/pipes
cargo run --bin alder -- run tests/e2e/explicit_async
```

Run the development checks with:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Releases use [sampo](https://github.com/bruits/sampo) changesets and
cargo-dist. Push a changeset to `main`, merge the release PR, and CI
publishes crates, binaries, the Homebrew formula
(`brew install orbistry/tap/alder`), and the npm package
(`npx @alder-script/cli`).
