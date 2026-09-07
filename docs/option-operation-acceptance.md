# Option operation acceptance

This map describes the integrated hardening worktree, not an isolated commit or
final release gate. It closes the representation/operation audit in requirement
9; broader generic-contract and stdlib reviews remain separate.

## Representation and active paths

None is null. Some normally retains its payload directly; null and already
boxed payloads get a private registered box. Unit's undefined payload therefore
remains Some(unit), not None. Only `optionValue`/`$optionUnbox` unwrap these boxes;
user objects named Some are not interpreted as compiler boxes.

| Boundary | Active implementation and evidence |
| --- | --- |
| Constructors and patterns | Canonical builtin identity selects `$optionSome`, null, and one `$optionUnbox` per matched layer in `oxc_backend.rs`; nested extraction, first-class constructors, qualified constructors, and refutable bindings have source snapshots and `traits/option_constructors.ald` / `pattern_bindings` CLI coverage. |
| Eq | `$equalContainer` checks None before delegating exactly one unwrapped payload to the selected child. Structural records and derived enums retain child dictionaries; `option_equality_unwraps_nested_payloads` and `option_record_equality_uses_option_dictionaries` check both paths. |
| Show, Hash, Ord | Container adapters unwrap one layer and preserve selected payload operations. Hash/Ord superclass evidence comes from those same children. `option_operations_preserve_layers_and_payload_identity`, derived record tests, and compiled `hash_equality` / `traits` cases check nested values, unit, field indices, and custom dictionaries. |
| JSON | Option container codecs encode through the child codec, using an explicit JSON box for null or reserved-box payloads. Decoding reverses that rule. Primitive unit is null on the wire; missing optional derived fields become None through the Option dictionary marker. Nullable/nested/unit/reserved-box and derived-record tests cover these distinctions. |
| Functor / Applicative / Monad | Codegen selects `$optionMap`, `$optionPure`/`$optionApply`, and `$optionFlatMap`. Map/apply unwrap input payloads and wrap returned payloads; flatMap returns its callback's Option directly. No additional wrapping or flattening is inferred from JavaScript object shape. |
| Traversable / lookup | `$optionTraverse` uses the selected applicative's map/pure and wraps only the traversed payload. `$mapGet` tests membership before wrapping, preserving present None/unit. Existing kernel and `record_options/collections.ald` checks retain payload aliases. |
| Cyclic values | Active derived traversal, not erased Option delegation, detects recursive edges. `docs/cyclic-values.md` maps generic/mutual cycles, nested Options, custom Ord delegation, mutation, and exception cleanup. |

The source review includes constructor/pattern lowering, intrinsic and container
dictionary generation, and the corresponding kernel adapters. Representation
handling remains centralized; no new Option runtime representation was needed.

## Laws and their bounds

`nested_option_operations_obey_payload_laws_across_four_layers` checks a finite
matrix of None/Some values through four layers with Number (including both signed
zeros) and unit payloads. It verifies reflexivity/symmetry, equal hashes for equal
values, JSON decode/encode equality and hash preservation, map identity and
composition, applicative identity, flatMap right identity, and traverse identity.
The composition check changes the payload type by introducing extra Some layers.

The compiled `hash_equality/option_laws.ald` fixture independently tests generic
Hash/Json evidence, Eq obtained through Hash, nested Number/unit values, signed
zero, decode/encode round trips, and option.map identity. Existing numeric tests
cover infinities and NaN: NaN intentionally retains primitive non-reflexive Eq,
and non-finite Number JSON encoding is rejected rather than asserted to round-trip.

Existing derived positional/record enum tests cover equal-hash behavior, optional
field positions, custom payload dictionaries, and JSON round trips. Anonymous
records do not acquire Hash merely because the kernel has a raw-object fallback.
Map/Set continue using their documented native identity-key semantics.

These tests do not prove every algebraic law for arbitrary user instances or
every runtime value. Cyclic Hash/Ord/JSON behavior follows the approved defect
policy, not an invented graph format. Final committed-tree verification and
packaging remain required.
