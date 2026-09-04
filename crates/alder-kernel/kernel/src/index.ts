const tests = [];

export function $equal(left, right) {
    return equalInner(left, right, new WeakMap());
}

export function $equalDerived(left, right, variants) {
    if (left?.$ !== right?.$) return false;
    const shape = left && variants[left.$];
    if (!shape) throw new TypeError("$: unknown derived Eq variant");
    return shape.fields.every((field, index) => {
        const dictionary = shape.dictionaries?.[index];
        return dictionary ? dictionary.eq(left[field], right[field]) : $equal(left[field], right[field]);
    });
}

export function $equalContainer(left, right, kind, dictionaries) {
    if (kind === "array") {
        return Array.isArray(left) && Array.isArray(right)
            && left.length === right.length
            && left.every((value, index) => dictionaries[0].eq(value, right[index]));
    }
    if (kind === "option") {
        return left === null || right === null
            ? left === right
            : dictionaries[0].eq(left, right);
    }
    if (kind === "result") {
        return left?.$ === right?.$
            && (left?.$ === "Ok" ? dictionaries[0] : dictionaries[1]).eq(left._0, right._0);
    }
    throw new TypeError(`unknown Eq container: ${kind}`);
}

export function $equalStructural(left, right, kind, fields, dictionaries) {
    if (["array", "option", "result"].includes(kind)) {
        return $equalContainer(left, right, kind, dictionaries);
    }
    if (kind === "tuple") {
        return Array.isArray(left) && Array.isArray(right)
            && left.length === right.length
            && left.every((value, index) => dictionaries[index].eq(value, right[index]));
    }
    if (kind === "record") {
        return fields.every((field, index) => dictionaries[index].eq(left[field], right[field]));
    }
    if (kind === "error_row") {
        if (left?.$ !== right?.$) return false;
        return fields.every((field, dictionaryIndex) => {
            const separator = field.lastIndexOf(":");
            const tag = field.slice(0, separator);
            if (tag !== left.$) return true;
            const payloadIndex = Number(field.slice(separator + 1));
            return dictionaries[dictionaryIndex].eq(
                left[`_${payloadIndex}`],
                right[`_${payloadIndex}`],
            );
        });
    }
    throw new TypeError(`unknown structural Eq shape: ${kind}`);
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

export function $showDerived(value, variants) {
    const shape = value && variants[value.$];
    if (!shape) throw new TypeError("$: unknown derived Show variant");
    if (shape.record) {
        const fields = shape.fields.flatMap((field, index) => {
                if (!Object.hasOwn(value, field)) return [];
                const dictionary = shape.dictionaries?.[index];
                return [`${field}: ${dictionary ? dictionary.show(value[field]) : $show(value[field])}`];
            });
        return `${value.$} { ${fields.join(", ")} }`;
    }
    const fields = shape.fields.map((field, index) => {
        const dictionary = shape.dictionaries?.[index];
        return dictionary ? dictionary.show(value[field]) : $show(value[field]);
    });
    return fields.length === 0 ? value.$ : `${value.$}(${fields.join(", ")})`;
}

export function $showContainer(value, kind, dictionaries) {
    if (kind === "array") return `[${value.map(dictionaries[0].show).join(", ")}]`;
    if (kind === "option") return value === null ? "None" : `Some(${dictionaries[0].show(value)})`;
    if (kind === "result") {
        const dictionary = value.$ === "Ok" ? dictionaries[0] : dictionaries[1];
        return `${value.$}(${dictionary.show(value._0)})`;
    }
    throw new TypeError(`unknown Show container: ${kind}`);
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

export function $compareDerived(left, right, variants) {
    const names = Object.keys(variants);
    const leftIndex = names.indexOf(left?.$);
    const rightIndex = names.indexOf(right?.$);
    if (leftIndex < 0 || rightIndex < 0) {
        throw new TypeError("$: unknown derived Ord variant");
    }
    if (leftIndex !== rightIndex) return leftIndex < rightIndex ? -1 : 1;
    const shape = variants[left.$];
    for (const [index, field] of shape.fields.entries()) {
        const dictionary = shape.dictionaries?.[index];
        const ordering = dictionary
            ? (() => {
                const result = dictionary.compare(left[field], right[field]);
                return result.$ === "Less" ? -1 : result.$ === "Greater" ? 1 : 0;
            })()
            : $compare(left[field], right[field]);
        if (ordering !== 0) return ordering;
    }
    return 0;
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

export function $hashDerived(value, typeName, variants) {
    const names = Object.keys(variants);
    const variantIndex = names.indexOf(value?.$);
    if (variantIndex < 0) throw new TypeError("$: unknown derived Hash variant");
    const bytes = [0x12];
    pushText(bytes, typeName);
    pushU64(bytes, BigInt(variantIndex));
    const fields = variants[value.$].fields;
    pushU64(bytes, BigInt(fields.length));
    fields.forEach((field, index) => {
        const dictionary = variants[value.$].dictionaries?.[index];
        pushChildHashValue(bytes, index, dictionary ? dictionary.hash(value[field]) : $hash(value[field]));
    });
    return hashBytes(bytes);
}

export function $hashContainer(value, kind, dictionaries) {
    const bytes = [];
    if (kind === "array") {
        bytes.push(0x10);
        pushU64(bytes, BigInt(value.length));
        value.forEach((item, index) => pushChildHashValue(bytes, index, dictionaries[0].hash(item)));
        return hashBytes(bytes);
    }
    if (kind === "option") {
        bytes.push(0x12);
        pushText(bytes, value === null ? "None" : "Some");
        pushU64(bytes, value === null ? 0n : 1n);
        if (value !== null) pushChildHashValue(bytes, 0, dictionaries[0].hash(value));
        return hashBytes(bytes);
    }
    if (kind === "result") {
        bytes.push(0x12);
        pushText(bytes, value.$);
        pushU64(bytes, 1n);
        const dictionary = value.$ === "Ok" ? dictionaries[0] : dictionaries[1];
        pushChildHashValue(bytes, 0, dictionary.hash(value._0));
        return hashBytes(bytes);
    }
    throw new TypeError(`unknown Hash container: ${kind}`);
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
    pushChildHashValue(bytes, index, $hash(value));
}

function pushChildHashValue(bytes, index, value) {
    pushU64(bytes, BigInt(index));
    pushU64(bytes, value);
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
export function $arrayFilter(values, predicate) { return values.filter(predicate); }
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
    catch (error) { return $jsonErr(String(error)); }
}
export function $jsonEncodeContainer(value, kind, dictionaries) {
    const encode = (dictionary, item) => JSON.parse(dictionary.encode(item));
    if (kind === "array") return JSON.stringify(value.map((item) => encode(dictionaries[0], item)));
    if (kind === "option") {
        return value === null ? "null" : JSON.stringify(encode(dictionaries[0], value));
    }
    if (kind === "result") {
        const index = value.$ === "Ok" ? 0 : 1;
        return JSON.stringify({ $: value.$, _0: encode(dictionaries[index], value._0) });
    }
    throw new TypeError(`unknown Json container: ${kind}`);
}
export function $jsonDecodeContainer(value, kind, dictionaries) {
    try {
        const parsed = JSON.parse(value);
        const decode = (dictionary, item, path) => {
            const result = dictionary.decode(JSON.stringify(item));
            return result.$ === "Ok" ? result : $jsonErr(prefixJsonPath(path, result._0));
        };
        if (kind === "array") {
            if (!Array.isArray(parsed)) return $jsonErr("$: expected an array");
            const result = [];
            for (const [index, item] of parsed.entries()) {
                const decoded = decode(dictionaries[0], item, `$[${index}]`);
                if (decoded.$ !== "Ok") return decoded;
                result.push(decoded._0);
            }
            return $resultOk(result);
        }
        if (kind === "option") {
            if (parsed === null) return $resultOk(null);
            return decode(dictionaries[0], parsed, "$" );
        }
        if (kind === "result") {
            if (!parsed || typeof parsed !== "object" || !["Ok", "Err"].includes(parsed.$)) {
                return $jsonErr("$: expected an `Ok` or `Err` result");
            }
            const index = parsed.$ === "Ok" ? 0 : 1;
            const decoded = decode(dictionaries[index], parsed._0, "$._0");
            return decoded.$ === "Ok" ? $resultOk({ $: parsed.$, _0: decoded._0 }) : decoded;
        }
        return $jsonErr(`$: unknown Json container: ${kind}`);
    } catch (error) {
        return $jsonErr(`$: ${String(error)}`);
    }
}
export function $jsonEncodeDerived(value, variants) {
    const shape = value && variants[value.$];
    if (!shape) throw new TypeError("$: unknown derived JSON variant");
    if (shape.record) {
        const record = {};
        const optional = new Set(shape.optional ?? []);
        for (const field of shape.fields) {
            if (optional.has(field) && (!Object.hasOwn(value, field) || value[field] === null)) continue;
            const index = shape.fields.indexOf(field);
            const dictionary = shape.dictionaries?.[index];
            record[field] = dictionary ? JSON.parse(dictionary.encode(value[field])) : value[field];
        }
        return JSON.stringify({ tag: value.$, value: record });
    }
    return JSON.stringify({
        tag: value.$,
        fields: shape.fields.map((field, index) => {
            const dictionary = shape.dictionaries?.[index];
            return dictionary ? JSON.parse(dictionary.encode(value[field])) : value[field];
        }),
    });
}
export function $jsonDecodeDerived(value, variants) {
    try {
        const parsed = JSON.parse(value);
        if (!parsed || typeof parsed !== "object" || typeof parsed.tag !== "string") {
            return $jsonErr("$: expected an object with a string `tag`");
        }
        const shape = variants[parsed.tag];
        if (!shape) return $jsonErr(`$.tag: unknown variant ${JSON.stringify(parsed.tag)}`);
        const result = { $: parsed.tag };
        if (shape.record) {
            if (Object.keys(parsed).some((key) => key !== "tag" && key !== "value")) {
                return $jsonErr("$: expected only `tag` and `value`");
            }
            if (!parsed.value || typeof parsed.value !== "object" || Array.isArray(parsed.value)) {
                return $jsonErr("$.value: expected an object");
            }
            const optional = new Set(shape.optional ?? []);
            const expected = new Set(shape.fields);
            for (const field of Object.keys(parsed.value)) {
                if (!expected.has(field)) return $jsonErr(`$.value.${field}: unexpected field`);
            }
            for (const field of shape.fields) {
                if (!Object.hasOwn(parsed.value, field)) {
                    if (optional.has(field)) continue;
                    return $jsonErr(`$.value.${field}: missing field`);
                }
                const index = shape.fields.indexOf(field);
                const dictionary = shape.dictionaries?.[index];
                if (!dictionary) {
                    result[field] = parsed.value[field];
                    continue;
                }
                const decoded = dictionary.decode(JSON.stringify(parsed.value[field]));
                if (decoded.$ !== "Ok") {
                    return $jsonErr(prefixJsonPath(`$.value.${field}`, decoded._0));
                }
                result[field] = decoded._0;
            }
        } else {
            if (Object.keys(parsed).some((key) => key !== "tag" && key !== "fields")) {
                return $jsonErr("$: expected only `tag` and `fields`");
            }
            if (!Array.isArray(parsed.fields) || parsed.fields.length !== shape.fields.length) {
                return $jsonErr(`$.fields: expected ${shape.fields.length} values`);
            }
            for (const [index, field] of shape.fields.entries()) {
                const dictionary = shape.dictionaries?.[index];
                if (!dictionary) {
                    result[field] = parsed.fields[index];
                    continue;
                }
                const decoded = dictionary.decode(JSON.stringify(parsed.fields[index]));
                if (decoded.$ !== "Ok") {
                    return $jsonErr(prefixJsonPath(`$.fields.${index}`, decoded._0));
                }
                result[field] = decoded._0;
            }
        }
        return $resultOk(result);
    } catch (error) {
        return $jsonErr(`$: ${String(error)}`);
    }
}

function $jsonErr(message) {
    return $resultErr({ $: ":invalid_json", _0: String(message) });
}

function prefixJsonPath(prefix, message) {
    if (message?.$ === ":invalid_json") message = message._0;
    if (typeof message !== "string") return `${prefix}: ${String(message)}`;
    if (message.startsWith("$:")) return `${prefix}${message.slice(1)}`;
    return `${prefix}: ${message}`;
}
export function $ioPrint(value) { console.log(value); }
export function $refSame(left, right) { return left === right; }
export function $cliArgs() { return globalThis.__alderHost?.args ?? []; }

const taskType = Symbol.for("alder/Task");
const maxOperationsBeforeYield = 1024;
let nextFiberId = 0;
let currentFiber = null;
const synchronousProviderContext = new Map();

class ForeignDefect extends Error {
    constructor(cause, origin) {
        super(`Foreign task failed at ${origin}: ${describe(cause)}`, { cause });
        this.name = "AlderForeignDefect";
        this.origin = origin;
    }
}

class Interrupted extends Error {
    constructor(fiberId) {
        super(`Fiber ${fiberId} was interrupted`);
        this.name = "AlderInterrupted";
        this.fiberId = fiberId;
    }
}

function success(value) { return { $: "Success", value }; }
function failure(error) { return { $: "Failure", error }; }

function cloneContext(context) {
    return new Map([...context].map(([key, values]) => [key, [...values]]));
}

function taskIterator(task) {
    if (!task || task[taskType] !== true || typeof task[Symbol.iterator] !== "function") {
        throw new TypeError("Expected an Alder Task value");
    }
    const iterator = task[Symbol.iterator]();
    if (!iterator || typeof iterator.next !== "function" || typeof iterator.throw !== "function") {
        throw new TypeError("An Alder Task factory did not return a generator");
    }
    return iterator;
}

export function $task(factory) {
    if (typeof factory !== "function") throw new TypeError("Task factory must be a function");
    return Object.freeze({
        [taskType]: true,
        [Symbol.iterator]: factory,
    });
}

class FiberScope {
    constructor(owner) {
        this.owner = owner;
        this.state = "Empty";
        this.finalizers = [];
        this.exit = null;
        this.closePromise = null;
    }

    add(finalizer) {
        if (this.state === "Closed") {
            return runFinalizer(finalizer(this.exit), this.owner.context);
        }
        if (this.state === "Empty") this.state = "Open";
        this.finalizers.push(finalizer);
        return null;
    }

    async close(exit) {
        if (this.state === "Closed") return this.exit;
        if (this.state === "Closing") return this.closePromise;
        this.state = "Closing";
        this.exit = exit;
        this.closePromise = (async () => {
            const children = [...this.owner.children];
            for (const child of children) child.interruptUnsafe();
            await Promise.all(children.map((child) => child.awaitExit()));

            while (this.finalizers.length > 0) {
                const finalizer = this.finalizers.pop();
                let finalizerExit;
                try {
                    finalizerExit = await runFinalizer(finalizer(exit), this.owner.context);
                } catch (error) {
                    finalizerExit = failure(error);
                }
                if (exit.$ === "Success" && finalizerExit.$ === "Failure") exit = finalizerExit;
            }
            this.exit = exit;
            this.state = "Closed";
            return exit;
        })();
        return this.closePromise;
    }
}

class FiberImpl {
    constructor(task, parent = null, context = null) {
        this.id = ++nextFiberId;
        this.parent = parent;
        this.context = cloneContext(context ?? parent?.context ?? synchronousProviderContext);
        this.children = new Set();
        this.scope = new FiberScope(this);
        this.state = "Running";
        this.exit = null;
        this.observers = new Set();
        this.interruptRequested = false;
        this.interruptDelivered = false;
        this.interruptError = new Interrupted(this.id);
        this.interruptMask = 0;
        this.suspendedCancel = null;
        this.queued = false;
        this.resumeMethod = "next";
        this.resumeValue = undefined;
        this.iterator = taskIterator(task);
        this.exitPromise = new Promise((resolve) => { this.resolveExit = resolve; });
        if (parent) parent.children.add(this);
    }

    start() {
        schedule(this, false);
        return this;
    }

    awaitExit() { return this.exitPromise; }

    observe(observer) {
        if (this.state === "Done") {
            observer(this.exit);
            return () => {};
        }
        this.observers.add(observer);
        let active = true;
        return () => {
            if (!active) return;
            active = false;
            this.observers.delete(observer);
        };
    }

    interruptUnsafe() {
        if (this.state === "Done" || this.interruptRequested) return;
        this.interruptRequested = true;
        if (this.interruptMask === 0 && this.state === "Suspended") {
            const cancel = this.suspendedCancel;
            this.suspendedCancel = null;
            if (cancel) cancel();
            this.state = "Running";
            this.resumeMethod = "throw";
            this.resumeValue = this.interruptError;
            this.interruptDelivered = true;
            schedule(this, false);
        }
    }

    run() {
        if (this.state !== "Running") return;
        const previousFiber = currentFiber;
        currentFiber = this;
        try {
            let operations = 0;
            while (this.state === "Running" && operations < maxOperationsBeforeYield) {
                operations += 1;
                if (this.interruptRequested && !this.interruptDelivered && this.interruptMask === 0) {
                    this.resumeMethod = "throw";
                    this.resumeValue = this.interruptError;
                    this.interruptDelivered = true;
                }
                const method = this.resumeMethod;
                const value = this.resumeValue;
                this.resumeMethod = "next";
                this.resumeValue = undefined;
                let step;
                try {
                    step = this.iterator[method](value);
                } catch (error) {
                    this.beginClose(failure(error));
                    return;
                }
                if (step.done) {
                    this.beginClose(success(step.value));
                    return;
                }
                if (!step.value || typeof step.value.$ !== "string") {
                    this.beginClose(failure(new TypeError("A task yielded an invalid runtime operation")));
                    return;
                }
                if (this.handle(step.value)) return;
            }
            if (this.state === "Running") schedule(this, true);
        } finally {
            currentFiber = previousFiber;
        }
    }

    handle(operation) {
        switch (operation.$) {
            case "Promise": return this.handlePromise(operation);
            case "Fork": {
                let child;
                try { child = new FiberImpl(operation.task, this).start(); }
                catch (error) { this.resumeMethod = "throw"; this.resumeValue = error; return false; }
                this.resumeValue = child;
                return false;
            }
            case "Join": return this.handleJoin(operation.fiber);
            case "Interrupt": return this.handleInterrupt(operation.fiber);
            case "All": return this.handleAll(operation.tasks);
            case "Race": return this.handleRace(operation.tasks);
            case "Scope": return this.handleScope(operation.task);
            case "Finalizer": return this.handleFinalizer(operation.finalizer);
            case "Mask":
                this.interruptMask = Math.max(0, this.interruptMask + operation.delta);
                return false;
            default:
                this.resumeMethod = "throw";
                this.resumeValue = new TypeError(`Unknown task operation: ${operation.$}`);
                return false;
        }
    }

    suspend(register) {
        this.state = "Suspended";
        let active = true;
        let cleanup = null;
        const resume = (method, value) => {
            if (!active || this.state === "Done") return;
            active = false;
            this.suspendedCancel = null;
            this.state = "Running";
            this.resumeMethod = method;
            this.resumeValue = value;
            schedule(this, false);
        };
        try {
            cleanup = register(resume) ?? null;
        } catch (error) {
            resume("throw", error);
        }
        if (active && this.state === "Suspended") {
            this.suspendedCancel = () => {
                if (!active) return;
                active = false;
                if (cleanup) cleanup();
            };
        }
        return true;
    }

    handlePromise(operation) {
        return this.suspend((resume) => {
            let controller = null;
            let promise;
            try {
                controller = operation.abort ? new AbortController() : null;
                const returned = operation.thunk(controller?.signal);
                let then;
                try { then = returned?.then; }
                catch (error) { throw new ForeignDefect(error, operation.origin); }
                if ((typeof returned !== "object" && typeof returned !== "function")
                    || returned === null || typeof then !== "function") {
                    throw new ForeignDefect(
                        new TypeError("an asynchronous extern did not return a Promise"),
                        operation.origin,
                    );
                }
                promise = Promise.resolve(returned);
            } catch (error) {
                resume("throw", error instanceof ForeignDefect
                    ? error
                    : new ForeignDefect(error, operation.origin));
                return null;
            }
            promise.then(
                (value) => resume("next", value),
                (error) => {
                    if (operation.mapRejected) {
                        try { resume("next", operation.mapRejected(error)); }
                        catch (mappingError) {
                            resume("throw", new ForeignDefect(mappingError, operation.origin));
                        }
                    } else {
                        resume("throw", new ForeignDefect(error, operation.origin));
                    }
                },
            );
            let aborted = false;
            return () => {
                if (controller && !aborted) {
                    aborted = true;
                    controller.abort();
                }
            };
        });
    }

    handleJoin(fiber) {
        if (!(fiber instanceof FiberImpl)) {
            this.resumeMethod = "throw";
            this.resumeValue = new TypeError("Fiber.join expected a Fiber value");
            return false;
        }
        if (fiber.state === "Done") {
            this.resumeMethod = fiber.exit.$ === "Success" ? "next" : "throw";
            this.resumeValue = fiber.exit.$ === "Success" ? fiber.exit.value : fiber.exit.error;
            return false;
        }
        return this.suspend((resume) => fiber.observe((exit) => {
            resume(exit.$ === "Success" ? "next" : "throw", exit.$ === "Success" ? exit.value : exit.error);
        }));
    }

    handleInterrupt(fiber) {
        if (!(fiber instanceof FiberImpl)) {
            this.resumeMethod = "throw";
            this.resumeValue = new TypeError("Fiber.interrupt expected a Fiber value");
            return false;
        }
        fiber.interruptUnsafe();
        if (fiber.state === "Done") return false;
        return this.suspend((resume) => fiber.observe(() => resume("next", undefined)));
    }

    createChildren(tasks) {
        const children = [];
        try {
            for (const task of tasks) children.push(new FiberImpl(task, this));
        } catch (error) {
            for (const child of children) child.interruptUnsafe();
            throw error;
        }
        for (const child of children) child.start();
        return children;
    }

    handleAll(tasks) {
        if (!Array.isArray(tasks)) {
            this.resumeMethod = "throw";
            this.resumeValue = new TypeError("Fiber.all expected an Array of tasks");
            return false;
        }
        let children;
        try { children = this.createChildren(tasks); }
        catch (error) { this.resumeMethod = "throw"; this.resumeValue = error; return false; }
        if (children.length === 0) { this.resumeValue = []; return false; }
        return this.suspend((resume) => {
            const results = new Array(children.length);
            const removers = [];
            let remaining = children.length;
            let settled = false;
            const finishFailure = async (error, winner) => {
                if (settled) return;
                settled = true;
                removers.forEach((remove) => remove());
                children.forEach((child) => { if (child !== winner) child.interruptUnsafe(); });
                await Promise.all(children.map((child) => child.awaitExit()));
                resume("throw", error);
            };
            children.forEach((child, index) => {
                removers.push(child.observe((exit) => {
                    if (settled) return;
                    if (exit.$ === "Failure") {
                        finishFailure(exit.error, child);
                        return;
                    }
                    results[index] = exit.value;
                    remaining -= 1;
                    if (remaining === 0) {
                        settled = true;
                        removers.forEach((remove) => remove());
                        resume("next", results);
                    }
                }));
            });
            return () => {
                if (settled) return;
                settled = true;
                removers.forEach((remove) => remove());
                children.forEach((child) => child.interruptUnsafe());
            };
        });
    }

    handleRace(tasks) {
        if (!Array.isArray(tasks) || tasks.length === 0) {
            this.resumeMethod = "throw";
            this.resumeValue = new TypeError("Fiber.race expected a non-empty Array of tasks");
            return false;
        }
        let children;
        try { children = this.createChildren(tasks); }
        catch (error) { this.resumeMethod = "throw"; this.resumeValue = error; return false; }
        return this.suspend((resume) => {
            const removers = [];
            let settled = false;
            const choose = async (winner, exit) => {
                if (settled) return;
                settled = true;
                removers.forEach((remove) => remove());
                children.forEach((child) => { if (child !== winner) child.interruptUnsafe(); });
                await Promise.all(children.filter((child) => child !== winner).map((child) => child.awaitExit()));
                resume(exit.$ === "Success" ? "next" : "throw", exit.$ === "Success" ? exit.value : exit.error);
            };
            children.forEach((child) => removers.push(child.observe((exit) => choose(child, exit))));
            return () => {
                if (settled) return;
                settled = true;
                removers.forEach((remove) => remove());
                children.forEach((child) => child.interruptUnsafe());
            };
        });
    }

    handleScope(task) {
        let child;
        try { child = new FiberImpl(task, this).start(); }
        catch (error) { this.resumeMethod = "throw"; this.resumeValue = error; return false; }
        return this.suspend((resume) => {
            const remove = child.observe((exit) => {
                resume(exit.$ === "Success" ? "next" : "throw", exit.$ === "Success" ? exit.value : exit.error);
            });
            return () => { remove(); child.interruptUnsafe(); };
        });
    }

    handleFinalizer(finalizer) {
        let pending;
        try { pending = this.scope.add(finalizer); }
        catch (error) { this.resumeMethod = "throw"; this.resumeValue = error; return false; }
        if (!pending) return false;
        return this.suspend((resume) => pending.then(
            (exit) => resume(exit.$ === "Success" ? "next" : "throw", exit.$ === "Success" ? undefined : exit.error),
        ));
    }

    beginClose(exit) {
        if (this.state === "Closing" || this.state === "Done") return;
        this.state = "Closing";
        this.suspendedCancel = null;
        this.scope.close(exit).then(
            (closedExit) => this.complete(closedExit),
            (error) => this.complete(failure(error)),
        );
    }

    complete(exit) {
        if (this.state === "Done") return;
        this.state = "Done";
        this.exit = exit;
        if (this.parent) this.parent.children.delete(this);
        const observers = [...this.observers];
        this.observers.clear();
        this.resolveExit(exit);
        for (const observer of observers) {
            try { observer(exit); }
            catch (error) { console.error("Fiber observer failed", error); }
        }
    }
}

function schedule(fiber, yieldToHost) {
    if (fiber.queued || fiber.state !== "Running") return;
    fiber.queued = true;
    const run = () => {
        fiber.queued = false;
        fiber.run();
    };
    if (yieldToHost) setTimeout(run, 0);
    else queueMicrotask(run);
}

function runFinalizer(task, context) {
    let fiber;
    try {
        fiber = new FiberImpl(task, null, context);
        fiber.interruptMask = 1;
        fiber.start();
    } catch (error) {
        return Promise.resolve(failure(error));
    }
    return fiber.awaitExit();
}

export function $runTask(task) {
    let fiber;
    try { fiber = new FiberImpl(task).start(); }
    catch (error) { return Promise.reject(error); }
    return fiber.awaitExit().then((exit) => {
        if (exit.$ === "Success") return exit.value;
        throw exit.error;
    });
}

export function $runMain(value) {
    return value?.[taskType] === true ? $runTask(value) : Promise.resolve(value);
}

export function $tryPromise(thunk, abort = false, origin = "JavaScript extern", mapRejected = null) {
    return $task(function* () {
        return yield { $: "Promise", thunk, abort, origin, mapRejected };
    });
}

export function $taskSleep(milliseconds) {
    return $tryPromise((signal) => new Promise((resolve, reject) => {
        const timer = setTimeout(resolve, milliseconds);
        signal.addEventListener("abort", () => {
            clearTimeout(timer);
            reject(new Interrupted("sleep"));
        }, { once: true });
    }), true, "Task.sleep");
}

export function $fiberFork(task) {
    return $task(function* () { return yield { $: "Fork", task }; });
}

export function $fiberJoin(fiber) {
    return $task(function* () { return yield { $: "Join", fiber }; });
}

export function $fiberInterrupt(fiber) {
    return $task(function* () { return yield { $: "Interrupt", fiber }; });
}

export function $fiberAll(tasks) {
    return $task(function* () { return yield { $: "All", tasks }; });
}

export function $fiberRace(tasks) {
    return $task(function* () { return yield { $: "Race", tasks }; });
}

export function $fiberScope(task) {
    return $task(function* () { return yield { $: "Scope", task }; });
}

export function $fiberAddFinalizer(task) {
    return $task(function* () {
        return yield { $: "Finalizer", finalizer: () => task };
    });
}

export function $fiberAddFinalizerExit(finalizer) {
    return $task(function* () { return yield { $: "Finalizer", finalizer }; });
}

export function $fiberUninterruptible(task) {
    return $task(function* () {
        yield { $: "Mask", delta: 1 };
        try { return yield* task; }
        finally { yield { $: "Mask", delta: -1 }; }
    });
}

export function $tryCatch(thunk) {
    try {
        return { $: "Ok", _0: thunk() };
    } catch (error) {
        return { $: "Err", _0: error };
    }
}

export function $providerPush(key, value) {
    const context = currentFiber?.context ?? synchronousProviderContext;
    let stack = context.get(key);
    if (!stack) context.set(key, stack = []);
    stack.push(value);
}

export function $providerPop(key) {
    const context = currentFiber?.context ?? synchronousProviderContext;
    const stack = context.get(key);
    if (!stack?.length) throw new Error(`Provider stack underflow: ${key}`);
    stack.pop();
    if (stack.length === 0) context.delete(key);
}

export function $providerGet(key) {
    const context = currentFiber?.context ?? synchronousProviderContext;
    const stack = context.get(key);
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
            const result = await $runMain(test.run());
            if (result?.$ === "Err") throw result._0;
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
