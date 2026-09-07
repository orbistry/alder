# Typed JSON hardening

Status: primitive trait codecs and module dictionary dispatch fixed; wider audit remains open.

## Structural error-row codecs

Closed error rows now receive Json evidence from their payload codecs, using
the same canonical structural evidence path as Show. The new full-solver
regression first failed with MissingInstance for two reordered named groups;
it now passes encoding and decoding without group derives. Function payloads
without Json remain rejected, with a reviewed source-aware missing-codec
diagnostic and obligation chain.

Direct Oxc emission builds both codec methods with canonical tag descriptors
and explicit payload dictionaries. It reuses the existing validated derived
encoder/decoder, preserving the `{ tag, fields }` error payload envelope inside
the Result `{ $, _0 }` envelope. No runtime source or compatibility codec was
added. The reviewed emitted snapshot shows both methods and typed child codecs.

The CLI traits fixture uses generic imported codec functions, an enum payload
with a custom array-based codec, and two equivalent groups in different orders.
Exact JSON output and cross-group round-trip equality pass. Missing Result
payloads, unknown error tags, wrong tag arity, and invalid custom payloads are
rejected; a nested Option error payload round-trips without losing Some(None).
This closes the specific structural-codec absence recorded below, not the
broader migration of nominal group derives, open-row codec composition, recursive
and stored codec contracts, or final packaging gates.

The generated-code audit reproduced exponential child-codec duplication:
Array nesting depths 2, 4, and 8 emitted 1,650, 6,770, and 133,362 bytes.
Both methods recursively emitted complete child dictionaries. Container and
structural error Json dictionaries now close over a shared descriptor thunk,
built directly as Oxc AST. Each child expression is emitted once; the thunk is
evaluated on method invocation, not dictionary initialization, preserving
recursive references without memoizing potentially captured evidence.
Regression tests count exactly one encode/decode kernel call per nesting level
for arrays and nested structural errors; a bounded output-size check also passes.
The reviewed structural-codec snapshot shrank from 123 emitted lines to 57.
The real CLI fixture additionally round-trips recursive generic error trees with
both nested Some(None) and custom-codec payloads through imported codec functions.
This addresses Json code-size growth, not an audit of every other dictionary
capability or a claim of stack-safe JSON encoding for arbitrarily deep values.

Validation after descriptor sharing: the full workspace `cargo test -- --quiet`
run passes, including doctests (three existing ignored doctests), 394 inference,
150 driver, 58 codegen, 52 kernel, and 12 CLI tests among the suites. Workspace
all-target/all-feature Clippy with denied warnings passes, as do formatting/diff
checks; no pending snapshots remain. Release packaging was not rerun for this
checkpoint.

## Confirmed failure and fix

`let value: Result[Number, [:invalid_json(String)]] = decode("\"text\"")`
compiled and returned Ok containing a JavaScript string. The same issue affected
String and Bool and propagated into container and derived decoders. Every
primitive Json instance selected `Intrinsic::JsonKernel`, whose decode method
was the unvalidated `$jsonDecode` / `JSON.parse` wrapper.

Primitive instance evidence now retains the requested primitive. Direct Oxc
dictionary generation calls `$jsonDecodePrimitive(text, kind)` and
`$jsonEncodePrimitive(value, kind)`. Recursive container dictionaries retain their
existing child-dictionary protocol. No generated JavaScript strings are parsed.

Codecs:

- Number, String, and Bool require matching parsed JSON primitive types.
- Number rejects parse overflow to infinity. Encoding non-finite numbers throws
  a TypeError: JSON has no numeric representation for these values, and silently
  producing null would not preserve the checked payload type.
- Unit uses JSON null and restores runtime undefined.
- BigInt uses a decimal JSON string. Numeric JSON inputs are rejected to avoid
  accepting values after precision was lost by JSON.parse. Fractional/exponent
  strings and leading-zero forms are rejected.

The runtime test covers valid round trips, wrong primitive kinds, parse errors,
overflow, malformed BigInt strings, and non-finite encoding. CLI regressions
exercise actual trait dispatch, nested array errors, derived record-payload
errors with source paths, and BigInt/unit round trips. The initial CLI mismatch
assertion failed before the fix.

## Module API and imported dictionaries

`json.encode` and `json.decode` now require `a: Json`. Their built-in module
exports call `$jsonEncodeWith` and `$jsonDecodeWith`, forwarding the selected
dictionary to its encode/decode method. The old unchecked kernel wrappers were
removed. These APIs therefore share the primitive encoding limitations above
and honor user-defined codecs instead of bypassing them.

The initial module-level wrong-payload CLI assertion failed before this fix.
Adding the bound also exposed imported direct dictionary calls requesting a
private `$v_` binding; foreign top-level references now import the public name.
CLI regressions exercise direct, first-class, and imported generic calls,
custom codecs, valid round trips, and nested invalid payloads. Full-solver tests
reject unsupported types and missing generic bounds. A reviewed, colorless
diagnostic snapshot labels the module decode reference and names its missing
instance. A follow-up CLI regression also exposed ordinary constrained extern
wrappers omitting their leading dictionary slots. Those adapters now consume
the slots and forward only source arguments to foreign JS, with an optional
trailing AbortSignal. Built-in Json exports instead use internal dictionary-aware
helpers. Wider foreign-boundary and codec audits remain open.

## Remaining work

### JSON envelope boundary review

The kernel rejects Result envelopes missing `_0` before invoking the payload
decoder, preserving its String input contract. Explicit null remains a present
payload. Derived JSON decoding accepts only own entries of the variant map,
so inherited names such as `constructor` and `toString` produce the same
path-qualified unknown-tag error as other unknown variants. Two granular kernel
tests cover these cases and a valid-tag control. All five JSON-filtered kernel
tests pass, as do formatting and strict Clippy; the preceding full workspace
and doctest pass covers this unchanged production source state. Record optional
presence semantics and the broader structural codec integration remain separate.

### Error payload equality

Structural error-row equality metadata must use the same tag names as runtime
values: `:payload:0`, not `payload:0`. The missing prefix skipped payload
comparisons and made unequal error messages compare equal, weakening earlier
JSON assertions. Direct AST lowering now includes the prefix. A source-aware
emission snapshot checks both payload slots; CLI regressions compare unequal
Number and String payloads, equal errors in both directions, and nested
Option[Result] values. This fix does not change the error-row representation or
introduce JavaScript source generation.
The emission snapshot and actual standalone CLI e2e test were rerun and pass.
JSON boundary checks were committed as `13e97ef`; broader row capabilities,
dictionary sharing, and optional record presence remain separate checkpoints.

### Result envelope validation checkpoint

The Result container decoder now rejects a missing `_0` field before invoking
the payload dictionary. Previously `JSON.stringify(parsed._0)` produced
JavaScript `undefined`, violating the child decoder's String parameter; a
permissive custom decoder could also accept the malformed envelope. Explicit
JSON null remains a present payload and is delegated to the child codec.
The permanent kernel regression covers both Ok and Err, asserts that a missing
payload never invokes the child codec, and verifies that present null reaches
the codec as a String. It failed before the fix; all 45 kernel tests pass after.

The initial end-to-end attempt using the then-derived
`Failure` error group in `tests/e2e/traits` was rejected before execution:

```alder
import json
let decoded: Result[Result[JsonSecret, Failure], [:invalid_json(String)]] =
    json.decode("{\"$\":\"Ok\"}")
```

The diagnostic reports missing `Json[[:first(Number) | :later]]`, required by
`Json[Result[JsonSecret, [:first(Number) | :later]]]`, despite `Failure` having
`#[derive(Show, Ord, Hash, Json)]`. Investigate error-group dictionary identity
and propagation into generic containers. Do not weaken the declared codec
bound or treat the kernel test as proof of module-level Result support.
The separate Result error-kind decision remains tracked in
`docs/result-error-kind-audit.md`.

Subsequent resolution: closed rows now supply conditional structural Json,
Show, Eq, and Hash without per-group derives. The CLI traits fixture passes
with the group derive removed, including nested Result decoding and custom
payload codecs. The nominal fallback described below is historical; no such
fallback is used to resolve closed-row capabilities.

Tracing this failure identifies the boundary: `Infer::from_ast` uses
`convert_ast_error_type` for Result's second argument, expanding a named group
to `Ty::ErrorRow`. Derived instance heads remain nominal, and `match_type`
matches named heads only against nominal goals. Structural Eq has its own row
rule, but Show/Ord/Hash/Json do not. Selecting an arbitrary named group with
matching tags would not be coherent: groups can declare the same tags in
different orders, while derived Ord and Hash retain declaration indices (Hash
also retains canonical type identity). The user approved canonical structural
row capabilities, conditional on payload capabilities, with no custom trait
implementations for individual named error groups and no blanket automatic
derivation. Named identity and declaration order must not affect behavior.
See `plans/hardening-language-decisions.md`; implementation remains pending.
No shortcut instance lookup was added.

### Unknown tags and error-equality checkpoint

Unknown JSON tags that coincide with Object.prototype properties previously
entered inherited values as if they were variant metadata, producing a generic
caught TypeError instead of the documented path-qualified unknown-tag error.
Derived JSON decoding now requires an own variant-map entry. A kernel
regression checks ordinary unknown names and `toString`, `constructor`,
`__proto__`, and `hasOwnProperty`, plus a valid-tag control.

The initial CLI regression unexpectedly passed. Inspecting its emitted AST
output exposed an independent error-equality bug: structural row field metadata
used `payload:0`, but runtime tags use `:payload`. The kernel skipped every
payload comparison, so different invalid_json messages compared equal. Codegen
now emits `:payload:0`, retaining the runtime's existing explicit metadata
protocol. Permanent CLI checks reject unequal first and second payloads,
compare equal independently constructed errors in both directions, and test
nested Option[Result] inequality. They failed before the metadata fix; after
that fix, the unknown-JSON-tag assertion failed as expected before the decoder
fix. This finding means earlier CLI assertions comparing Err payloads alone
were weaker evidence than intended; rerun affected suites after this correction.

### Option record fields

Optional declaration shorthand means an ordinary Option field. Omission and
explicit None are equivalent; construction and decoding materialize None rather
than preserving a missing-property state. Eq/Ord/Hash/Show visit every declared
field and use its dictionary. JSON encoding likewise includes every field.

Builtin Option JSON dictionaries carry `$option: true`. Derived record decoding
uses that codec identity to fill missing fields with None; a missing non-Option
field remains a path-specific error. No optional-field list appears in derived
variant descriptors. Generic fields instantiated with Option and transparent
aliases therefore receive the same default, independent of declaration syntax.
Nested Some(None) and Some(()) retain their normal Option JSON envelopes.

Current evidence includes all 54 kernel tests and the compiled `record_options`
fixture, covering omission/None equality, nested values, generic/alias decoding,
and required-field rejection. Broader fixture/snapshot migration remains open;
earlier full-workspace checkpoints predate these semantic changes.

- Audit container envelopes, optional/unit/nested Option representation, derived
  payloads, aliases, generic dictionaries, error groups, and round-trip behavior
  at the module API as well as trait calls.
- Review packaging and the final hardening acceptance matrix; this checkpoint
  does not establish full JSON or compiler soundness.
