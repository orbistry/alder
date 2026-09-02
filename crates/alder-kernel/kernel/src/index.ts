const providerStacks = new Map();
const tests = [];

export function $equal(left, right) {
    return equalInner(left, right, new WeakMap());
}

function equalInner(left, right, seen) {
    if (Object.is(left, right)) return true;
    if (typeof left !== "object" || left === null || typeof right !== "object" || right === null) {
        return false;
    }
    let rights = seen.get(left);
    if (rights?.has(right)) return true;
    if (!rights) {
        rights = new WeakSet();
        seen.set(left, rights);
    }
    rights.add(right);
    if (Array.isArray(left) !== Array.isArray(right)) return false;
    const leftKeys = Object.keys(left);
    const rightKeys = Object.keys(right);
    return leftKeys.length === rightKeys.length
        && leftKeys.every((key, index) => key === rightKeys[index]
            && equalInner(left[key], right[key], seen));
}

export function $matchFailure(moduleName, location, value) {
    throw new Error(`Non-exhaustive match at ${moduleName}:${location}: ${describe(value)}`);
}

export function $assert(value) {
    if (!value) throw new Error("Assertion failed");
}

export function $optionBox(value) {
    return { $: "Some", _0: value };
}

function optionValue(value) {
    return value !== null && value?.$ === "Some" ? value._0 : value;
}

export function $optionUnbox(value) { return optionValue(value); }

export function $optionSome(value) { return value === null ? $optionBox(value) : value; }
export function $optionNone() { return null; }
export function $optionMap(value, transform) {
    return value === null ? null : transform(optionValue(value));
}
export function $resultOk(value) { return { $: "Ok", _0: value }; }
export function $resultErr(error) { return { $: "Err", _0: error }; }
export function $resultMap(value, transform) {
    return value.$ === "Ok" ? $resultOk(transform(value._0)) : value;
}
export function $arrayLength(values) { return values.length; }
export function $arrayPush(values, value) { values.push(value); }
export function $arrayMap(values, transform) { return values.map(transform); }
export function $stringLength(value) { return [...value].length; }
export function $stringConcat(left, right) { return left + right; }
export function $numberParse(value) {
    const parsed = Number(value);
    return Number.isNaN(parsed) ? null : parsed;
}
export function $bigIntParse(value) {
    try { return BigInt(value); } catch { return null; }
}
export function $mapNew() { return new Map(); }
export function $mapGet(values, key) { return values.has(key) ? values.get(key) : null; }
export function $mapSet(values, key, value) { values.set(key, value); }
export function $setNew() { return new Set(); }
export function $setHas(values, value) { return values.has(value); }
export function $setAdd(values, value) { values.add(value); }
export function $jsonEncode(value) { return JSON.stringify(value); }
export function $jsonDecode(value) {
    try { return $resultOk(JSON.parse(value)); }
    catch (error) { return $resultErr(String(error)); }
}
export function $ioPrint(value) { console.log(value); }
export function $cliArgs() { return globalThis.__alderHost?.args ?? []; }
export function $taskSleep(milliseconds) {
    return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
export function $fiberAll(tasks) { return Promise.all(tasks); }
export function $fiberRace(tasks) { return Promise.race(tasks); }

export function $tryCatch(thunk) {
    try {
        return { $: "Ok", _0: thunk() };
    } catch (error) {
        return { $: "Err", _0: error };
    }
}

export async function $tryCatchAsync(thunk) {
    try {
        return { $: "Ok", _0: await thunk() };
    } catch (error) {
        return { $: "Err", _0: error };
    }
}

export function $providerPush(key, value) {
    let stack = providerStacks.get(key);
    if (!stack) providerStacks.set(key, stack = []);
    stack.push(value);
}

export function $providerPop(key) {
    const stack = providerStacks.get(key);
    if (!stack?.length) throw new Error(`Provider stack underflow: ${key}`);
    stack.pop();
    if (stack.length === 0) providerStacks.delete(key);
}

export function $providerGet(key) {
    const stack = providerStacks.get(key);
    if (!stack?.length) throw new Error(`No value was provided for ${key}`);
    return stack[stack.length - 1];
}

export function $html(name, attributes, children) {
    return { name, attributes, children };
}

export function $style(value) {
    return value;
}

export function $query() {
    throw new Error("Queries are not executable until M7");
}

export function $registerTest(moduleName, name, run) {
    tests.push({ moduleName, name, run });
}

export async function $runTests(report = console.log) {
    let failed = 0;
    for (const test of tests) {
        try {
            await test.run();
            report(`pass ${test.moduleName} — ${test.name}`);
        } catch (error) {
            failed += 1;
            report(`fail ${test.moduleName} — ${test.name}\n  ${describe(error)}`);
        }
    }
    report(`\n${tests.length - failed} passed; ${failed} failed`);
    return failed;
}

function describe(value) {
    if (value instanceof Error) return value.stack ?? value.message;
    try { return JSON.stringify(value); } catch { return String(value); }
}
