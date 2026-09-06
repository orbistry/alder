# Stdlib declaration/runtime audit

Status: active. The Array Iterator progress defect remains unresolved pending
the API decision recorded in `plans/array-iterator-hardening.md`. This document
does not close the stdlib audit or the final compiler release gates.

## Inventory and linking

The workspace contains 16 builtin value modules plus `Traits.ald`. A read-only
inventory compared all 17 files byte-for-byte with `alder-can/stdlib`, checked
all 50 `#[extern("alder:kernel", ...)]` targets against kernel exports, and checked
their exported-name mappings in the bundle facades. All match. `Fiber.unbounded`
is the separate typed Number constant backed by `$fiberUnbounded = Infinity`.
The canonicalizer's permanent packaged-source/signature tests cover the copies
and type declarations; the separate unbounded test checks its monomorphic type.

`every_builtin_module_links_all_of_its_exports` retains every builtin namespace
in a bundler fixture and links the facades against the embedded kernel. It checks
actual linking, not merely occurrences of symbol names. It includes the empty
Http facade, which supplies no callable API. This uses ordinary test-fixture JS;
production Alder output still arrives as directly constructed Oxc ASTs.

## Reviewed boundaries

| Surface | Contract and evidence |
| --- | --- |
| Number, BigInt, String | Parse returns a payload or None; BigInt parsing preserves values above Number's exact integer range. String length counts code points, not UTF-16 units or grapheme clusters. Concat preserves the text. New scalar kernel and actual CLI checks cover these paths, including first-class Number.parse through Array.map. |
| Ref.same, Cli, Io | Ref.same uses strict JS equality without coercion. Cli.args exposes host-supplied strings (or an empty array without that host). The scalar runtime test passes nonempty Unicode arguments. Io.print delegates to console.log and returns unit; existing hello/externs CLI execution covers host printing. |
| Array, Map, Set, Option, Result | Collection mutation/unit returns, identity keys, unary callback arity, Option wrapping/unwrapping, and Result payload forwarding are mapped in `collection-runtime-acceptance.md` and `option-operation-acceptance.md`. |
| Json | Module encode/decode carry a leading selected Json dictionary and delegate to it. Primitive validation, envelopes, custom/container/derived codecs, Option defaults and structural errors are mapped in `json-hardening.md`, `option-operation-acceptance.md`, and `cyclic-values.md`. No unchecked JSON.parse public bypass remains. |
| Task, Fiber | Declarations return lazy Task values with the documented result layers. Traversals use the public options-record adapters, not the numeric internal adapters. Optional omission becomes None; the unbounded constant is a concurrency value. Lifecycle, callbacks, cancellation, fairness and result shapes are mapped in `async-hardening-acceptance.md`. |
| Ref, SynchronizedRef, Semaphore | Callback argument/result order matches the declarations: modify returns the first tuple component and stores the second. Ref callbacks are synchronous; synchronized callbacks return Tasks. Set/update return unit. Lazy allocation, aliasing, commit/cleanup behavior and permit validation have kernel and compiled tests mapped in the async acceptance document. |
| Traits | Canonical source headers and bootstrap parity enter ordinary instance selection. Primitive/container/structural payload evidence, higher-kinded adapters and their superclass slots remain part of the wider dictionary/codegen audit. The current Iterator[Array] intrinsic does not advance and is not accepted as complete. |

The new scalar kernel test checks successful/failed parses, exact BigInt values,
Unicode text, same/different object identity, heterogeneous primitives, signed
zero/NaN identity behavior, and actual host argument values. The externs CLI
fixture exercises the public compiled counterparts. Both pass without production
changes, as does the all-builtin linking test. Symbol/type inventories do not
prove behavioral correctness; the iterator counterexample demonstrates why the
runtime contract review remains necessary.

Validation: full workspace tests/doctests pass, including four bundler, 70 kernel,
193 driver and 17 CLI tests. Formatting, strict all-target/all-feature Clippy and
whitespace checks pass; no snapshots changed or remain pending. This is evidence
for the integrated worktree, not final clean-tree/package validation. No
production changes were made in this audit checkpoint.
