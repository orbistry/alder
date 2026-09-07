# Array iterator progress

Status: implemented; final integrated release gates remain open.

The original `std/traits.ald` declared `Iterator[Array[a]]` with
`next(iterator: Array[a]) Option[a]`. Codegen selects `$arrayNext`, which returns
`$optionSome(values[0])` for a nonempty array without advancing anything.
The existing CLI test calls `next([7, 8])` only once, so it does not establish
iterator progress or exhaustion.

A bounded runtime probe of the current kernel called `$arrayNext` four times
on the same `[7, 8]` array and observed `[7, 7, 7, 7]`; the array remained
`[7, 8]`. An ordinary repeated-next iteration would never reach None.

The user approved the recommended separate iterator value. Advancing an iterator
must not consume elements from the source array. Independent iterators have
independent progress; do not give each array a hidden permanent cursor. Introduce
an explicit construction/state API and migrate the current Array instance.
Destructive `next(array)` consumption is not the chosen design.

## Implemented contract

`array.iter(values)` constructs a fresh opaque `ArrayIterator[a]`. The builtin
`Iterator[ArrayIterator[a]]` instance exposes `Item = a`; `next(iterator)` returns
the next value in Some or None on exhaustion. An alias to the same iterator
shares its progress. Separate calls to `array.iter` have independent cursors.
There is no longer a builtin Iterator instance on Array itself.

The kernel uses JavaScript's native array value iterator. It observes unread
element replacements and appended elements until exhaustion, then stays
exhausted even if the source grows. Shrinking the array can exhaust a cursor.
Elements remain shared references, not copies. This is a live iterator, not a
snapshot. Ordinary `for` lowering is unchanged. No Effect implementation was
adapted; this is a synchronous native-iterator adapter, not a scheduler change.

`$arrayIteratorNext` checks `done`, never the payload, and uses the centralized
Option constructor so nested None and unit elements remain distinguishable
from exhaustion. `$arrayNext` was removed, with no compatibility shim.

## Verification

- Two bounded kernel tests cover repeated next, exhaustion, iterator aliases,
  independent progress, unchanged source values, unit/nested Option payloads,
  live replacement/growth/shrinkage, and permanent exhaustion. They failed on
  the missing new helper before implementation and pass afterward; the original
  non-progress counterexample is recorded above.
- The actual imported `traits/array_iterators.ald` CLI fixture covers those
  public operations plus shared record mutation, first-class `next`, and a
  generic associated-type-constrained helper called across a module boundary.
- Solver tests check Item normalization, reject incompatible instantiation of
  an iterator over a shared mutable array, and allow independently typed cursors.
- All 17 packaged stdlib copies match; the inventory now has 51 kernel externs.
  Strict workspace Clippy, formatting, whitespace checks, and full workspace
  tests/doctests pass (72 kernel, 194 driver, 17 CLI, 11 mutation regressions).
  No pending snapshots remain. The first full run identified one additional
  old test call and an ambiguous unqualified Some in a fixture; both were
  corrected before the successful rerun. Validation covers the integrated
  hardening worktree; final packaging remains a separate gate.
