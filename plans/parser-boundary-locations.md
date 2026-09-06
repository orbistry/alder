# Parser diagnostic boundary locations

Follow-up to parser-diagnostic parity: missing punctuation must not be attributed
blindly to the next declaration after whitespace/comments. The earlier review
missed Elm's shared retention of pre-whitespace expression endpoints.

## Design and constraints

- Preserve the parser cursor (detection location) and the boundary before skipped
  whitespace/comments independently. Boundary tracking must participate in saved
  parser state and survive redundant `chomp` calls without drifting forward.
- Carry the actual opening punctuation and boundary evidence in shared delimiter-error data, used by delimited
  expression, pattern, type, and declaration lists. Do not reconstruct it by
  scanning backwards through source text in the reporter.
- On a cross-line missing separator/closer, show the insertion boundary. Keep an
  actual mismatched closer highlighted at its own location. Keep ambiguous
  separator-versus-closer expectations honest; do not assume every newline ends a
  construct or apply Elm's indentation rules to Alder.
- Preserve accepted syntax, nested leaf errors, source identity, Unicode/CRLF
  coordinates, and valid multiline/comment-heavy constructs.
- Preserve the user's intentionally broken `tests/e2e/async/src/main.ald` fixture.

## Acceptance

- [x] Shared transactional boundary tracking and parser-level regressions.
- [x] Delimited-parser inventory and broad use of shared error evidence.
- [x] Rendered regressions for missing closers before later declarations, missing
  separators, mismatched closers, aliases, comments, nested constructs, and EOF.
- [x] Valid multiline and speculative/backtracking cases retain behavior.
- [x] CLI/LSP primary and related locations verified, including Unicode and CRLF.
- [x] Formatting, strict Clippy, and workspace tests checked; intentional fixture
  failures distinguished from regressions without losing the user's edit.
- [x] Documentation, changesets, and final evidence updated without claiming more
  coverage or safety assurance than the checks establish.

## Implementation and reference

Elm's `Parse/Space.hs:62` (`checkIndent`) accepts the preceding expression's
endpoint and uses it when indentation ends a construct. `chompAndCheckIndent`
similarly retains the position before spaces. `Parse/Expression.hs` applies that
to list/tuple/record continuation. Alder adopts the retained evidence, not Elm's
indentation-sensitive grammar: no newline becomes a parse failure by this change.

`Parser::chomp` records `(trivia_end_byte, preceding_position)`; `ParserState`
saves/restores it. Redundant `chomp` calls do not drift. Consuming a new token
invalidates the old boundary by cursor comparison. `ExpectedEnd` carries the
opening, boundary and detection `Position`s inline. `word_end` shares the same data for
single-byte closers. The reporter uses a zero-width boundary only across lines,
labels the explicitly captured opening punctuation, keeps detection evidence
internal (not a secondary label on valid following code), and keeps
actual wrong closers (including closing markup tags) at the cursor. EOF wording
uses the detection location even when the insertion point precedes trailing trivia.

All 46 delimiter-end variants now carry this shared data:

- Declarations: `Attribute::{ArgEnd, End}`, `Import::NamesEnd`, `Params::End`,
  `TypeParams::End`, `Enum::{VariantArgEnd, End}`, `Impl::ArgEnd`,
  `ErrorDecl::End`, `TagVariant::ArgEnd`, `Table::{ModifierArgEnd, End}`,
  `Schema::{RuleArgEnd, End}`, `Macro::ParamEnd`, `Tests::End`.
- Expressions/blocks: `Block::End`, `Template::HoleEnd`, `Array::End`,
  `Tuple::End`, `Record::End`, `Match::End`, `Call::End`, `Index::End`,
  `Tag::End`, `State::End`, `Style::End`, `Query::{End, OperationEnd}`, `Select::ProjectionEnd`.
- Markup: `Markup::{TagEnd, CloseEnd}`, `Attr::ExprEnd`, `Child::HoleEnd`,
  `DirMatch::End`, `ChildBlock::End`.
- Patterns/types: `PCtor::End`, `PTuple::End`, `PArray::End`, `PRecord::End`,
  `TArgs::End`, `TFn::ParamEnd`, `TTuple::End`, `TRecord::End`, `TErrorRow::{End, ExtEnd}`.

This does not relabel entry/operand errors as missing delimiters, infer a missing
brace from a declaration-looking word that is legal inside the current construct,
or change raw-token/string/markup unclosed errors that already retain their own
opening evidence. It is not an error-recovery or safety-certification claim.

## Initial boundary verification (2026-09-06)

The following records the first boundary implementation. Its presentation was
subsequently corrected below: retaining useful evidence does not mean every
retained position should be shown to the user.

- 1,311 parser tests and 610 driver tests pass. New tests cover transactional
  boundary retention, 36 cross-line malformed prefixes with comments/CRLF,
  wrong closers, five valid multiline/comment-heavy programs, and nine granular
  rendered snapshots. Existing speculative tuple/lambda and record parsing tests
  retain behavior. All 80 changed parser snapshot payloads were mechanically
  compared against their former hierarchy and detection coordinates; only the
  new boundary data differs. One old function-type snapshot description was stale
  (`fn(a b) -> c` versus the existing test's `fn(a b) c`) and refreshed as well.
- All 11 CLI diagnostic integration tests pass, including real check/build/test
  commands and LSP import/Unicode-array cases with CRLF, zero-width primary ranges,
  same-file related locations, and the line-3 detection label.
- `cargo fmt --all` and strict all-target/all-feature Clippy pass.
- The full workspace run finishes with exactly two failing tests:
  `standalone_e2e_projects_execute` and `every_repository_source_is_idempotent`.
  Both fail on the user's intentionally unclosed async import. Re-running with
  just those two tests excluded passes 2,765 tests plus two passing doctests
  (two other doctests remain ignored).
- The exact user fixture reports `src/main.ald:1:26`, with detection context on
  line 3. A separate temporary async project with only the missing `}` restored
  executes successfully. The later standalone examples (`externs`, `control_flow`,
  `records`, `traits`, `docs_traits`) also execute successfully, ensuring the
  early async failure did not hide their runtime checks.
- The user fixture remains byte-for-byte unchanged by this work. Diff whitespace
  checking passes for task edits; the preserved fixture has its original trailing
  space on the broken import line.

## Delimiter presentation follow-up (2026-09-06)

The initial review incorrectly accepted a detection label on valid following code
and declaration-start labels standing in for delimiters. The shared renderer now
shows only the primary location and the actual owning opener. All delimiter-end
constructors require an explicit opening `Position`, captured before consumption
and threaded through helpers; the renderer does not search source text for it.
The opening label names the punctuation (`this \`{\` opens here`, etc.). The
detection position remains available for EOF/mismatched-closer classification.

All delimiter messages were reviewed against their parser branches. Required
separators are named explicitly instead of asking for another list item before a
comma. Grammar-valid alternatives remain for statement, style, schema and query
constructs. `TErrorRow::ExtEnd` and `Query::OperationEnd` distinguish states that
accept only a closer, so hints no longer suggest a tag after an error-row extension
or additional clauses after a completed non-select query. A defensive modifier
entry failure uses its nested entry error instead of fabricating an opener.

New verification includes an exact-opener matrix for every one of the 46 variants,
with completed nested delimiters that would defeat a naive last-bracket search.
The cross-line matrix requires exactly two labels, neither on the later declaration.
Closing-only advice is checked for error rows and insert/update/delete queries.
CLI/LSP regressions check absence of the detection label, exact opener ranges, and
Unicode before a nested opener as well as before the insertion boundary.

Final checks: formatting and strict all-target/all-feature Clippy pass; 1,311
parser tests, 614 driver tests, and all 11 CLI diagnostic integration tests pass.
The full workspace run still has exactly the two failures on the preserved broken
async source documented above. With only those two fixture-dependent tests
excluded, 2,769 tests and two doctests pass (two doctests remain ignored).
The actual CLI output for the user's source now labels line 1 column 26 and the
opening `{` on line 1; there is no label on line 3 or on the `import` keyword.

## Pre-commit validation

The user restored the async fixture before committing. A fresh run of
`cargo fmt --all --check`, strict all-target/all-feature Clippy, and
`cargo test --workspace --quiet` now passes without test exclusions: 2,771 tests
and two doctests pass; two doctests remain ignored. The earlier fixture-dependent
failures above are historical, not failures in the committed tree. The intentionally
invalid markup demo lives under ignored `target/parser-demo`, outside the commit.
