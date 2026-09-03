const providerStacks = new Map();
const tests = [];

export function $equal(left, right) {
    return equalInner(left, right, new WeakMap());
}

export function $show(value) {
    if (typeof value === "string") return JSON.stringify(value);
    if (value === undefined) return "()";
    if (value === null) return "None";
    if (Array.isArray(value)) return `[${value.map($show).join(", ")}]`;
    if (typeof value === "object" && typeof value.$ === "string") {
        const arguments_ = Object.keys(value)
            .filter((key) => /^_\d+$/.test(key))
            .sort((left, right) => Number(left.slice(1)) - Number(right.slice(1)))
            .map((key) => $show(value[key]));
        return arguments_.length === 0 ? value.$ : `${value.$}(${arguments_.join(", ")})`;
    }
    if (typeof value === "object") {
        return `{ ${Object.keys(value).sort().map((key) => `${key}: ${$show(value[key])}`).join(", ")} }`;
    }
    return String(value);
}

export function $compare(left, right) {
    if ($equal(left, right)) return 0;
    if ((typeof left === "number" && typeof right === "number")
        || (typeof left === "bigint" && typeof right === "bigint")
        || (typeof left === "string" && typeof right === "string")) {
        return left < right ? -1 : 1;
    }
    return stableText(left) < stableText(right) ? -1 : 1;
}

export function $compareEnum(left, right, variants) {
    if (left && right && typeof left.$ === "string" && typeof right.$ === "string") {
        const leftIndex = variants.indexOf(left.$);
        const rightIndex = variants.indexOf(right.$);
        if (leftIndex !== rightIndex && leftIndex >= 0 && rightIndex >= 0) {
            return leftIndex < rightIndex ? -1 : 1;
        }
    }
    return $compare(left, right);
}

export function $arrayPure(value) {
    return [value];
}

export function $arrayApply(functions, values) {
    return functions.flatMap((function_) => values.map(function_));
}

export function $arrayFlatMap(values, transform) {
    return values.flatMap(transform);
}

export function $optionPure(value) {
    return $optionSome(value);
}

export function $optionApply(function_, value) {
    return function_ === null || value === null ? null : function_(value);
}

export function $optionFlatMap(value, transform) {
    return value === null ? null : transform(value);
}

export function $resultPure(value) {
    return $resultOk(value);
}

export function $resultApply(function_, value) {
    if (function_.$ !== "Ok") return function_;
    if (value.$ !== "Ok") return value;
    return $resultOk(function_._0(value._0));
}

export function $resultFlatMap(value, transform) {
    return value.$ === "Ok" ? transform(value._0) : value;
}

export function $arrayTraverse(applicative, values, transform) {
    let result = applicative.pure([]);
    for (const value of values) {
        const append = applicative.$super0.map(result, (items) => (item) => [...items, item]);
        result = applicative.apply(append, transform(value));
    }
    return result;
}

export function $arrayNext(values) {
    return values.length === 0 ? null : $optionSome(values[0]);
}

export function $optionTraverse(applicative, value, transform) {
    if (value === null) return applicative.pure(null);
    return applicative.$super0.map(transform(value), $optionSome);
}

export function $resultTraverse(applicative, value, transform) {
    if (value.$ !== "Ok") return applicative.pure(value);
    return applicative.$super0.map(transform(value._0), $resultOk);
}

export function $hash(value) {
    return hashBytes(hashStream(value));
}

function hashBytes(bytes) {
    let hash = 14695981039346656037n;
    for (const byte of bytes) {
        hash ^= BigInt(byte);
        hash = (hash * 1099511628211n) & 0xffffffffffffffffn;
    }
    return hash;
}

function pushU64(bytes, value) {
    let remaining = BigInt.asUintN(64, value);
    for (let index = 0; index < 8; index += 1) {
        bytes.push(Number(remaining & 0xffn));
        remaining >>= 8n;
    }
}

function pushText(bytes, value) {
    const encoded = new TextEncoder().encode(value);
    pushU64(bytes, BigInt(encoded.length));
    bytes.push(...encoded);
}

function pushChildHash(bytes, index, value) {
    pushU64(bytes, BigInt(index));
    pushU64(bytes, $hash(value));
}

function hashStream(value) {
    const bytes = [];
    if (value === undefined) {
        bytes.push(0x00);
        return bytes;
    }
    if (typeof value === "boolean") {
        bytes.push(0x01, value ? 1 : 0);
        return bytes;
    }
    if (typeof value === "number") {
        bytes.push(0x02);
        const storage = new ArrayBuffer(8);
        const view = new DataView(storage);
        if (Number.isNaN(value)) {
            view.setBigUint64(0, 0x7ff8000000000000n, true);
        } else {
            view.setFloat64(0, Object.is(value, -0) ? 0 : value, true);
        }
        bytes.push(...new Uint8Array(storage));
        return bytes;
    }
    if (typeof value === "bigint") {
        bytes.push(0x03, value < 0n ? 1 : 0);
        let magnitude = value < 0n ? -value : value;
        const encoded = [];
        while (magnitude !== 0n) {
            encoded.push(Number(magnitude & 0xffn));
            magnitude >>= 8n;
        }
        encoded.reverse();
        pushU64(bytes, BigInt(encoded.length));
        bytes.push(...encoded);
        return bytes;
    }
    if (typeof value === "string") {
        bytes.push(0x04);
        pushText(bytes, value);
        return bytes;
    }
    if (Array.isArray(value)) {
        bytes.push(0x10);
        pushU64(bytes, BigInt(value.length));
        value.forEach((item, index) => pushChildHash(bytes, index, item));
        return bytes;
    }
    if (value !== null && typeof value.$ === "string") {
        bytes.push(0x12);
        pushText(bytes, value.$);
        const fields = Object.keys(value)
            .filter((key) => /^_\d+$/.test(key))
            .sort((left, right) => Number(left.slice(1)) - Number(right.slice(1)));
        pushU64(bytes, BigInt(fields.length));
        fields.forEach((field, index) => pushChildHash(bytes, index, value[field]));
        return bytes;
    }
    if (value !== null && typeof value === "object") {
        bytes.push(0x11);
        const fields = Object.keys(value);
        pushU64(bytes, BigInt(fields.length));
        fields.forEach((field, index) => {
            pushU64(bytes, BigInt(index));
            pushText(bytes, field);
            pushU64(bytes, $hash(value[field]));
        });
        return bytes;
    }
    throw new TypeError(`Value is not hashable: ${String(value)}`);
}

function stableText(value) {
    if (value === undefined) return "null";
    if (typeof value === "bigint") return `{"$bigint":${JSON.stringify(String(value))}}`;
    if (value === null || typeof value !== "object") return JSON.stringify(value);
    if (Array.isArray(value)) return `[${value.map(stableText).join(",")}]`;
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableText(value[key])}`).join(",")}}`;
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
export function $jsonEncodeDerived(value, variants) {
    const shape = value && variants[value.$];
    if (!shape) throw new TypeError("$: unknown derived JSON variant");
    if (shape.record) {
        const record = {};
        for (const field of shape.fields) record[field] = value[field];
        return JSON.stringify({ tag: value.$, value: record });
    }
    return JSON.stringify({ tag: value.$, fields: shape.fields.map((field) => value[field]) });
}
export function $jsonDecodeDerived(value, variants) {
    try {
        const parsed = JSON.parse(value);
        if (!parsed || typeof parsed !== "object" || typeof parsed.tag !== "string") {
            return $resultErr("$: expected an object with a string `tag`");
        }
        const shape = variants[parsed.tag];
        if (!shape) return $resultErr(`$.tag: unknown variant ${JSON.stringify(parsed.tag)}`);
        const result = { $: parsed.tag };
        if (shape.record) {
            if (!parsed.value || typeof parsed.value !== "object" || Array.isArray(parsed.value)) {
                return $resultErr("$.value: expected an object");
            }
            for (const field of shape.fields) {
                if (!Object.hasOwn(parsed.value, field)) {
                    return $resultErr(`$.value.${field}: missing field`);
                }
                result[field] = parsed.value[field];
            }
        } else {
            if (!Array.isArray(parsed.fields) || parsed.fields.length !== shape.fields.length) {
                return $resultErr(`$.fields: expected ${shape.fields.length} values`);
            }
            shape.fields.forEach((field, index) => { result[field] = parsed.fields[index]; });
        }
        return $resultOk(result);
    } catch (error) {
        return $resultErr(`$: ${String(error)}`);
    }
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
