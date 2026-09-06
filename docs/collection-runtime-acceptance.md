# Collection runtime contract audit

## Unary array callbacks

Array.map/filter declarations take unary functions. The kernel previously
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

- Array.push, Map.set, and Set.add mutate the original collection and return
  JavaScript undefined, matching unit declarations.
- Map.get checks membership separately and uses the centralized Option Some
  producer, distinguishing a present None payload from a missing key.
- Map/Set use native identity keys, as explicitly documented in language.md;
  structural Eq/Hash lookup is not part of their current contract.

These checks do not close the entire stdlib audit. Broader dictionary/JSON
capabilities, adversarial mutation interactions, and final committed-tree
validation remain tracked by the compiler-hardening plan.

## Independent checkpoint verification

The six-file fix/test/documentation/changeset checkpoint was exported from the
Git index to `/tmp/alder-array-checkpoint.MxBNrT`. That isolated source tree
passed full workspace tests, strict all-target/all-feature Clippy, and formatting,
with no pending snapshots. Its 51 kernel and 12 CLI tests include these new
regressions; the integrated worktree has additional uncommitted hardening tests.
Other record, Option, and fiber changes were not required for this fix.
