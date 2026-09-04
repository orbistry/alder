# Type-alias hardening: implementation plan

Status: core expansion implemented; acceptance audit pending. These are existing language
contracts (`type Name[a] = ...`), not a new milestone feature.

## Evidence

Before the fix, canonicalization predeclared aliases with only name and arity, then
translates all named references to `Type::Named`. Alias declarations are
canonicalized separately and exported with their bodies, but references do not
use those bodies. Active inference ignores alias declarations. Its existing
`Type::Alias` branches recurse into the target without applying arguments, so
merely emitting an open alias node would not fix generic substitution.

New solver regressions cover optional-record aliases, chained generic aliases
declared before their dependencies, independent Number/String instantiations,
and `type Identity[a] = a` in a universally checked function signature. A
canonicalization regression covers direct and indirect recursive aliases in
both full and header-only modes. Cycle rejection and all three solver expansion
regressions now pass.

`types::alias_order` now computes deterministic dependency-first order with an
explicit traversal stack, rejecting cycles before enum/trait canonicalization.
It scans nested type arguments, function types, tuples, records and error-row
payloads without following nominal enum bodies. Both canonicalization modes
run the check. The ordered output now drives body registration before enum and
trait canonicalization. Imported bodies are indexed by qualified identity, and
normal name lookup still controls visibility. A reviewed
no-color driver snapshot covers the new source-aware recursive-alias error.

`aliases::instantiate` constructs Filled canonical alias targets. Substitution
does not descend into caller replacements, avoiding accidental capture when
caller variable names match declaration parameters. It traverses functions,
tuples, applications, projections, partial-constructor slots, record/error rows,
and nested aliases. Current row merging diagnoses overlapping labels instead of
silently discarding them. That policy still needs comparison with the intended
row-extension rules. Applied replacements currently support variable and named
heads, not Partial heads; higher-kinded alias argument validation remains open.

The records CLI fixture now executes imported optional and generic aliases,
including independent Number/String instances. One existing higher-kinded
snapshot was reviewed and corrected: the test claimed transparent expansion
but previously recorded Wrapped rather than its Result target.

Additional regressions now cover open/concrete record-tail arguments, collisions
between caller and declaration variable names, and rejection of generic
specialization and wrong payloads. The AbortSignal extern Task check now follows
alias targets; canonicalization and executable Promise-backed CLI cases pass.

Error-tail regressions now cover open and concrete arguments, propagation,
tag payload checking and rejection of unlisted tags. These exposed an existing
solver defect in generic Result identities: unifying empty residual error rows
wrapped universal variables in a row structure. Empty residuals now bind directly
to the shared variable. Direct and aliased forms pass, with imported execution
coverage in the records fixture.

Still required: broader error-row inclusion checks, full higher-kinded alias applications,
serialized imported alias use, private alias publication, and remaining consumers
that inspect a type's outer constructor. Core positive regressions are not evidence that
all alias acceptance work is finished.

## Implementation direction

The local Elm reference, `Canonicalize/Environment/Local.hs` (`addAliases`,
`addAlias`, `getEdges`), establishes alias dependencies before other type
declarations and rejects cyclic SCCs. `Canonicalize/Type.hs` distinguishes
alias bindings from nominal union bindings and records their arguments.

Apply that structure to Alder:

1. Keep nominal enums/extern types distinct from alias definitions. Register
   imported alias bodies by canonical qualified identity, preserving visibility
   and renamed/qualified lookup; never resolve by spelling alone.
2. Resolve local alias dependencies before enum payloads, trait signatures,
   and value annotations, independent of source order. Diagnose cycles even
   when no function uses the alias, in both canonicalization modes. Reuse the
   resolved bodies when emitting alias declarations rather than doing a second
   inconsistent expansion.
3. Instantiate alias parameters capture-avoidantly. Ordinary type parameters,
   higher-kinded application arguments, record tails, and error tails all need
   substitution. Do not substitute only leaf field types or erase row identity.
   Repeated independent uses cannot share inference variables accidentally.
4. Prefer a canonical `Type::Alias` with a genuinely filled target so all active
   consumers see the instantiated type while retaining diagnostic identity.
   Audit every alias consumer first; an Open target cannot be treated as Filled
   by simply ignoring its argument map.
5. Preserve alias bodies and parameters through owned interfaces and binary
   round trips. Imported aliases now execute in the CLI records fixture; binary
   serialization followed by imported use still needs dedicated coverage.
6. Test incompatible payload rejection, universal contracts, recursive enums
   using nonrecursive aliases, and source-aware cycle diagnostics. Preserve
   normal CLI color and use no-color diagnostic snapshots.

The implemented core and its passing regressions are described above. This is
not a completed acceptance audit for every alias interaction.
