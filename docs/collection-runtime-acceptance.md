# Collection runtime contract audit

## JSON field-name boundary

The compiled `record_options/src/json_fields.ald` fixture exercises derived JSON
with fields named `constructor`, `toString`, and `hasOwnProperty`. Round trips
preserve their payloads, and an omitted optional `hasOwnProperty` field becomes
None rather than reading an inherited property. The dedicated
`option_record_defaults_execute` test and fresh packaged CLI execution pass.
Source review confirms decoding checks `Object.hasOwn` and uses
the declared field list. Alder identifiers cannot start with underscores, so
`__proto__` is not a source-level field name; this is not a claim about arbitrary
host objects supplied by externs. No production change was needed.

## Unary array callbacks

array.map/filter declarations take unary functions. The kernel previously
passed them directly to native JavaScript map/filter/flatMap, which supply
three arguments. This also affected the callback inside array applicative
application. Additional arguments are observable for functions returned from
externs: `numberParser()` returning JavaScript `parseInt`, declared in Alder as
`fn(String) Number`, produced incorrect results for `["10", "10", "10"]` because
the array index became the radix.

The kernel regression failed before the fix. The actual externs CLI fixture
also fails with the pre-fix packaged CLI in
`/tmp/alder-package-current.6ns25p/debug/alder`. That fixture obtains the function
through a local JS wrapper, not by injecting an untyped Alder callback.

The four adapters now call a unary closure from each native array operation.
This preserves native iteration/length/mutation behavior while supplying only
the declared element argument. The kernel test checks parseInt results and
the exact callback argument count across map, filter, flatMap, and apply. No
Effect implementation was needed or adapted; scheduler/task protocols are
unchanged.

The separate iteration regression exercises all four adapters with a callback
that updates an unvisited element and appends another. Callbacks observe the
updated element but do not extend iteration beyond the initial length. A thrown
sentinel stops later callbacks and propagates unchanged. All 56 integrated
kernel tests pass after adding this regression.

Validation after the fix: all 55 kernel tests and 14 CLI tests (including the
externs project), associated doctests, formatting, and strict all-target/
all-feature Clippy pass. The last release archives predate this kernel change.

## Source-reviewed baseline

- array.push, map.set, and set.add mutate the original collection and return
  JavaScript undefined, matching unit declarations.
- map.get checks membership separately and uses the centralized Option Some
  producer, distinguishing a present None payload from a missing key.
- Map/Set use native identity keys, as explicitly documented in language.md;
  structural Eq/Hash lookup is not part of their current contract.

These checks do not close the entire stdlib audit. Broader dictionary/JSON
capabilities, adversarial mutation interactions, and final committed-tree
validation remain tracked by the compiler-hardening plan.

## Compiled collection/Option boundary

`examples/record_options/src/collections.ald`, imported and called by the
fixture's main module, now executes the following through the CLI pipeline:

- Map lookup distinguishes a missing entry from a present None, Some payload,
  and unit. Overwriting an entry through an alias updates the original map.
- A record obtained from map.get retains its shared nested array; mutation
  through the retrieved record affects the original payload.
- Map and Set retain reference-identity keys: the original record key succeeds,
  while a newly constructed structurally equal key does not.
- map.set and set.add return unit; aliases retain the collection's identity.
- option.map returning None produces Some(None), and returning unit produces
  Some(()), rather than collapsing either to outer None.

The standalone end-to-end CLI test passes with these assertions, and formatting
and whitespace checks pass. No production change was required. These are
compiled-boundary checks in addition to the existing direct kernel Option tests.

## Primitive Hash superclass coherence

The numeric-law CLI regression exposed a codegen defect: `hash_equal(0, -0)`
failed in a generic function whose Eq evidence came from its Hash bound.
Primitive Hash dictionaries used `$equal`, whose Object.is behavior disagrees
with primitive `===` equality for signed zero and NaN. Hash normalization itself
was already correct. Bare primitive Hash evidence now emits strict equality in
its Eq superclass; container and derived dictionaries continue delegating to
selected child evidence. No runtime representation change is needed.

The cross-module CLI fixture checks equal hashes for signed zero through nested
Options, arrays, derived positional and record payloads, plus infinities and
other primitives. NaN remains unequal both directly and through Hash superclass
evidence; its canonical hash does not imply equality. Reviewed codegen snapshots
cover primitive, container, and structural-error dictionary emission.

Anonymous records do not currently have an automatic Hash instance; the record
payload test explicitly derives Hash on an enum. The audit also corrected stale
traits-internals prose that still described separate optional-field presence.

Validation after the superclass fix: full `cargo test` exits successfully,
including 67 codegen, 183 driver, 56 kernel, 456 solver integration, and 14 CLI
tests, plus doctests (two existing ignored examples). Strict all-target/all-feature
Clippy, formatting, and whitespace checks pass. All three affected codegen
snapshots were reviewed; no pending snapshots remain. Release packaging still
requires refreshing after this production change.

The self-contained six-file checkpoint is committed as `b8924f2`. Its new
`hash_equality` CLI fixture compares direct Eq evidence with inherited Hash
evidence across module boundaries, including every primitive Hash instance,
signed zero, infinities, NaN, nested Options/arrays, and a derived enum.
The pre-fix packaged CLI fails this fixture at its first signed-zero check;
the integrated compiler passes. The exact staged source tree (Git tree
`603c7fe47499f72a283cd8161af024860e7ff838`) was exported to
`/tmp/alder-hash-checkpoint.3hwM8n` and passed full workspace tests/doctests,
strict all-target/all-feature Clippy, and formatting with no pending snapshots.
The isolated tree has 13 CLI tests; the integrated worktree now has 15. This
checkpoint does not commit or require the other pending record/Option changes.

## Variable-length hash payloads

A 250 KB UTF-8 string (`"a😀"` repeated 50,000 times) reproduced a RangeError
in `pushText`: `bytes.push(...encoded)` creates one JavaScript call argument per
byte. String length was therefore accidentally bounded by the engine's argument
limit. Both variable-length byte append sites (text and BigInt magnitude) now
append iteratively. The fixed eight-byte Number append remains bounded.

The kernel string regression compares the result against an independent,
streaming implementation of the documented tagged, length-prefixed FNV-1a
format. A separate BigInt regression verifies positive and negative 256-byte
magnitudes against independently decoded hexadecimal bytes. The CLI Hash fixture
constructs a 320 KiB string with ordinary string.concat and hashes it directly
and inside Option through imported generic functions. No generated JavaScript
source construction or compiler ABI change is involved.

This preserves byte order and existing hash outputs; it does not claim streaming
memory usage, stack safety for deeply recursive values, or host fairness during
synchronous hashing. The pinned Effect reference in effects-internals was
revisited for scope: this byte-encoding fix does not change scheduler/task
invariants and adapts no Effect code. Final package archives predate this fix.

Validation: all 58 kernel tests and 15 CLI tests, associated doctests, strict
all-target/all-feature Clippy, formatting, and whitespace checks pass. No pending
snapshots remain. The subsequent full workspace test/doctest run also passed
(459 solver integration, 184 driver, 67 codegen, 58 kernel, and 15 CLI tests;
two existing doctest ignores). The four-file fix, regression tests, and Sampo
changeset are committed as `eaea2b7`. This is integrated-worktree validation,
not a claim that the remaining uncommitted hardening changes are release-ready.

## Independent array checkpoint verification

The six-file fix/test/documentation/changeset checkpoint was exported from the
Git index to `/tmp/alder-array-checkpoint.MxBNrT`. That isolated source tree
passed full workspace tests, strict all-target/all-feature Clippy, and formatting,
with no pending snapshots. Its 51 kernel and 12 CLI tests include these new
regressions; the integrated worktree has additional uncommitted hardening tests.
Other record, Option, and fiber changes were not required for this fix.
