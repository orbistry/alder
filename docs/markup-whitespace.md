# Markup whitespace and formatting

## Contract

Ordinary literal markup text uses source-line folding. Split on LF (CRLF is
treated as one line ending, and lone CR becomes LF), remove indentation from continuation lines and
trailing horizontal whitespace from lines followed by a newline, discard empty
lines, then join the remaining lines with one ASCII space. The first line's
leading spaces and the last line's trailing spaces remain intentional. A
single-line text run is unchanged, including repeated spaces and tabs.

Whitespace-only runs containing a line ending disappear. No space is implicitly
inserted between distinct children. Use `{" "}` when a space must survive a
line break next to an element or expression. Runtime string expressions are
never folded. Non-ASCII spaces are content, not indentation.

Within literal `<pre>` and `<textarea>` subtrees, preserve all literal text,
including blank lines and leading/trailing spaces (CRLF and lone CR become LF, matching
HTML parsing). This lexical rule extends through fragments and directives, but
does not change the contents of an independently declared component. CSS does
not affect parsing. The formatter must not reindent or wrap preserved text.

## Input → text matrix (before implementation)

`\n` and `\t` below denote actual source newline and tab characters. The text
column is DOM text content, not CSS layout; adjacent paragraphs can appear on
separate visual lines without a text-node space.

| Input | Text content |
| --- | --- |
| `<main>\n  <p>A</p>\n  <p>B</p>\n</main>` | `AB` |
| `<p>\n  Hello\n  world\n</p>` | `Hello world` |
| `<p>Hello\n\n  world</p>` | `Hello world` |
| `<p>Hello <strong>Ada</strong>!</p>` | `Hello Ada!` |
| `<p>Hello\n  <strong>Ada</strong>!</p>` | `HelloAda!` |
| `<p>Hello{" "}\n  <strong>Ada</strong>!</p>` | `Hello Ada!` |
| `<p>Hello {name}!</p>` with `name = "Ada"` | `Hello Ada!` |
| `<p>{name}\n  !</p>` with `name = "Ada"` | `Ada!` |
| `<p>A  B\tC</p>` | `A  B\tC` |
| `<p>{"  A\n B  "}</p>` | `  A\n B  ` |
| `<pre>\n  A\n\n B  </pre>` | `\n  A\n\n B  ` |
| `<textarea>\n  A\n B  </textarea>` | `\n  A\n B  ` |

Formatting must preserve this meaning, not historical indentation embedded in
multiline text. Inline content can remain inline; if the formatter moves a
meaningful boundary space onto its own line it must represent it explicitly.
Attributes and embedded code may wrap only at syntactically valid boundaries.

## Acceptance

- [x] Parser snapshots cover folding, boundaries, explicit strings, and preservation.
- [x] Markup formatting is readable, semantic-preserving, comment-preserving, and idempotent.
- [x] Compiled DOM/SSR/hydration regressions cover the matrix before and after formatting.
- [x] Full example and counter fixture formatted; real-browser verification passes.
- [x] Alder format checks, Rust formatting, strict Clippy, and workspace tests pass.

Verified on 2026-09-14:

- `cargo fmt --all` and `cargo fmt --all --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test --quiet` passed across the workspace (two existing ignored doctests).
- `alder fmt --check examples/web-full` passed for 12 files;
  `alder fmt --check examples/web-counter` passed for two files.
- `alder build examples/web-full` built both bundles and prerendered About,
  Ada, and Grace. About's generated HTML contains folded prose without layout
  whitespace between its elements.
- Chrome against a fresh `alder dev` build on port 3001 verified shared counter
  updates, the asynchronous command handler, About navigation and exact DOM
  text, Ada form submission, and Grace navigation. No browser warnings/errors
  were reported. About's `main` has three element children and no indentation
  text nodes.
- The direct Rust regression
  `markup_whitespace_before_and_after_formatting_renders_and_hydrates_identically`
  compiles both source forms and checks sync/async SSR, exact text content,
  client mounting, reactive updates, and reuse of every SSR node during
  hydration with no additional DOM allocations.
