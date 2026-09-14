// Tagged graph transport shared by hydration, navigation, and remote calls.
// User records never double as protocol tags. References preserve aliases and
// cycles; boxed Option values retain their otherwise non-serializable identity.
export function $webEncode(value) {
  const nodes = [], seen = new Map();
  let visits = 0;
  const data = (value, key) => {
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    if (!descriptor || !Object.hasOwn(descriptor, "value")) throw new TypeError("Alder transport cannot encode accessors or sparse arrays");
    return descriptor.value;
  };
  function encode(value, depth = 0) {
    if (++visits > 1000000 || depth > 128) throw new RangeError("Alder transport traversal budget exceeded");
    if (value === null || typeof value === "string" || typeof value === "boolean") return value;
    if (typeof value === "number") {
      if (Number.isFinite(value) && !Object.is(value, -0)) return value;
      return ["number", String(value), Object.is(value, -0)];
    }
    if (value === undefined) return ["undefined"];
    if (typeof value === "bigint") return ["bigint", String(value)];
    if (typeof value !== "object") throw new TypeError("Alder transport cannot encode functions or symbols");
    if (seen.has(value)) return ["ref", seen.get(value)];
    if (nodes.length >= 100000) throw new RangeError("Alder transport node budget exceeded");
    if (Object.getOwnPropertySymbols(value).length) throw new TypeError("Alder transport cannot encode symbol fields");
    const index = nodes.length;
    seen.set(value, index);
    nodes.push(null);
    let node;
    const prototype = Object.getPrototypeOf(value);
    const ownKeys = Object.getOwnPropertyNames(value);
    const nested = value => encode(value, depth + 1);
    if (optionBoxes.has(value)) {
      if (ownKeys.length !== 2 || data(value, "$") !== "Some") throw new TypeError("Alder transport requires unmodified Option boxes");
      node = ["option", nested(data(value, "_0"))];
    } else if (Array.isArray(value)) {
      if (prototype !== Array.prototype || ownKeys.length !== value.length + 1) throw new TypeError("Alder transport requires dense plain arrays");
      node = ["array", Array.from({length: value.length}, (_, index) => nested(data(value, String(index))))];
    } else if (value instanceof Map) {
      if (prototype !== Map.prototype || ownKeys.length) throw new TypeError("Alder transport requires plain Maps");
      node = ["map", [...Map.prototype.entries.call(value)].map(([key, item]) => [nested(key), nested(item)])];
    } else if (value instanceof Set) {
      if (prototype !== Set.prototype || ownKeys.length) throw new TypeError("Alder transport requires plain Sets");
      node = ["set", [...Set.prototype.values.call(value)].map(nested)];
    } else if (value instanceof Date) {
      if (prototype !== Date.prototype || ownKeys.length) throw new TypeError("Alder transport requires plain Dates");
      node = ["date", Date.prototype.toISOString.call(value)];
    }
    else {
      if (prototype !== Object.prototype && prototype !== null) throw new TypeError("Alder transport requires plain records");
      if (ownKeys.length !== Object.keys(value).length) throw new TypeError("Alder transport cannot encode hidden record fields");
      node = ["record", Object.keys(value).sort().map(key => {
        return [key, nested(data(value, key))];
      })];
    }
    nodes[index] = node;
    return ["ref", index];
  }
  const root = encode(value);
  // Decode also validates every graph entry after visiting the root.
  if (visits + nodes.length > 1000000) throw new RangeError("Alder transport traversal budget exceeded");
  return JSON.stringify({ version: 1, root, nodes })
    .replaceAll("<", "\\u003c").replaceAll(">", "\\u003e")
    .replaceAll("&", "\\u0026").replaceAll("\u2028", "\\u2028").replaceAll("\u2029", "\\u2029");
}

export function $webDecode(text) {
  const document = JSON.parse(text);
  const fail = () => { throw new TypeError("Invalid Alder transport payload"); };
  if (document?.version !== 1 || !Array.isArray(document.nodes)) fail();
  if (document.nodes.length > 100000) throw new RangeError("Alder transport node budget exceeded");
  const values = [], done = new Set();
  let visits = 0;
  for (const node of document.nodes) {
    if (!Array.isArray(node) || node.length !== 2) fail();
    switch (node[0]) {
      case "record": values.push({}); break;
      case "array": values.push([]); break;
      case "map": values.push(new Map()); break;
      case "set": values.push(new Set()); break;
      case "option": values.push($optionBox(undefined)); break;
      case "date": {
        if (typeof node[1] !== "string") fail();
        const date = new Date(node[1]);
        if (!Number.isFinite(date.getTime())) fail();
        values.push(date); break;
      }
      default: fail();
    }
  }
  function decode(token, depth = 0) {
    if (++visits > 1000000 || depth > 128) throw new RangeError("Alder transport traversal budget exceeded");
    if (token === null || typeof token === "string" || typeof token === "boolean") return token;
    if (typeof token === "number" && Number.isFinite(token)) return token;
    if (!Array.isArray(token)) return fail();
    if (token[0] === "undefined" && token.length === 1) return undefined;
    if (token[0] === "bigint" && token.length === 2 && typeof token[1] === "string" && /^-?\d+$/.test(token[1])) return BigInt(token[1]);
    if (token[0] === "number" && token.length === 3 && typeof token[2] === "boolean") {
      if (token[1] === "0" && token[2]) return -0;
      if (!token[2] && ["NaN", "Infinity", "-Infinity"].includes(token[1])) return Number(token[1]);
      return fail();
    }
    if (token[0] !== "ref" || token.length !== 2 || !Number.isInteger(token[1]) || token[1] < 0 || token[1] >= values.length) return fail();
    const index = token[1], value = values[index], [kind, content] = document.nodes[index];
    if (done.has(index)) return value;
    done.add(index);
    if (kind === "option") value._0 = decode(content, depth + 1);
    else if (kind !== "date") {
      if (!Array.isArray(content)) fail();
      const keys = new Set();
      for (const item of content) {
        if (kind === "array") value.push(decode(item, depth + 1));
        else if (kind === "set") value.add(decode(item, depth + 1));
        else {
          if (!Array.isArray(item) || item.length !== 2) fail();
          if (kind === "map") value.set(decode(item[0], depth + 1), decode(item[1], depth + 1));
          else {
            if (typeof item[0] !== "string" || keys.has(item[0])) fail();
            keys.add(item[0]);
            Object.defineProperty(value, item[0], { value: decode(item[1], depth + 1), enumerable: true, writable: true, configurable: true });
          }
        }
      }
    }
    return value;
  }
  const root = decode(document.root);
  // Validate even unreachable graph entries in untrusted HTTP payloads.
  for (let index = 0; index < values.length; index++) decode(["ref", index]);
  return root;
}
