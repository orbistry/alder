# Source and deferred-construct boundaries

This reconciles the remaining source-fidelity/deferred-declaration audit with
the active driver. It does not claim implementation of later language milestones
or final release readiness.

## Deferred declarations versus execution

The canonicalizer registers tables/schemas in the type namespace. Their owned
interfaces retain opaque type identities. Macros have no callable interface
value. These provisional declarations do not create runtime values or execute
database/macro behavior. Check mode remains available for the provisional
syntax; unused declarations do not become fake runtime implementations.

`deferred_declarations_publish_only_provisional_types` builds a table/schema/
macro producer and an importing consumer under Check, Build, and Test modes.
Only the actual `answer` function appears in the value interface; the table and
schema appear as types in source order. The consumer passes through an imported
schema type without constructing it. Emitted producer code has no table/schema/
macro symbol, and Check emits no artifacts.

`deferred_type_names_cannot_be_used_as_runtime_values` rejects `data.users` in
every mode, with no artifact/interface for the failed consumer. Its colorless
diagnostic snapshot labels the consumer's actual `users` reference.
`declared_macro_invocation_cannot_publish_a_runtime_stub` rejects invocation
in every mode before an artifact or public interface exists.

Existing driver tests separately establish that markup, styles, reactive state,
components, and queries may pass provisional checking but fail Build/Test without
runtime placeholders. Source macro invocations and comptime fail canonicalization.
Provider context remains an implemented runtime seam; provider checking and M5–M10
remain outside this hardening goal.

## Diagnostic source fidelity

`alder-report::Source` owns named source text. Region conversion treats columns
as one-based byte positions, maps line starts through newline bytes, and bounds
offsets to the source. Multiline and Unicode unit tests exercise this conversion.
The driver carries that source into parse, canonicalization, inference, warning,
and codegen reports; build-level ownership failures do not fabricate source spans.

`unicode_before_deferred_expression_preserves_diagnostic_snippet` compiles
`pub fn view() { ("café 😀", <div />) }` through the real checking/lowering boundary.
It asserts that the primary label's byte span slices out exactly `<div />`, then
snapshots the named-source diagnostic without ANSI color. This checks the offset
contract directly, not just visual similarity of a rendered message. Normal CLI
color behavior is unchanged.

The existing module/cache identity acceptance is mapped in
`module-identity-hardening.md`; canonical ownership, stored-interface validation,
and reproducible initialization are separate from source-display locations.
Inactive Elm-port files and the two lower-level unchecked APIs are identified in
`compiler-implementation-map.md`, so their presence is not counted as validation.

## Verification

The four new driver checks pass. Both new diagnostic snapshots were reviewed.
No compiler semantic change was needed: an initial test assumed sorted type
names, but the verified interface preserves declaration order.

The first full run exposed a flaky cache fixture: the deleted-implementation
case unexpectedly read the complete interface/index pair used by another case.
All three cases used the same process ID plus a clock timestamp for their path,
which does not guarantee unique directories for concurrent tests. Test setup now
uses an atomic counter and exclusive `create_dir`, retrying existing paths instead
of sharing them. A 64-directory concurrent regression verifies independent roots.
The cache validation itself is unchanged; all 12 project tests pass afterward.

Full workspace tests/doctests (199 driver, 72 kernel, 17 CLI, 69 codegen), strict
Clippy, formatting, and whitespace checks pass. No snapshots are pending. This
concludes the specified source/deferred-boundary audit together with the existing
module/cache evidence map, not arbitrary filesystem fault recovery or future
language execution. Final packaging and a clean committed-tree audit remain.
