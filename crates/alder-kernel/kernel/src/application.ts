// HTTP orchestration is a Task so user hooks/providers and nested loads share
// the scheduler's existing request-local fiber context across suspension.
function webInvoke(call) {
  return $task(function* () {
    const value = call();
    if (value?.[taskType]) return yield* value;
    if (value && typeof value.then === "function") return yield* $tryPromise(() => value);
    return value;
  });
}

async function webRunRequest(task, signal) {
  const fiber = new FiberImpl(task).start();
  const abort = () => fiber.interruptUnsafe();
  signal.addEventListener("abort", abort, {once:true});
  if (signal.aborted) abort();
  try {
    const exit = await fiber.awaitExit();
    if (exit.$ !== "Success") throw exit.error;
    return exit.value;
  } finally {signal.removeEventListener("abort", abort);}
}

const webRemoteCache = new Map();
const webRemoteVersions = new Map();

function webResetRemoteCache() {
  webRemoteCache.clear();
  // Queries that started on the old page must not repopulate the new page's cache.
  for (const [module, version] of webRemoteVersions) webRemoteVersions.set(module, version + 1);
}

/** Server facades record dependencies without crossing HTTP or caching users. */
export function $webRemoteServer(module, kind, call) {
  return $task(function* () {
    const scope = $webCurrentStoreScope();
    if (kind === "query") $webResourceTrack(module);
    try {return yield* webInvoke(call);}
    finally {if (kind === "command") $webResourceInvalidate(module, scope);}
  });
}

/** Browser stubs share a bounded, short-lived query cache per remote module. */
export function $webRemote(module, name, args, kind) {
  return $task(function* () {
    const scope = $webCurrentStoreScope();
    const force = kind === "query" && $webResourceTrack(module);
    return yield* $tryPromise(async signal => {
    const encoded = $webEncode(args), key = $webEncode([module, name, args]);
    const cached = webRemoteCache.get(key);
    if (kind === "query" && !force && cached && cached.expires > Date.now()) return $webDecode(cached.value);
    const version = webRemoteVersions.get(module) ?? 0;
    webRemoteVersions.set(module, version);
    try {
      const response = await fetch("/_alder/remote/" + encodeURIComponent(module) + "/" + encodeURIComponent(name), {
        method: "POST", headers: {"content-type":"application/x-alder-value", "x-alder-remote":"1"},
        body: encoded, signal, credentials:"same-origin",
      });
      if (!response.ok || response.headers.get("content-type") !== "application/x-alder-value") {
        throw new Error(`Alder remote ${name} failed (${response.status})`);
      }
      const value = await response.text(), result = $webDecode(value);
      if (kind === "query" && version === (webRemoteVersions.get(module) ?? 0)) {
        if (webRemoteCache.size >= 256) webRemoteCache.delete(webRemoteCache.keys().next().value);
        webRemoteCache.set(key, {module, value, expires:Date.now() + 30000});
      }
      return result;
    } finally {
      if (kind === "command") {
        webRemoteVersions.set(module, (webRemoteVersions.get(module) ?? 0) + 1);
        for (const [key, entry] of webRemoteCache) if (entry.module === module) webRemoteCache.delete(key);
        $webResourceInvalidate(module, scope);
      }
    }
    }, true);
  });
}

/** Route-local typed actions preserve the real page URL and route parameters. */
export function $webAction(route, name, input) {
  return $task(function* () {
    const scope = $webCurrentStoreScope();
    return yield* $tryPromise(async signal => {
    try {
      const response = await fetch(location.href, {method:"POST", credentials:"same-origin", signal,
        headers:{"content-type":"application/x-alder-value", "x-alder-remote":"1", "x-alder-route":encodeURIComponent(route), "x-alder-action":encodeURIComponent(name)},
        body:$webEncode([input]),
      });
      if (!response.ok || response.headers.get("content-type") !== "application/x-alder-value") throw new Error(`Alder action ${name} failed (${response.status})`);
      return $webDecode(await response.text());
    } finally {
      webResetRemoteCache();
      $webResourceInvalidate(null, scope);
    }
    }, true);
  });
}

async function webRemoteBody(request) {
  const reader = request.body?.getReader();
  if (!reader) throw new TypeError("Missing remote arguments");
  const chunks = []; let size = 0;
  try {
    while (true) {
      const {done, value} = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > 1048576) {await reader.cancel(); throw new RangeError("Remote arguments exceed 1 MiB");}
      chunks.push(value);
    }
  } finally {reader.releaseLock();}
  const bytes = new Uint8Array(size); let offset = 0;
  for (const chunk of chunks) {bytes.set(chunk, offset); offset += chunk.byteLength;}
  return $webDecode(new TextDecoder("utf-8", {fatal:true}).decode(bytes));
}

function webDispatchRemote(descriptor, event) {
  return $task(function* () {
    const {request, url} = event;
    if (request.method !== "POST") return new Response("Method not allowed", {status:405});
    if (request.headers.get("x-alder-remote") !== "1" ||
        request.headers.get("content-type") !== "application/x-alder-value" ||
        (request.headers.has("origin") && request.headers.get("origin") !== url.origin) ||
        request.headers.get("sec-fetch-site") === "cross-site") return new Response("Forbidden", {status:403});
    let args;
    try {args = descriptor.args(yield* $tryPromise(() => webRemoteBody(request)));}
    catch (error) {return new Response("Invalid remote arguments", {status:error instanceof RangeError || error?.cause instanceof RangeError ? 413 : 400});}
    const result = descriptor.result(yield* webInvoke(() => descriptor.call(...args)));
    return new Response($webEncode(result), {headers:{"content-type":"application/x-alder-value", "cache-control":"no-store"}});
  });
}

function webRouteEncode(value) {
  if (typeof value !== "string" || !value || value === "." || value === ".." || value.includes("/") || value.includes("\\") || value.includes("\0")) throw new URIError("Invalid route segment");
  // encodeURIComponent leaves these punctuation characters unescaped, while
  // generated Rust hrefs use the RFC 3986 unreserved set exclusively.
  return encodeURIComponent(value).replace(/[!'()*]/g, character => "%" + character.charCodeAt(0).toString(16).toUpperCase());
}

function webRouteParts(pathname) {
  if (typeof pathname !== "string" || !pathname.startsWith("/") || /[?#\\]/.test(pathname)) throw new URIError("Expected an absolute URL pathname");
  if (pathname === "/") return [];
  let inner = pathname.slice(1);
  if (inner.endsWith("/")) inner = inner.slice(0, -1);
  return inner.split("/").map(part => {
    const value = decodeURIComponent(part);
    webRouteEncode(value);
    return value;
  });
}

function webMatchSegments(segments, parts, index = 0, cursor = 0, params = {}) {
  if (index === segments.length) return cursor === parts.length ? params : null;
  const [kind, name] = segments[index];
  if (kind === "static") return parts[cursor] === name ? webMatchSegments(segments, parts, index + 1, cursor + 1, params) : null;
  if (kind === "rest") {
    for (let end = parts.length; end >= cursor; end--) {
      const found = webMatchSegments(segments, parts, index + 1, end, {...params, [name]:parts.slice(cursor, end).join("/")});
      if (found) return found;
    }
    return null;
  }
  if (cursor < parts.length) {
    const found = webMatchSegments(segments, parts, index + 1, cursor + 1, {...params, [name]:parts[cursor]});
    if (found) return found;
  }
  return kind === "optional" ? webMatchSegments(segments, parts, index + 1, cursor, {...params, [name]:null}) : null;
}

export function $webMatch(routes, pathname) {
  const parts = webRouteParts(pathname);
  for (const route of routes) {
    const params = webMatchSegments(route.segments, parts);
    if (params) return {route, params};
  }
  return null;
}

export function $webHref(segments, params) {
  if (!params || typeof params !== "object" || Array.isArray(params)) throw new TypeError("Expected route parameter record");
  const expected = new Set(segments.filter(([kind]) => kind !== "static").map(([,name]) => name));
  const fields = Object.getOwnPropertyDescriptors(params);
  for (const name of Reflect.ownKeys(fields)) {
    if (!expected.has(name)) throw new TypeError(`Unknown Alder route parameter ${String(name)}`);
    if (!("value" in fields[name])) throw new TypeError("Route parameters must be data fields");
  }
  const parts = [];
  for (const [kind, name] of segments) {
    if (kind === "static") {parts.push(webRouteEncode(name)); continue;}
    const value = fields[name]?.value;
    if (kind === "optional" && (value == null || value === "")) continue;
    if (kind === "rest" && (value === undefined || value === "")) continue;
    if (typeof value !== "string") throw new TypeError(`Alder route parameter ${name} must be a String`);
    if (kind === "rest") {
      for (const part of value.split("/")) parts.push(webRouteEncode(part));
    } else parts.push(webRouteEncode(value));
  }
  const path = "/" + parts.join("/");
  const matched = webMatchSegments(segments, webRouteParts(path));
  if (!matched) throw new URIError("Route parameters do not form a matching URL");
  for (const [name, {value}] of Object.entries(fields)) {
    if (value !== undefined && value !== null && value !== "" && matched[name] !== value) throw new URIError("Optional route parameters produce an ambiguous URL");
  }
  return path;
}

export async function $webServe(application, client, assets = []) {
  const args = globalThis.__alderHost?.args ?? [];
  let port = 3000, hostname = "127.0.0.1";
  for (let index = 0; index < args.length; index++) {
    if (args[index] === "--port") port = Number(args[++index]);
    else if (args[index] === "--host") hostname = args[++index];
    else throw new Error(`Unknown web server argument: ${args[index]}`);
  }
  if (!Number.isInteger(port) || port < 0 || port > 65535 || !hostname) throw new Error("Invalid web server host or port");
  const background = new Set();
  const context = {waitUntil(promise) {
    const pending = Promise.resolve(promise);
    background.add(pending);
    pending.finally(() => background.delete(pending)).catch(console.error);
  }};
  const worker = $webWorker(application, client, undefined, assets);
  const server = __alderHost.serve(request => worker.fetch(request, {}, context),
    {port, hostname, onListen: address => console.log(`Alder listening on http://${address.hostname}:${address.port}`)});
  await server.finished;
  await Promise.allSettled([...background]);
}

export function $webWorker(application, client, queue, assets = [], assetBinding = true) {
  const publicAssets = new Map(assets.map(([path, type, bytes]) => [path, {type, bytes: new Uint8Array(bytes)}]));
  const worker = {async fetch(request, env, context) {
    if (new URL(request.url).pathname === "/_alder/client.mjs") {
      if (request.method !== "GET" && request.method !== "HEAD") return new Response("Method not allowed", {status: 405});
      return new Response(request.method === "HEAD" ? null : client, {headers:{"content-type":"text/javascript; charset=utf-8", "x-content-type-options":"nosniff"}});
    }
    let path;
    try {path = decodeURIComponent(new URL(request.url).pathname);} catch {return new Response("Invalid asset path", {status: 400});}
    const asset = publicAssets.get(path);
    if (asset) {
      if (request.method !== "GET" && request.method !== "HEAD") return new Response("Method not allowed", {status: 405, headers:{allow:"GET, HEAD"}});
      return new Response(request.method === "HEAD" ? null : asset.bytes, {headers:{"content-type":asset.type, "content-length":String(asset.bytes.byteLength), "x-content-type-options":"nosniff"}});
    }
    const response = await application.fetch(request, env, context);
    if (assetBinding && response.status === 404 && env?.ASSETS && (request.method === "GET" || request.method === "HEAD")) return env.ASSETS.fetch(request);
    return response;
  }};
  if (queue) worker.queue = queue;
  return worker;
}

function webOptions(route) {
  const options = {ssr: true, csr: true, prerender: false, trailingSlash: "Never"};
  for (const source of route.options ?? []) {
    for (const key of ["ssr", "csr", "prerender", "trailingSlash"]) {
      if (source[key] !== undefined) options[key] = source[key];
    }
  }
  options.trailingSlash = options.trailingSlash?.$ ?? options.trailingSlash;
  if (!options.ssr && !options.csr) throw new Error("Alder page cannot disable both ssr and csr");
  return options;
}

class WebLoadFailure extends Error {
  constructor(value) {super("Alder load returned a typed error"); this.value = value;}
}

function webReportedError(error) {
  return {name:error instanceof Error ? error.name : "Error", message:error instanceof Error ? error.message : String(error), stack:error instanceof Error && typeof error.stack === "string" ? error.stack : null};
}

function webPageError(error, development) {
  return error instanceof WebLoadFailure ? {$:"Expected", _0:error.value} : {$:"Unexpected", _0:{status:500, message:development ? webReportedError(error).message : "Internal server error"}};
}

function webLoaded(value) {
  if (value?.$ === "Err") throw new WebLoadFailure(value._0);
  if (value?.$ === "Ok") value = value._0;
  if (value === undefined) return {};
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new TypeError("Alder load must return a record");
  return value;
}

class WebRouteLoadFailure {
  constructor(cause, layout) {this.cause = cause; this.layout = layout;}
}

function webLoadNavigation(route, payload, href, signal) {
  if (payload.error !== null && payload.error !== undefined) return Promise.resolve(payload);
  const modules = [...route.layouts.map(pair => pair[0]), route.page];
  if (!modules.some(module => module?.load)) return Promise.resolve(payload);
  if (!Array.isArray(payload.serverData) || payload.serverData.length !== modules.length) throw new TypeError("Alder navigation is missing server load data");
  const request = new Request(href, {signal});
  const event = {request, url:new URL(request.url), params:payload.params, data:{}, fetch:input => $httpFetch(input)};
  return webRunRequest($task(function* () {
    let data = {};
    for (let index = 0; index < modules.length; index++) {
      // Only explicit server return records cross this boundary. Browser data
      // is never sent back as trusted input to a later server load.
      data = {...data, ...webLoaded(payload.serverData[index])};
      if (modules[index]?.load) {
        event.data = data;
        try {data = {...data, ...webLoaded(yield* webInvoke(() => modules[index].load(event)))};}
        catch (cause) {throw new WebRouteLoadFailure(cause, index);}
      }
    }
    return {...payload, data, resources:[]};
  }), signal);
}

function webView(route, data, error = null, boundary = -1) {
  let view;
  if (boundary >= 0) view = route.errors[boundary].error({error, data});
  else view = route.page.page({data});
  const layoutCount = boundary < 0 ? route.layouts.length : route.errorLayouts?.[boundary] ?? route.layouts.length;
  for (let index = layoutCount - 1; index >= 0; index--) {
    const layout = route.layouts[index][0];
    if (layout?.layout) view = layout.layout({data, children: view});
  }
  return view;
}

function webDocument(content, payload, options) {
  const client = options.csr ? `<script type="application/json" id="alder-data">${$webEncode(payload)}</script><script type="module" src="/_alder/client.mjs"></script>` : "";
  return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"></head><body><div id="alder-app">${content}</div>${client}</body></html>`;
}

export function $webApplication(config) {
  return {
    async entries(call, env) {
      return $cloudflareRun(env, config.providers ?? [], () => $webWithStoreScope(null, () => webInvoke(call)));
    },
    async fetch(request, env = {}, context = {}, prerender = false) {
      const url = new URL(request.url);
      let found;
      let remote;
      try {
        if (url.pathname.startsWith("/_alder/remote/")) {
          const parts = url.pathname.slice("/_alder/remote/".length).split("/").map(decodeURIComponent);
          remote = parts.length === 2 && config.remotes?.find(item => item.module === parts[0] && item.name === parts[1]);
          found = remote ? {route:null, params:{}} : null;
        } else found = $webMatch(config.routes, url.pathname);
      }
      catch { return new Response("Invalid URL path", {status: 400}); }
      if (!found) found = {route:null, params:{}};
      const {route, params} = found;
      const storeScope = $webCreateStoreScope();
      const event = {request, url, params, env, context, headers: request.headers, data: {},
        fetch: input => $httpFetch(input)};
      const requestTask = call => $cloudflareTask(env, config.providers ?? [], () => $webWithStoreScope(storeScope, () => $task(function* () {
        if (config.hook?.handleFetch) $providerPush(httpFetchHookKey, input => webInvoke(() => config.hook.handleFetch(event, input, httpNativeFetch)));
        try {return yield* webInvoke(call);}
        finally {if (config.hook?.handleFetch) $providerPop(httpFetchHookKey);}
      })));
      let reported = false;
      const report = error => $task(function* () {
        if (error instanceof WebLoadFailure) return;
        reported = true;
        if (config.hook?.handleError) {
          try {yield* webInvoke(() => config.hook.handleError(webReportedError(error), event));}
          catch (reportingError) {console.error(reportingError);}
        } else console.error(error);
      });
      const resolve = event => $task(function* () {
        if (remote) return yield* webDispatchRemote(remote, event);
        if (!route) return new Response("Not found", {status:404});
        if (request.headers.has("x-alder-action")) {
          let name, id;
          try {name = decodeURIComponent(request.headers.get("x-alder-action")); id = decodeURIComponent(request.headers.get("x-alder-route") ?? "");}
          catch {return new Response("Invalid action", {status:400});}
          const action = id === route.id && config.actions?.find(action => action.route === id && action.name === name);
          if (!action) return new Response("Action not found", {status:404});
          return yield* webDispatchRemote(action, event);
        }
        const method = request.method.toLowerCase();
        const dataRequest = request.headers.get("accept") === "application/x-alder-data";
        if (route.endpoint?.[method] && (!route.page || !request.headers.get("accept")?.includes("text/html")) && !dataRequest) {
          const response = yield* webInvoke(() => route.endpoint[method](event));
          if (!(response instanceof Response)) throw new TypeError("Alder HTTP handler must return a Response");
          return response;
        }
        if (!route.page) return new Response("Method not allowed", {status: 405});
        if (method !== "get" && method !== "head") return new Response("Method not allowed", {status: 405});
        const options = webOptions(route);
        const slash = url.pathname.endsWith("/");
        if (url.pathname !== "/" && ((options.trailingSlash === "Always" && !slash) || (options.trailingSlash === "Never" && slash))) {
          url.pathname = options.trailingSlash === "Always" ? url.pathname + "/" : url.pathname.replace(/\/+$/, "");
          return Response.redirect(url.href, 308);
        }
        const cached = !prerender && config.prerendered?.find(page => page.path === url.pathname);
        if (cached) return new Response(method === "head" ? null : dataRequest ? cached.data : cached.html, {
          headers:{"content-type":dataRequest ? "application/x-alder-data" : "text/html; charset=utf-8", "vary":"accept, x-alder-navigation", "cache-control":"no-store"},
        });
        let data = {}, boundary = -1, error = null, content = "", resources = [];
        const serverData = [];
        const pairs = [...route.layouts, [route.page, route.server]];
        const navigation = dataRequest && request.headers.get("x-alder-navigation") === "1";
        const lastServer = navigation ? pairs.findLastIndex(pair => pair[1]?.load) : pairs.length;
        let failedLayout = pairs.length;
        try {
          for (let index = 0; index < pairs.length; index++) {
            failedLayout = index;
            const [universal, server] = pairs[index];
            let returned = {};
            if (server?.load) {
              event.data = data;
              returned = webLoaded(yield* webInvoke(() => server.load(event)));
              data = {...data, ...returned};
            }
            serverData.push(returned);
            // Ancestor universal data remains authoritative server-side when a
            // later server load depends on it. Otherwise navigation defers it
            // entirely to the browser. SSR, prerender and HMR run all loads.
            if (universal?.load && (!navigation || index < lastServer)) {
              event.data = data;
              data = {...data, ...webLoaded(yield* webInvoke(() => universal.load(event)))};
            }
          }
          failedLayout = pairs.length;
          if (options.ssr && !navigation) {
            const rendered = yield* $tryPromise(() => $webStoreRun(storeScope, () => $webSsrAsync(webView(route, data), {storeScope})));
            content = rendered.html; resources = rendered.resources;
          }
        } catch (caught) {
          yield* report(caught);
          let cause = caught;
          error = webPageError(caught, config.development);
          for (let index = route.errors.length - 1; index >= 0; index--) {
            if ((route.errorLayouts?.[index] ?? 0) > failedLayout) continue;
            try {
              const rendered = yield* $tryPromise(() => $webStoreRun(storeScope, () => $webSsrAsync(webView(route, data, error, index), {storeScope})));
              content = rendered.html; resources = rendered.resources; boundary = index; break;
            }
            catch (next) { cause = next; error = webPageError(next, config.development); yield* report(next); }
          }
          if (boundary < 0) throw cause;
        }
        const payload = {route: route.id, params, data, serverData, boundary, error, options, resources, stores:$webStoreSnapshot(storeScope, config.clientStoreKeys ?? [])};
        return new Response(method === "head" ? null : prerender ? JSON.stringify({html:webDocument(content, payload, options), data:$webEncode(payload)}) : dataRequest ? $webEncode(payload) : webDocument(content, payload, options), {
          status: error === null ? 200 : 500,
          headers: {"content-type": prerender ? "application/json" : dataRequest ? "application/x-alder-data" : "text/html; charset=utf-8", "vary": "accept, x-alder-navigation", "cache-control":"no-store"},
        });
      });
      try {
        const response = await webRunRequest(requestTask(() => config.hook?.handle ? config.hook.handle(event, resolve) : resolve(event)), request.signal);
        if (!(response instanceof Response)) throw new TypeError("Alder handle hook must return a Response");
        return response;
      } catch (error) {
        if (request.signal.aborted) return new Response(null, {status:499});
        if (!reported) await $runTask(requestTask(() => report(error)));
        return new Response("Internal server error", {status: 500});
      } finally {$webDisposeStoreScope(storeScope);}
    },
  };
}

export async function $webPrerender(application, targets, env = {}, context = {}) {
  const result = [], seen = new Set();
  for (const target of targets) {
    let paths = target.paths;
    if (target.entries) {
      const values = target.validate(await application.entries(target.entries, env));
      paths = values.map(params => {
        const path = $webHref(target.segments, params);
        return target.trailingSlash === "Always" && path !== "/" ? path + "/" : path;
      });
    }
    for (const path of paths) {
      if (seen.has(path)) throw new Error(`Duplicate prerender path ${path}`);
      seen.add(path);
      const response = await application.fetch(new Request(new URL(path, "http://alder-prerender"), {headers:{accept:"text/html"}}), env, context, true);
      if (response.status !== 200 || response.headers.get("content-type") !== "application/json") throw new Error(`Prerender ${path} failed with HTTP ${response.status}`);
      const page = await response.json();
      const payload = $webDecode(page.data);
      if (payload.route !== target.route) throw new Error(`Prerender entry ${path} belongs to ${payload.route}, not ${target.route}`);
      result.push({path, html:page.html, data:page.data});
    }
  }
  return result.sort((left, right) => left.path.localeCompare(right.path));
}

export function $webPrerenderWorker(application, targets, queue) {
  const worker = {async fetch(request, env, context) {return Response.json(await $webPrerender(application, targets, env, context));}};
  if (queue) worker.queue = queue;
  return worker;
}

export function $webStartClient(config) {
  const sessionKey = Symbol.for("alder.dev.session"), transferKey = Symbol.for("alder.dev.transfer");
  const transfer = config.development ? globalThis[transferKey] : null;
  const previousSession = transfer ? globalThis[sessionKey] : null;
  if (transfer) delete globalThis[transferKey];
  const target = document.getElementById("alder-app");
  const serialized = document.getElementById("alder-data");
  if (!target || !serialized) throw new Error("Alder bootstrap document is missing");
  let mounted, controller, generation = 0, disposed = false, currentPayload, events, overlay;
  const report = error => {
    if (disposed) return;
    return config.hook?.handleError ? $runTask(webInvoke(() => config.hook.handleError(webReportedError(error)))).catch(console.error) : console.error(error);
  };
  const render = (payload, hydrate, state) => {
    const route = config.routes.find(route => route.id === payload.route);
    if (!route) throw new Error(`Alder client route is missing: ${payload.route}`);
    const view = webView(route, payload.data, payload.error, payload.boundary);
    const options = {resources:payload.resources, state, onError:report};
    // Mount appends its owned roots and rolls them back on failure. Keep the
    // last good tree and handlers alive until replacement setup has succeeded.
    const next = hydrate ? $webHydrate(view, target, options) : $webMount(view, target, options);
    mounted?.dispose();
    mounted = next;
    currentPayload = payload;
    if (mounted.restoration.status === "incompatible") console.info("Alder HMR reset:", mounted.restoration.reason);
  };
  const initial = $webDecode(transfer?.payload ?? serialized.textContent);
  $webRestoreStores(initial.stores ?? []);
  try {render(initial, !transfer && initial.options.ssr, transfer?.state ? $webDecode(transfer.state) : undefined);}
  catch (error) {report(error); throw error;}
  previousSession?.dispose();
  const navigate = async (href, historyMode = "push") => {
    controller?.abort();
    const navigationController = controller = new AbortController();
    const current = ++generation;
    const response = await fetch(href, {headers: {accept: "application/x-alder-data", "x-alder-navigation":"1"}, signal: navigationController.signal});
    if (disposed || current !== generation) return;
    if (response.headers.get("content-type") !== "application/x-alder-data") { location.assign(response.url || href); return; }
    let payload = $webDecode(await response.text());
    if (disposed || current !== generation) return;
    if (!payload.options.csr) { location.assign(href); return; }
    const route = config.routes.find(route => route.id === payload.route);
    if (!route) throw new Error(`Alder client route is missing: ${payload.route}`);
    webResetRemoteCache();
    $webRestoreStores(payload.stores ?? [], true);
    try {
      payload = await webLoadNavigation(route, payload, new URL(response.url || href, location.href).href, navigationController.signal);
      if (disposed || current !== generation || navigationController.signal.aborted) return;
      render(payload, false);
    } catch (caught) {
      if (disposed || current !== generation || navigationController.signal.aborted) return;
      const failedLayout = caught instanceof WebRouteLoadFailure ? caught.layout : route.layouts.length + 1;
      let cause = caught instanceof WebRouteLoadFailure ? caught.cause : caught, handled = false;
      for (let index = (route.errors ?? []).length - 1; index >= 0; index--) {
        if ((route.errorLayouts?.[index] ?? 0) > failedLayout) continue;
        if (!(cause instanceof WebLoadFailure)) report(cause);
        try {
          payload = {...payload, error:webPageError(cause, config.development), boundary:index, resources:[]};
          render(payload, false);
          handled = true; break;
        } catch (next) {cause = next;}
      }
      if (!handled) throw cause;
    }
    if (historyMode === "push") history.pushState(null, "", response.url || href);
    window.scrollTo(0, 0);
  };
  const click = event => {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const anchor = event.target.closest?.("a[href]");
    if (!anchor || anchor.hasAttribute("download") || (anchor.target && anchor.target !== "_self")) return;
    const url = new URL(anchor.href, location.href);
    if (url.origin !== location.origin || (url.pathname === location.pathname && url.search === location.search && url.hash)) return;
    if (!$webMatch(config.routes, url.pathname)) return;
    event.preventDefault();
    navigate(url.href).catch(error => { if (error.name !== "AbortError") report(error); });
  };
  const pop = () => navigate(location.href, "none").catch(error => { if (error.name !== "AbortError") report(error); });
  document.addEventListener("click", click);
  window.addEventListener("popstate", pop);
  if (config.hook?.init) $runTask(webInvoke(() => config.hook.init())).catch(report);
  const api = {navigate, dispose() {
    if (disposed) return;
    disposed = true; generation++; controller?.abort(); mounted?.dispose();
    events?.close(); overlay?.remove();
    document.removeEventListener("click", click); window.removeEventListener("popstate", pop);
  }};
  if (config.development) {
    const showError = message => {
      if (!overlay) {
        overlay = document.createElement("pre");
        overlay.id = "alder-dev-error";
        overlay.setAttribute("role", "alert");
        overlay.style.cssText = "position:fixed;inset:16px;z-index:2147483647;overflow:auto;padding:24px;background:#211b1b;color:#ffe5e5;border:2px solid #d66;white-space:pre-wrap;font:14px/1.6 monospace";
        document.body.appendChild(overlay);
      }
      overlay.textContent = "Alder compilation failed\n\n" + message + "\n\nFix the source to resume. Your current application state is preserved.";
    };
    const documentRevision = document.querySelector?.('meta[name="alder-dev-revision"]')?.getAttribute("content");
    let revision = transfer?.revision ?? (documentRevision == null ? null : Number(documentRevision)), updating = Promise.resolve();
    const update = async nextRevision => {
      if (disposed) return;
      const response = await fetch(location.href, {headers:{accept:"application/x-alder-data"}});
      if (response.headers.get("content-type") !== "application/x-alder-data") {location.reload(); return;}
      const servedRevision = response.headers.get("x-alder-dev-revision");
      if (servedRevision !== null) nextRevision = Number(servedRevision);
      const nextPayload = $webDecode(await response.text());
      nextPayload.stores = $webStoreSnapshot();
      const payload = $webEncode(nextPayload);
      const snapshot = mounted.snapshot();
      const state = snapshot.compatible ? $webEncode(snapshot) : undefined;
      if (!snapshot.compatible) console.info("Alder HMR reset: writable state contains executable values");
      globalThis[transferKey] = {payload, state, revision:nextRevision};
      await import(`/_alder/client.mjs?revision=${nextRevision}`);
    };
    events = new EventSource("/_alder/events");
    events.addEventListener("message", event => {
      const message = JSON.parse(event.data);
      if (message.kind === "error") {showError(message.message); return;}
      if (message.kind === "ready" && revision === null) {revision = message.revision; return;}
      if ((message.kind === "build" || message.kind === "ready") && message.revision > (revision ?? -1)) {
        revision = message.revision;
        updating = updating.then(() => update(message.revision)).catch(error => {showError(error.stack ?? String(error));});
      }
    });
    api.payload = () => currentPayload;
    globalThis[sessionKey] = api;
  }
  return api;
}
