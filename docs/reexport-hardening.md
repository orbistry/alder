# Public re-export hardening

Status: named/wildcard re-export boundary audit complete for the contracts below.
The broader module/cache review is reconciled in `module-identity-hardening.md`;
final clean-tree release gates remain open.

## Acceptance reconciliation at 21b4510 plus the integrated worktree

Reviewed `alder-can/src/interface.rs`: each public name selection copies the
checked entry, changing only `exported_as`. Repeated aliases are processed
independently, and the copying loop does not republish dependency instances or
private-name metadata. Foreign value bindings retain their original qualified
identity and cannot be reassigned as local storage; object mutation remains
shared. Foreign type/trait insertion checks the shared declaration namespace,
including enums through type insertion. These paths match the stored alias,
private-name, wildcard, and cross-namespace regression assertions below.

Reviewed the direct AST backend: explicit imports are emitted in source order
before generated owner-directed value imports. Keeping the facade as an explicit
dependency preserves its effects without giving it ownership of another module's
values or dictionaries. The package-instance fixture asserts the original
instance module is present in application dependencies and executes renamed
trait bounds, qualified methods, and prerequisite dictionaries.

Fresh runs pass all 12 driver tests selected by `reexport`, the three CLI tests
selected by `dependency_`, and `artifact_insertion_order_preserves_bundles_and_initialization`.
The last test makes six independent compilations, changes artifact insertion
order, compares complete bundles byte-for-byte, and executes each under a bound.
The driver selection includes stored mutable-state/safe-polymorphism consumers,
not just interface-name checks. These establish the specified alias, privacy,
namespace, ownership, and initialization audit boundaries without a production
change. They do not establish every module/cache contract or whole-goal release
readiness. Historical unfinished-audit statements below are superseded by this
reconciliation; source-level public module-namespace imports remain unsupported.

The driver accepted a facade containing `pub import ~/leaf.*` or
`pub import ~/leaf.{ answer }`, but a consumer calling `facade.answer()` failed
with unknown name. The facade's published interface contained no values.
Permanent driver regressions reproduced both failures before the fix.

The interface builders now receive the same dependency interfaces used during
canonicalization. Public named and wildcard imports copy selected public entries
for values, types, enums, traits, and module namespaces. Each entry keeps its
defining identity and checked scheme/body, changing only its exported name when
aliased. Each name selection is processed independently, so one source name can
be exported under multiple aliases. Private entries and dependency instance
definitions are not republished as facade-owned definitions.

`alder_can::from_module` and `headers_from_module` now require the dependency
interface slice. The driver supplies it for preliminary headers and final
publication; isolated callers use an empty slice only when there are no imports.
No owned-interface wire-format fields were added. Canonical value references
retain the original owner, so Alder codegen calls the defining module directly
rather than adding a facade wrapper with a second dictionary convention.

Coverage at this checkpoint:

- Named, wildcard, and repeated-source alias publication through a three-module
  driver build, including identity/scheme equality in the owned interfaces.
- Actual CLI execution of a generic Show/Eq function through a wildcard facade.
- Actual CLI execution across a named-renaming module and a wildcard facade for
  an enum, generic record alias, and trait. Both a generic trait-bound function
  and a qualified call through the renamed trait use the original instance.
- A package-root interface containing a generic alias/function is serialized
  with bincode, deserialized, and consumed without supplying the defining leaf
  interface or its arena. The consumer checks successfully.
- Explicitly re-exporting a private value fails at the imported name, with a
  reviewed colorless diagnostic; the failed facade publishes no interface or
  executable artifact.
- Wildcard publication has exact interface assertions for public values, aliases,
  enums, and traits. A consumer supplied only the facade interface can use the
  public declarations but cannot import private functions, aliases, enums,
  traits, or methods. Private diagnostic names and dependency instances are not
  copied into the facade. Conflicting wildcard value exports reject facade
  publication with a reviewed diagnostic labeling both source imports.
- Shared array exports preserve reference identity through repeated aliases and
  a named/wildcard chain. Mutations through each route affect the original.
- Renamed trait methods retain their method identity but bind under the selected
  local name. Previously import binding ignored `as`, making distinct aliases
  collide under the original name and missing actual local-name collisions.
  CLI execution covers two aliases through a named/wildcard chain and another
  consumer-side rename. Negative driver tests verify that the original name is
  not introduced and that a real alias collision rejects interface/artifact
  publication; colorless snapshots label the relevant source names.
- Facade initialization is retained: codegen emits explicit imports as bare
  AST import declarations in source order, before generated value imports, and
  includes them in dependency metadata even without local value uses. Previously
  owner-directed references bypassed facades entirely, dropping their top-level
  effects. The CLI regression failed before the fix; it now checks ordered,
  exactly-once mutations from both intermediate modules despite multiple routes
  to their shared dependency. Two unused sibling imports deliberately reverse
  filename order and verify source-ordered effects rather than sorted graph
  order. Driver tests check the retained metadata too.

Package runtime checkpoint: the path-dependency CLI tests now execute a named
renaming facade followed by a wildcard package-root export. Calls through the
leaf, facade and root all resolve the dependency's sibling Promise wrapper,
not the application's same-named JS file. The facade/root effects run once in
order, and array mutation through the root remains visible through every route.
The missing-wrapper diagnostic still identifies the original extern declaration.
A separate package-instance test executes renamed enum, trait and method exports
through the root; generic bounds and qualified renamed-trait calls use original
instances from two other dependency modules, including a prerequisite Array
instance. Both tests pass using compiled package interfaces and actual bundles.

Initialization determinism follow-up: the CLI regression
`artifact_insertion_order_preserves_bundles_and_initialization` compiles the
traits fixture six times, then sorts, rotates, and reverses artifact insertion
into fresh hash maps. Every final bundle is byte-identical. Each bundle executes
under a 30-second bound, checking the fixture's source-ordered unused imports,
exactly-once facade effects, and shared mutations. This adds actual bundler and
runtime evidence beyond the driver graph-order comparisons; no production fix
was needed. It does not claim that source import order may be changed without
changing effects, nor does it allow module cycles.

Cross-namespace follow-up: local type and trait declarations already reject a
shared name, but imported types checked only the type table and imported traits
checked only the trait table. A two-module public wildcard facade incorrectly
published both as `Shared`; the permanent driver regression failed before the
fix. Foreign type/trait insertion now checks the other table before insertion,
matching local declaration rules. Four regression cases cover both import
orders and wildcard versus explicit renamed imports, asserting that the failed
facade publishes neither an interface nor an artifact. The reviewed colorless
snapshot embeds the facade source and labels both conflicting imports.
A positive renamed `Payload`/`Reader` facade survives serialization and is
consumed without leaf interfaces; its generic bound and return alias resolve.

Validation: 84 canonicalization, 178 driver, and 14 CLI tests plus associated
doctests passed after the production fix. The subsequently added positive
stored-interface test and updated diagnostic snapshot both passed in the
two-test focused run. Strict all-target/all-feature Clippy and formatting passed;
whitespace checks are clean and no pending snapshots remain. Full-workspace/
package validation predates this change.

Enum/privacy/cycle acceptance follow-up:

- Extended the collision regression to enum-versus-trait declarations: both
  import orders and named aliases/wildcards reject the facade with one
  diagnostic and no facade interface or artifact. Enum imports use the same
  foreign type insertion check before registering constructors.
- Explicitly renaming a private alias, enum, trait, or private trait method
  cannot publish it. The first three retain private-name diagnostics; the
  private trait method is deliberately absent from the public interface and
  yields a name-not-exported diagnostic. This complements wildcard privacy
  tests rather than treating a rename as permission to expose declarations.
- A named/wildcard facade cycle is rejected by the actual parsed-source graph
  builder before compilation. Six rotated/reversed discovery orders produce
  the same `left -> right -> left` cycle, including a downstream consumer.
  This extends the generic graph tests to public import syntax.

All 181 driver tests and its doctest passed after these additions. No production
change was required for these cases; the existing collision snapshot remains
unchanged. This completes these specific enum/privacy/cycle checks, not the
entire module/cache audit.

Committed checkpoint: `08c77e5` isolates the six-line foreign-binding fix, four
regression tests, reviewed diagnostic snapshot, and changeset from the other
ongoing compiler work. Exactly the staged tree was exported with
`git checkout-index` to `/tmp/alder-import-checkpoint.dgj8wi`; its full
`cargo test --quiet`, strict all-target/all-feature Clippy, and formatting check
all exited 0, with no pending snapshots. That isolated tree has 137 driver
tests; the integrated working tree has 181. These counts refer to different
trees, not dropped regressions. The branch remains dirty with other required
hardening work; this checkpoint is neither a merge nor goal completion.

The formerly outstanding alias/chain, namespace, mutable binding, dictionary
ownership, and initialization reviews are reconciled above using both source
inspection and permanent boundary tests. Shared copying paths alone were not
treated as sufficient evidence.
Source syntax currently requires public imports to have a names or wildcard
tail, so direct public module-namespace imports are rejected by the parser;
do not infer source-level support merely from the canonical Module import arm.

Stored mutable-contract follow-up: the new driver test
`stored_reexport_aliases_preserve_shared_state_and_safe_polymorphism` verifies
named/wildcard chains for a concrete shared array and a function alias capturing
replaceable state. A final facade supplied without leaf interfaces retains
Number restrictions while an independent identity function remains polymorphic.
Invalid consumers fail with type mismatches and publish nothing. See
`docs/mutation-generalization-hardening.md` for the source/solver boundary review.
