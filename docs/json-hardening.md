# Typed JSON hardening

Status: primitive trait codecs fixed; module API and wider audit remain open.

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

## Remaining work

- `std/Json.ald` exports unconstrained extern encode/decode functions pointing
  directly to `$jsonEncode` and `$jsonDecode`. They bypass trait dictionaries and
  remain unsound. Require and forward the corresponding Json evidence; verify
  direct, indirect, generic, and imported module calls. A type bound without
  dictionary-based execution is insufficient.
- Validate missing-instance diagnostics and honor user-defined Json instances.
- Audit container envelopes, optional/unit/nested Option representation, derived
  payloads, aliases, generic dictionaries, error groups, and round-trip behavior
  at the module API as well as trait calls.
- Document public encoding limitations consistently when the module API is fixed.
- Review packaging and the final hardening acceptance matrix; this checkpoint
  does not establish full JSON or compiler soundness.
