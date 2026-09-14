// Validate decoded remote arguments/results against compiler-generated schemas.
// No coercions, source evaluation, user getters, or prototype setters are used.
export function $webValidate(value, schema, limits = {}) {
  const maxDepth = limits.maxDepth ?? 128;
  const maxVisits = limits.maxVisits ?? 100000;
  if (!Number.isInteger(maxDepth) || maxDepth < 1 || maxDepth > 128
      || !Number.isInteger(maxVisits) || maxVisits < 1 || maxVisits > 100000) {
    throw new TypeError("Invalid remote validation limits");
  }
  if (!schema || !Array.isArray(schema.nodes) || schema.nodes.length > 4096
      || !Number.isInteger(schema.root) || schema.root < 0 || schema.root >= schema.nodes.length) {
    throw new TypeError("Invalid remote wire schema");
  }
  const stack = [], seen = new WeakMap();
  let visits = 0;
  const fail = (path, expected) => { throw new TypeError(`Invalid remote value at ${path}: expected ${expected}`); };
  const push = (value, index, depth, path) => {
    if (index !== -1 && (!Number.isInteger(index) || index < 0 || index >= schema.nodes.length)) {
      throw new TypeError("Invalid remote schema reference");
    }
    if (++visits > maxVisits || depth > maxDepth) fail(path, "a value within remote validation limits");
    stack.push([value, index, depth, path]);
  };
  const plain = (value, path) => {
    if (value === null || typeof value !== "object" || optionBoxes.has(value)) fail(path, "a plain record");
    const prototype = Object.getPrototypeOf(value);
    if (prototype !== Object.prototype && prototype !== null) fail(path, "a plain record");
  };
  const field = (value, name, path) => {
    const descriptor = Object.getOwnPropertyDescriptor(value, name);
    if (!descriptor || !Object.hasOwn(descriptor, "value") || !descriptor.enumerable) fail(path, "an own data field");
    return descriptor.value;
  };
  const keys = (value, expected, path) => {
    const actual = Reflect.ownKeys(value);
    const names = new Set(expected);
    if (actual.length !== expected.length || names.size !== expected.length
        || actual.some(key => typeof key !== "string" || !names.has(key))) {
      fail(path, "exactly the declared record fields");
    }
  };
  const record = (value, fields, depth, path, tag = null) => {
    plain(value, path);
    if (!Array.isArray(fields) || fields.some(item => !Array.isArray(item) || item.length !== 2 || typeof item[0] !== "string")) {
      throw new TypeError("Invalid remote record schema");
    }
    keys(value, [...fields.map(item => item[0]), ...(tag === null ? [] : ["$"])], path);
    if (tag !== null && field(value, "$", path) !== tag) fail(path, `the ${tag} variant`);
    for (const [name, index] of fields) push(field(value, name, `${path}.${name}`), index, depth + 1, `${path}.${name}`);
  };
  const array = (value, depth, path, item, tuple = null) => {
    if (!Array.isArray(value) || Object.getPrototypeOf(value) !== Array.prototype) fail(path, "an array");
    if (tuple !== null && (!Array.isArray(tuple) || value.length !== tuple.length)) fail(path, "the exact tuple/argument arity");
    if (value.length + visits > maxVisits) fail(path, "an array within remote validation limits");
    const actual = Reflect.ownKeys(value);
    if (actual.length !== value.length + 1 || actual.some(key => key !== "length" && (typeof key !== "string" || !/^(0|[1-9][0-9]*)$/.test(key) || Number(key) >= value.length))) {
      fail(path, "a dense array without extra properties");
    }
    for (let index = 0; index < value.length; index++) {
      push(field(value, String(index), `${path}[${index}]`), tuple === null ? item : tuple[index], depth + 1, `${path}[${index}]`);
    }
  };
  push(value, schema.root, 0, "$value");
  while (stack.length) {
    const [value, index, depth, path] = stack.pop();
    const node = index === -1 ? ["data"] : schema.nodes[index];
    if (!Array.isArray(node) || typeof node[0] !== "string") throw new TypeError("Invalid remote schema node");
    const kind = node[0];
    // Option is unboxed unless needed to distinguish nested None. It does not
    // guard structural recursion, so never memoize an Option-only cycle.
    if (value !== null && typeof value === "object" && kind !== "option") {
      let visited = seen.get(value);
      if (visited?.has(index)) continue;
      if (!visited) { visited = new Set(); seen.set(value, visited); }
      visited.add(index);
    }
    switch (kind) {
      case "unit": if (value !== undefined) fail(path, "Unit"); break;
      case "bool": if (typeof value !== "boolean") fail(path, "Bool"); break;
      case "number": if (typeof value !== "number") fail(path, "Number"); break;
      case "bigint": if (typeof value !== "bigint") fail(path, "BigInt"); break;
      case "string": if (typeof value !== "string") fail(path, "String"); break;
      case "option": {
        if (value === null) break;
        if (optionBoxes.has(value)) {
          keys(value, ["$", "_0"], path);
          if (field(value, "$", path) !== "Some") fail(path, "a boxed Option");
          const payload = field(value, "_0", path);
          if (payload !== null && !optionBoxes.has(payload)) fail(path, "a canonical nested Option box");
          push(payload, node[1], depth + 1, path);
        } else push(value, node[1], depth + 1, path);
        break;
      }
      case "array": array(value, depth, path, node[1]); break;
      case "tuple": array(value, depth, path, null, node[1]); break;
      case "record": record(value, node[1], depth, path); break;
      case "enum":
      case "error": {
        plain(value, path);
        const tag = field(value, "$", path);
        if (typeof tag !== "string" || !Array.isArray(node[1])) fail(path, "a tagged enum value");
        const variant = node[1].find(variant => Array.isArray(variant) && variant[0] === tag);
        if (variant) record(value, variant[1], depth, path, tag);
        else if (kind === "error" && node[2] === true && /^:[A-Za-z_][A-Za-z0-9_]*$/.test(tag)) {
          const count = Reflect.ownKeys(value).length - 1;
          if (count + visits > maxVisits) fail(path, "an error payload within remote validation limits");
          record(value, Array.from({length: count}, (_, index) => [`_${index}`, -1]), depth, path, tag);
        } else fail(path, "a declared enum/error variant");
        break;
      }
      case "map": {
        if (value === null || typeof value !== "object" || Object.getPrototypeOf(value) !== Map.prototype) fail(path, "Map");
        keys(value, [], path);
        for (const [key, item] of Map.prototype.entries.call(value)) {
          push(key, node[1], depth + 1, `${path}.key`);
          push(item, node[2], depth + 1, `${path}.value`);
        }
        break;
      }
      case "set": {
        if (value === null || typeof value !== "object" || Object.getPrototypeOf(value) !== Set.prototype) fail(path, "Set");
        keys(value, [], path);
        for (const item of Set.prototype.values.call(value)) push(item, node[1], depth + 1, `${path}.item`);
        break;
      }
      case "date": {
        if (value === null || typeof value !== "object" || Object.getPrototypeOf(value) !== Date.prototype
            || !Number.isFinite(Date.prototype.getTime.call(value))) fail(path, "Date");
        keys(value, [], path);
        break;
      }
      case "data": {
        if (value === null || value === undefined || ["string", "number", "boolean", "bigint"].includes(typeof value)) break;
        if (typeof value !== "object") fail(path, "serializable error data");
        if (optionBoxes.has(value)) {
          keys(value, ["$", "_0"], path);
          if (field(value, "$", path) !== "Some") fail(path, "a boxed Option");
          push(field(value, "_0", path), -1, depth + 1, path);
        } else if (Array.isArray(value)) array(value, depth, path, -1);
        else if (Object.getPrototypeOf(value) === Map.prototype) {
          keys(value, [], path);
          for (const [key,item] of Map.prototype.entries.call(value)) { push(key,-1,depth+1,path); push(item,-1,depth+1,path); }
        } else if (Object.getPrototypeOf(value) === Set.prototype) {
          keys(value, [], path);
          for (const item of Set.prototype.values.call(value)) push(item,-1,depth+1,path);
        } else if (Object.getPrototypeOf(value) === Date.prototype) {
          if (!Number.isFinite(Date.prototype.getTime.call(value))) fail(path,"Date");
          keys(value, [], path);
        } else {
          plain(value, path);
          const names = Reflect.ownKeys(value);
          if (names.some(name => typeof name !== "string") || names.length + visits > maxVisits) fail(path, "serializable record fields within validation limits");
          for (const name of names) push(field(value,name,path),-1,depth+1,path);
        }
        break;
      }
      default: throw new TypeError("Invalid remote schema kind");
    }
  }
  return value;
}
