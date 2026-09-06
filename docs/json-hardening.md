# Typed JSON hardening

Status: primitive trait codecs and module dictionary dispatch fixed; wider audit remains open.

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

`Json.encode` and `Json.decode` now require `a: Json`. Their built-in module
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

- Audit container envelopes, optional/unit/nested Option representation, derived
  payloads, aliases, generic dictionaries, error groups, and round-trip behavior
  at the module API as well as trait calls.
- Review packaging and the final hardening acceptance matrix; this checkpoint
  does not establish full JSON or compiler soundness.
