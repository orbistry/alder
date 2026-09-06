# Dictionary lowering audit

Status: acceptance reconciliation in the integrated hardening worktree. This
extends generic-contract checking with evidence construction, runtime dictionary
layout, closure capture, and imported implementation checks. Final coherent
integration commits and release verification remain separate gates.

## Selection and representation audit

`resolve_obligations` runs after generic contracts are checked. Predicate
selection compares givens first, then matches every implementation-head argument
in a separate template-binding map. It does not unify away checked universals.
Coherence rejects overlapping heads, invalid kinds/orphans, superclass and
associated-projection cycles, and non-decreasing prerequisites before search.
Resolved uses retain dictionary order; direct calls and captured references
consume the same evidence, while implementation superclass slots are recorded
separately.

| Boundary | Permanent evidence |
| --- | --- |
| Universal signatures and interface publication | `generic-contract-hardening.md`; stored method/recursive-local contracts accept independent Number/String consumers and reject specialization. |
| Selection/coherence | Solver open/closed record and error-row overlap cases, explicit HKT sections in either argument position, kind checks, decreasing/non-decreasing prerequisites, and superclass cycles. |
| Package visibility and ownership | Driver package-index, sibling-order, import-form and re-export tests; `reexport-hardening.md` includes runtime dictionary ownership. |
| Defaults and method-local slot ordering | Stored default-helper test plus explicit_async's distinct Label instances, reordered/duplicate bounds, foreign singleton/factory defaults, and reused tasks. |
| Superclass initialization and recursion | `dictionary_initialization.ald`, recursive default snapshots, and recursive generic Hash/Json CLI trees. Only eager evidence dependencies determine dictionary declaration order. |
| Multi-method evidence size | Json, Hash, structural error Hash/Json, and Option Ord tests count exactly one kernel call per nested layer at depths 2/4/8; single-method Eq/Show controls do the same. |
| First-class ordering and equality | `ordering_evidence.ald` captures an imported generic Ord method and its Eq superclass, then checks custom reverse order, nested Option absence, repeated calls, and unit payloads. |

Source inspection raised a possible multi-parameter cycle-key concern because
diagnostic search frames identify a trait and rendered first argument. Current
source prerequisites are unary: SPEC's bound is a path and canonicalization
rejects a bound whose trait arity is not one. Therefore no valid source-level
multi-parameter prerequisite chain was reproduced; no speculative semantic or
parser change was made. A future multi-parameter-bound feature must revisit the
search key and diagnostic frame together.

The lazy descriptor helper is shared by Hash/Ord and Json; it emits child
evidence once but computes descriptors when operations run, allowing recursive
dictionary references to initialize safely. This is a code-size guarantee for
the tested nested shapes, not global memoization or a bound on arbitrary
user-defined trait computation.

## Imported trait defaults

An accepted `impl Format[Foreign] {}` omitted the inherited default method at
runtime. Codegen looked for the trait declaration among the implementation
module's own items; its foreign-trait branch emitted only explicitly provided
methods. The CLI probe captured the missing method and failed with
`TypeError: Cannot read properties of undefined (reading 'bind')`.

Canonicalization now records omitted foreign defaults as `ImplItem::Default`,
retaining the canonical method owner, checked signature, and helper symbol.
Codegen imports the helper from its owning module and installs a dictionary
method forwarding self, method-local dictionaries, and source arguments in that
order. Implementation prerequisites live in the selected self dictionary; they
must not be passed as extra default-helper arguments. Local defaults and provided
methods retain their existing lowering.

Trait interface metadata previously recorded the source method name instead of
the emitted `$default$Trait$method` symbol. Both canonical publication and local
solver headers now record the actual helper. Foreign-trait implementation
interfaces also retain provided/inherited method entries rather than publishing
an empty method list. Interface format 7 rejects old cached metadata without a
legacy reader. All compiler output is constructed directly as Oxc AST nodes.

The `explicit_async` fixture imports a generic capture function and implements
the trait in another module. Distinct custom Label dictionaries expose argument
swaps: local defaults, reordered/duplicate-bound overrides, a foreign singleton
default, and a foreign prerequisite factory all produce distinct expected text.
First-class methods and nested closures retain evidence across suspension;
reusing a task retains its original captured evidence.

`stored_traits_supply_default_helpers_to_new_implementations` serializes and
discards a trait producer, then builds a fresh consumer implementing that trait.
It checks the helper import, method-local dictionary slot order, and the
consumer's published inherited-method metadata. This is paired with actual CLI
execution, not treated as proof that printed code alone runs correctly.

The first broad standalone CLI selection did not include `explicit_async`.
The dedicated `explicit_async_laziness_capture_and_nested_tasks_execute` test is
the reproduction/verification command for this case. A duplicate local name in
the initial fixture was corrected before reaching the runtime failure; it is
not a compiler defect.

## Superclass initialization

The stored consumer's suspicious ordering was reproduced through the actual CLI:
`ReferenceError: Cannot access '$dict$Show$1' before initialization`. A source
implementation assigned its `$super0` from a derived Show dictionary declared
later. Only synthetic Eq dictionaries had previously been prioritized.

The emitter now orders local implementations by their solver-selected superclass
evidence dependencies, including references inside factory arguments and nested
structural/container evidence. Factory bodies also participate in dependency
ordering because superclass construction can invoke them. Independent nodes
retain deterministic source order. Unsolved AST snapshot lowering retains the
synthetic Ord-to-Eq dependency. A dependency cycle fails lowering instead of
silently emitting a temporal-dead-zone access.

All dictionaries are emitted before ordinary source initializers. Their method
bodies remain closures; construction does not invoke user methods. Source value
initializers retain their relative order. This is not a general solution for
forward references between arbitrary source values.

The `dictionary_initialization` CLI fixture verifies derived and later-declared
manual superclasses, a superclass factory with concrete dictionary arguments,
and first-class method capture/call during top-level initialization. Recursive
method defaults remain valid: method-body evidence is not an eager dependency.
Ten codegen snapshots were reviewed for declaration movement; their JS line
contents are unchanged modulo generated temporary names. Updated descriptions
use current return-type syntax. The dedicated CLI regression and strict Clippy
pass, as do the full workspace tests/doctests (194 driver, 70 kernel, 17 CLI,
68 codegen, 465 inference). No pending snapshots remain.

The imported-default and initialization changes are committed in the cross-layer
integration `556a21c`. Final packaging and clean-tree gates remain open.

The selection and dictionary-size reconciliation above supersedes the earlier
open follow-up. Final integrated validation is recorded in the main plan;
none of these finite regressions establish general type soundness.

The latest integrated pass includes 72 codegen, 199 driver, 72 kernel, 17 CLI,
and 467 inference tests, all doctests, strict all-target/all-feature Clippy,
and formatting. The dedicated standalone CLI test also passes independently.
The added depth and captured-ordering tests required no production changes.
