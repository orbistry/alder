# alder

A programming language that compiles to JavaScript, with a special focus on
Cloudflare. Forked from the Elm compiler and ported to Rust.

Design lives in [`docs/`](docs/). The draft grammar and the roadmap live in
[`SPEC.md`](SPEC.md). All of it is current direction, not final.

## Development

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
