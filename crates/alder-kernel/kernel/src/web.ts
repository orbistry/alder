// The first M6 renderer uses the JavaScript subset of TypeScript, like index.ts.
// Dependencies are supplied by the compiler. Getters never track reads.
const webStoreRegistryKey = Symbol.for("alder/webStores/v1");
const webStoreScopeKey = Symbol.for("alder/webStoreScope/v1");
const webStoreHandleKey = Symbol.for("alder/webStore/v1");
const webResourceContextKey = Symbol.for("alder/webResource/v1");
const webStoreRegistry = globalThis[webStoreRegistryKey] ??= { definitions: new Map(), browser: null };

export function $webCreateStoreScope(snapshot = []) {
  return { cells: new Map(), snapshot: new Map(snapshot.map(entry => [entry.key, entry])), initializing: new Set(), incompatible: new Set(), renderOwners: new Set(), owner: null, closed: false,
    scheduler: { memos: new Set(), effects: new Set(), flushing: false }, queryResources: new Map() };
}

function webAmbientStoreScope() {
  const stack = (currentFiber?.context ?? synchronousProviderContext).get(webStoreScopeKey);
  return stack?.[stack.length - 1] ?? null;
}

export function $webCurrentStoreScope() { return webAmbientStoreScope() ?? webStoreRegistry.browser; }

function webBrowserStoreScope() { return webStoreRegistry.browser ??= $webCreateStoreScope(); }

export function $webStoreRun(scope, callback) {
  if (!scope) return callback();
  if (scope.closed) throw new Error("Alder store request scope is closed");
  const context = currentFiber?.context ?? synchronousProviderContext;
  const previous = context.get(webStoreScopeKey);
  context.set(webStoreScopeKey, [...(previous ?? []), scope]);
  try { return callback(); }
  finally { if (previous) context.set(webStoreScopeKey, previous); else context.delete(webStoreScopeKey); }
}

export function $webDisposeStoreScope(scope) {
  if (scope.closed) return;
  scope.closed = true;
  for (const owner of [...scope.renderOwners]) webDispose(owner);
  if (scope.owner) webDispose(scope.owner);
}

export function $webWithStoreScope(scopeOrSnapshot, createTask) {
  const owned = !scopeOrSnapshot?.cells;
  const scope = owned ? $webCreateStoreScope(scopeOrSnapshot ?? []) : scopeOrSnapshot;
  return $task(function* () {
    $providerPush(webStoreScopeKey, scope);
    try {
      const task = createTask();
      return task?.[taskType] ? yield* task : yield* $tryPromise(() => Promise.resolve(task));
    } finally {
      $providerPop(webStoreScopeKey);
      if (owned) $webDisposeStoreScope(scope);
    }
  });
}

export function $webStore(module, binding, initialize, signature) {
  const handle = { [webStoreHandleKey]: true, key: module + "#" + binding, initialize, signature };
  webStoreRegistry.definitions.set(handle.key, handle);
  return handle;
}

export function $webStoreCell(handle) {
  if (!handle?.[webStoreHandleKey]) throw new TypeError("Expected an Alder module store");
  const scope = $webCurrentStoreScope();
  if (!scope) throw new Error("Alder module store read requires a request or component context: " + handle.key);
  if (scope.closed) throw new Error("Alder store request scope is closed");
  const existing = scope.cells.get(handle.key);
  if (existing?.signature === handle.signature) return existing.cell;
  if (scope.initializing.has(handle.key)) throw new Error("Alder cyclic module store initialization: " + handle.key);
  scope.initializing.add(handle.key);
  try {
    const saved = scope.snapshot.get(handle.key);
    const compatible = saved?.signature === handle.signature;
    if (existing || (saved && !compatible)) scope.incompatible.add(handle.key);
    if (!scope.owner) scope.owner = webOwner(null, "stores", scope);
    if (existing) webDispose(existing.owner);
    const initial = compatible ? saved.value : $webStoreRun(scope, handle.initialize);
    const owner = webOwner(scope.owner, handle.key);
    const cell = $webState(owner, initial, handle.key);
    scope.cells.set(handle.key, {signature: handle.signature, cell, owner});
    scope.snapshot.delete(handle.key);
    return cell;
  } finally { scope.initializing.delete(handle.key); }
}

export function $webStoreSnapshot(scope = $webCurrentStoreScope(), allowedKeys = null) {
  if (!scope) return [];
  const entries = new Map(scope.snapshot);
  for (const [key, entry] of scope.cells) entries.set(key, {key, signature: entry.signature, value: entry.cell.value});
  const allowed = allowedKeys == null ? null : new Set(allowedKeys);
  return [...entries.values()].filter(entry => !allowed || allowed.has(entry.key)).sort((left, right) => left.key.localeCompare(right.key));
}

export function $webRestoreStores(snapshot, preserveExisting = false) {
  const scope = webBrowserStoreScope();
  for (const entry of snapshot) {
    if (preserveExisting && (scope.cells.has(entry.key) || scope.snapshot.has(entry.key))) continue;
    const existing = scope.cells.get(entry.key);
    if (existing?.signature === entry.signature) existing.cell.value = entry.value;
    else { if (existing) webDispose(existing.owner); scope.cells.delete(entry.key); scope.snapshot.set(entry.key, entry); }
  }
  return scope;
}

function webOwner(parent = null, key = "root", storeScope = null) {
  storeScope = parent?.storeScope ?? storeScope ?? $webCurrentStoreScope();
  const owner = { disposed: false, cleanups: [], children: new Set(), cells: [], nextRegion: 0, id: parent ? parent.id + "/" + key : key, restore: parent?.restore ?? null,
    report: parent?.report ?? (error => { queueMicrotask(() => { throw error; }); }),
    storeScope, context: cloneContext(parent?.context ?? currentFiber?.context ?? synchronousProviderContext), hydrationQueries: parent?.hydrationQueries ?? null,
    mode: parent?.mode ?? "host", hydrating: parent?.hydrating ?? false, resources: new Map(), resourceTasks: parent?.resourceTasks ?? new Set(), hydrationResources: parent?.hydrationResources ?? null,
    scheduler: parent?.scheduler ?? storeScope?.scheduler ?? { memos: new Set(), effects: new Set(), flushing: false } };
  if (parent) { parent.children.add(owner); owner.cleanups.push(() => parent.children.delete(owner)); }
  else if (storeScope && key !== "stores") { storeScope.renderOwners.add(owner); owner.cleanups.push(() => storeScope.renderOwners.delete(owner)); }
  return owner;
}

function webOwnerRun(owner, callback) {
  const context = currentFiber?.context ?? synchronousProviderContext;
  const previous = new Map(context);
  context.clear();
  for (const [key, values] of owner.context) context.set(key, [...values]);
  try { return $webStoreRun(owner.storeScope, callback); }
  finally {
    context.clear();
    for (const [key, values] of previous) context.set(key, values);
  }
}

function webDispose(owner) {
  if (owner.disposed) return;
  owner.disposed = true;
  for (const child of [...owner.children]) webDispose(child);
  for (const cleanup of owner.cleanups.reverse()) cleanup();
  owner.cleanups.length = 0;
}

function webSubscribe(owner, dependencies, run, memo = false) {
  if (!dependencies.length) return;
  const subscriber = { run: () => { if (!owner.disposed) webOwnerRun(owner, run); }, memo };
  for (const dependency of new Set(dependencies)) dependency.subscribers.add(subscriber);
  owner.cleanups.push(() => {
    for (const dependency of dependencies) dependency.subscribers.delete(subscriber);
    owner.scheduler.memos.delete(subscriber);
    owner.scheduler.effects.delete(subscriber);
  });
}

function webFlush(owner) {
  if (owner.disposed) return;
  const scheduler = owner.scheduler;
  if (scheduler.flushing) return;
  scheduler.flushing = true;
  try {
    while (scheduler.memos.size || scheduler.effects.size) {
      const queue = scheduler.memos.size ? scheduler.memos : scheduler.effects;
      const subscriber = queue.values().next().value;
      queue.delete(subscriber);
      subscriber.run();
    }
  } finally {
    scheduler.flushing = false;
    scheduler.memos.clear();
    scheduler.effects.clear();
  }
}

function webShape(value, seen = new Set()) {
  if (value === null) return "null";
  if (typeof value === "object") {
    if (seen.has(value)) return "recursive";
    seen.add(value);
    if (Array.isArray(value)) return "array:" + [...new Set(value.map(item => webShape(item, seen)))].sort().join("|");
    return "object:" + Object.keys(value).sort().map(key => key + ":" + webShape(value[key], seen)).join(",");
  }
  return typeof value;
}

class WebRestoreMismatch extends Error {}

export function $webState(owner, initial, key = null, writable = true) {
  const id = owner.id + (writable ? "/state/" : "/input/") + (key ?? owner.cells.length);
  const saved = writable ? owner.restore?.cells.get(id) : null;
  if (saved && saved.shape !== webShape(initial)) throw new WebRestoreMismatch("state shape changed at " + id);
  let value = saved ? saved.value : initial;
  const subscribers = new Set();
  const cell = {
    id, writable, shape: webShape(initial),
    subscribers,
    get value() { return value; },
    set value(next) {
      if (owner.disposed || Object.is(value, next)) return;
      value = next;
      for (const subscriber of subscribers) {
        (subscriber.memo ? owner.scheduler.memos : owner.scheduler.effects).add(subscriber);
      }
      webFlush(owner);
    },
  };
  owner.cells.push(cell);
  return cell;
}

const webProps = Symbol("alder/webProps");
export function $webProps(owner, dependencies, read) { return { [webProps]: true, owner, dependencies, read }; }
export function $webInput(owner, input) {
  return input?.[webProps] ? $webMemo(owner, input.dependencies, input.read) : $webState(owner, input, null, false);
}

export function $webMemo(owner, dependencies, compute) {
  const cell = $webState(owner, compute(), null, false);
  webSubscribe(owner, dependencies, () => { cell.value = compute(); }, true);
  return cell;
}

export function $webResourceDeclaration() {
  throw new Error("Alder resource must initialize a direct component or directive binding");
}

export function $webResourceTrack(module) {
  const stack = (currentFiber?.context ?? synchronousProviderContext).get(webResourceContextKey);
  const token = stack?.[stack.length - 1];
  if (token?.active) token.track(module);
  return token?.active === true && token.force === true;
}

export function $webResourceInvalidate(module, scope = $webCurrentStoreScope()) {
  if (!scope || scope.closed) return;
  const resources = module == null
    ? new Set([...scope.queryResources.values()].flatMap(group => [...group]))
    : new Set(scope.queryResources.get(module) ?? []);
  for (const resource of resources) resource.refresh(true);
}

export function $webResource(owner, dependencies, load, key) {
  if (owner.mode === "ssr-sync") throw new Error("Alder resources require asynchronous SSR via $webSsrAsync");
  const id = owner.id + "/resource/" + key;
  if (owner.hydrating && !owner.hydrationResources?.has(id)) throw new Error("Alder hydration mismatch: missing resource " + id);
  const hydrated = owner.hydrationResources?.get(id);
  const cell = $webState(owner, hydrated ?? { $: "Loading" }, key, false);
  cell.resourceId = id;
  cell.defect = null;
  let generation = 0;
  let running = null;
  let token = null;
  const queries = new Set();
  cell.queries = queries;
  function track(module) {
    queries.add(module);
    if (!owner.storeScope) return;
    let resources = owner.storeScope.queryResources.get(module);
    if (!resources) owner.storeScope.queryResources.set(module, resources = new Set());
    resources.add(cell);
  }
  const context = cloneContext(currentFiber?.context ?? synchronousProviderContext);
  function defect(error, current) {
    cell.defect = error;
    if (owner.mode === "browser") queueMicrotask(() => { if (!owner.disposed && current === generation) owner.report(error); });
  }
  cell.cancel = () => {
    generation++;
    if (token) token.active = false;
    for (const module of queries) {
      const resources = owner.storeScope?.queryResources.get(module);
      resources?.delete(cell);
      if (resources?.size === 0) owner.storeScope.queryResources.delete(module);
    }
    queries.clear();
    running?.interruptUnsafe();
    running = null;
  };
  cell.refresh = (force = false) => {
    if (owner.disposed) return;
    cell.cancel();
    const current = generation;
    cell.defect = null;
    cell.value = { $: "Loading" };
    let fiber;
    token = {active: true, track, force};
    const resourceContext = cloneContext(context);
    resourceContext.set(webResourceContextKey, [token]);
    try { fiber = new FiberImpl(webOwnerRun(owner, load), null, resourceContext).start(); }
    catch (error) { defect(error, current); return; }
    running = fiber;
    const pending = fiber.awaitExit().then(exit => {
      if (owner.disposed || current !== generation) return;
      running = null;
      if (exit.$ === "Failure") defect(exit.error, current);
      else if (exit.value?.$ === "Ok") cell.value = { $: "Ready", _0: exit.value._0 };
      else if (exit.value?.$ === "Err") cell.value = { $: "Failed", _0: exit.value._0 };
      else defect(new TypeError("Alder resource loader must return Task[Result]"), current);
    }).finally(() => { owner.resourceTasks.delete(pending); });
    owner.resourceTasks.add(pending);
    cell.pending = pending;
  };
  owner.resources.set(id, cell);
  owner.cleanups.push(cell.cancel);
  webSubscribe(owner, dependencies, cell.refresh, true);
  if (hydrated) for (const module of owner.hydrationQueries?.get(id) ?? []) track(module);
  if (!hydrated) cell.refresh();
  return cell;
}

export function $webResourceRefresh(resource) {
  if (!resource?.refresh) throw new TypeError("refresh requires an owned resource binding");
  resource.refresh(true);
}

export function $webResourceCancel(resource) {
  if (!resource?.cancel) throw new TypeError("cancel requires an owned resource binding");
  resource.cancel();
}

async function webWaitResources(owner) {
  while (owner.resourceTasks.size) await Promise.all([...owner.resourceTasks]);
  function check(owner) {
    if (owner.disposed) throw new Error("Alder resource owner was disposed during SSR");
    for (const resource of owner.resources.values()) if (resource.defect) throw resource.defect;
    for (const child of owner.children) check(child);
  }
  check(owner);
}

export function $webComponent(setup, identity = "anonymous", signature = "") {
  return { identity, signature, setup(owner) {
    owner.signature = identity + ":" + signature;
    const previous = owner.restore?.components.get(owner.id);
    if (previous !== undefined && previous !== owner.signature) throw new WebRestoreMismatch("component signature changed at " + owner.id);
    return webOwnerRun(owner, () => setup(owner));
  } };
}

function webEscape(value, attribute = false) {
  const escaped = String(value).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
  return attribute ? escaped.replaceAll('"', "&quot;").replaceAll("'", "&#39;") : escaped;
}

function webAttribute(value) {
  return value === false ? null : value === true ? "" : String(value);
}

const webVoidTags = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"]);

export function $webSsr(component) {
  const ambientScope = webAmbientStoreScope();
  const storeScope = ambientScope ?? $webCreateStoreScope();
  let owner = webOwner(null, component.identity ?? "root", storeScope);
  owner.mode = "ssr-sync";
  const rootOwner = owner;
  const output = [];
  const tags = [];
  let pending = false;
  const finishOpen = () => {
    if (pending) output.push(tags[tags.length - 1] === "textarea" ? ">\n" : ">");
    pending = false;
  };
  const renderer = {
    open(tag) {
      finishOpen();
      output.push("<", tag);
      tags.push(tag);
      pending = true;
    },
    attr(name, read) {
      const value = webAttribute(read());
      if (value !== null) output.push(" ", name, '="', webEscape(value, true), '"');
    },
    event() {},
    eventValue() {},
    value(read) {
      const value = read();
      if (value?.setup) {
        renderer.region(() => ["component", undefined, child => value.setup(child)]);
      } else renderer.text(() => value);
    },
    region(read) {
      finishOpen();
      output.push("<!--alder:region-->");
      const selected = read();
      if (selected) {
        const parent = owner;
        owner = webOwner(parent, selected[0]);
        try { selected[2](owner, $webState(owner, selected[1], null, false))(renderer); } finally { owner = parent; }
      }
      finishOpen();
      output.push("<!--/alder:region-->");
    },
    list(read, dependencies, key, setup, empty) {
      finishOpen();
      output.push("<!--alder:list-->");
      const items = Array.from(read());
      const keys = new Set();
      for (let index = 0; index < items.length; index++) {
        const id = key(items[index], index);
        if (keys.has(id)) throw new Error("Alder duplicate list key: " + id);
        keys.add(id);
        renderer.region(() => [id, items[index], setup]);
      }
      if (!items.length && empty) renderer.region(() => ["empty", undefined, empty]);
      output.push("<!--/alder:list-->");
    },
    text(read) {
      finishOpen();
      if (["textarea", "title"].includes(tags[tags.length - 1])) output.push(webEscape(read()));
      else output.push("<!--alder:text-->", webEscape(read()), "<!--/alder:text-->");
    },
    close() {
      finishOpen();
      const tag = tags.pop();
      if (!webVoidTags.has(tag)) output.push("</", tag, ">");
    },
  };
  try {
    webOwnerRun(owner, () => component.setup(owner)(renderer));
    return output.join("");
  } finally {
    webDispose(rootOwner);
    if (!ambientScope) $webDisposeStoreScope(storeScope);
  }
}

export async function $webSsrAsync(component, options = {}) {
  const ambientScope = options.storeScope ?? webAmbientStoreScope();
  const storeScope = ambientScope ?? $webCreateStoreScope();
  const root = webOwner(null, component.identity ?? "root", storeScope);
  root.mode = "ssr-async";
  const output = [];
  const tags = [];
  let pending = false;
  const finishOpen = () => { if (pending) output.push(tags[tags.length - 1] === "textarea" ? ">\n" : ">"); pending = false; };
  function text(value) {
    finishOpen();
    if (["textarea", "title"].includes(tags[tags.length - 1])) output.push(webEscape(value));
    else output.push("<!--alder:text-->", webEscape(value), "<!--/alder:text-->");
  }
  async function setup(owner, initialize) {
    if (owner.disposed) throw new Error("Alder resource owner was disposed during SSR");
    const program = webOwnerRun(owner, () => initialize(owner));
    await webWaitResources(owner);
    const steps = [];
    const renderer = Object.fromEntries(["open", "attr", "event", "eventValue", "text", "value", "region", "list", "close"].map(name => [name, (...args) => steps.push([name, args])]));
    webOwnerRun(owner, () => program(renderer));
    for (const [operation, args] of steps) {
      switch (operation) {
        case "open":
          finishOpen(); output.push("<", args[0]); tags.push(args[0]); pending = true;
          break;
        case "attr": {
          const value = webAttribute(webOwnerRun(owner, args[1]));
          if (value !== null) output.push(" ", args[0], '="', webEscape(value, true), '"');
          break;
        }
        case "text": text(webOwnerRun(owner, args[0])); break;
        case "value": {
          const value = webOwnerRun(owner, args[0]);
          if (value?.setup) await region(owner, [value, undefined, child => value.setup(child)]);
          else text(value);
          break;
        }
        case "region": await region(owner, webOwnerRun(owner, args[0])); break;
        case "list": {
          const [read, , key, initialize, empty] = args;
          const identity = "list/" + owner.nextRegion++;
          finishOpen(); output.push("<!--alder:list-->");
          const items = Array.from(webOwnerRun(owner, read));
          const keys = new Set();
          for (let index = 0; index < items.length; index++) {
            const id = webOwnerRun(owner, () => key(items[index], index));
            if (keys.has(id)) throw new Error("Alder duplicate list key: " + id);
            keys.add(id);
            await region(owner, [id, items[index], initialize], identity + "/key/" + typeof id + ":" + encodeURIComponent(String(id)));
          }
          if (!items.length && empty) await region(owner, ["empty", undefined, empty], identity + "/empty");
          output.push("<!--/alder:list-->");
          break;
        }
        case "close": {
          finishOpen();
          const tag = tags.pop();
          if (!webVoidTags.has(tag)) output.push("</", tag, ">");
          break;
        }
      }
    }
  }
  async function region(parent, selected, identity = "region/" + parent.nextRegion++) {
    finishOpen(); output.push("<!--alder:region-->");
    if (selected) {
      const owner = webOwner(parent, identity + "/" + String(selected[0]?.identity ?? selected[0]));
      const item = $webState(owner, selected[1], null, false);
      await setup(owner, child => selected[2](child, item));
    }
    finishOpen(); output.push("<!--/alder:region-->");
  }
  const abort = () => webDispose(root);
  options.signal?.addEventListener("abort", abort, {once: true});
  try {
    if (options.signal?.aborted) abort();
    await setup(root, owner => component.setup(owner));
    const resources = [];
    function collect(owner) {
      for (const [id, resource] of owner.resources) resources.push({id, value: resource.value, queries: [...resource.queries]});
      for (const child of owner.children) collect(child);
    }
    collect(root);
    return {html: output.join(""), resources, stores: $webStoreSnapshot(storeScope)};
  } finally {
    options.signal?.removeEventListener("abort", abort);
    webDispose(root);
    await Promise.allSettled([...root.resourceTasks]);
    if (!ambientScope) $webDisposeStoreScope(storeScope);
  }
}

export function $webSsrAsyncString(component) {
  return $tryPromise(signal => $webSsrAsync(component, {signal}).then(result => result.html), true, "html.renderToStringAsync");
}

function webRender(component, target, hydrate, options = {}) {
  let owner = webOwner(null, component.identity ?? "root", webBrowserStoreScope());
  owner.mode = "browser";
  if (options.onError) owner.report = options.onError;
  owner.hydrating = hydrate;
  owner.hydrationResources = options.resources ? new Map(options.resources.map(entry => [entry.id, entry.value])) : null;
  owner.hydrationQueries = options.resources ? new Map(options.resources.map(entry => [entry.id, entry.queries ?? []])) : null;
  owner.restore = options.state ?? null;
  const rootOwner = owner;
  const document = target.ownerDocument;
  const roots = new Set();
  const frames = [{ node: target, cursor: target.firstChild, attrs: null }];
  const current = () => frames[frames.length - 1];
  const container = frame => frame.node.content ?? frame.node;
  const mismatch = (what) => { throw new Error("Alder hydration mismatch: " + what); };
  function append(node) {
    container(current()).insertBefore(node, current().end ?? null);
    if (container(current()) === target) roots.add(node);
    return node;
  }
  function consume(check, description) {
    const frame = current();
    const node = frame.cursor;
    if (!node || !check(node)) mismatch(description);
    frame.cursor = node.nextSibling;
    if (frames.length === 1) roots.add(node);
    return node;
  }
  function effect(dependencies, update) {
    webSubscribe(owner, dependencies, update);
  }
  function anchor(label) {
    return hydrate ? consume(node => node.nodeType === 8 && node.data === label, label)
      : append(document.createComment(label));
  }
  function erase(start, end, inclusive = false) {
    let node = inclusive ? start : start.nextSibling;
    while (node && node !== end) { const next = node.nextSibling; node.parentNode.removeChild(node); roots.delete(node); node = next; }
    if (inclusive) { end.parentNode?.removeChild(end); roots.delete(end); }
  }
  function range(read, dependencies, identity = "region/" + owner.nextRegion++) {
    const parent = owner;
    const start = anchor("alder:region");
    let end;
    let selectedOwner;
    let cell;
    let selectedKey;
    let mounted = false;
    function renderSelected(selected) {
      if (!selected) return;
      selectedKey = selected[0];
      selectedOwner = webOwner(parent, identity + "/" + String(selectedKey?.identity ?? selectedKey));
      cell = $webState(selectedOwner, selected[1], null, false);
      const previous = owner;
      owner = selectedOwner;
      try { webOwnerRun(owner, () => selected[2](owner, cell)(renderer)); } finally { owner = previous; }
      mounted = true;
    }
    renderSelected(read());
    end = anchor("/alder:region");
    effect(dependencies, () => {
      const selected = read();
      if (mounted && selected && Object.is(selectedKey, selected[0])) { cell.value = selected[1]; return; }
      if (!mounted && !selected) return;
      if (selectedOwner) webDispose(selectedOwner);
      erase(start, end);
      mounted = false;
      const previousOwner = owner;
      const previousHydrate = hydrate;
      owner = parent;
      hydrate = false;
      frames.push({ node: end.parentNode, cursor: null, end, attrs: null });
      try { renderSelected(selected); } finally { frames.pop(); owner = previousOwner; hydrate = previousHydrate; }
    });
    return { start, end, update(value) { if (cell) cell.value = value; }, dispose() { if (selectedOwner) webDispose(selectedOwner); erase(start, end, true); } };
  }
  const renderer = {
    open(tag) {
      const node = hydrate
        ? consume(node => node.nodeType === 1 && node.localName === tag && node.namespaceURI === "http://www.w3.org/1999/xhtml", "expected <" + tag + ">")
        : append(document.createElement(tag));
      frames.push({ node, cursor: (node.content ?? node).firstChild, attrs: new Set(), raw: ["textarea", "title"].includes(tag) ? [] : null });
    },
    attr(name, read, dependencies) {
      const frame = current();
      const node = frame.node;
      frame.attrs.add(name);
      const initial = webAttribute(read());
      const write = value => value === null ? node.removeAttribute(name) : node.setAttribute(name, value);
      if (hydrate) {
        if (node.getAttribute(name) !== initial) mismatch("attribute " + name);
      } else write(initial);
      effect(dependencies, () => write(webAttribute(read())));
    },
    event(name, handler) {
      const node = current().node;
      const eventOwner = owner;
      const listener = event => {
        if (eventOwner.disposed) return;
        const report = error => { if (!eventOwner.disposed) eventOwner.report(error); };
        try { webOwnerRun(eventOwner, () => $runMain(handler(event))).catch(report); }
        catch (error) { report(error); }
      };
      node.addEventListener(name, listener);
      owner.cleanups.push(() => node.removeEventListener(name, listener));
    },
    eventValue(name, read, dependencies) {
      let handler = read();
      effect(dependencies, () => { handler = read(); });
      renderer.event(name, event => handler(event));
    },
    value(read, dependencies) {
      const initial = read();
      if (!initial?.setup) { renderer.text(read, dependencies); return; }
      range(() => { const value = read(); return [value, undefined, child => value.setup(child)]; }, dependencies);
    },
    region: range,
    list(read, dependencies, key, setup, empty) {
      const parent = owner;
      const identity = "list/" + owner.nextRegion++;
      const start = anchor("alder:list");
      let entries = new Map();
      let emptyRange;
      function collect() {
        const items = Array.from(read());
        const keys = new Set();
        return items.map((item, index) => {
          const id = key(item, index);
          if (keys.has(id)) throw new Error("Alder duplicate list key: " + id);
          keys.add(id);
          return [id, item];
        });
      }
      const initial = collect();
      for (const [id, item] of initial) entries.set(id, range(() => [id, item, setup], [], identity + "/key/" + typeof id + ":" + encodeURIComponent(String(id))));
      if (!initial.length && empty) emptyRange = range(() => ["empty", undefined, empty], [], identity + "/empty");
      const end = anchor("/alder:list");
      effect(dependencies, () => {
        const next = collect();
        const wanted = new Set(next.map(([id]) => id));
        for (const [id, entry] of entries) if (!wanted.has(id)) { entry.dispose(); entries.delete(id); }
        if (next.length && emptyRange) { emptyRange.dispose(); emptyRange = undefined; }
        const previousOwner = owner;
        const previousHydrate = hydrate;
        owner = parent;
        hydrate = false;
        frames.push({ node: end.parentNode, cursor: null, end, attrs: null });
        try {
          for (const [id, item] of next) {
            if (entries.has(id)) entries.get(id).update(item);
            else entries.set(id, range(() => [id, item, setup], [], identity + "/key/" + typeof id + ":" + encodeURIComponent(String(id))));
          }
          if (!next.length && empty && !emptyRange) emptyRange = range(() => ["empty", undefined, empty], [], identity + "/empty");
          let before = end;
          for (let index = next.length - 1; index >= 0; index--) {
            const entry = entries.get(next[index][0]);
            if (entry.end.nextSibling !== before) {
              const moving = [];
              for (let node = entry.start; node; node = node.nextSibling) { moving.push(node); if (node === entry.end) break; }
              for (const node of moving) end.parentNode.insertBefore(node, before);
            }
            before = entry.start;
          }
        } finally { frames.pop(); owner = previousOwner; hydrate = previousHydrate; }
      });
    },
    text(read, dependencies) {
      const frame = current();
      if (frame.raw) {
        const index = frame.raw.length;
        frame.raw.push(String(read()));
        effect(dependencies, () => {
          frame.raw[index] = String(read());
          const text = frame.raw.join("");
          if (!frame.rawNode && text !== "") frame.rawNode = frame.node.appendChild(document.createTextNode(text));
          else if (frame.rawNode && frame.rawNode.data !== text) frame.rawNode.data = text;
        });
        return;
      }
      const initial = String(read());
      let node;
      let end;
      if (hydrate) {
        consume(node => node.nodeType === 8 && node.data === "alder:text", "text start");
        if (current().cursor?.nodeType === 3) {
          node = consume(node => node.data === initial, "text value");
        } else if (initial !== "") mismatch("missing text");
        end = consume(node => node.nodeType === 8 && node.data === "/alder:text", "text end");
      } else {
        append(document.createComment("alder:text"));
        node = append(document.createTextNode(initial));
        end = append(document.createComment("/alder:text"));
      }
      effect(dependencies, () => {
        const next = String(read());
        if (!node && next !== "") {
          node = document.createTextNode(next);
          end.parentNode.insertBefore(node, end);
          if (end.parentNode === target) roots.add(node);
        } else if (node && node.data !== next) node.data = next;
      });
    },
    close() {
      const frame = current();
      if (frame.raw) {
        const text = frame.raw.join("");
        if (hydrate) {
          if (frame.cursor?.nodeType === 3) frame.rawNode = consume(node => node.data === text, "raw text value");
          else if (text !== "") mismatch("missing raw text");
        } else if (text !== "") frame.rawNode = append(document.createTextNode(text));
      }
      if (hydrate) {
        if (frame.cursor) mismatch("unexpected child");
        for (const attr of frame.node.attributes) {
          if (!frame.attrs.has(attr.name)) mismatch("unexpected attribute " + attr.name);
        }
      }
      frames.pop();
    },
  };
  try {
    if (options.state?.compatible === false) throw new WebRestoreMismatch("snapshot contains executable state");
    if (options.state && options.state.identity !== component.identity) throw new WebRestoreMismatch("root component identity changed");
    webOwnerRun(owner, () => component.setup(owner)(renderer));
    if (hydrate && current().cursor) mismatch("unexpected root");
  } catch (error) {
    webDispose(rootOwner);
    if (!hydrate) for (const root of roots) root.parentNode?.removeChild(root);
    if (error instanceof WebRestoreMismatch && options.state) {
      const fresh = webRender(component, target, hydrate, {...options, state: undefined});
      fresh.restoration = { status: "incompatible", reason: error.message };
      return fresh;
    }
    throw error;
  }
  function finishRestore(owner) { owner.restore = null; owner.hydrationResources = null; owner.hydrationQueries = null; owner.hydrating = false; for (const child of owner.children) finishRestore(child); }
  finishRestore(rootOwner);
  return {
    restoration: { status: options.state ? "restored" : "fresh" },
    snapshot() {
      if (rootOwner.disposed) throw new Error("Cannot snapshot a disposed Alder component");
      const cells = new Map();
      const components = new Map();
      let compatible = true;
      function executable(value, seen = new Set()) {
        if (typeof value === "function") return true;
        if (!value || typeof value !== "object" || seen.has(value)) return false;
        seen.add(value);
        const values = value instanceof Map ? [...value.keys(), ...value.values()] : value instanceof Set ? [...value] : Object.values(value);
        return values.some(item => executable(item, seen));
      }
      function visit(owner) {
        if (owner.signature !== undefined) components.set(owner.id, owner.signature);
        for (const cell of owner.cells) if (cell.writable) {
          if (executable(cell.value)) compatible = false;
          cells.set(cell.id, { value: cell.value, shape: cell.shape });
        }
        for (const child of owner.children) visit(child);
      }
      visit(rootOwner);
      return { identity: component.identity, cells, components, compatible };
    },
    dispose() {
      webDispose(rootOwner);
      for (const root of roots) root.parentNode?.removeChild(root);
      roots.clear();
    },
  };
}

export function $webSnapshot(instance) { return instance.snapshot(); }
export function $webMount(component, target, options) { return webRender(component, target, false, options); }
export function $webHydrate(component, target, options) { return webRender(component, target, true, options); }
