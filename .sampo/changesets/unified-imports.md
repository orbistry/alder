---
cargo/alder-source: minor
cargo/alder-parse: minor
cargo/alder-ast: patch
cargo/alder-can: minor
cargo/alder-solve: patch
cargo/alder-codegen: minor
cargo/alder-driver: minor
cargo/alder-bundle: minor
cargo/alder-fmt: minor
cargo/alder-cli: minor
cargo/alder-language-server: patch
---

Unify bundled, local, and external imports with grouped syntax, lowercase
standard-library namespaces, explicit utility imports, identity-preserving
namespace re-exports, and canonical comment-preserving formatting. Share public
interfaces between prelude and explicit imports, reject conflicting bindings,
and make module initialization independent of import declaration order.

Interface format 8 replaces earlier contracts without compatibility readers.
