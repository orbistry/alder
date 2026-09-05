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
instance. These tests do not establish correctness of arbitrary constrained
extern wrappers, whose hidden dictionary argument handling needs a separate audit.

## Remaining work

- Audit container envelopes, optional/unit/nested Option representation, derived
  payloads, aliases, generic dictionaries, error groups, and round-trip behavior
  at the module API as well as trait calls.
- Review packaging and the final hardening acceptance matrix; this checkpoint
  does not establish full JSON or compiler soundness.
