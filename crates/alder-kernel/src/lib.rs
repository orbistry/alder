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
            "$arrayApply",
            "$arrayFilter",
            "$optionFlatMap",
            "$resultPure",
            "$arrayTraverse",
            "$arrayNext",
            "$jsonEncodeDerived",
            "$jsonDecodeDerived",
            "$jsonEncodeContainer",
            "$jsonDecodeContainer",
            "$hash",
            "$hashDerived",
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
