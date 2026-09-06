# Array iterator progress

Status: confirmed defect; awaiting the user-visible iterator-state decision.

`std/Traits.ald` declares `Iterator[Array[a]]` with
`next(iterator: Array[a]) Option[a]`. Codegen selects `$arrayNext`, which returns
`$optionSome(values[0])` for a nonempty array without advancing anything.
The existing CLI test calls `next([7, 8])` only once, so it does not establish
iterator progress or exhaustion.

A bounded runtime probe of the current kernel called `$arrayNext` four times
on the same `[7, 8]` array and observed `[7, 7, 7, 7]`; the array remained
`[7, 8]`. An ordinary repeated-next iteration would never reach None.

The public decision was requested during the stdlib audit:

- Recommended: introduce a separate iterator value so advancing it does not
  consume the source array. This needs an explicit construction/state API and
  migration of the current Array instance; do not silently give every array a
  hidden permanent cursor.
- Smaller alternative: retain `next(array)` and consume its first element on
  each call, with mutation visible through aliases.

Do not implement one policy as though it were already approved. This is a
remaining existing-stdlib correctness issue, not a waiver or a completed audit.
The compiler-hardening goal stays active while other independent work continues.

After the choice, add bounded repeated-next, exhaustion, aliasing, independent
iteration, Option/unit payload, and actual imported CLI regressions. Record
mutation/snapshot behavior explicitly and keep ordinary `for` loop semantics
separate from the trait's behavior. Update declarations, packaged copies,
kernel/codegen evidence, documentation, and changesets as needed; no legacy
compatibility path is required.
