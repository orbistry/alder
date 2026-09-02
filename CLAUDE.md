# Claude Code Guidelines for Alder Compiler

## Project Overview

Alder is a programming language compiler forked from the Elm compiler and ported from Haskell to Rust, adapting to Rust idioms where appropriate. It compiles to JavaScript with a focus on targeting Cloudflare. Unlike Elm it has no TEA (components use compile-time-tracked signals), supports SSR, ships a built-in Drizzle-like data layer, uses curly-brace Rust-flavored syntax, and is a general-purpose language via an embedded V8 runtime. Source files use the `.ald` extension.

The pipeline (`alder-parse`, `alder-can`, `alder-constrain`, `alder-solve`, `alder-driver`) started as a finished Elm port. `alder-parse` (over `alder-source`) now implements the new grammar in `SPEC.md` (milestone M1, done; design in `docs/parser-internals.md`); `alder-can` onward still consume the old AST and do not compile until M2 adapts them, so the workspace build and CI are red on the `parser-rewrite` branch by design.

## Design Documents

`docs/` holds the language, runtime, web, data, and tooling design (all provisional). `SPEC.md` holds the draft grammar and the milestone task list. Update them when a decision changes or a task lands. When the Elm reference and `docs/` disagree, `docs/` wins; Elm is a guide to compiler structure, not to language semantics.

## Reference Material

The Elm compiler source is cloned locally at `elm/` (gitignored). Use this as the primary reference for:

- Parser structure: `elm/compiler/src/Parse/`
- Error types: `elm/compiler/src/Reporting/Error/Syntax.hs`
- AST definitions: `elm/compiler/src/AST/`

## Memory Management

We use **bumpalo** for arena allocation:

- One `Bump` arena per module
- All AST nodes are allocated in the arena
- Source text is loaded into the arena first, so `&'a str` slices point into arena memory
- Unified `'a` lifetime throughout - drop the arena, everything is freed
- bumpalo does NOT call `Drop` on items - this is fine since we only store `Copy` types or references

```rust
let bump = Bump::new();
let src: &str = bump.alloc_str(&file_contents);
let mut parser = Parser::new(&bump, src.as_bytes());
```

### AST Type Guidelines

**Inline small `Copy` types** - don't put them behind `&'a`:

- Small enums (e.g., `VarType`, `Associativity`) - just store the value
- Newtypes around primitives (e.g., `Precedence(u16)`) - just store the value
- `Region` (16 bytes: two `u32` line/column pairs) - two words, still cheaper to copy than to chase a pointer

**Use `&'a T` for**:

- Large types
- Recursive types (must break infinite size)
- Slices (`&'a [T]`, `&'a str`)

**Use `Option<&'a T>` not `&'a Option<T>`**:

- Null pointer optimization makes `Option<&'a T>` the same size as `&'a T`
- `None` is free (null pointer, no arena allocation)
- Only allocates when `Some`

## Testing Strategy

We use **insta** for snapshot testing with extremely granular tests:

- Separate macros for success vs error cases
- `assert_X_snapshot!` - expects Ok, panics on Err, snapshots the parsed value
- `assert_X_error_snapshot!` - expects Err, panics on Ok, snapshots the error

Macros are defined at module level under `#[cfg(test)]` and re-exported with `pub(crate) use` where submodules import them (a `macro_rules!` inside `mod tests` would be invisible to child modules); modules with no child importers keep only the definitions.

Example snapshot output:

```
---
source: crates/alder-parse/src/lib.rs
description: "42"
---
42
```

Run tests with:

```bash
cargo insta test
cargo insta accept  # to accept new snapshots
cargo insta test --unreferenced delete  # delete stale snapshots
```

### Test String Formatting

The test macros use `indoc!` which strips leading indentation, so:

- **Simple one-liners** stay simple: `assert_expression_snapshot!("if x { y } else { z }");`
- **Multiline tests** are plain `indoc` strings (raw `r#"…"#` literals
  indented to taste); there are no indented variants and no column rule.
- No need to slam strings to the left - `indoc` handles it
- Only use multiline raw strings when testing actual multiline syntax

## Progress Tracking

**SPEC.md** at the repo root tracks:

- The roadmap as milestone checklists (M1 parser rewrite through M10 TUI)
- Draft grammar in EBNF notation for the new syntax

Update SPEC.md as features are implemented.

## Development Workflow

1. Add grammar rule to SPEC.md
2. Write snapshot test(s) first
3. Implement parser code
4. Run `cargo insta test`, review snapshots
5. Mark complete in SPEC.md

## Validation

After making changes, run these checks before committing:

```bash
cargo fmt --all          # format code
cargo clippy --all-targets --all-features -- -D warnings  # lint (warnings = errors)
cargo test               # run tests
```

These same checks run in CI on every push/PR.

## Error Messages

We are porting Elm's full error hierarchy from `Reporting/Error/Syntax.hs`. Error types are nested (Module contains Decl, Decl contains Expr, etc.) to enable Elm-quality error messages.

## Versioning & Changesets

We use [**sampo**](https://github.com/bruits/sampo/blob/main/crates/sampo/README.md) for changelog and version management:

- Each crate has its own independent version (no shared `version.workspace = true`)
- When making notable changes, create a changeset with `sampo add`
- Sampo handles granular per-crate version bumps based on what actually changed
- CI runs the Sampo GitHub Action to automate release PRs and publishing

```bash
# single crate
sampo add -p alder-parse -m "Add support for let expressions"

# multiple crates in one changeset
sampo add -p alder-parse -p alder-region -m "Add position tracking to let expressions"
```

Since `sampo add` requires interactive bump level selection, Claude should write changeset files directly to `.sampo/changesets/` instead:

```markdown
---
cargo/alder-parse: minor
cargo/alder-region: patch
---

Add position tracking to let expressions.
```

Use a short descriptive filename like `.sampo/changesets/let-expr-positions.md`.

## Code Style

- Recursive descent parser (not parser combinators)
- Single `Parser<'a>` struct combining arena + state
- Incremental implementation - test each feature before moving on
- No premature abstraction
