# Removing mut without weakening generalization

Status: assignment-aware generalization, ordinary binding writes, and removal
of parser/source/canonical mutation syntax are implemented. Embedded Alder test
sources and snapshots are converted. Dedicated removal diagnostics are gone;
`mut` is an ordinary identifier, not a keyword. Final documentation audit,
packaging and the broader hardening audits remain open.
Approved semantics are recorded in section 2 of compiler-hardening.md.

Latest user decision: Alder is pre-1.0 and requires no backward compatibility.
Removed syntax should behave as though it never existed, with ordinary parse
errors rather than special removal errors or migration hints. Remove the
ObsoleteMut variants, explicit parser branches, lambda backtracking exception,
renderer helper, and dedicated obsolete-syntax tests/snapshots added earlier.
Audit keyword handling against the current grammar, not legacy syntax. This
supersedes the earlier diagnostic plan and the historical evidence below.

Current follow-up: removed all ObsoleteMut variants/branches, the lambda
backtracking special case, migration renderer, dedicated tests, and their five
snapshots. Updated the keyword list and SPEC. Converted embedded Alder strings
without changing Rust bindings. Parser tests pass (1292); reviewed all 113
changed parser snapshots by comparing structure after removing mutation fields
and source-position numbers: no other structural changes. Reviewed the two
driver diagnostic differences (source text/positions only) and refreshed six
stale source-description headers whose output bodies had not changed.
Formatting, strict workspace Clippy, and full `cargo test --quiet` pass after
this conversion, including CLI/runtime suites and all doctests. No pending
snapshots or whitespace errors remain. Release packaging is still pending.

Documentation checkpoint: canonical/trait AST excerpts no longer carry mutation
flags, canonicalization docs describe writable storage and assignment metadata,
and web examples use ordinary lets. Parser fixture names were updated to describe
their current coverage. Renamed snapshots retain the reviewed contents. Parser
tests and strict workspace Clippy were rerun successfully after those renames.

## Current implementation evidence

- Parser parameters and lets reject `mut` with ObsoleteMut; source parameters
  and lets no longer store a mutation region.
- Canonicalization now permits writes through ordinary lets and named/lambda
  parameters independently of the old syntax flag. Named function declarations
  and imported values still retain their existing nonassignable status. The
  distinction is now called assignability, not mutability: every local pattern
  binding is writable, while imported/function/constructor references are not
  local storage. Nonassignable-reference diagnostics no longer recommend `mut`.
- Canonicalization collects resolved top-level assignment roots across cloned
  environments, including nested writers, into module metadata. The solver
  restricts those bindings and their captured free variables independently of
  the old permission flag. Nonexpansive-initializer restrictions still apply.
  A never-assigned function binding retains independent instantiations even
  under the transitional `let mut` syntax. Shadowed locals do not restrict it.
- Local lets/parameters are monomorphic already. Removing their permission
  checks is not equivalent to changing global generalization.
- Three permanent source regressions verify direct replacement cannot make
  one function binding accept Number and String independently, captured
  forwarding functions cannot generalize that shared state, and consistent
  Number replacement remains usable. They currently use `mut` solely to reach
  the existing checker; migrate their syntax without weakening assertions.
- Assignment dependency analysis now includes resolved top-level roots, not
  just index/RHS expressions. A permanent regression first reproduced a missed
  recursive group when a function only wrote its peer binding. Additional
  tests cover escaping nested writers, computed indices, and local shadowing.
  This repairs inference grouping and supports assignment-aware generalization.
- Validation for that dependency fix: `cargo fmt --all`, strict all-target /
  all-feature workspace Clippy, and full `cargo test --quiet` passed, including
  68 canonicalization tests and 258 solver integration tests. No pending
  snapshots were found. Release packaging and the syntax migration remain open.
- Assignment-aware follow-up validation: formatting, strict workspace Clippy,
  and full `cargo test --quiet` pass with 261 solver integration tests. Two
  pre-existing negative snapshot fixtures now include actual replacement writes;
  their diagnostic payloads/locations are unchanged and their source metadata
  was explicitly refreshed and reviewed. No pending snapshots remain. Further
  async-capture coverage is still required.
- Added an owned-interface roundtrip regression: discard the producer, reload
  its serialized interface, accept Number/String instantiations of a
  never-assigned function, and reject String use through a forwarding function
  capturing Number-replaced state. The negative consumer publishes no artifact
  or interface; its colorless source-aware diagnostic snapshot was reviewed.
  The actual CLI fixture executes the exported nested writer and observes the
  forwarding function change from identity to increment while the independent
  identity binding remains usable at Number and String. Source-order variants
  also check that a writer need not precede the invalid reader in the source.
  Validation: all 112 driver tests, 262 solver integration tests, the standalone
  CLI execution suite, formatting, and strict workspace Clippy pass. No pending
  snapshots or whitespace errors remain for this follow-up.

## Required implementation sequence

1. Replace the declaration permission flag as a generalization criterion.
   Account for actual assignments by resolved binding identity, including
   writes inside nested/escaping closures and recursive groups. Restrict
   replaceable shared bindings and variables reachable by capturing functions.
   Do not conflate shadowed names or use declaration/source order as evidence
   that a binding is never reassigned. A flow-insensitive analysis may retain
   assignments in unreachable code; document any resulting inference limits.
2. Preserve useful safe polymorphism. Blanket monomorphism for every top-level
   let is not the requested solution. Conversely, simply deleting the
   `!decl.mutable` condition while allowing assignment would leave previously
   generalized schemes vulnerable to incompatible replacements.
3. Audit assignment dependencies and exported interfaces. In particular,
   `value_scc::stmt` previously omitted the assignment root; that dependency
   is now included and covered by regressions. Further verify inference ordering and
   captured-variable relationships before relying on SCC ordering for safety.
   Handle writes to binding contents as well as rebinding; existing mutable
   collection restrictions still apply independently of assignment syntax.
4. Remove mut syntax and obsolete fields/checks throughout active compiler
   layers. Preserve invalid-target checks (e.g. imported bindings are not local
   assignable storage). Clarify declaration-kind constraints before treating
   every reference as a writable binding. Retain RHS/target type compatibility.
5. Migrate stdlib, examples, test source, snapshots, formatting, diagnostics, and
   specification. Preserve each negative test's intended rejection reason;
   obsolete `mut` syntax must not make semantic tests pass accidentally.
6. Add ordinary-let and parameter reassignment, closure capture/rebinding,
   aliases, recursive groups, source-order permutations, and owned-interface
   tests. Integrate async-block captures when that syntax is implemented.

Do not claim this amendment is implemented from the baseline tests alone.

## Ordinary binding writes

The new positive regression first failed with ImmutableAssignment on both named
and lambda parameters. Canonicalization now admits their assignments and local /
top-level let assignments. Existing direct/captured incompatible-replacement
regressions were migrated to ordinary `let`, retaining their rejection behavior.
Codegen already lowers bound patterns with JavaScript `let`; no new codegen
change was needed to make these bindings writable. The CLI fixture now exercises
parameter rebinding, array/record mutation visible through caller aliases, local
rebinding, lambda-parameter rebinding, and imported shared-function replacement
without `mut`. The obsolete immutable-let diagnostic snapshot was removed and
replaced with a positive canonicalization test; Git retains its history.
Validation: formatting, strict workspace Clippy, and full `cargo test --quiet`
pass, including 263 solver integration tests and the CLI execution suite. No
pending snapshots or whitespace errors were found.

Canonical TopLevelLet, LocalLet, Param, and Place no longer store mutable flags.
BindingMode no longer carries local mutation permissions. The source AST still
contains the old syntax regions. A reviewed colorless diagnostic snapshot covers
assignment to a named function declaration without recommending `mut`; actual
CLI coverage now includes rebinding for-loop and match-pattern variables.
Validation for canonical cleanup: all 68 canonicalization tests, 113 driver
tests, 263 solver integration tests, the standalone CLI execution suite,
formatting, and strict workspace Clippy pass. No pending snapshots or whitespace
errors remain. Full-workspace/release validation remains required after the
pending parser migration.

The parser now rejects `mut` and source AST fields are removed. Two focused
parser error snapshots and three rendered driver snapshots cover lets and
named/lambda parameters. Lambda backtracking initially hid the new error behind
an invalid-tuple diagnostic; it now preserves the ObsoleteMut parameter error.
All new snapshots were reviewed. Standalone `.ald` fixtures are migrated and
the CLI execution suite passes. Workspace checking and strict Clippy passed
before the final lambda diagnostic adjustment; rerun final gates after migration.

Next: remove the dedicated obsolete-syntax machinery described above, then
migrate embedded Alder test strings without changing Rust `let mut` or literal
payloads. Update source-aware snapshot
descriptions and expected ASTs/regions, explicitly review differences, and
retain every negative test's intended semantic error. Old positive parser tests
such as `params_mut` still use removed syntax and must be converted, not accepted
as new errors. Old snapshots still contain removed `mutable: None/Some` fields.
Remaining docs/SPEC historical references and reserved-keyword handling need
audit. Full tests/release packaging and coherent commits remain required.
