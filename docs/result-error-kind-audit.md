# Result error-kind contract audit

Status: confirmed inconsistency; language-policy decision requested before fixing.

## Reproductions

At `570ee29`, the full trait-solving entry point (`solve_input`, not the legacy
inference-only helper) accepts:

```alder
fn identity(value: Result[Number, String]) { value }
```

It rejects each of these independent programs:

```alder
fn success() Result[Number, String] { Ok(42) }
```

```alder
fn failure() Result[Number, String] { Err("bad") }
```

```alder
fn read(value: Result[Number, String]) Number {
    match value { Ok(n) => n, Err(_) => 0 }
}
```

The first error reports `[_]` versus `String`; the latter two report `a` versus
`String`. These were executed through a temporary solver audit test, subsequently
removed rather than committing assertions that bless the inconsistent behavior.
The earlier real CLI extern fixture independently exposed the pattern failure.

## Root causes and conflicting evidence

- `Infer::from_ast` delegates the second Result argument to
  `convert_ast_error_type`. A bare variable there is assigned `ErrorRow` kind,
  including the built-in Ok/Err constructor's generic error parameter.
- An ordinary concrete named type in that same position falls back to ordinary
  type conversion unless it is a named error group. Thus String annotations
  survive while constructor instantiation and patterns cannot unify with them.
- `unify_return` uses error-row inclusion for Result returns, independently of
  whether the concrete error argument is actually a row.
- Existing higher-kinded tests intentionally exercise `Result[Number, String]`;
  they are not proof that constructing, matching, or propagating it works.
- `docs/language.md` and the M4 plan describe tagged rows and named error groups,
  with separately kinded row variables, but do not explicitly settle support for
  arbitrary ordinary error types.

## Decision and required follow-through

Do not silently broaden or narrow the language to make one regression pass.
The user was asked whether to require error rows/groups or also support ordinary
error types. Either decision needs consistent annotation validation, constructors,
patterns, return checking, `?`, aliases, higher-kinded applications, externs,
stdlib signatures, and cross-module interfaces.

If rows are required, diagnose invalid annotations at their error argument and
replace obsolete non-row fixtures with actual supported row cases. If ordinary
types are allowed too, preserve the distinction between exact ordinary error
types and row inclusion/union; do not permit ordinary values as open row tails.
Add positive and negative source-aware diagnostics and CLI regressions for the
chosen contract. No solver semantics were changed by this audit.
