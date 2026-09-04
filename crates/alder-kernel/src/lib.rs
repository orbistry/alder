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
