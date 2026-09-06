//! The kernel is authored as TypeScript and embedded in every compiler build.

pub const KERNEL_SPECIFIER: &str = "alder:kernel";
pub const KERNEL_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/kernel.mjs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_kernel_exports_the_codegen_contract() {
        for symbol in [
            "$equal",
            "$equalDerived",
            "$equalContainer",
            "$equalStructural",
            "$show",
            "$showDerived",
            "$showContainer",
            "$compare",
            "$compareEnum",
            "$compareDerived",
            "$compareContainer",
            "$arrayApply",
            "$arrayFilter",
            "$optionFlatMap",
            "$resultPure",
            "$arrayTraverse",
            "$arrayIter",
            "$arrayIteratorNext",
            "$jsonEncodeDerived",
            "$jsonDecodeDerived",
            "$jsonEncodeContainer",
            "$jsonDecodeContainer",
            "$jsonEncodePrimitive",
            "$jsonDecodePrimitive",
            "$jsonEncodeWith",
            "$jsonDecodeWith",
            "$hash",
            "$hashDerived",
            "$hashErrorRow",
            "$hashContainer",
            "$refSame",
            "$matchFailure",
            "$optionBox",
            "$providerPush",
            "$registerTest",
            "$task",
            "$tryPromise",
            "$runTask",
            "$runMain",
            "$fiberFork",
            "$fiberJoin",
            "$fiberInterrupt",
            "$fiberMapWithOptions",
            "$fiberForEachWithOptions",
            "$fiberTryMapWithOptions",
            "$fiberTryForEachWithOptions",
            "$fiberAll",
            "$fiberRace",
            "$fiberScope",
            "$fiberAddFinalizer",
            "$fiberAddFinalizerExit",
            "$fiberUninterruptible",
        ] {
            assert!(KERNEL_JS.contains(&format!("export function {symbol}")));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn array_iterators_advance_independently_without_consuming_values() {
        let harness = indoc::indoc! {r#"
            const values = [7, 8];
            const first = $arrayIter(values);
            const alias = first;
            const second = $arrayIter(values);
            $assert($arrayIteratorNext(first) === 7);
            $assert($arrayIteratorNext(alias) === 8);
            $assert($arrayIteratorNext(first) === null);
            $assert($arrayIteratorNext(first) === null);
            $assert($arrayIteratorNext(second) === 7);
            $assert($arrayIteratorNext(second) === 8);
            $assert(values.length === 2 && values[0] === 7 && values[1] === 8);
            const units = $arrayIter([undefined]);
            $assert($arrayIteratorNext(units) !== null);
            $assert($arrayIteratorNext(units) === null);
            const options = $arrayIter([null, $optionSome(null), 42]);
            $assert(optionValue($arrayIteratorNext(options)) === null);
            $assert(optionValue(optionValue($arrayIteratorNext(options))) === null);
            $assert($arrayIteratorNext(options) === 42);
            $assert($arrayIteratorNext(options) === null);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            alder_runtime::execute(code, vec![]),
        )
        .await
        .expect("bounded iterator checks must terminate")
        .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn array_iterators_observe_live_values_and_stay_exhausted() {
        let harness = indoc::indoc! {r#"
            const values = [1, 2];
            const iterator = $arrayIter(values);
            $assert($arrayIteratorNext(iterator) === 1);
            values[1] = 20;
            values.push(30);
            $assert($arrayIteratorNext(iterator) === 20);
            $assert($arrayIteratorNext(iterator) === 30);
            $assert($arrayIteratorNext(iterator) === null);
            values.push(40);
            $assert($arrayIteratorNext(iterator) === null);
            const empty = [];
            const finished = $arrayIter(empty);
            $assert($arrayIteratorNext(finished) === null);
            empty.push(1);
            $assert($arrayIteratorNext(finished) === null);
            const shrinking = [1, 2];
            const cursor = $arrayIter(shrinking);
            $assert($arrayIteratorNext(cursor) === 1);
            shrinking.length = 0;
            $assert($arrayIteratorNext(cursor) === null);
            shrinking.push(3, 4);
            $assert($arrayIteratorNext(cursor) === null);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            alder_runtime::execute(code, vec![]),
        )
        .await
        .expect("bounded iterator mutation checks must terminate")
        .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scalar_stdlib_helpers_preserve_declared_value_shapes() {
        let harness = indoc::indoc! {r#"
            $assert($numberParse("42") === 42);
            $assert($numberParse("-4.5") === -4.5);
            $assert($numberParse("not a number") === null);
            $assert($bigIntParse("9007199254740993") === 9007199254740993n);
            $assert($bigIntParse("-42") === -42n);
            $assert($bigIntParse("4.5") === null);
            $assert($bigIntParse("not an integer") === null);
            $assert($stringLength("") === 0);
            $assert($stringLength("a😀e\u0301") === 4);
            $assert($stringConcat("left", "😀right") === "left😀right");
            const value = { items: [] };
            $assert($refSame(value, value));
            $assert(!$refSame(value, { items: [] }));
            $assert(!$refSame(42, "42"));
            $assert($refSame(0, -0));
            $assert(!$refSame(NaN, NaN));
            const args = $cliArgs();
            $assert(Array.isArray(args));
            $assert(args.length === 2 && args[0] === "first" && args[1] === "😀");
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, vec!["first".to_owned(), "😀".to_owned()]),
            )
            .await
            .expect("scalar stdlib checks must terminate")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn structural_cycle_guards_allow_shared_children_and_later_mutation() {
        let harness = indoc::indoc! {r#"
            for (const value of [[], {}]) {
                if (Array.isArray(value)) value.push(value);
                else value.self = value;
                $assert($show(value) === (Array.isArray(value) ? "[<cycle>]" : "{ self: <cycle> }"));
                let rejected = false;
                try { $hash(value); }
                catch (error) { rejected = error instanceof TypeError && error.message === "Hash: cyclic value"; }
                $assert(rejected);
                if (Array.isArray(value)) value[0] = 42;
                else value.self = 42;
                const copy = Array.isArray(value) ? [42] : { self: 42 };
                $assert($hash(value) === $hash(copy));
                $assert($show(value) === $show(copy));
                $assert($hash([value, value]) === $hash([value, copy]));
                $assert($show([value, value]) === $show([value, copy]));
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new()),
            )
            .await
            .expect("structural cycle checks must terminate")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cyclic_show_has_a_marker() {
        check_cyclic_derived_operation("Show").await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cyclic_hash_has_an_explicit_error() {
        check_cyclic_derived_operation("Hash").await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cyclic_ord_has_an_explicit_error() {
        check_cyclic_derived_operation("Ord").await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cyclic_json_has_an_explicit_error() {
        check_cyclic_derived_operation("JSON").await;
    }

    async fn check_cyclic_derived_operation(operation: &str) {
        let harness = indoc::indoc! {r#"
            let fail = false;
            const dictionary = {
                show: value => { if (fail && value.$ === "End") throw new Error("payload"); return $showDerived(value, variants); },
                hash: value => { if (fail && value.$ === "End") throw new Error("payload"); return $hashDerived(value, "Node", variants); },
                encode: value => { if (fail && value.$ === "End") throw new Error("payload"); return $jsonEncodeDerived(value, variants); },
                compare: (left, right) => {
                    if (fail && left.$ === "End") throw new Error("payload");
                    const result = $compareDerived(left, right, variants);
                    return { $: result < 0 ? "Less" : result > 0 ? "Greater" : "Equal" };
                }
            };
            const variants = {
                End: { fields: [], dictionaries: [] },
                Link: { fields: ["_0"], dictionaries: [dictionary] }
            };
            const run = value => operation === "Show" ? dictionary.show(value)
                : operation === "Hash" ? dictionary.hash(value)
                : operation === "JSON" ? dictionary.encode(value)
                : dictionary.compare(value, value);
            const end = { $: "End" };
            const value = { $: "Link", _0: end };
            const expected = run(value);
            value._0 = value;
            for (let attempt = 0; attempt < 2; attempt++) {
                if (operation === "Show") {
                    $assert(run(value) === "Link(<cycle>)");
                } else {
                    let error;
                    try { run(value); } catch (caught) { error = caught; }
                    $assert(error instanceof TypeError && error.message === `${operation}: cyclic value`);
                }
            }
            value._0 = end;
            $assert($equal(run(value), expected));
            fail = true;
            try { run(value); } catch (error) { $assert(error.message === "payload"); }
            fail = false;
            $assert($equal(run(value), expected));
            // Visiting the same child twice in distinct branches is not a cycle.
            variants.Pair = { fields: ["_0", "_1"], dictionaries: [dictionary, dictionary] };
            const shared = { $: "Pair", _0: value, _1: value };
            const copied = { $: "Pair", _0: value, _1: { $: "Link", _0: end } };
            $assert($equal(run(shared), run(copied)));
        "#};
        let code = format!("{KERNEL_JS}\nconst operation = {operation:?};\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new()),
            )
            .await
            .expect("cyclic operation must terminate")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ref_operations_are_lazy_reusable_and_commit_after_callback_success() {
        let harness = indoc::indoc! {r#"
            await $runTask($task(function* () {
                const make = $refMake(0);
                const ref = yield* make;
                const independent = yield* make;
                $assert(ref !== independent);
                let calls = 0;
                const update = $refUpdate(ref, (value) => { calls++; return value + 1; });
                $assert(calls === 0);
                yield* update;
                yield* update;
                $assert(calls === 2 && (yield* $refGet(ref)) === 2);
                $assert((yield* $refGet(independent)) === 0);
                const read = $refGet(ref);
                const write = $refSet(ref, 10);
                $assert((yield* read) === 2);
                yield* write;
                $assert((yield* read) === 10);
                const defect = new Error("transition failed");
                for (const operation of [$refUpdate, $refModify]) {
                    let caught = false;
                    try { yield* operation(ref, () => { throw defect; }); }
                    catch (error) { caught = error === defect; }
                    $assert(caught && (yield* read) === 10);
                }
                $assert((yield* $refModify(ref, (value) => ["previous: " + value, 42])) === "previous: 10");
                $assert((yield* read) === 42);
                const workers = Array.from({ length: 16 }, () => $task(function* () {
                    for (let index = 0; index < 100; index++) {
                        yield* $refUpdate(ref, (value) => value + 1);
                    }
                }));
                yield* $fiberAll(workers);
                $assert((yield* read) === 1642);
            }));
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ref_interruption_skips_pending_updates_and_preserves_committed_updates() {
        let harness = indoc::indoc! {r#"
            const ref = await $runTask($refMake(0));
            let callbacks = 0;
            const update = $refUpdate(ref, (value) => { callbacks++; return value + 1; });
            const pending = new FiberImpl(update).start();
            pending.interruptUnsafe();
            const skipped = await pending.awaitExit();
            $assert(skipped.$ === "Failure" && skipped.error instanceof Interrupted);
            $assert(callbacks === 0 && (await $runTask($refGet(ref))) === 0);

            let entered;
            const suspended = new Promise((resolve) => { entered = resolve; });
            let cleaned = 0;
            const committed = new FiberImpl($task(function* () {
                try {
                    yield* update;
                    yield* $tryPromise(() => {
                        entered();
                        return new Promise(() => {});
                    });
                } finally { cleaned++; }
            })).start();
            await suspended;
            committed.interruptUnsafe();
            committed.interruptUnsafe();
            const cancelled = await committed.awaitExit();
            $assert(cancelled.$ === "Failure" && cancelled.error instanceof Interrupted);
            $assert(callbacks === 1 && cleaned === 1);
            $assert((await $runTask($refGet(ref))) === 1);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        let execution = alder_runtime::execute(code, Vec::new());
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(10), execution)
                .await
                .expect("Ref interruption must complete")
                .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ref_preserves_payload_aliases_and_does_not_interpret_task_or_result_values() {
        let harness = indoc::indoc! {r#"
            const payload = [];
            const allocation = $refMake(payload);
            payload.push(1);
            const left = await $runTask(allocation);
            const right = await $runTask(allocation);
            $assert(left !== right);
            $assert((await $runTask($refGet(left))) === payload);
            const defect = new Error("after alias mutation");
            let caught = false;
            try {
                await $runTask($refUpdate(left, (value) => { value.push(2); throw defect; }));
            } catch (error) { caught = error === defect; }
            $assert(caught && (await $runTask($refGet(right))) === payload);
            $assert(payload.length === 2 && payload[1] === 2);
            let ran = false;
            const task = $task(function* () { ran = true; return 42; });
            const taskCell = await $runTask($refMake(task));
            $assert((await $runTask($refGet(taskCell))) === task && !ran);
            await $runTask($refUpdate(taskCell, () => task));
            $assert(!ran);
            const err = { $: "Err", _0: { $: ":expected" } };
            const resultCell = await $runTask($refMake(err));
            $assert((await $runTask($refGet(resultCell))) === err);
            $assert((await $runTask($refModify(resultCell, () => [err, err]))) === err);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn interrupted_scope_finishes_child_cleanup_before_resuming_caller() {
        let harness = indoc::indoc! {r#"
            let entered;
            const ready = new Promise((resolve) => { entered = resolve; });
            const events = [];
            const child = $task(function* () {
                try {
                    yield* $tryPromise(() => {
                        entered();
                        return new Promise(() => {});
                    });
                } finally {
                    yield* $tryPromise(() => Promise.resolve());
                    events.push("child cleanup");
                }
            });
            const parent = new FiberImpl($task(function* () {
                try { yield* $fiberScope(child); }
                finally { events.push("caller cleanup"); }
            })).start();
            await ready;
            parent.interruptUnsafe();
            const exit = await parent.awaitExit();
            $assert(exit.$ === "Failure" && exit.error instanceof Interrupted);
            $assert(events.join(",") === "child cleanup,caller cleanup");
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("scope cancellation must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn interrupted_combinators_join_cleanup_even_after_selecting_an_exit() {
        let harness = indoc::indoc! {r#"
            for (const kind of ["all", "race"]) {
                for (const selected of [false, true]) {
                    const events = [];
                    let entered, cleaning, release;
                    const ready = new Promise((resolve) => { entered = resolve; });
                    const cleanupStarted = new Promise((resolve) => { cleaning = resolve; });
                    const gate = new Promise((resolve) => { release = resolve; });
                    const child = $task(function* () {
                        try {
                            yield* $tryPromise(() => { entered(); return new Promise(() => {}); });
                        } finally {
                            yield* $tryPromise(() => { cleaning(); return gate; });
                            events.push("child cleanup");
                        }
                    });
                    const tasks = [child];
                    if (selected) tasks.push($task(function* () {
                        yield* $tryPromise(() => ready);
                        if (kind === "all") throw new Error("selected failure");
                        return "selected winner";
                    }));
                    const parent = new FiberImpl($task(function* () {
                        try { yield* (kind === "all" ? $fiberAll(tasks) : $fiberRace(tasks)); }
                        finally { events.push("caller cleanup"); }
                    })).start();
                    await ready;
                    if (selected) await cleanupStarted;
                    parent.interruptUnsafe();
                    parent.interruptUnsafe();
                    await cleanupStarted;
                    release();
                    const exit = await parent.awaitExit();
                    $assert(exit.$ === "Failure" && exit.error instanceof Interrupted);
                    $assert(events.join(",") === "child cleanup,caller cleanup");
                    $assert(parent.children.size === 0);
                }
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("combinator cancellation must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn interrupted_partial_construction_waits_for_owned_children() {
        let harness = indoc::indoc! {r#"
            for (const combine of [$fiberAll, $fiberRace]) {
                let cleaning, release;
                const cleanupStarted = new Promise((resolve) => { cleaning = resolve; });
                const gate = new Promise((resolve) => { release = resolve; });
                const events = [];
                let bodies = 0;
                const valid = $task(function* () { bodies++; });
                const invalid = $task(() => {
                    // White-box factory probe: attach a suspending finalizer to
                    // the already-owned, not-yet-started sibling.
                    const sibling = [...currentFiber.children][0];
                    sibling.scope.add(() => $task(function* () {
                        yield* $tryPromise(() => { cleaning(); return gate; });
                        events.push("child cleanup");
                    }));
                    throw new Error("factory failed");
                });
                const parent = new FiberImpl($task(function* () {
                    try { yield* combine([valid, invalid]); }
                    finally {
                        events.push("caller cleanup");
                        $assert(currentFiber.children.size === 0);
                    }
                })).start();
                await cleanupStarted;
                parent.interruptUnsafe();
                release();
                const exit = await parent.awaitExit();
                $assert(exit.$ === "Failure" && exit.error instanceof Interrupted);
                $assert(events.join(",") === "child cleanup,caller cleanup");
                $assert(bodies === 0);
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("partial construction cancellation must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn semaphore_bounds_work_and_releases_after_scoped_cleanup() {
        let harness = indoc::indoc! {r#"
            const make = $semaphoreMake(2);
            const semaphore = await $runTask(make);
            $assert(semaphore !== await $runTask(make));
            let active = 0, peak = 0, completed = 0;
            const tasks = Array.from({ length: 12 }, () => $semaphoreWithPermits(semaphore, 1,
                $task(function* () {
                    active++;
                    peak = Math.max(peak, active);
                    yield* $fiberAddFinalizer($task(function* () {
                        yield* $tryPromise(() => Promise.resolve());
                        active--;
                        completed++;
                    }));
                    yield* $tryPromise(() => Promise.resolve());
                    return 42;
                })));
            $assert(active === 0);
            const results = await $runTask($fiberAll(tasks));
            $assert(results.length === 12 && results.every((value) => value === 42));
            $assert(peak === 2 && active === 0 && completed === 12);
            $assert(semaphore.available === 2 && semaphore.waiters.size === 0);
            const defect = new Error("protected failure");
            let caught = false;
            try {
                await $runTask($semaphoreWithPermits(semaphore, 2,
                    $task(() => { throw defect; })));
            } catch (error) { caught = error === defect; }
            $assert(caught && semaphore.available === 2);
            const err = { $: "Err", _0: { $: ":expected" } };
            $assert(await $runTask($semaphoreWithPermits(semaphore, 2,
                $task(function* () { return err; }))) === err);
            for (const count of [0, -1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1]) {
                let invalid = false;
                try { await $runTask($semaphoreMake(count)); }
                catch (error) { invalid = error instanceof RangeError; }
                $assert(invalid);
            }
            for (const count of [0, -1, 1.5, 3, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1]) {
                let invalid = false, started = false;
                const protectedTask = $semaphoreWithPermits(semaphore, count,
                    $task(function* () { started = true; }));
                try { await $runTask(protectedTask); }
                catch (error) { invalid = error instanceof RangeError; }
                $assert(invalid && !started);
                $assert(semaphore.available === 2 && semaphore.waiters.size === 0);
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("semaphore test must complete")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn semaphore_cancellation_removes_waiters_and_recovers_unconsumed_grants() {
        let harness = indoc::indoc! {r#"
            for (const granted of [false, true]) {
                const semaphore = await $runTask($semaphoreMake(1));
                const holder = { permits: 1, acquired: false };
                semaphore.enqueue(holder, () => {});
                let started = 0;
                const waiting = new FiberImpl($semaphoreWithPermits(semaphore, 1,
                    $task(function* () { started++; }))).start();
                await $runTask($task(function* () {}));
                $assert(semaphore.waiters.size === 1);
                if (granted) semaphore.release(holder);
                waiting.interruptUnsafe();
                const exit = await waiting.awaitExit();
                $assert(exit.$ === "Failure" && exit.error instanceof Interrupted);
                $assert(started === 0 && semaphore.waiters.size === 0);
                if (!granted) semaphore.release(holder);
                $assert(semaphore.available === 1);
                $assert(await $runTask($semaphoreWithPermits(semaphore, 1,
                    $task(function* () { return 42; }))) === 42);
            }
            const semaphore = await $runTask($semaphoreMake(2));
            const holder = { permits: 1, acquired: false };
            semaphore.enqueue(holder, () => {});
            const large = { permits: 2, acquired: false };
            const small = { permits: 1, acquired: false };
            const grants = [];
            semaphore.enqueue(large, () => grants.push("large"));
            semaphore.enqueue(small, () => grants.push("small"));
            $assert(grants.length === 0);
            semaphore.release(large);
            $assert(grants.join(",") === "small");
            semaphore.release(small);
            semaphore.release(small);
            semaphore.release(holder);
            $assert(semaphore.available === 2 && semaphore.waiters.size === 0);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("semaphore cancellation must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn semaphore_holds_permits_during_cancelled_finalization() {
        let harness = indoc::indoc! {r#"
            const semaphore = await $runTask($semaphoreMake(1));
            let entered, cleaning, release;
            const ready = new Promise((resolve) => { entered = resolve; });
            const cleanupStarted = new Promise((resolve) => { cleaning = resolve; });
            const gate = new Promise((resolve) => { release = resolve; });
            const events = [];
            const holder = new FiberImpl($semaphoreWithPermits(semaphore, 1,
                $task(function* () {
                    yield* $fiberAddFinalizer($task(function* () {
                        yield* $tryPromise(() => { cleaning(); return gate; });
                        events.push("cleanup");
                    }));
                    yield* $tryPromise(() => { entered(); return new Promise(() => {}); });
                }))).start();
            await ready;
            const successor = new FiberImpl($semaphoreWithPermits(semaphore, 1,
                $task(function* () { events.push("successor"); return 42; }))).start();
            await $runTask($task(function* () {}));
            $assert(semaphore.waiters.size === 1);
            holder.interruptUnsafe();
            await cleanupStarted;
            holder.interruptUnsafe();
            $assert(semaphore.available === 0 && semaphore.waiters.size === 1);
            $assert(events.length === 0 && holder.exit === null && successor.exit === null);
            release();
            const exit = await holder.awaitExit();
            const next = await successor.awaitExit();
            $assert(exit.$ === "Failure" && exit.error instanceof Interrupted);
            $assert(next.$ === "Success" && next.value === 42);
            $assert(events.join(",") === "cleanup,successor");
            $assert(semaphore.available === 1 && semaphore.waiters.size === 0);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("semaphore finalization must complete")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn semaphore_joins_owned_child_cleanup_before_releasing_permits() {
        let harness = indoc::indoc! {r#"
            for (const interrupt of [false, true]) {
                const semaphore = await $runTask($semaphoreMake(1));
                let entered, cleaning, release;
                const ready = new Promise((resolve) => { entered = resolve; });
                const cleanupStarted = new Promise((resolve) => { cleaning = resolve; });
                const gate = new Promise((resolve) => { release = resolve; });
                const events = [];
                const holder = new FiberImpl($semaphoreWithPermits(semaphore, 1,
                    $task(function* () {
                        yield* $fiberFork($task(function* () {
                            try {
                                yield* $tryPromise(() => {
                                    entered();
                                    return new Promise(() => {});
                                });
                            } finally {
                                yield* $tryPromise(() => { cleaning(); return gate; });
                                events.push("child cleanup");
                            }
                        }));
                        yield* $tryPromise(() => ready);
                        if (interrupt) yield* $tryPromise(() => new Promise(() => {}));
                        return 7;
                    }))).start();
                await ready;
                const successor = new FiberImpl($semaphoreWithPermits(semaphore, 1,
                    $task(function* () { events.push("successor"); return 42; }))).start();
                await $runTask($task(function* () {}));
                if (interrupt) holder.interruptUnsafe();
                await cleanupStarted;
                $assert(semaphore.available === 0 && semaphore.waiters.size === 1);
                $assert(holder.exit === null && successor.exit === null && events.length === 0);
                release();
                const exit = await holder.awaitExit();
                const next = await successor.awaitExit();
                $assert(interrupt
                    ? exit.$ === "Failure" && exit.error instanceof Interrupted
                    : exit.$ === "Success" && exit.value === 7);
                $assert(next.$ === "Success" && next.value === 42);
                $assert(events.join(",") === "child cleanup,successor");
                $assert(semaphore.available === 1 && semaphore.waiters.size === 0);
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("semaphore child cleanup must complete")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn semaphore_handoffs_yield_to_host_and_preserve_provider_context() {
        let harness = indoc::indoc! {r#"
            const semaphore = await $runTask($semaphoreMake(1));
            let timerRan = false, operations = 0;
            const timer = setTimeout(() => { timerRan = true; }, 0);
            try {
                await $runTask($task(function* () {
                    $providerPush("service", "parent");
                    try {
                        const workers = Array.from({ length: 4 }, () => $task(function* () {
                            for (let index = 0; index < 512; index++) {
                                yield* $semaphoreWithPermits(semaphore, 1, $task(function* () {
                                    $assert($providerGet("service") === "parent");
                                    $providerPush("service", "protected");
                                    try {
                                        yield* $tryPromise(() => Promise.resolve());
                                        $assert($providerGet("service") === "protected");
                                    } finally { $providerPop("service"); }
                                    operations++;
                                    if (operations === 1024) $assert(timerRan);
                                }));
                                $assert($providerGet("service") === "parent");
                            }
                        }));
                        yield* $fiberAll(workers);
                        $assert($providerGet("service") === "parent");
                    } finally { $providerPop("service"); }
                }));
            } finally { clearTimeout(timer); }
            $assert(timerRan && operations === 2048);
            $assert(semaphore.available === 1 && semaphore.waiters.size === 0);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("semaphore handoffs must yield")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn synchronized_ref_serializes_suspended_updates_and_preserves_failure_state() {
        let harness = indoc::indoc! {r#"
            const make = $synchronizedRefMake(0);
            const cell = await $runTask(make);
            $assert(cell !== await $runTask(make));
            let calls = 0;
            const update = $synchronizedRefUpdate(cell, (value) => {
                calls++;
                return $task(function* () {
                    yield* $tryPromise(() => Promise.resolve());
                    return value + 1;
                });
            });
            $assert(calls === 0);
            await $runTask($fiberAll(Array.from({ length: 32 }, () => update)));
            $assert(calls === 32 && await $runTask($synchronizedRefGet(cell)) === 32);
            const defect = new Error("suspended transition failed");
            for (const transform of [
                () => { throw defect; },
                () => $task(function* () {
                    yield* $tryPromise(() => Promise.resolve());
                    throw defect;
                })
            ]) {
                let caught = false;
                try { await $runTask($synchronizedRefUpdate(cell, transform)); }
                catch (error) { caught = error === defect; }
                $assert(caught && await $runTask($synchronizedRefGet(cell)) === 32);
            }
            const err = { $: "Err", _0: { $: ":expected" } };
            $assert(await $runTask($synchronizedRefModify(cell,
                (value) => $task(function* () { return [err, value]; }))) === err);
            $assert(await $runTask($synchronizedRefGet(cell)) === 32);
            await $runTask($synchronizedRefSet(cell, 42));
            $assert(await $runTask($synchronizedRefGet(cell)) === 42);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("synchronized updates must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn synchronized_ref_cancellation_preserves_commit_boundary_and_write_order() {
        let harness = indoc::indoc! {r#"
            for (const committed of [false, true]) {
                const cell = await $runTask($synchronizedRefMake(1));
                let entered, cleaning, release;
                const ready = new Promise((resolve) => { entered = resolve; });
                const cleanupStarted = new Promise((resolve) => { cleaning = resolve; });
                const gate = new Promise((resolve) => { release = resolve; });
                let cleanupCount = 0;
                const writer = new FiberImpl($synchronizedRefUpdate(cell, (value) =>
                    $task(function* () {
                        yield* $fiberAddFinalizer($task(function* () {
                            yield* $tryPromise(() => { cleaning(); return gate; });
                            cleanupCount++;
                        }));
                        if (!committed) {
                            yield* $tryPromise(() => { entered(); return new Promise(() => {}); });
                        }
                        return value + 1;
                    }))).start();
                if (committed) await cleanupStarted;
                else await ready;
                // Reads do not wait for the write lock, even during cleanup.
                $assert(await $runTask($synchronizedRefGet(cell)) === (committed ? 2 : 1));
                const setter = new FiberImpl($synchronizedRefSet(cell, 10)).start();
                let observed;
                const successor = new FiberImpl($synchronizedRefUpdate(cell, (value) => {
                    observed = value;
                    return $task(function* () { return value + 1; });
                })).start();
                await $runTask($task(function* () {}));
                $assert(cell.semaphore.waiters.size === 2 && observed === undefined);
                writer.interruptUnsafe();
                await cleanupStarted;
                $assert(await $runTask($synchronizedRefGet(cell)) === (committed ? 2 : 1));
                $assert(observed === undefined && setter.exit === null);
                release();
                const exit = await writer.awaitExit();
                $assert(exit.$ === "Failure" && exit.error instanceof Interrupted);
                $assert((await setter.awaitExit()).$ === "Success");
                $assert((await successor.awaitExit()).$ === "Success");
                $assert(observed === 10 && cleanupCount === 1);
                $assert(await $runTask($synchronizedRefGet(cell)) === 11);
                $assert(cell.semaphore.available === 1 && cell.semaphore.waiters.size === 0);
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("synchronized cancellation must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn synchronized_ref_preserves_aliases_and_does_not_roll_back_cleanup_defects() {
        let harness = indoc::indoc! {r#"
            const payload = [];
            const allocation = $synchronizedRefMake(payload);
            const left = await $runTask(allocation);
            const right = await $runTask(allocation);
            const defect = new Error("aliased mutation");
            let caught = false;
            try {
                await $runTask($synchronizedRefUpdate(left, (value) => $task(function* () {
                    $assert((yield* $synchronizedRefGet(left)) === value);
                    value.push(42);
                    yield* $tryPromise(() => Promise.resolve());
                    throw defect;
                })));
            } catch (error) { caught = error === defect; }
            $assert(caught && (await $runTask($synchronizedRefGet(left))) === payload);
            $assert((await $runTask($synchronizedRefGet(right))) === payload && payload[0] === 42);
            const number = await $runTask($synchronizedRefMake(1));
            const cleanupDefect = new Error("after commit");
            caught = false;
            try {
                await $runTask($synchronizedRefUpdate(number, () => $task(function* () {
                    yield* $fiberAddFinalizer($task(function* () { throw cleanupDefect; }));
                    return 2;
                })));
            } catch (error) { caught = error === cleanupDefect; }
            $assert(caught && await $runTask($synchronizedRefGet(number)) === 2);
            await $runTask($synchronizedRefSet(number, 3));
            $assert(await $runTask($synchronizedRefGet(number)) === 3);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("synchronized alias probes must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn promise_mapper_failure_preserves_origin_cause_and_cleanup() {
        let harness = indoc::indoc! {r#"
            const cause = new Error("mapper failed");
            let mapped = 0, cleaned = 0, continued = false, defect;
            try {
                await $runTask($task(function* () {
                    try {
                        yield* $tryPromise(() => Promise.reject("foreign"), false,
                            "mapper-origin", error => {
                                $assert(error === "foreign");
                                mapped++;
                                throw cause;
                            });
                        continued = true;
                    } finally { cleaned++; }
                }));
            } catch (error) { defect = error; }
            $assert(defect?.name === "AlderForeignDefect");
            $assert(defect.origin === "mapper-origin" && defect.cause === cause);
            $assert(mapped === 1 && cleaned === 1 && !continued);
            $assert(await $runTask($task(function* () { return 42; })) === 42);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("mapper failure and cleanup must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn promise_then_getter_failure_is_a_defect_not_a_mapped_rejection() {
        let harness = indoc::indoc! {r#"
            const cause = new Error("then getter failed");
            let reads = 0, mapped = 0, cleaned = 0, defect;
            try {
                await $runTask($task(function* () {
                    try {
                        yield* $tryPromise(() => ({ get then() {
                            reads++;
                            throw cause;
                        }}), false, "getter-origin", error => {
                            mapped++;
                            return $resultErr(error);
                        });
                    } finally { cleaned++; }
                }));
            } catch (error) { defect = error; }
            $assert(defect?.name === "AlderForeignDefect");
            $assert(defect.origin === "getter-origin" && defect.cause === cause);
            $assert(reads === 1 && mapped === 0 && cleaned === 1);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("then getter failure must unwind cleanup")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_promise_does_not_run_a_late_rejection_mapper() {
        let harness = indoc::indoc! {r#"
            let reject, registered;
            const ready = new Promise(resolve => { registered = resolve; });
            let mapped = 0, cleaned = 0, continued = 0, aborted = 0;
            const fiber = new FiberImpl($task(function* () {
                try {
                    yield* $tryPromise(signal => {
                        signal.addEventListener("abort", () => { aborted++; });
                        const pending = new Promise((resolve, fail) => { reject = fail; });
                        registered();
                        return pending;
                    }, true, "cancelled mapper", error => {
                        mapped++;
                        return $resultErr(error);
                    });
                    continued++;
                } finally { cleaned++; }
            })).start();
            await ready;
            fiber.interruptUnsafe();
            const exit = await fiber.awaitExit();
            reject(new Error("late rejection"));
            await Promise.resolve();
            await Promise.resolve();
            $assert(exit.$ === "Failure" && exit.error === fiber.interruptError);
            $assert(mapped === 0 && continued === 0 && cleaned === 1 && aborted === 1);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("late rejection must be observed without running its mapper")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn promise_registration_reentrant_interruption_aborts_and_ignores_settlement() {
        let harness = indoc::indoc! {r#"
            for (const settles of ["resolve", "reject", "pending", "throw", "malformed"]) {
                let aborted = 0, cleaned = 0, mapped = 0, continued = false;
                const parent = new FiberImpl($task(function* () {
                    try {
                        yield* $tryPromise((signal) => {
                            signal.addEventListener("abort", () => { aborted++; });
                            currentFiber.interruptUnsafe();
                            if (settles === "throw") throw new Error("registration failed after interruption");
                            if (settles === "malformed") return 42;
                            if (settles === "resolve") return Promise.resolve(42);
                            if (settles === "reject") return Promise.reject(new Error("late"));
                            return new Promise(() => {});
                        }, true, "reentrant interruption", error => {
                            mapped++;
                            return $resultErr(error);
                        });
                        continued = true;
                    } finally { cleaned++; }
                })).start();
                const exit = await parent.awaitExit();
                await Promise.resolve();
                $assert(exit.$ === "Failure" && exit.error === parent.interruptError);
                $assert(aborted === 1 && cleaned === 1 && mapped === 0 && !continued);
                $assert(parent.children.size === 0);
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("registration interruption must not leave a waiter alive")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_plain_traversals_interrupt_siblings_before_defect_cleanup_finishes() {
        let harness = indoc::indoc! {r#"
            for (const traverse of [$fiberMap, $fiberForEach]) {
                let entered, stopped, release;
                const ready = new Promise((resolve) => { entered = resolve; });
                const interrupted = new Promise((resolve) => { stopped = resolve; });
                const gate = new Promise((resolve) => { release = resolve; });
                const defect = new Error("item failure");
                const calls = [];
                const parent = new FiberImpl(traverse([0, 1, 2, 3], (value) => {
                    calls.push(value);
                    return $task(function* () {
                        if (value === 0) {
                            try {
                                yield* $tryPromise(() => { entered(); return new Promise(() => {}); });
                            } finally { stopped(); }
                        }
                        yield* $tryPromise(() => ready);
                        yield* $fiberAddFinalizer($task(function* () {
                            yield* $tryPromise(() => gate);
                        }));
                        throw defect;
                    });
                }, 2)).start();
                await interrupted;
                $assert(parent.exit === null && calls.join(",") === "0,1");
                release();
                const exit = await parent.awaitExit();
                $assert(exit.$ === "Failure" && exit.error === defect);
                $assert(parent.children.size === 0);
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("defects must interrupt siblings before awaiting failing-item cleanup")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_try_map_interrupts_siblings_before_error_cleanup_finishes() {
        let harness = indoc::indoc! {r#"
            let release, started, stopped;
            const gate = new Promise((resolve) => { release = resolve; });
            const ready = new Promise((resolve) => { started = resolve; });
            const interrupted = new Promise((resolve) => { stopped = resolve; });
            const err = { $: "Err", _0: { $: ":failed" } };
            const calls = [];
            const parent = new FiberImpl($fiberTryMap([0, 1, 2, 3], (value) => {
                calls.push(value);
                return $task(function* () {
                    if (value === 0) {
                        try { yield* $tryPromise(() => { started(); return new Promise(() => {}); }); }
                        finally { stopped(); }
                    }
                    yield* $tryPromise(() => ready);
                    yield* $fiberAddFinalizer($task(function* () {
                        yield* $tryPromise(() => gate);
                    }));
                    return err;
                });
            }, 2)).start();
            await interrupted;
            $assert(parent.exit === null && calls.join(",") === "0,1");
            release();
            const exit = await parent.awaitExit();
            $assert(exit.$ === "Success" && exit.value === err);
            $assert(parent.children.size === 0);
            const mapped = await $runTask($fiberTryMap([3, 2, 1], (n) => $task(function* () {
                return { $: "Ok", _0: n * 2 };
            }), 2));
            $assert(mapped.$ === "Ok" && mapped._0.join(",") === "6,4,2");
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("typed failure must interrupt siblings and join cleanup")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_traversals_deliver_pending_interruption_after_the_mask_is_removed() {
        let harness = indoc::indoc! {r#"
            for (const traverse of [$fiberMap, $fiberForEach, $fiberTryMap, $fiberTryForEach]) {
                const typed = traverse === $fiberTryMap || traverse === $fiberTryForEach;
                let ready, release, calls = 0, cleaned = 0, inside = false, outside = false;
                const started = new Promise((resolve) => { ready = resolve; });
                const gate = new Promise((resolve) => { release = resolve; });
                const err = { $: "Err", _0: { $: ":expected" } };
                const parent = new FiberImpl($task(function* () {
                    yield* $fiberUninterruptible($task(function* () {
                        const value = yield* traverse([0, 1, 2, 3], () => $task(function* () {
                            calls++;
                            yield* $fiberAddFinalizer($task(function* () { cleaned++; }));
                            yield* $tryPromise(() => {
                                if (calls === 2) ready();
                                return gate;
                            });
                            return typed ? err : undefined;
                        }), 2);
                        if (typed) $assert(value === err);
                        $assert(cleaned === calls);
                        inside = true;
                    }));
                    outside = true;
                })).start();
                await started;
                parent.interruptUnsafe();
                $assert(parent.interruptMask === 1 && parent.exit === null);
                release();
                const exit = await parent.awaitExit();
                $assert(exit.$ === "Failure" && exit.error === parent.interruptError);
                $assert(inside && !outside && cleaned === (typed ? 2 : 4));
                $assert(parent.interruptMask === 0 && parent.children.size === 0);
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("masked traversal must eventually deliver pending interruption")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_typed_traversals_select_the_first_observed_error_not_input_order() {
        let harness = indoc::indoc! {r#"
            for (const traverse of [$fiberTryMap, $fiberTryForEach]) {
                for (const first of [0, 1]) {
                    let ready, cleaning, release, calls = 0;
                    const started = new Promise((resolve) => { ready = resolve; });
                    const cleanupStarted = new Promise((resolve) => { cleaning = resolve; });
                    const gate = new Promise((resolve) => { release = resolve; });
                    const settle = [];
                    const errors = [0, 1].map((n) => ({ $: "Err", _0: { $: ":failed", _0: n } }));
                    const parent = new FiberImpl(traverse([0, 1, 2, 3], (index) => {
                        calls++;
                        return $task(function* () {
                            if (index === first) yield* $fiberAddFinalizer($task(function* () {
                                yield* $tryPromise(() => { cleaning(); return gate; });
                            }));
                            return yield* $tryPromise(() => new Promise((resolve) => {
                                settle[index] = resolve;
                                if (settle[0] && settle[1]) ready();
                            }));
                        });
                    }, 2)).start();
                    await started;
                    settle[first](errors[first]);
                    settle[1 - first](errors[1 - first]);
                    await cleanupStarted;
                    $assert(parent.exit === null && calls === 2);
                    release();
                    const exit = await parent.awaitExit();
                    $assert(exit.$ === "Success" && exit.value === errors[first]);
                    $assert(parent.children.size === 0);
                }
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("competing errors must select once and join cleanup")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_typed_traversal_cancellation_during_selected_error_cleanup_wins() {
        let harness = indoc::indoc! {r#"
            for (const traverse of [$fiberTryMap, $fiberTryForEach]) {
                let release, cleaning;
                const gate = new Promise((resolve) => { release = resolve; });
                const ready = new Promise((resolve) => { cleaning = resolve; });
                const err = { $: "Err", _0: { $: ":failed" } };
                let cleaned = 0, calls = 0, observed = 0;
                const parent = new FiberImpl($task(function* () {
                    try {
                        return yield* traverse([0, 1, 2], () => {
                            calls++;
                            return $task(function* () {
                                yield* $fiberAddFinalizer($task(function* () {
                                    yield* $tryPromise(() => { cleaning(); return gate; });
                                    cleaned++;
                                }));
                                return err;
                            });
                        });
                    } finally { $assert(cleaned === 1); }
                })).start();
                parent.observe(() => { observed++; });
                await ready;
                parent.interruptUnsafe();
                parent.interruptUnsafe();
                for (let index = 0; index < 8; index++) await $runTask($task(function* () {}));
                $assert(parent.exit === null && cleaned === 0 && calls === 1);
                release();
                const exit = await parent.awaitExit();
                $assert(exit.$ === "Failure" && exit.error === parent.interruptError);
                $assert(cleaned === 1 && observed === 1 && parent.children.size === 0);
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("parent interruption must join selected-error cleanup")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_typed_traversal_keeps_defects_out_of_the_typed_error_channel() {
        let harness = indoc::indoc! {r#"
            for (const traverse of [$fiberTryMap, $fiberTryForEach]) {
                const err = { $: "Err", _0: { $: ":failed" } };
                const cleanupDefect = new Error("cleanup failed");
                const parent = new FiberImpl(traverse([0, 1], () => $task(function* () {
                    yield* $fiberAddFinalizer($task(function* () { throw cleanupDefect; }));
                    return err;
                }))).start();
                const exit = await parent.awaitExit();
                $assert(exit.$ === "Failure" && exit.error === cleanupDefect);
                // Falsy thrown values are still defects, not an absent failure.
                for (const defect of [undefined, null, false, 0]) {
                    let calls = 0;
                    const broken = new FiberImpl(traverse([0, 1, 2], () => {
                        calls++;
                        throw defect;
                    }, 2)).start();
                    const failed = await broken.awaitExit();
                    $assert(failed.$ === "Failure" && failed.error === defect);
                    $assert(calls === 1 && broken.children.size === 0);
                }
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("traversal defects must finish as runtime failures")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_typed_traversals_yield_to_host_timers() {
        let harness = indoc::indoc! {r#"
            for (const traverse of [$fiberTryMap, $fiberTryForEach]) {
                for (const suspend of [false, true]) {
                    let timerRan = false, calls = 0;
                    const timer = setTimeout(() => { timerRan = true; }, 0);
                    try {
                        const output = await $runTask(traverse(
                            Array.from({ length: 2048 }, (_, index) => index),
                            (value) => $task(function* () {
                                calls++;
                                if (calls === 1024) $assert(timerRan);
                                if (suspend) yield* $tryPromise(() => Promise.resolve());
                                return { $: "Ok", _0: traverse === $fiberTryMap ? value : undefined };
                            }), 3));
                        $assert(output.$ === "Ok" && calls === 2048 && timerRan);
                        if (traverse === $fiberTryMap) $assert(output._0[2047] === 2047);
                        else $assert(output._0 === undefined);
                    } finally { clearTimeout(timer); }
                }
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("typed traversal must yield to the host")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_traversals_isolate_each_items_provider_context() {
        let harness = indoc::indoc! {r#"
            for (const traverse of [$fiberMap, $fiberForEach, $fiberTryMap, $fiberTryForEach]) {
                let cleaned = 0;
                await $runTask($task(function* () {
                    $providerPush("service", "parent");
                    try {
                        yield* traverse([0, 1, 2, 3, 4, 5], (value) => $task(function* () {
                            $assert($providerGet("service") === "parent");
                            $providerPush("service", value);
                            yield* $fiberAddFinalizer($task(function* () {
                                $assert($providerGet("service") === value);
                                cleaned++;
                            }));
                            yield* $tryPromise(() => Promise.resolve());
                            $assert($providerGet("service") === value);
                            // Deliberately leave this context installed: the next
                            // item on this worker must still inherit the parent.
                            return traverse === $fiberTryMap || traverse === $fiberTryForEach
                                ? { $: "Ok", _0: undefined } : undefined;
                        }), 2);
                        $assert($providerGet("service") === "parent");
                    } finally { $providerPop("service"); }
                }));
                $assert(cleaned === 6);
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("item scopes must finish with isolated context")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_typed_traversals_preserve_cancellation_and_validate_results() {
        let harness = indoc::indoc! {r#"
            for (const traverse of [$fiberTryMap, $fiberTryForEach]) {
                let ready, calls = 0, cleaned = 0;
                const started = new Promise((resolve) => { ready = resolve; });
                const parent = new FiberImpl(traverse([1, 2, 3], () => $task(function* () {
                    calls++;
                    try {
                        yield* $tryPromise(() => {
                            if (calls === 2) ready();
                            return new Promise(() => {});
                        });
                    } finally {
                        yield* $tryPromise(() => Promise.resolve());
                        cleaned++;
                    }
                }), 2)).start();
                await started;
                parent.interruptUnsafe();
                const exit = await parent.awaitExit();
                $assert(exit.$ === "Failure" && exit.error === parent.interruptError);
                $assert(calls === 2 && cleaned === 2 && parent.children.size === 0);
                for (const malformed of [undefined, null, 42, { $: "Other" }]) {
                    let rejected = false;
                    try { await $runTask(traverse([0], () => $task(function* () { return malformed; }))); }
                    catch (error) { rejected = error instanceof TypeError; }
                    $assert(rejected);
                }
                const empty = await $runTask(traverse([], () => { throw new Error("unreachable"); }));
                $assert(empty.$ === "Ok");
            }
            const unit = await $runTask($fiberTryForEach([1, 2], () => $task(function* () {
                return { $: "Ok", _0: undefined };
            })));
            $assert(unit.$ === "Ok" && unit._0 === undefined);
            let rejected = false;
            try {
                await $runTask($fiberTryForEach([0], () => $task(function* () { return { $: "Ok", _0: 42 }; })));
            } catch (error) { rejected = error instanceof TypeError; }
            $assert(rejected);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("typed traversals must preserve cancellation")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_public_options_are_lazy_reusable_and_validated() {
        let harness = indoc::indoc! {r#"
            for (const traverse of [$fiberMapWithOptions, $fiberForEachWithOptions,
                                    $fiberTryMapWithOptions, $fiberTryForEachWithOptions]) {
                const typed = traverse === $fiberTryMapWithOptions || traverse === $fiberTryForEachWithOptions;
                const collect = traverse === $fiberMapWithOptions || traverse === $fiberTryMapWithOptions;
                let active = 0, peak = 0, calls = 0, reads = 0, limit = 0;
                const options = { get concurrency() { reads++; return limit; } };
                const callback = (value) => $task(function* () {
                    calls++; active++; peak = Math.max(peak, active);
                    yield* $fiberAddFinalizer($task(function* () { active--; }));
                    yield* $tryPromise(() => Promise.resolve());
                    const result = collect ? value * 2 : undefined;
                    return typed ? { $: "Ok", _0: result } : result;
                });
                const task = traverse([1, 2, 3], callback, $optionSome(options));
                $assert(reads === 0 && calls === 0);
                limit = 2;
                let result = await $runTask(task);
                if (typed) { $assert(result.$ === "Ok"); result = result._0; }
                $assert(collect ? result.join(",") === "2,4,6" : result === undefined);
                $assert(reads === 1 && calls === 3 && peak === 2 && active === 0);
                limit = 1; peak = 0;
                await $runTask(task);
                $assert(reads === 2 && calls === 6 && peak === 1 && active === 0);
                for (const missing of [undefined, null, { concurrency: null }]) {
                    peak = 0;
                    await $runTask(traverse([1, 2, 3], callback, missing));
                    $assert(peak === 1 && active === 0);
                }
                for (const invalid of [0, -1, 1.5, NaN, -Infinity, Number.MAX_SAFE_INTEGER + 1]) {
                    limit = invalid;
                    const before = calls;
                    let rejected = false;
                    try { await $runTask(task); }
                    catch (error) { rejected = error instanceof RangeError; }
                    $assert(rejected && calls === before);
                }
                $assert($fiberUnbounded === Infinity);
                limit = $fiberUnbounded; peak = 0;
                await $runTask(task);
                $assert(peak === 3 && active === 0);
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("public traversal adapters must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_traversals_snapshot_membership_but_preserve_payload_aliases() {
        let harness = indoc::indoc! {r#"
            for (const traverse of [$fiberMap, $fiberForEach, $fiberTryMap, $fiberTryForEach]) {
                for (const limit of [1, 2, Infinity]) {
                    const typed = traverse === $fiberTryMap || traverse === $fiberTryForEach;
                    const collect = traverse === $fiberMap || traverse === $fiberTryMap;
                    const first = { value: 1 }, second = { value: 2 };
                    const replacement = { value: 100 };
                    const input = [first, second];
                    let seen = [];
                    const task = traverse(input, (item) => $task(function* () {
                        seen.push(item);
                        if (item === first) {
                            input.splice(1, 1, replacement);
                            input.push({ value: 200 });
                            second.value = 42;
                        }
                        yield* $tryPromise(() => Promise.resolve());
                        const value = collect ? item : undefined;
                        return typed ? { $: "Ok", _0: value } : value;
                    }), limit);
                    $assert(seen.length === 0);
                    const firstOutcome = await $runTask(task);
                    const firstValues = typed ? firstOutcome._0 : firstOutcome;
                    if (typed) $assert(firstOutcome.$ === "Ok");
                    $assert(seen.length === 2 && seen[0] === first && seen[1] === second);
                    $assert(seen[1].value === 42);
                    if (collect) {
                        $assert(firstValues.length === 2);
                        $assert(firstValues[0] === first && firstValues[1] === second);
                    } else $assert(firstValues === undefined);

                    input.splice(0, input.length, replacement);
                    seen = [];
                    const secondOutcome = await $runTask(task);
                    const secondValues = typed ? secondOutcome._0 : secondOutcome;
                    if (typed) $assert(secondOutcome.$ === "Ok");
                    $assert(seen.length === 1 && seen[0] === replacement);
                    if (collect) {
                        $assert(secondValues.length === 1 && secondValues[0] === replacement);
                        $assert(firstValues !== secondValues && firstValues.length === 2);
                        second.value = 43;
                        $assert(firstValues[1].value === 43);
                    } else $assert(secondValues === undefined);
                }
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("reusable traversal must preserve shallow snapshot semantics")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_map_is_lazy_bounded_ordered_and_snapshots_each_execution() {
        let harness = indoc::indoc! {r#"
            const input = [1, 2, 3];
            let calls = 0, active = 0, peak = 0;
            const task = $fiberMap(input, (value) => {
                calls++;
                return $task(function* () {
                    active++;
                    peak = Math.max(peak, active);
                    yield* $fiberAddFinalizer($task(function* () { active--; }));
                    yield* $tryPromise(() => Promise.resolve());
                    input.push(99);
                    return value * 2;
                });
            }, 2);
            input[0] = 10;
            $assert(calls === 0);
            $assert((await $runTask(task)).join(",") === "20,4,6");
            $assert(calls === 3 && peak === 2 && active === 0);
            input.length = 1;
            $assert((await $runTask(task)).join(",") === "20");
            $assert(calls === 4 && active === 0);
            const err = { $: "Err", _0: { $: ":expected" } };
            const values = await $runTask($fiberMap([1, 2], () => $task(function* () { return err; })));
            $assert(values.length === 2 && values.every((value) => value === err));
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("bounded map must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_map_stops_callbacks_when_a_failure_is_waiting_for_cleanup() {
        let harness = indoc::indoc! {r#"
            let cleaning, release;
            const cleanupStarted = new Promise((resolve) => { cleaning = resolve; });
            const gate = new Promise((resolve) => { release = resolve; });
            const calls = [];
            const defect = new Error("callback failed");
            const parent = new FiberImpl($fiberMap([0, 1, 2, 3, 4, 5], (value) => {
                calls.push(value);
                if (value === 0) {
                    currentFiber.scope.add(() => $task(function* () {
                        yield* $tryPromise(() => { cleaning(); return gate; });
                    }));
                    throw defect;
                }
                return $task(function* () { return value; });
            }, 2)).start();
            await cleanupStarted;
            // Give runnable siblings bounded scheduler turns while failed-item
            // cleanup stays gated. No timing threshold controls the assertion.
            for (let index = 0; index < 16; index++) {
                await $runTask($task(function* () {}));
            }
            release();
            const exit = await parent.awaitExit();
            $assert(exit.$ === "Failure" && exit.error === defect);
            $assert(calls.every((value) => value < 2));
            $assert(parent.children.size === 0);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("failed traversal must clean up")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_map_cancellation_stops_work_and_joins_active_cleanup() {
        let harness = indoc::indoc! {r#"
            let ready;
            const started = new Promise((resolve) => { ready = resolve; });
            let callbacks = 0, cleaned = 0;
            const map = $fiberMap([0, 1, 2, 3, 4, 5], () => {
                callbacks++;
                return $task(function* () {
                    try {
                        yield* $tryPromise(() => {
                            if (callbacks === 2) ready();
                            return new Promise(() => {});
                        });
                    } finally {
                        yield* $tryPromise(() => Promise.resolve());
                        cleaned++;
                    }
                });
            }, 2);
            const parent = new FiberImpl($task(function* () {
                try { yield* map; }
                finally { $assert(cleaned === 2); }
            })).start();
            await started;
            parent.interruptUnsafe();
            parent.interruptUnsafe();
            const exit = await parent.awaitExit();
            $assert(exit.$ === "Failure" && exit.error instanceof Interrupted);
            $assert(callbacks === 2 && cleaned === 2 && parent.children.size === 0);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("map cancellation must clean up")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_map_keeps_input_order_when_later_items_finish_first() {
        let harness = indoc::indoc! {r#"
            const releases = [], starts = [];
            const ready = Array.from({ length: 4 }, (_, index) =>
                new Promise((resolve) => { starts[index] = resolve; }));
            const completed = [];
            const execution = $runTask($fiberMap([0, 1, 2, 3], (value) => $task(function* () {
                yield* $tryPromise(() => new Promise((resolve) => {
                    releases[value] = resolve;
                    starts[value]();
                }));
                completed.push(value);
                return value * 2;
            }), 2));
            await Promise.all([ready[0], ready[1]]);
            $assert(releases[2] === undefined && releases[3] === undefined);
            releases[1]();
            await ready[2];
            releases[2]();
            await ready[3];
            releases[3]();
            // Queue a host-level task behind the released item's resumption.
            for (let index = 0; index < 4; index++) await $runTask($task(function* () {}));
            releases[0]();
            const results = await execution;
            $assert(completed.join(",") === "1,2,3,0");
            $assert(results.join(",") === "0,2,4,6");
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("ordered traversal must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_map_validates_limits_and_defaults_to_sequential_execution() {
        let harness = indoc::indoc! {r#"
            let calls = 0;
            const callback = () => { calls++; return $task(function* () {}); };
            $assert((await $runTask($fiberMap([], callback))).length === 0 && calls === 0);
            for (const limit of [0, -1, 0.5, NaN, -Infinity, Number.MAX_SAFE_INTEGER + 1]) {
                let rejected = false;
                try { await $runTask($fiberMap([1], callback, limit)); }
                catch (error) { rejected = error instanceof RangeError; }
                $assert(rejected && calls === 0);
            }
            for (const limit of [undefined, Infinity]) {
                let active = 0, peak = 0;
                await $runTask($fiberMap([1, 2, 3], () => $task(function* () {
                    active++;
                    peak = Math.max(peak, active);
                    try { yield* $tryPromise(() => Promise.resolve()); }
                    finally { active--; }
                }), limit));
                $assert(active === 0 && peak === (limit === undefined ? 1 : 3));
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_map_immediate_work_yields_to_host_timers() {
        let harness = indoc::indoc! {r#"
            let timerRan = false, calls = 0;
            const timer = setTimeout(() => { timerRan = true; }, 0);
            try {
                const results = await $runTask($fiberMap(
                    Array.from({ length: 2048 }, (_, index) => index),
                    (value) => $task(function* () {
                        calls++;
                        if (calls === 1024) $assert(timerRan);
                        return value;
                    }), 3));
                $assert(results.length === 2048 && results[2047] === 2047 && timerRan);
            } finally { clearTimeout(timer); }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("traversal must yield to the host")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_for_each_is_bounded_unit_returning_and_does_not_discard_values() {
        let harness = indoc::indoc! {r#"
            let active = 0, peak = 0, cleaned = 0;
            const visited = [];
            const task = $fiberForEach([1, 2, 3, 4], (value) => $task(function* () {
                active++;
                peak = Math.max(peak, active);
                yield* $fiberAddFinalizer($task(function* () { active--; cleaned++; }));
                yield* $tryPromise(() => Promise.resolve());
                visited.push(value);
            }), 2);
            $assert(visited.length === 0);
            $assert(await $runTask(task) === undefined);
            $assert(visited.length === 4 && active === 0 && peak === 2 && cleaned === 4);
            $assert(await $runTask($fiberForEach([], () => { throw new Error("empty callback"); })) === undefined);
            for (const value of [42, null, { $: "Err", _0: { $: ":error" } }]) {
                let rejected = false;
                try { await $runTask($fiberForEach([1], () => $task(function* () { return value; }))); }
                catch (error) { rejected = error instanceof TypeError; }
                $assert(rejected);
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new())
            )
            .await
            .expect("unit traversal must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn derived_equality_handles_cycles_without_skipping_payloads() {
        let harness = indoc::indoc! {r#"
            const number = { eq: (a, b) => a === b };
            const node = { eq: (a, b) => $equalDerived(a, b, {
                Link: { fields: ["_0", "_1"], dictionaries: [children, number] },
            }, "probe::Node") };
            const children = { eq: (a, b) => $equalContainer(a, b, "array", [node]) };
            const make = value => {
                const result = { $: "Link", _0: [], _1: value };
                result._0.push(result);
                return result;
            };
            const left = make(42), right = make(42), wrong = make(7);
            $assert(node.eq(left, left));
            $assert(node.eq(left, right) && node.eq(right, left));
            $assert(!node.eq(left, wrong) && !node.eq(wrong, left));
            const nan = make(NaN);
            $assert(!node.eq(nan, nan));
            right._1 = 7;
            $assert(!node.eq(left, right));
            right._1 = 42;
            $assert(node.eq(left, right));
            const payloadFailure = new Error("payload failure");
            number.eq = () => { throw payloadFailure; };
            let caught;
            try { node.eq(left, wrong); } catch (error) { caught = error; }
            $assert(caught === payloadFailure);
            number.eq = (a, b) => a === b;
            $assert(!node.eq(left, wrong));
            const innerShape = { Link: { fields: ["_1"], dictionaries: [number] } };
            const outerShape = { Link: { fields: ["_1"], dictionaries: [{
                eq: () => $equalDerived(left, wrong, innerShape, "probe::Other"),
            }] } };
            $assert(!$equalDerived(left, wrong, outerShape, "probe::Node"));
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn option_record_equality_uses_option_dictionaries() {
        let harness = indoc::indoc! {r#"
            const child = { eq: (left, right) => {
                $assert(Array.isArray(left) && Array.isArray(right));
                return $equal(left, right);
            }};
            const option = { eq: (left, right) => $equalContainer(left, right, "option", [child]) };
            const variants = { Value: { record: true, fields: ["items"], dictionaries: [option] } };
            for (const equal of [
                (left, right) => $equalStructural(left, right, "record", ["items"], [option]),
                (left, right) => $equalDerived(left, right, variants, "probe::Value"),
            ]) {
                const absent = { $: "Value", items: null }, empty = { $: "Value", items: [] };
                $assert(equal(absent, absent) && equal(absent, { $: "Value", items: null }));
                $assert(!equal(absent, empty) && !equal(empty, absent));
                $assert(equal(empty, { $: "Value", items: [] }));
                $assert(!equal(empty, { $: "Value", items: [1] }));
            }
            const unit = { eq: (left, right) => left === right };
            $assert($equalStructural({ value: undefined }, { value: undefined }, "record", ["value"], [unit]));
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn option_derived_ordering_delegates_to_field_dictionaries() {
        let harness = indoc::indoc! {r#"
            const variants = { Value: { record: true, fields: ["value"], dictionaries: [{ compare: (a, b) => {
                if (a === null || b === null) return { $: a === b ? "Equal" : a === null ? "Less" : "Greater" };
                $assert(typeof a === "number" && typeof b === "number");
                return { $: a < b ? "Less" : a > b ? "Greater" : "Equal" };
            }}] } };
            const absent = { $: "Value", value: null }, present = { $: "Value", value: 1 };
            $assert($compareDerived(absent, { $: "Value", value: null }, variants) === 0);
            $assert($compareDerived(absent, present, variants) === -1);
            $assert($compareDerived(present, absent, variants) === 1);
            $assert($compareDerived(present, { $: "Value", value: 2 }, variants) === -1);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_runner_counts_failures_and_continues_after_async_exits() {
        let harness = indoc::indoc! {r#"
            const events = [], reports = [];
            $registerTest("probe", "sync", () => {
                events.push("sync");
                return { $: "Err", _0: "sync failure" };
            });
            $registerTest("probe", "typed", () => $task(function* () {
                yield* $tryPromise(() => Promise.resolve());
                events.push("typed");
                return { $: "Err", _0: "async failure" };
            }));
            $registerTest("probe", "defect", () => $task(function* () {
                yield* $tryPromise(() => Promise.resolve());
                events.push("defect");
                throw new Error("async defect");
            }));
            $registerTest("probe", "success", () => $task(function* () {
                yield* $tryPromise(() => Promise.resolve());
                events.push("success");
            }));
            $assert(await $runTests(message => reports.push(message)) === 3);
            $assert(JSON.stringify(events) === '["sync","typed","defect","success"]');
            $assert(reports.length === 5);
            $assert(reports[0].startsWith("fail probe — sync"));
            $assert(reports[1].startsWith("fail probe — typed"));
            $assert(reports[2].startsWith("fail probe — defect"));
            $assert(reports[3] === "pass probe — success");
            $assert(reports[4] === "\n1 passed; 3 failed");
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn large_string_hash_preserves_the_utf8_byte_stream() {
        let harness = indoc::indoc! {r#"
            const value = "a😀".repeat(50000);
            const encoded = new TextEncoder().encode(value);
            let expected = 14695981039346656037n;
            const feed = byte => {
                expected = ((expected ^ BigInt(byte)) * 1099511628211n) & 0xffffffffffffffffn;
            };
            feed(0x04);
            for (let shift = 0n; shift < 64n; shift += 8n) {
                feed(Number((BigInt(encoded.length) >> shift) & 0xffn));
            }
            for (const byte of encoded) feed(byte);
            $assert($hash(value) === expected);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bigint_hash_preserves_signed_magnitude_bytes() {
        let harness = indoc::indoc! {r#"
            const hex = "fedcba9876543210".repeat(32);
            const magnitude = BigInt(`0x${hex}`);
            for (const sign of [0, 1]) {
                let expected = 14695981039346656037n;
                const feed = byte => {
                    expected = ((expected ^ BigInt(byte)) * 1099511628211n) & 0xffffffffffffffffn;
                };
                feed(0x03);
                feed(sign);
                for (let shift = 0n; shift < 64n; shift += 8n) {
                    feed(Number((BigInt(hex.length / 2) >> shift) & 0xffn));
                }
                for (let index = 0; index < hex.length; index += 2) {
                    feed(parseInt(hex.slice(index, index + 2), 16));
                }
                $assert($hash(sign === 0 ? magnitude : -magnitude) === expected);
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn structural_error_hash_ignores_row_width_and_declaration_order() {
        let harness = indoc::indoc! {r#"
            let calls = 0;
            const child = { hash: value => { calls++; return $hash(value % 10); } };
            const failed = { fields: ["_0"], dictionaries: [child] };
            const empty = { fields: [], dictionaries: [] };
            const narrow = { ":failed": failed };
            const wide = { ":absent": empty, ":failed": failed, ":other": empty };
            const reordered = { ":other": empty, ":failed": failed, ":absent": empty };
            const value = { $: ":failed", _0: 1 };
            const expected = $hashErrorRow(value, narrow);
            $assert($hashErrorRow(value, wide) === expected);
            $assert($hashErrorRow(value, reordered) === expected);
            $assert($hashErrorRow({ $: ":failed", _0: 11 }, wide) === expected);
            $assert(calls === 4);
            $assert($hashErrorRow({ $: ":absent" }, wide) === $hashErrorRow({ $: ":absent" }, reordered));
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn option_derived_hash_includes_none_and_retains_field_indices() {
        let harness = indoc::indoc! {r#"
            const payload = { hash: value => { $assert(Array.isArray(value)); return $hash(value); } };
            const child = { hash: value => $hashContainer(value, "option", [payload]) };
            const variants = { Value: { record: true, fields: ["left", "right"], dictionaries: [child, child] } };
            const hash = value => $hashDerived(value, "Probe", variants);
            $assert(hash({ $: "Value", left: null, right: null }) === hash({ $: "Value", left: null, right: null }));
            $assert(hash({ $: "Value", left: [], right: null }) === hash({ $: "Value", left: [], right: null }));
            $assert(hash({ $: "Value", left: null, right: null }) !== hash({ $: "Value", left: [], right: null }));
            $assert(hash({ $: "Value", left: [], right: null }) !== hash({ $: "Value", left: null, right: [] }));
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn option_derived_json_defaults_missing_fields_and_preserves_nested_values() {
        let harness = indoc::indoc! {r#"
            const number = { encode: value => $jsonEncodePrimitive(value, "number"), decode: text => $jsonDecodePrimitive(text, "number") };
            const optionOf = child => ({ $option: true, encode: value => $jsonEncodeContainer(value, "option", [child]), decode: text => $jsonDecodeContainer(text, "option", [child]) });
            const option = optionOf(optionOf(number));
            const unit = { encode: value => $jsonEncodePrimitive(value, "unit"), decode: text => $jsonDecodePrimitive(text, "unit") };
            const variants = { Value: { record: true, fields: ["nested", "unit"], dictionaries: [option, optionOf(unit)] } };
            const missing = $jsonDecodeDerived('{"tag":"Value","value":{}}', variants);
            $assert(missing.$ === "Ok" && missing._0.nested === null && missing._0.unit === null);
            for (const value of [{ $: "Value", nested: null, unit: null }, { $: "Value", nested: $optionSome(null), unit: undefined }]) {
                const decoded = $jsonDecodeDerived($jsonEncodeDerived(value, variants), variants);
                $assert(decoded.$ === "Ok");
                for (const field of ["nested", "unit"]) {
                    $assert(Object.hasOwn(decoded._0, field));
                }
                $assert($jsonEncodeDerived(value, variants) === $jsonEncodeDerived(decoded._0, variants));
            }
            const required = { Value: { record: true, fields: ["unit"], dictionaries: [unit] } };
            const rejected = $jsonDecodeDerived('{"tag":"Value","value":{}}', required);
            $assert(rejected.$ === "Err" && rejected._0._0 === "$.value.unit: missing field");
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn derived_json_rejects_inherited_variant_names_at_tag_path() {
        let harness = indoc::indoc! {r#"
            const variants = { Known: { record: false, fields: [] } };
            for (const tag of ["missing", "toString", "constructor", "__proto__", "hasOwnProperty"]) {
                const text = JSON.stringify({ tag, fields: [] });
                const decoded = $jsonDecodeDerived(text, variants);
                $assert(decoded.$ === "Err" && decoded._0.$ === ":invalid_json");
                $assert(decoded._0._0 === `$.tag: unknown variant ${JSON.stringify(tag)}`);
            }
            $assert($jsonDecodeDerived('{"tag":"Known","fields":[]}', variants)._0.$ === "Known");
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn result_json_requires_payload_before_calling_child_decoder() {
        let harness = indoc::indoc! {r#"
            let calls = 0;
            const child = { decode: text => {
                calls++;
                $assert(typeof text === "string");
                return $resultOk(undefined);
            }};
            for (const tag of ["Ok", "Err"]) {
                const missing = $jsonDecodeContainer(JSON.stringify({ $: tag }), "result", [child, child]);
                $assert(missing.$ === "Err" && missing._0.$ === ":invalid_json");
                $assert(calls === 0);
            }
            for (const tag of ["Ok", "Err"]) {
                const present = $jsonDecodeContainer(JSON.stringify({ $: tag, _0: null }), "result", [child, child]);
                $assert(present.$ === "Ok" && present._0.$ === tag && present._0._0 === undefined);
            }
            $assert(calls === 2);
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn primitive_json_codecs_validate_types_and_round_trip() {
        let harness = indoc::indoc! {r#"
            for (const [value, kind] of [[42, "number"], ["text", "string"], [true, "boolean"],
                [undefined, "unit"], [123456789012345678901234567890n, "bigint"]]) {
                const result = $jsonDecodePrimitive($jsonEncodePrimitive(value, kind), kind);
                $assert(result.$ === "Ok" && result._0 === value);
            }
            for (const [text, kind] of [['"bad"', "number"], ["1e400", "number"], ["null", "number"],
                ["42", "string"], ["[]", "boolean"], ["false", "unit"], ["42", "bigint"],
                ['"01"', "bigint"], ['"1.5"', "bigint"], ['"1e3"', "bigint"], ["{", "number"]]) {
                const result = $jsonDecodePrimitive(text, kind);
                $assert(result.$ === "Err" && result._0.$ === ":invalid_json");
            }
            for (const value of [NaN, Infinity, -Infinity]) {
                let rejected = false;
                try { $jsonEncodePrimitive(value, "number"); } catch { rejected = true; }
                $assert(rejected);
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn nested_option_operations_obey_payload_laws_across_four_layers() {
        let harness = indoc::indoc! {r#"
            const option = child => ({
                eq: (a, b) => $equalContainer(a, b, "option", [child]),
                hash: value => $hashContainer(value, "option", [child]),
                encode: value => $jsonEncodeContainer(value, "option", [child]),
                decode: value => $jsonDecodeContainer(value, "option", [child]),
            });
            const identity = { pure: value => ({ value }), $super0: { map: (box, f) => ({ value: f(box.value) }) } };
            for (const [kind, payloads] of [["number", [-0, 0, 1, -1]], ["unit", [undefined]]]) {
                let dictionary = {
                    eq: (a, b) => a === b,
                    hash: $hash,
                    encode: value => $jsonEncodePrimitive(value, kind),
                    decode: text => $jsonDecodePrimitive(text, kind),
                };
                let values = payloads;
                for (let depth = 1; depth <= 4; depth++) {
                    dictionary = option(dictionary);
                    values = [null, ...values.map(value => $optionSome(value))];
                    for (const value of values) {
                        $assert(dictionary.eq(value, value));
                        const decoded = dictionary.decode(dictionary.encode(value));
                        $assert(decoded.$ === "Ok" && dictionary.eq(value, decoded._0));
                        $assert(dictionary.hash(value) === dictionary.hash(decoded._0));
                        $assert(dictionary.eq($optionMap(value, x => x), value));
                        $assert(dictionary.eq($optionFlatMap(value, $optionSome), value));
                        $assert(dictionary.eq($optionApply($optionSome(x => x), value), value));
                        $assert(dictionary.eq($optionTraverse(identity, value, identity.pure).value, value));
                        const twice = option(option(dictionary));
                        $assert(twice.eq(
                            $optionMap($optionMap(value, $optionSome), $optionSome),
                            $optionMap(value, x => $optionSome($optionSome(x)))
                        ));
                        for (const other of values) {
                            $assert(dictionary.eq(value, other) === dictionary.eq(other, value));
                            if (dictionary.eq(value, other)) {
                                $assert(dictionary.hash(value) === dictionary.hash(other));
                            }
                        }
                    }
                }
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                alder_runtime::execute(code, Vec::new()),
            )
            .await
            .expect("bounded Option law matrix must finish")
            .unwrap(),
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn option_equality_unwraps_nested_payloads() {
        let harness = r#"
const inner = { eq: (a, b) => $equalContainer(a, b, "option", [{ eq: (x, y) => x === y }]) };
const left = $optionSome($optionNone());
const right = $optionSome($optionNone());
$assert($equalContainer(left, right, "option", [inner]));
$assert($equalContainer(right, left, "option", [inner]));
$assert(!$equalContainer(left, null, "option", [inner]));
"#;
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn array_callbacks_receive_only_the_declared_value_argument() {
        let harness = indoc::indoc! {r#"
            const values = ["10", "10", "10"];
            $assert($equal($arrayMap(values, parseInt), [10, 10, 10]));
            const observed = [];
            function unary(value) {
                observed.push(arguments.length);
                return value;
            }
            $assert($equal($arrayMap([1, 2], unary), [1, 2]));
            $assert($equal($arrayFilter([1, 2], unary), [1, 2]));
            $assert($equal($arrayFlatMap([1, 2], function(value) {
                observed.push(arguments.length);
                return [value];
            }), [1, 2]));
            $assert($equal($arrayApply([unary], [1, 2]), [1, 2]));
            $assert($equal(observed, [1, 1, 1, 1, 1, 1, 1, 1]));
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unary_array_adapters_preserve_iteration_and_exceptions() {
        let harness = indoc::indoc! {r#"
            const operations = [
                (values, callback) => $arrayMap(values, callback),
                (values, callback) => $arrayFilter(values, callback),
                (values, callback) => $arrayFlatMap(values, value => [callback(value)]),
                (values, callback) => $arrayApply([callback], values),
            ];
            for (const operation of operations) {
                const values = [1, 2];
                const visited = [];
                const result = operation(values, value => {
                    visited.push(value);
                    if (value === 1) {
                        values[1] = 4;
                        values.push(3);
                    }
                    return value;
                });
                $assert($equal(result, [1, 4]));
                $assert($equal(visited, [1, 4]));
                $assert($equal(values, [1, 4, 3]));
                const marker = new Error("callback failure");
                const beforeFailure = [];
                let caught;
                try {
                    operation([1, 2, 3], value => {
                        beforeFailure.push(value);
                        if (value === 2) throw marker;
                        return value;
                    });
                } catch (error) { caught = error; }
                $assert(caught === marker);
                $assert($equal(beforeFailure, [1, 2]));
            }
        "#};
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn option_map_and_map_lookup_preserve_present_none() {
        let harness = r#"
const mapped = $optionMap($optionSome(42), () => null);
$assert(mapped !== null);
$assert($optionUnbox(mapped) === null);
const values = $mapNew();
$mapSet(values, "present", null);
$assert($mapGet(values, "present") !== null);
$assert($optionUnbox($mapGet(values, "present")) === null);
$assert($mapGet(values, "absent") === null);
"#;
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn option_operations_preserve_layers_and_payload_identity() {
        let harness = r#"
const one = $optionSome(null);
const two = $optionSome(one);
const three = $optionSome(two);
$assert($optionUnbox(three) === two);
$assert($optionUnbox(two) === one);
$assert($optionUnbox(one) === null);
const userSome = { $: "Some", _0: 42 };
$assert($optionUnbox($optionSome(userSome)) === userSome);
$assert($optionMap($optionSome(userSome), x => x) === userSome);
$assert($optionUnbox($optionSome(undefined)) === undefined);
$assert($optionSome(undefined) !== null);
$assert($optionApply($optionSome(x => { $assert(x === null); return null; }), one) !== null);
$assert($optionFlatMap(one, x => { $assert(x === null); return null; }) === null);
const traversed = $optionTraverse({
    pure: $resultOk,
    $super0: { map: $resultMap },
}, one, x => { $assert(x === null); return $resultOk(x); });
$assert(traversed.$ === "Ok" && traversed._0 !== null);
$assert($optionUnbox(traversed._0) === null);
const payload = { eq: (a, b) => a === b, show: $show, hash: $hash };
const option = child => ({
    eq: (a, b) => $equalContainer(a, b, "option", [child]),
    show: a => $showContainer(a, "option", [child]),
    hash: a => $hashContainer(a, "option", [child]),
});
const nested = option(option(payload));
const equal = $optionSome(null);
$assert(nested.eq(one, one) && nested.eq(one, equal) && nested.eq(equal, one));
$assert(nested.hash(one) === nested.hash(equal));
$assert(nested.show(one) === "Some(None)");
$assert(option(nested).show(two) === "Some(Some(None))");
"#;
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn option_json_round_trips_nullable_and_nested_payloads() {
        let harness = r#"
const option = child => ({
    encode: value => $jsonEncodeContainer(value, "option", [child]),
    decode: value => $jsonDecodeContainer(value, "option", [child]),
});
const json = { encode: JSON.stringify, decode: text => $resultOk(JSON.parse(text)) };
const nested = option(option(option(json)));
for (const value of [null, $optionSome(null), $optionSome($optionSome(null)), 42]) {
    const decoded = nested.decode(nested.encode(value));
    $assert(decoded.$ === "Ok");
    $assert(nested.encode(decoded._0) === nested.encode(value));
    let left = value, right = decoded._0;
    for (let depth = 0; depth < 3; depth++) {
        $assert((left === null) === (right === null));
        left = $optionUnbox(left); right = $optionUnbox(right);
    }
}
const unit = option({ encode: () => "null", decode: () => $resultOk(undefined) });
$assert(unit.decode(unit.encode(undefined))._0 !== null);
const reserved = { $alderSome: null };
const decoded = option(json).decode(option(json).encode(reserved));
$assert(JSON.stringify($optionUnbox(decoded._0)) === JSON.stringify(reserved));
"#;
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn immediately_ready_work_yields_to_host_timers() {
        let harness = r#"
const unit = $task(function* () {});
const ready = $tryPromise(() => Promise.resolve());
for (const mode of ["promise", "join", "fork", "all", "race", "finalizers"]) {
    let timerRan = false;
    let progress = 0;
    const timer = setTimeout(() => { timerRan = true; }, 0);
    await $runTask($task(function* () {
        const completed = yield* $fiberFork(unit);
        yield* $fiberJoin(completed);
        if (mode === "finalizers") {
            for (let i = 0; i < 4096; i++) {
                yield* $fiberAddFinalizer($task(function* () {
                    progress++;
                    if (progress === 2048 && !timerRan) throw new Error("finalizer starvation");
                }));
            }
            timerRan = false;
            setTimeout(() => { timerRan = true; }, 0);
            return;
        }
        for (let i = 0; i < 4096; i++) {
            if (mode === "promise") yield* ready;
            else if (mode === "join") yield* $fiberJoin(completed);
            else if (mode === "fork") yield* $fiberJoin(yield* $fiberFork(unit));
            else if (mode === "all") yield* $fiberAll([unit]);
            else if (mode === "race") yield* $fiberRace([unit]);
            progress++;
            if (progress === 2048 && !timerRan) throw new Error(`${mode} starvation`);
        }
    }));
    clearTimeout(timer);
    if (!timerRan) throw new Error(`${mode} did not yield`);
}
"#;
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn host_timer_can_interrupt_immediately_fulfilled_awaits() {
        let harness = r#"
let steps = 0;
let finalized = 0;
let interrupted = false;
await $runTask($task(function* () {
    const child = yield* $fiberFork($task(function* () {
        yield* $fiberAddFinalizer($task(function* () { finalized++; }));
        for (let i = 0; i < 4096; i++) {
            yield* $tryPromise(() => Promise.resolve());
            steps++;
        }
    }));
    const timer = setTimeout(() => child.interruptUnsafe(), 0);
    try { yield* $fiberJoin(child); }
    catch (error) {
        if (error.name !== "AlderInterrupted") throw error;
        interrupted = true;
    } finally { clearTimeout(timer); }
}));
if (!interrupted || steps >= 4096) throw new Error("timer interruption was starved");
if (finalized !== 1) throw new Error("interrupted child must finalize exactly once");
"#;
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn partial_child_construction_failure_does_not_strand_owned_fibers() {
        let harness = r#"
for (const combine of [$fiberAll, $fiberRace]) {
  for (const catchFailure of [false, true]) {
    let bodiesStarted = 0;
    let parentFinalized = 0;
    const expected = new Error("task factory failed");
    const valid = $task(function* () { bodiesStarted++; });
    const invalid = $task(() => { throw expected; });
    const execution = $runTask($task(function* () {
        yield* $fiberAddFinalizer($task(function* () { parentFinalized++; }));
        if (catchFailure) {
            try { yield* combine([valid, invalid, valid]); }
            catch (error) {
                if (error !== expected) throw error;
                if (currentFiber.children.size !== 0) throw new Error("failure preceded child cleanup");
                return 42;
            }
            throw new Error("missing caught failure");
        }
        yield* combine([valid, invalid, valid]);
    }));
    let timer;
    const timeout = new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error("partial construction stranded a child")), 1000);
    });
    let observed;
    let result;
    try { result = await Promise.race([execution, timeout]); }
    catch (error) { observed = error; }
    finally { clearTimeout(timer); }
    if (catchFailure) {
        if (observed || result !== 42) throw observed ?? new Error("parent could not recover");
    } else if (observed !== expected) throw observed ?? new Error("missing construction failure");
    if (bodiesStarted !== 0) throw new Error("partially constructed children ran user code");
    if (parentFinalized !== 1) throw new Error("parent finalizer did not run exactly once");
  }
}
"#;
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn task_composition_is_stack_safe_before_and_after_suspension() {
        let harness = r#"
let entries = 0;
let owner;
const descend = (depth, suspend) => $task(function* () {
    entries++;
    if (owner === undefined) owner = currentFiber;
    if (currentFiber !== owner) throw new Error("sequential await forked a fiber");
    if (suspend) yield* $tryPromise(() => Promise.resolve());
    if (depth === 0) return 42;
    return yield* descend(depth - 1, suspend);
});
const reusable = descend(20000, false);
if (entries !== 0) throw new Error("task construction was not lazy");
for (const task of [reusable, reusable, descend(20000, true)]) {
    owner = undefined;
    if (await $runTask(task) !== 42) throw new Error("deep task lost its result");
}
if (entries !== 60003) throw new Error("reusable task did not execute independently");
"#;
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn deep_task_failure_and_interruption_unwind_suspending_cleanup() {
        let harness = r#"
for (const interrupt of [false, true]) {
    let unwound = 0;
    let finalized = 0;
    const expected = new Error("deep failure");
    const descend = (depth) => $task(function* () {
        try {
            if (depth > 0) return yield* descend(depth - 1);
            if (!interrupt) throw expected;
            const owner = currentFiber;
            yield* $tryPromise(() => new Promise((resolve) => {
                setTimeout(() => { owner.interruptUnsafe(); resolve(); }, 0);
            }));
        } finally {
            yield* $tryPromise(() => Promise.resolve());
            if (unwound !== depth) throw new Error("cleanup order changed");
            unwound++;
        }
    });
    let observed;
    try {
        await $runTask($task(function* () {
            yield* $fiberAddFinalizer($task(function* () { finalized++; }));
            yield* descend(20000);
        }));
    } catch (error) { observed = error; }
    if (interrupt ? observed?.name !== "AlderInterrupted" : observed !== expected) {
        throw observed ?? new Error("deep failure was lost");
    }
    if (unwound !== 20001 || finalized !== 1) throw new Error("cleanup was skipped or repeated");
}
"#;
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn malformed_operations_unwind_task_cleanup() {
        let harness = r#"
let cleaned = 0;
let observed;
try {
    await $runTask($task(function* () {
        try {
            yield* $task(function* () {
                try { yield null; }
                finally { yield* $tryPromise(() => Promise.resolve()); cleaned++; }
            });
        } finally { cleaned++; }
    }));
} catch (error) { observed = error; }
if (!(observed instanceof TypeError)) throw new Error("missing invalid-operation defect");
if (cleaned !== 2) throw new Error("invalid operation skipped task cleanup");
"#;
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn operation_handler_exceptions_do_not_strand_the_scheduler() {
        let harness = r#"
const expected = new Error("operation getter failed");
for (const operation of [
    { get $() { throw expected; } },
    { $: "Mask", delta: Symbol("invalid delta") },
]) {
    let cleaned = 0;
    const bad = $runTask($task(function* () {
        try { yield operation; }
        finally { yield* $tryPromise(() => Promise.resolve()); cleaned++; }
    }));
    const good = $runTask($task(function* () { return 42; }));
    let timer;
    let exits;
    try {
        exits = await Promise.race([
            Promise.allSettled([bad, good]),
            new Promise((_, reject) => {
                timer = setTimeout(() => reject(new Error("scheduler was stranded")), 1000);
            }),
        ]);
    } finally { clearTimeout(timer); }
    if (exits[0].status !== "rejected") throw new Error("operation failure disappeared");
    if (exits[0].reason !== expected && !(exits[0].reason instanceof TypeError)) {
        throw exits[0].reason;
    }
    if (cleaned !== 1 || exits[1].value !== 42) throw new Error("operation failure damaged other work");
}
"#;
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fiber_runtime_obeys_lifecycle_and_promise_invariants() {
        let harness = r#"
const check = (condition, message) => { if (!condition) throw new Error(message); };
const unit = $task(function* () { return undefined; });

let starts = 0;
const lazy = $tryPromise(() => {
    starts += 1;
    return Promise.resolve(42);
}, false, "lazy-test");
check(starts === 0, "Promise externs must be lazy");
check(await $runTask(lazy) === 42, "a fulfilled Promise must become task success");
check(await $runTask(lazy) === 42 && starts === 2, "tasks must be reusable");

for (const broken of [
    $tryPromise(() => { throw new Error("sync"); }, false, "sync-throw"),
    $tryPromise(() => Promise.reject(new Error("async")), false, "rejection"),
    $tryPromise(() => 42, false, "malformed"),
]) {
    let defect = null;
    try { await $runTask(broken); } catch (error) { defect = error; }
    check(defect?.name === "AlderForeignDefect", "foreign failures must become defects");
}

const joined = $task(function* () {
    const fiber = yield* $fiberFork($task(function* () { return 7; }));
    let first = 0;
    let reentrant = 0;
    fiber.observe(() => {
        first += 1;
        fiber.observe(() => { reentrant += 1; });
    });
    const value = yield* $fiberJoin(fiber);
    check(first === 1 && reentrant === 1, "observers must run exactly once under reentrancy");
    return value;
});
check(await $runTask(joined) === 7, "forked fibers must be joinable");

check(JSON.stringify(await $runTask($fiberAll([
    $task(function* () { return 1; }),
    $task(function* () { return 2; }),
]))) === "[1,2]", "children that complete immediately must remain observable");

check(JSON.stringify(await $runTask($fiberAll([
    $task(function* () { yield* $taskSleep(2); return 1; }),
    $task(function* () { yield* $taskSleep(1); return 2; }),
]))) === "[1,2]", "Fiber.all must retain input order");

let raceCleanup = 0;
const slowRace = $task(function* () {
    yield* $fiberAddFinalizer($task(function* () { raceCleanup += 1; }));
    yield* $taskSleep(50);
    return "slow";
});
const fastRace = $task(function* () { yield* $taskSleep(1); return "fast"; });
check(await $runTask($fiberRace([slowRace, fastRace])) === "fast", "race must select the first exit");
check(raceCleanup === 1, "race must await loser cleanup");

let failedRaceCleanup = 0;
let failedRaceDefect = null;
try {
    await $runTask($fiberRace([
        $task(function* () {
            yield* $fiberAddFinalizer($task(function* () { failedRaceCleanup += 1; }));
            yield* $taskSleep(50);
        }),
        $tryPromise(() => Promise.reject(new Error("race failure")), false, "race-failure"),
    ]));
} catch (error) { failedRaceDefect = error; }
check(failedRaceDefect?.name === "AlderForeignDefect" && failedRaceCleanup === 1,
    "a failed race winner must still interrupt and clean up its loser");

let interruptedRaceCleanup = 0;
await $runTask($task(function* () {
    const fiber = yield* $fiberFork($fiberRace([
        $task(function* () {
            yield* $fiberAddFinalizer($task(function* () { interruptedRaceCleanup += 1; }));
            yield* $taskSleep(50);
        }),
        $task(function* () {
            yield* $fiberAddFinalizer($task(function* () { interruptedRaceCleanup += 1; }));
            yield* $taskSleep(50);
        }),
    ]));
    yield* $taskSleep(1);
    yield* $fiberInterrupt(fiber);
}));
check(interruptedRaceCleanup === 2,
    "interrupting a race must interrupt and clean up every contestant");

let allCleanup = 0;
const slowAll = $task(function* () {
    yield* $fiberAddFinalizer($task(function* () { allCleanup += 1; }));
    yield* $taskSleep(50);
});
let allFailed = false;
try {
    await $runTask($fiberAll([
        slowAll,
        $tryPromise(() => Promise.reject(new Error("all")), false, "all-failure"),
    ]));
} catch { allFailed = true; }
check(allFailed && allCleanup === 1, "all must interrupt and join siblings after a defect");

const finalizerOrder = [];
const interrupted = $task(function* () {
    yield* $fiberAddFinalizer($task(function* () { finalizerOrder.push("first"); }));
    yield* $fiberAddFinalizer($task(function* () { finalizerOrder.push("second"); }));
    yield* $taskSleep(50);
});
await $runTask($task(function* () {
    const fiber = yield* $fiberFork(interrupted);
    yield* $taskSleep(1);
    yield* $fiberInterrupt(fiber);
}));
check(finalizerOrder.join(",") === "second,first", "finalizers must run once in LIFO order");

const nestedOrder = [];
await $runTask($fiberScope($task(function* () {
    yield* $fiberAddFinalizer($task(function* () { nestedOrder.push("outer"); }));
    yield* $fiberScope($task(function* () {
        yield* $fiberAddFinalizer($task(function* () { nestedOrder.push("inner"); }));
    }));
})));
check(nestedOrder.join(",") === "inner,outer", "nested scopes must close from the inside out");

let parentCleanup = 0;
await $runTask($task(function* () {
    yield* $fiberFork($task(function* () {
        yield* $fiberAddFinalizer($task(function* () { parentCleanup += 1; }));
        yield* $taskSleep(50);
    }));
    yield* $taskSleep(1);
}));
check(parentCleanup === 1, "parent completion must interrupt and join scoped children");

let closingOwner;
const duringClose = [];
closingOwner = new FiberImpl($task(function* () {
    yield* $fiberAddFinalizer($tryPromise(() => {
        const pending = closingOwner.scope.add((exit) => $task(function* () {
            duringClose.push(`late:${exit.$}`);
        }));
        check(pending === null, "a finalizer added during closure must join the closing scope");
        duringClose.push("original");
        return Promise.resolve();
    }, false, "during-close-finalizer"));
})).start();
await closingOwner.awaitExit();
check(duringClose.join(",") === "original,late:Success", "finalizers added during closure must run exactly once");

const closedOwner = new FiberImpl(unit);
await closedOwner.scope.close(success(42));
let closedExit = null;
const immediateFinalizer = closedOwner.scope.add((exit) => $task(function* () {
    closedExit = exit;
}));
check(immediateFinalizer !== null, "a closed scope must run a newly added finalizer immediately");
await immediateFinalizer;
check(closedExit?.$ === "Success" && closedExit.value === 42,
    "a finalizer added after closure must receive the scope exit");

let finalizedAfterDefect = 0;
let finalizerDefect = null;
try {
    await $runTask($task(function* () {
        yield* $fiberAddFinalizer($task(function* () { finalizedAfterDefect += 1; }));
        yield* $fiberAddFinalizerExit(() => { throw new Error("broken finalizer"); });
    }));
} catch (error) { finalizerDefect = error; }
check(finalizerDefect?.message === "broken finalizer" && finalizedAfterDefect === 1,
    "a defective finalizer must not prevent the remaining finalizers from running");

let maskedFinished = false;
await $runTask($task(function* () {
    const fiber = yield* $fiberFork($fiberUninterruptible($task(function* () {
        yield* $taskSleep(3);
        maskedFinished = true;
    })));
    yield* $taskSleep(1);
    yield* $fiberInterrupt(fiber);
}));
check(maskedFinished, "interruption must remain pending during an uninterruptible region");

let signal = null;
let aborts = 0;
const cancellable = $tryPromise((value) => {
    signal = value;
    signal.addEventListener("abort", () => { aborts += 1; });
    return new Promise(() => {});
}, true, "abortable");
await $runTask($task(function* () {
    const fiber = yield* $fiberFork(cancellable);
    yield* $taskSleep(1);
    yield* $fiberInterrupt(fiber);
}));
check(signal?.aborted === true && aborts === 1,
    "interrupting an abort-aware extern must abort exactly once");

const mapped = await $runTask($tryPromise(
    () => Promise.reject("mapped"),
    false,
    "mapped-rejection",
    (error) => ({ $: "Err", _0: String(error) }),
));
check(mapped.$ === "Err" && mapped._0 === "mapped",
    "an explicit rejection mapper may produce a typed Alder value");

let hostileSettlements = 0;
const hostile = {
    then(resolve, reject) {
        hostileSettlements += 1;
        resolve(5);
        reject(new Error("too late"));
        resolve(6);
    },
};
check(await $runTask($tryPromise(() => hostile, false, "hostile")) === 5,
    "a hostile thenable must only settle a fiber once");
check(hostileSettlements === 1, "the Promise bridge must assimilate a thenable once");

let rejectLate;
const late = $tryPromise(() => new Promise((_, reject) => { rejectLate = reject; }), false, "late");
await $runTask($task(function* () {
    const fiber = yield* $fiberFork(late);
    yield* $taskSleep(1);
    yield* $fiberInterrupt(fiber);
}));
rejectLate(new Error("late rejection"));
await Promise.resolve();

let resolveLate;
let resumedAfterInterrupt = false;
const resolvesLate = $task(function* () {
    yield* $tryPromise(() => new Promise((resolve) => { resolveLate = resolve; }), false, "late-resolution");
    resumedAfterInterrupt = true;
});
await $runTask($task(function* () {
    const fiber = yield* $fiberFork(resolvesLate);
    yield* $taskSleep(1);
    yield* $fiberInterrupt(fiber);
}));
resolveLate(42);
await Promise.resolve();
check(!resumedAfterInterrupt, "a Promise resolution cannot resurrect an interrupted fiber");

const inherited = await $runTask($task(function* () {
    $providerPush("service", "parent");
    const fiber = yield* $fiberFork($task(function* () { return $providerGet("service"); }));
    $providerPop("service");
    return yield* $fiberJoin(fiber);
}));
check(inherited === "parent", "a child must inherit a snapshot of its parent's provider context");

const isolated = await $runTask($task(function* () {
    $providerPush("service", "root");
    try {
        return yield* $fiberAll([
            $task(function* () { $providerPush("service", "child"); return $providerGet("service"); }),
            $task(function* () { return $providerGet("service"); }),
        ]);
    } finally {
        $providerPop("service");
    }
}));
check(JSON.stringify(isolated) === '["child","root"]',
    "fiber-local provider changes must not leak to sibling fibers");

let hostYielded = false;
setTimeout(() => { hostYielded = true; }, 0);
await $runTask($task(function* () {
    for (let index = 0; index < 3000; index += 1) yield { $: "Mask", delta: 0 };
}));
check(hostYielded, "the operation budget must yield to the host event loop");

check(await $runTask(unit) === undefined, "a defect must not corrupt later scheduler work");
globalThis.__alderHost.exit(0);
"#;
        let code = format!("{KERNEL_JS}\n{harness}");
        assert_eq!(alder_runtime::execute(code, Vec::new()).await.unwrap(), 0);
    }
}
