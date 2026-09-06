# Assignment syntax and generalization hardening

Status: syntax removal and repository-consumer cleanup complete. The joint
generalization review and final clean-tree release gates remain open.

## Approved semantics

Ordinary let bindings and parameters permit reassignment and field/index writes.
Shared-reference mutation remains JavaScript-like; there is no borrowing or
mutation-permission keyword. Imported values and named function declarations
are not local assignable storage. Those reference-kind restrictions are distinct
from permission to mutate an object reached through a local binding.

Alder is pre-1.0. Removed syntax has no compatibility parser, deprecation path,
migration-specific error, or migration hint. Ordinary identifier and grammar
rules apply. Historical transitional implementations are retained in Git, not
described here as current behavior.

## Root cause and implementation

The previous generalization criterion used a declaration permission flag even
though unassigned bindings can contain shared mutable collections. Removing that
flag alone would allow incompatible instantiations and specialized replacements.

Canonicalization now collects assignments by resolved binding identity, including
top-level roots written inside nested closures. Assignment targets participate in
value dependencies and recursive groups. Shadowed local bindings do not mark
unrelated globals as assigned. The solver uses assignment metadata together with
its non-expansive initializer restriction and environment free-variable checks.
The analysis conservatively includes unreachable writes.

Parser/source/canonical AST mutation flags and permission branches are removed.
Binding patterns are writable storage where allowed by reference kind. Codegen
uses JavaScript let bindings; no copy semantics or permission checks are inserted.
Assignment target and value types still have to agree.

The detailed current restriction, its conservative limits, and mutation/alias/
constraint/interface regressions are mapped in
`docs/mutation-generalization-hardening.md`.

## Acceptance evidence

- SPEC's let grammar and parser implementation use ordinary patterns with an
  optional type annotation, without a mutation modifier.
- Source scans of active source/parser/canonical/driver layers find no
  `ObsoleteMut`, `ImmutableAssignment`, or mutation-permission fields. Rust's own
  mutable local bindings are unrelated and remain untouched.
- All repository stdlib, example, and end-to-end `.ald` sources use current
  syntax. Embedded-source conversion and 113 parser snapshot structural reviews
  were completed in the earlier syntax-removal checkpoint.
- Canonical and actual CLI regressions cover named/lambda parameter rebinding,
  local/top-level assignments, loop/match bindings, and caller-visible array and
  record mutation. Named-function assignment retains a source-aware diagnostic
  without recommending a modifier.
- Solver tests preserve safe independent function instantiation, reject direct
  and captured incompatible replacements, and retain writes through recursive
  groups. Stored assignment and async-interface tests preserve these contracts
  after serialization and reload.
- Canonical AST excerpts, language guide, milestone plans, web examples, and
  parser fixtures now describe current syntax. The final documentation scan
  removed stale active claims about the transitional removal diagnostics.
- Sampo changesets include `ordinary-binding-assignment.md`,
  `assignment-aware-generalization.md`, and `assignment-value-dependencies.md`.

The full workspace validation at the external-member checkpoint passed 1,310
parser tests, 172 driver tests, 436 solver integration tests, 13 CLI tests, and
all doctests (two existing ignores), with strict Clippy and formatting. The
subsequent stored-tuple regression raises driver coverage to 173 passing tests.
These are source-state checkpoints, not final clean-commit acceptance.

## Remaining work

Follow-up source-comment audit removed three remaining descriptions of a
mutation modifier in markup child blocks (source AST and parser comments).
The keyword table and parsed child-statement implementation already used current
syntax. Scans of crate Rust/JS sources found no obsolete mutation diagnostic,
keyword variant, FieldPresence, or optional-field read helper; all repository
stdlib/example/end-to-end Alder files remain free of mutation-modifier lets.
All 1,310 parser tests, source/parser doctests, formatting, and strict workspace
Clippy passed after this comment cleanup. Rust's own mutable variables and
historical evidence logs are not language syntax and were preserved.

Finish the joint deferred-constraint and generic-evidence audit, reconcile and
commit the accumulated hardening changes, refresh release packaging after the
last production change, and run the objective's final acceptance gates. The
syntax cleanup does not by itself establish sound generalization or complete
the compiler-hardening goal.
