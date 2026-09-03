# Effects internals

This document is the implementation contract for M4's error rows, async
tasks, and compile-time context. Error rows are implemented first as a full
front-end-to-runtime slice. Tasks and context use similar propagation ideas,
but Alder does not expose a general user-defined effect system in M4.

## 1. Design boundaries

Three rules keep the implementation understandable:

1. Error rows are types inside `Result`, not runtime effect handlers.
2. Asyncness and provider requirements are properties of functions and call
   paths, not variants inserted into error rows.
3. Runtime values contain only data needed at runtime. Row tails, inferred
   requirements, and exhaustiveness evidence are erased before codegen.

The canonical AST is arena allocated. Solver-owned working types may use
owned maps and vectors, but conversion back into `alder_ast` allocates all
published slices and names in the module arena. Compiler-generated JavaScript
is built directly in the Oxc/Rolldown AST format. Checked-in kernel and stdlib
sources may remain ordinary files because they are maintained and audited as
source code rather than synthesized fragments.

## 2. Existing pipeline surface

The parser already produces:

```alder
:not_found(id)
[:not_found(Id) | :invalid(String)]
[:not_found(Id) | errors]
Result[User]

error LookupError {
    :not_found(Id)
    :unavailable(String)
}
```

The canonical AST already has `Expr::Tag`, `Pattern::Tag`, `ErrorGroup`,
`ErrorTagType`, `Type::ErrorRow`, and `RowExtension::{Closed, Open}`. Public
interfaces and owned `.aldi` interfaces already have lossless shapes for
error groups and rows. M4 must preserve these shapes and replace the solver's
current opaque row placeholder.

## 3. Surface semantics

### 3.1 Structural tags

A tag is identified by its lowercase label and ordered payload types:

```alder
:not_found(Id)
:invalid(String, Number)
```

Rows are structural. Two rows containing `:not_found(Id)` agree even when the
tag originated in different modules. Two occurrences of the same label must
have the same arity and unify payloads position by position.

Tags are not general variant values. A tag expression is valid only when its
type flows into the error argument of `Result`. This includes `Err(:tag(...))`,
explicit `Result[a, [:tag(T)]]` values, and structural operations performed by
the compiler. Let-binding or passing a raw error row as an ordinary value is a
kind/placement error.

### 3.2 Open and closed rows

```alder
Result[User]                         // inferred open row
Result[User, [:not_found(Id) | e]]  // explicitly open row
Result[User, [:not_found(Id)]]      // explicitly closed row
Result[User, LookupError]            // named closed row
```

`Result[a]` is canonical sugar for a two-argument `Result` whose second
argument is an empty row with a fresh open tail. The tail can accumulate tags
from `Err` and `?` while solving.

An explicit closed row is a promise that no other tag escapes. An explicit
named error group makes the same promise. Inferred public functions may retain
an open row; their solved interface exposes the known tags plus openness.

User-facing rendering deliberately hides internal row-variable names:

```text
[:invalid(String) | :not_found(Id)]      closed
[:invalid(String) | :not_found(Id) | _]  open
```

Debug representations and snapshots of internal structures may use stable
generated names where the identity itself matters.

### 3.3 Named groups

An `error` declaration is a name for a closed structural row. It creates no
runtime wrapper and does not change tag object layout:

```alder
error LookupError {
    :not_found(Id)
    :unavailable(String)
}
```

When it appears in a `Result` error slot, `LookupError` normalizes to:

```text
[:not_found(Id) | :unavailable(String)]
```

Normalization consults local declarations and imported interfaces. A group
used as an ordinary value type remains nominal long enough for its compiler-
derived trait dictionaries to resolve; this is compile-time identity only and
does not add a runtime wrapper. Flattening a group through `?` includes all of
its tags in an open enclosing row. Writing the group explicitly as the
enclosing error type keeps that row closed.

## 4. Solver representation

The current `Ty::ErrorRow` unit variant is replaced by a structural form
equivalent to:

```rust,ignore
struct ErrorRowTy<'a> {
    tags: BTreeMap<&'a str, Vec<Ty<'a>>>,
    tail: ErrorTail,
}

enum ErrorTail {
    Closed,
    Open(ErrorRowVar),
}
```

`BTreeMap` gives deterministic tag ordering. Payload vectors preserve source
position within a tag. `ErrorRowVar` is a distinct kind from ordinary type
variables and record-row variables. Its substitution table can bind only to
another error-row tail/fragment. The occurs check rejects a tail that would
contain itself.

At minimum the solver distinguishes:

```text
Type        ordinary values and type constructors
RecordRow   record fields and record tails
ErrorRow    error tags and error tails
```

Unifying variables of different kinds is always an error. A `Result`'s second
slot expects `ErrorRow`; ordinary generics cannot silently turn records or
values into error rows.

Generalization and instantiation include free error-row variables exactly as
they include ordinary type variables, but preserve the variable kind. Each
instantiation receives fresh row tails. Conversion to the canonical AST emits
the fully pruned known tags and `RowExtension::Open` or `Closed`.

## 5. Row operations

### 5.1 Equality

Ordinary type unification uses row equality. For rows `L` and `R`:

1. Prune both tails.
2. Unify payloads for every common label, checking arity first.
3. Compute labels present only on the left and only on the right.
4. Reconcile those residual fragments through the opposite tails.
5. Two closed tails accept no residual labels. An open tail may bind to the
   residual labels plus the other tail, subject to the occurs check.

This is symmetric. It is used for annotations, branches, function arguments,
and repeated occurrences of a type.

### 5.2 Inclusion for `?`

Propagation is not equality. Given:

```alder
let value = operation()?
```

the operand must have `Result[value, operand_errors]`, and the enclosing
function must return `Result[return_value, function_errors]`. The solver adds:

```text
operand_errors ⊆ function_errors
```

For every known operand tag, inclusion inserts or unifies that tag in the
function row. If the operand tail is open, its future tags are linked into the
function tail. A closed function row rejects any operand tag not listed there.
An open function row absorbs tags from any number of `?` sites, producing a
union rather than making all operand rows equal.

`?` remains a runtime early return of the original `Err` value. The operand is
evaluated once. Row inclusion has no runtime representation.

### 5.3 Tag inference and patterns

`:label(a, b)` initially contributes a singleton row whose payloads are the
inferred types of `a` and `b`. Context then unifies that row with a `Result`
error slot. When a known row already contains `:label`, arity is checked before
payload unification so the diagnostic can point at the whole tag and explain
the expected payload count.

A tag pattern is checked against the solved/expected row. Its payload patterns
receive the corresponding payload types positionally. A label absent from a
closed row is an impossible-pattern error. An open row may admit an otherwise
unknown label, but its payload shape must become a consistent constraint on
the open tail.

## 6. Function inference

While inferring a function, the solver keeps its return type and, when that
return is a `Result`, the enclosing error row. `return Err(:tag(...))`, tail
expressions, and every `?` site constrain that same row. `Result[a]` creates
the open row before body inference so all propagation sites share it.

An explicit closed result supplies a closed row before the body is checked.
Consequently an extra direct `Err` and an extra propagated error fail at their
source sites rather than after generalization.

Raw tag placement is validated after types are pruned. Every tag-expression
site records its inferred type context; validation requires a path to a
`Result` error slot. This permits constructor inference without making error
rows ordinary first-class value types.

## 7. Exhaustiveness

Exhaustiveness runs after solving because openness and normalized group tags
are type information. Inference records each match site's subject type,
patterns, guards, and regions; the post-solve checker prunes the subject and
applies these rules:

- For a closed error row, every known tag must be covered by an unguarded arm,
  unless an unguarded `_` arm exists.
- For an open error row, an unguarded `_` arm is mandatory even when every
  currently known tag is listed.
- A guarded tag arm checks payload types but does not prove coverage because
  its guard may be false.
- Duplicate or unreachable cases may be warnings, but missing coverage and a
  missing open-row wildcard are errors.

The diagnostic lists missing tags in sorted order. For an open row it explains
that dependencies may add tags and labels the match plus the arm list. Wording,
labels, and hints follow Elm's reporting structure where applicable, adapted
to Alder syntax and semantics.

## 8. Interfaces

Solved public annotations are the source of truth. Conversion back to
`alder_ast::Type::ErrorRow` must emit every known tag in lexical order, every
payload type after full pruning, and the correct open/closed extension.

`Interface::from_module`, interface copying, owned serialization, and
hydration already carry these fields. Tests must prove an inferred open row
survives a dependency build and produces the same type when consumed from an
`.aldi` file. Internal row IDs must never leak into serialized output.

## 9. Diagnostics

Compiler stages return structured Rust errors, using `thiserror` where error
chaining or display plumbing is useful. User-facing conversion lives in
`alder-report` and implements `miette::Diagnostic` with a stable code, named
source, primary operation label, useful secondary labels, and concise help.

Required families cover illegal tag placement, closed-row missing/extra tags,
payload arity/type mismatch, cross-kind unification, non-exhaustive/open-row
matches, and cyclic group normalization. Wording and hints follow Elm's
reporting structure where applicable, adapted to Alder semantics.

Tests use parser-style source-aware snapshot macros and `indoc`. Renderer
snapshots explicitly disable color for deterministic ASCII output; production
rendering does not disable color globally.

## 10. Codegen and runtime representation

Rows erase completely. Tags keep the established representation:

```js
{ $: ":not_found", _0: id }
```

Tag patterns compare `$` and bind `_0`, `_1`, and so on. `Result` keeps its
existing `Ok`/`Err` representation. The canonical environment supplies these
constructors as built-ins, and codegen imports their kernel implementations
directly. `?` evaluates its operand once, returns the same `Err` object
unchanged, and extracts `_0` from `Ok`. Codegen constructs these forms as Oxc
nodes owned by the Rolldown AST container.

The built-in `Json.decode` contract returns the closed row
`[:invalid_json(String)]`. Kernel decoders construct that tag, including path
context in its payload, so JSON failures participate in the same typed row
machinery as user errors.

## 11. Async contract

`.await` remains postfix, and `.await?` is ordinary composition:

```alder
fetch(url).await
fetch(url).await?
```

Pipe forwarding happens before wrappers on the destination stage:

```alder
request |> send(client).await?
```

is `send(request, client).await?`. Pipe inference and lowering peel the
wrappers, forward into the call, then reapply wrappers in source order.

During the async wave, `.await` marks its enclosing function task-producing.
Task functions lower to generators and `.await` to the scheduler's `yield*`
protocol. Non-task functions remain plain functions. The kernel owns fibers,
cooperative interruption, child scopes, `fork`, `join`, `all`, `race`, timers,
and task entry execution. `Task[Result[a]]` needs no combined runtime type.

## 12. Context contract

`use Provider` adds a function requirement. Direct calls propagate it upward;
`provide Provider = value { ... }` lexically discharges it. Interfaces publish
requirements, and entry points must have an empty unsatisfied set.

At runtime the scheduler carries a fiber-local provider map. Forked fibers
capture their parent's context, while scoped changes do not leak to siblings.
Context stays a dedicated compiler structure, not a user-visible error row.

## 13. Verification order

The error-row slice is accepted only after canonical/solver snapshots, local
and imported inference tests, colorless miette renderer snapshots, interface
round trips, direct Oxc AST snapshots, standalone runtime paths, formatting,
full Clippy with warnings denied, full tests, and changed-crate package checks
all agree.
