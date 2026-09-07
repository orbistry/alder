# Approved hardening language decisions

Status: user-approved, implemented and verified by compiler hardening; see
`docs/compiler-hardening-final-report.md` for acceptance evidence and limits.
This document supersedes earlier checkpoints requesting these choices.

## Tuple projections

Infer fixed-length tuples, collecting all projection requirements before fixing
the length, with a minimum inferred length of two. Do not introduce arity
polymorphism. Preserve sparse constraints and element relationships until shape
resolution; never allocate a source-index-sized vector just to constrain an
unknown tuple. Follow `plans/tuple-projection-hardening.md` for coverage.

## Result errors and structural capabilities

Result's error argument supports error rows and named error groups, not ordinary
types such as String. Apply this consistently to annotations, constructors,
patterns, propagation, aliases, higher-kinded applications, externs, and stored
interfaces. Replace obsolete ordinary-error fixtures with supported row cases.

Named error groups are structural aliases: identity and behavior must not depend
on the originating group name or declaration order. No custom trait
implementations for individual named error groups. Provide selected structural
capabilities conditionally on payload capabilities; do not automatically promise
every derivable trait. Eq, Show, and Json motivate this work, but this decision
does not approve a blanket trait inventory. Do not automatically provide Ord.
If Hash is provided, it must respect structural equality across names, reordered
declarations, and module boundaries. No arbitrary nominal dictionary fallback.

## Optional function parameters

Implementation and remaining acceptance: `plans/optional-arguments-hardening.md`.

Use record-like `param?: Type` syntax as shorthand for `param: Option[Type]`.
Function-type annotations use ordinary Option types, with no separate optional
slot marker. Any trailing Option parameter is omittable, including an explicitly
written Option parameter. An omitted argument becomes None. This is not a
default-expression facility.

For supplied arguments and record-field initialization, prefer a direct type
match; otherwise insert the fewest Some wrappers needed to match the expected
type. Recursive lifting is allowed: Number can become Option[Option[Number]].
This is a contextual coercion, not a general unification rule; do not extend it
to equality, trait resolution, arbitrary expressions, or implicit final-return
wrapping. Preserve single evaluation of the supplied expression.

```alder
fn greet(name: String, title?: String) String {
    match title {
        Some(title) => `${title} ${name}`,
        None => name,
    }
}

greet("Ada")
greet("Ada", "Dr.")
```

Only the trailing consecutive Option parameters are omittable. Earlier Option
parameters remain required when followed by a non-Option parameter, so
`option.map(value: Option[a], transform: fn(a) b)` remains valid. This rule
applies equally to shorthand and explicit Option annotations and supersedes the
earlier blanket required-after-Option prohibition. Positional callers cannot
skip an earlier slot to fill a later one. No `opt` keyword, default expressions,
or labeled arguments are introduced.

For `value?: Option[Number]`, the body sees Option[Option[Number]]:

| Supplied argument | Parameter receives |
| --- | --- |
| omitted or None | None |
| 42 | Some(Some(42)) |
| Some(42) | Some(Some(42)) |
| Some(None) | Some(None) |
| Some(Some(42)) | Some(Some(42)) |

Bare None takes the direct match and denotes outer absence. Some(None) explicitly
denotes present inner absence. Preserve these distinctions in runtime lowering
and extern adapters; do not conflate them through a naive null/undefined
representation. Resolve lifting from expected types, not incidental inference
order; genuinely unresolved generic ambiguity must not silently choose a depth.

Optionality must survive function values, higher-order calls, and interfaces.

### Optional record fields: approved equivalence

The user explicitly confirmed the record-equivalence question with "yes" after
being asked whether `{ value?: Number }` means exactly
`{ value: Option[Number] }`. This is a resolved decision, not the earlier ambiguous
bare response to multiple questions.

`name?: T` in a record type is shorthand for `name: Option[T]`. Omission supplies
None; an explicitly supplied None is equivalent to omission. Payload lifting
uses the same direct-match/minimum-Some rule as optional function parameters.
`value?: Option[Number]` therefore has exactly two Option layers, not an extra
independent field-presence layer. Some(None) remains distinct from outer None.

Remove separate semantic field-presence tracking and migrate source consumers,
interfaces, codegen, and runtime operations without compatibility shims. Reads,
writes, patterns, spreads, equality, ordering, hashing, and JSON must not preserve
an observable omitted-versus-explicit-None distinction. Source syntax may retain
the question mark for formatting, but it must not create a second type contract.

Implementation and adversarial regressions are required before this decision is
considered delivered. The named public unbounded traversal spelling remains a
separate unanswered question.

### Remaining function-parameter coverage

Audit named functions, lambdas, trait methods, externs, pipe insertion,
placeholders, arity validation, evaluation order, formatting, diagnostics, and
direct Oxc AST emission. Optional destructuring syntax remains unsettled; do not
treat it as already approved syntax. The ordinary Option function-type decision
and recursive lifting above supersede the earlier separate optional-slot and
single-wrapper proposals.

Traversal APIs use positional order `(values, callback, options?: MapOptions)`
for map, forEach, tryMap, and tryForEach, retaining each operation's own callback
and result types:

```alder
values |> fiber.map(x -> async { process(x).await })
values |> fiber.map(
    x -> async { process(x).await },
    { concurrency: 8 },
)
```

Default execution remains sequential, with explicit unbounded execution and
validated positive safe-integer bounds as in the concurrency plan. The public
adapter unwraps optional options before passing a normalized limit to the
kernel. This replaces the earlier unresolved map/mapWith API choice.

## Postfix propagation with `?`

Support Result and Option only. For Result, Ok yields its value and Err
propagates through the enclosing Result return context under the error-row
rules. For Option, Some yields its value and None returns None from the
enclosing Option return context. Successful final returns still require their
normal wrapper; there is no implicit Option/Result conversion.

Keep explicit async boundaries: `.await` runs a Task and `?` propagates its
unsuccessful outcome, so `.await?` composes these operations. For an async body,
propagation checks its declared body result, not the outer Task wrapper. Nested
functions and async blocks establish their own return/propagation boundaries.

This is not generic Monad binding, continuation rewriting, or do notation.
A dedicated Try-like propagation protocol may be considered later if needed;
it is not part of this implementation. Parser libraries can use ordinary
Result-returning calls and `?` with explicit cursor management; propagation does
not roll back mutations or implement parser backtracking.

Required coverage includes success and early exit for both types, incompatible
return contexts, nested boundaries, async `.await?`, existing cleanup semantics,
source-aware colorless diagnostic snapshots, and actual CLI execution. Preserve
normal colored diagnostics outside snapshot rendering.
