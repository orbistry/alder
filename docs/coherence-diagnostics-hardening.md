# Coherence diagnostic ownership

## Reproduction and fix

A module containing an enum and an explicit Eq implementation conflicts with
the enum's generated Eq. Before this fix, a separate `pub fn answer() { 42 }`
module received the same overlap diagnostic with two zero-length labels at its
first character. A consumer of that inferred function could additionally report
an unknown import because the unrelated overlap prevented its interface from
being inferred. The new three-module driver regression reproduced the incorrect
attribution before implementation and now passes in both source input orders.

The driver's header pass filtered coherence errors by defining module, but its
later full solver validated the whole package and returned every error for every
module. Rendering those errors used the current source with fallback regions.

After header discovery reaches its fixed point, the driver now validates the
frozen registry before its final body pass. When source-owned coherence errors
exist, their defining modules retain diagnostics, while the other modules get
`ModuleResult::Blocked`. The failed build publishes no executable artifacts,
checked interfaces, or package indexes. CLI check output distinguishes blocked
modules from modules with their own errors. This is a build-level stop, not
filtering errors out of the solver or choosing between invalid implementations.

A second regression reproduced a phantom local label when two implementations
actually live in different modules. Each diagnostic now highlights only its
own implementation and names the other module in the hint. Both colorless,
source-aware snapshots were reviewed. Same-file overlap diagnostics retain
their two real labels. The existing test with conflicting implementations and
broken bodies still rejects both defining modules.

## Stored dependency-only errors

Two separately compiled dependency modules can each supply a valid implementation
yet overlap when their serialized indexes are combined. A regression reproduced
the resulting overlap attached to an unrelated application function with a
zero-length label. The frozen-registry stop now handles every coherence error,
not only errors with an owner among the current sources.

`BuildResult.diagnostics` retains errors with no owning source module. They use
the ordinary trait diagnostic message and advice naming the canonical dependency
modules, without invented source snippets or spans. Coherence errors expose a
shared, canonically ordered module-ownership set. CLI build/check collect these
build-level diagnostics alongside source diagnostics. `is_success` includes
them, so a build with no source modules cannot silently accept an invalid index.

The persisted-overlap regression covers zero, one, and two unrelated source
modules, plus a simultaneous independent source-owned conflict. Dependency
errors appear once, local errors remain, and no artifacts or interfaces are
published. The colorless snapshot was reviewed; three identical loop-generated
snapshots were consolidated into one intentional snapshot.

### Mixed ownership and stored cycles

A further regression combines a stored dependency implementation with a source
implementation in the same dependency package. It tests canonical local names
sorting before and after the stored module. In both cases, only the live source
owns the overlap diagnostic, with one real label and the stored module's full
package-qualified name in the hint. No duplicate build-level error is emitted.
Removing the stored conflict permits compilation. The colorless snapshot was
reviewed, and blocked builds publish no artifacts, interfaces, or indexes.

Two additional tests deliberately construct semantically cyclic interface
metadata and recompute its fingerprint before serialization. One creates a
superclass cycle; the other creates mutually recursive associated-type bindings.
Both are rejected even without application source modules, retain dependency
identity, and have no invented source spans. Their unmodified valid controls
pass. These tests required no new production changes. They verify these specific
coherence checks, not all possible malformed stored interfaces or a general
trust-boundary proof.

## Source declaration lookup

Implementation origins contain source ordinals, not indices into canonical
items: imports are omitted, and header-only canonicalization also omits value
declarations. Diagnostic lookup now matches implementation identity. Generated
implementations carry their originating declaration's region, so the same
lookup covers source implementations and derives.

A further renderer regression reproduced a superclass-cycle label at the first
character of a source file when the cycle's last trait belonged to another
module. The renderer now selects a local trait from the cycle and labels it as
participating in the cycle. It does not claim that this particular declaration
closes the cycle. The new colorless snapshot points to the local trait after an
unrelated declaration; the existing single-module snapshot retains its real
span with the corrected wording. Stored-dependency cycle diagnostics still have
no invented source spans.

Current checkpoint validation: the full working-tree workspace test/doctest run
passes, including 167 driver tests, 12 CLI tests, and 55 kernel tests (two
existing doctests remain ignored). Strict workspace Clippy and formatting pass.
The source-label regression failed before the renderer fix. Snapshots retain
literal miette output, including its trailing spaces; diff checks pass with
end-of-line whitespace exempted. Final packaging and the broader hardening audit
remain open.

Build-validation caution: switching distinct checkouts through one Cargo target
directory left stale local-crate artifacts during this review. The initial
compile and CLI failures were resolved by rebuilding the affected current-tree
crates, without changing their source semantics. Future isolated checkpoint
checks should use a distinct target directory.
