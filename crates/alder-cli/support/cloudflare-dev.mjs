// Compiler-controlled local Workers host. Stdout is a newline-JSON protocol;
// application code only executes in Miniflare/workerd, never in this Node host.
import { createServer } from "node:http";
import { access } from "node:fs/promises";
import { once } from "node:events";
import { resolve, isAbsolute } from "node:path";
import { createInterface } from "node:readline";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { Miniflare, convertV4MiniflareOptions, Log, LogLevel } from "miniflare";

const flags = new Map();
for (let index = 2; index < process.argv.length; index += 2) {
  if (!process.argv[index]?.startsWith("--") || process.argv[index + 1] === undefined) throw new Error("Expected --root PATH --host HOST --port PORT");
  flags.set(process.argv[index], process.argv[index + 1]);
}
const root = flags.get("--root");
if (!root || !isAbsolute(root)) throw new Error("--root must be an absolute project directory");
const host = flags.get("--host") ?? "127.0.0.1";
const port = Number(flags.get("--port") ?? "3000");
if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error("--port must be between 0 and 65535");

const send = message => process.stdout.write(`${JSON.stringify(message)}\n`);
const report = error => process.stderr.write(`${error?.stack ?? String(error)}\n`);
class StderrLog extends Log {
  log(message) { process.stderr.write(`${message}\n`); }
}
const log = new StderrLog(LogLevel.WARN);
let runtime = null, lastOptions = null, revision = 0, diagnostic = null;
let update = Promise.resolve(), closing = false;
const streams = new Set();
const clients = new Map();
const broadcast = value => {
  const bytes = `data: ${JSON.stringify(value)}\n\n`;
  for (const stream of streams) {
    if (stream.destroyed || !stream.write(bytes)) {
      // A stalled devtools/browser client must not accumulate unbounded events.
      streams.delete(stream);
      stream.end();
    }
  }
};
const failed = error => {
  diagnostic = error?.stack ?? String(error);
  report(error);
  broadcast({ kind: "error", message: diagnostic, revision });
  send({ type: "error", message: diagnostic });
};

async function optionsFor(message) {
  if (typeof message.server !== "string" || !message.config || typeof message.config !== "object") throw new TypeError("build requires server source and a Wrangler config object");
  const config = message.config;
  if (!config.name || !config.compatibility_date) throw new TypeError("build config requires name and compatibility_date");
  const durableObjects = {};
  const sqlite = new Set(Object.entries(config.exports ?? {}).filter(([,value]) => value.type === "durable-object" && value.storage === "sqlite").map(([name]) => name));
  for (const migration of config.migrations ?? []) for (const name of migration.new_sqlite_classes ?? []) sqlite.add(name);
  for (const binding of config.durable_objects?.bindings ?? []) {
    if (binding.script_name && binding.script_name !== config.name) throw new Error(`Cross-Worker Durable Object ${binding.name} needs a separately configured local Worker`);
    durableObjects[binding.name] = { className: binding.class_name, useSQLite: sqlite.has(binding.class_name) };
  }
  const boundClasses = new Set(Object.values(durableObjects).map(value => value.className));
  const additionalUnboundDurableObjects = [...sqlite].filter(name => !boundClasses.has(name)).map(className => ({className, useSQLite: true}));
  const queueConsumers = Object.fromEntries((config.queues?.consumers ?? []).map(value => [value.queue, {
    maxBatchSize: value.max_batch_size, maxBatchTimeout: value.max_batch_timeout,
    maxRetries: value.max_retries, deadLetterQueue: value.dead_letter_queue, retryDelay: value.retry_delay,
  }]));
  const hyperdrives = {};
  for (const binding of config.hyperdrive ?? []) {
    const connection = binding.localConnectionString ?? process.env[`CLOUDFLARE_HYPERDRIVE_LOCAL_CONNECTION_STRING_${binding.binding}`];
    if (!connection) throw new Error(`Hyperdrive ${binding.binding} needs localConnectionString or CLOUDFLARE_HYPERDRIVE_LOCAL_CONNECTION_STRING_${binding.binding} for local development`);
    hyperdrives[binding.binding] = connection;
  }
  const serviceBindings = {};
  for (const binding of config.services ?? []) {
    if (binding.service === config.name) {
      serviceBindings[binding.binding] = { name: config.name, ...(binding.entrypoint ? {entrypoint: binding.entrypoint} : {}) };
      continue;
    }
    const location = process.env[`ALDER_SERVICE_LOCAL_URL_${binding.binding}`];
    if (!location) throw new Error(`Service ${binding.binding} needs ALDER_SERVICE_LOCAL_URL_${binding.binding} pointing to its local HTTP server`);
    if (binding.entrypoint) throw new Error(`Service ${binding.binding} uses RPC entrypoint ${binding.entrypoint}; an HTTP local override cannot provide RPC`);
    const url = new URL(location);
    if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.pathname !== "/" || url.search || url.hash) throw new Error(`ALDER_SERVICE_LOCAL_URL_${binding.binding} must be an HTTP(S) origin without credentials or a path`);
    serviceBindings[binding.binding] = {external: {address: url.host, [url.protocol === "https:" ? "https" : "http"]: {}}};
  }
  const workflows = Object.fromEntries((config.workflows ?? []).map(value => {
    if (value.script_name && value.script_name !== config.name) throw new Error(`Workflow ${value.binding} needs a separately configured local Worker`);
    return [value.binding, {name: value.name, className: value.class_name}];
  }));
  let assets;
  if (config.assets?.directory) {
    const directory = resolve(root, "dist", config.assets.directory);
    try { await access(directory); assets = {directory, binding: config.assets.binding ?? "ASSETS", run_worker_first: config.assets.run_worker_first ?? true, routerConfig: {has_user_worker: true}}; }
    catch (error) { if (error.code !== "ENOENT") throw error; }
  }
  return convertV4MiniflareOptions({
    name: config.name, rootPath: root, modules: true, script: message.server,
    scriptPath: resolve(root, "dist", config.main ?? "worker.mjs"),
    compatibilityDate: config.compatibility_date, compatibilityFlags: config.compatibility_flags ?? [],
    host: "127.0.0.1", port: 0, cf: false, log,
    publicUrl: listeningUrl,
    resourcePersistencePath: resolve(root, ".alder", "miniflare"),
    telemetry: { enabled: false },
    handleStructuredLogs: entry => process.stderr.write(`[worker:${entry.level}] ${entry.message}\n`),
    handleUncaughtError: report,
    bindings: config.vars ?? {},
    kvNamespaces: Object.fromEntries((config.kv_namespaces ?? []).map(value => [value.binding, value.id])),
    r2Buckets: Object.fromEntries((config.r2_buckets ?? []).map(value => [value.binding, value.bucket_name])),
    d1Databases: Object.fromEntries((config.d1_databases ?? []).map(value => [value.binding, value.database_id])),
    durableObjects, additionalUnboundDurableObjects,
    queueProducers: Object.fromEntries((config.queues?.producers ?? []).map(value => [value.binding, {queueName: value.queue, deliveryDelay: value.delivery_delay}])),
    queueConsumers, hyperdrives, serviceBindings, workflows, ...(assets ? {assets} : {}),
  });
}

async function rebuild(message) {
  const options = await optionsFor(message);
  if (!runtime) {
    const next = new Miniflare(options);
    try { await next.ready; } catch (error) {await next.dispose().catch(report); throw error;}
    runtime = next;
  } else {
    try {await runtime.setOptions(options); await runtime.ready;}
    catch (error) {
      // A failed workerd reload can poison its current instance. Restore the
      // last successfully evaluated options before reopening request traffic.
      try {await runtime.setOptions(lastOptions); await runtime.ready;}
      catch (restoreError) {await runtime.dispose().catch(report);runtime=null;lastOptions=null;report(restoreError);}
      throw error;
    }
  }
  lastOptions = options;
  revision++;
  if (message.type === "build") {
    const client = await runtime.dispatchFetch("http://alder-dev/_alder/client.mjs");
    if (client.ok && client.headers.get("content-type")?.includes("javascript")) {
      clients.set(revision, await client.text());
      if (clients.size > 8) clients.delete(clients.keys().next().value);
    } else await client.body?.cancel();
  }
  diagnostic = null;
  broadcast({kind:"build", revision});
  send({type:"ready", revision});
}

const compiling = `<!doctype html><meta charset="utf-8"><title>Alder dev</title><pre id="status">Alder is compiling…</pre><script>const events=new EventSource('/_alder/events');events.onmessage=event=>{const message=JSON.parse(event.data);if(message.kind==='build'||(message.kind==='ready'&&message.revision>0))location.reload();if(message.kind==='error')document.getElementById('status').textContent=message.message;};</script>`;
const server = createServer(async (request, response) => {
  try {
    const base = new URL(listeningUrl);
    if (request.headers.host) base.host = request.headers.host;
    const url = new URL(request.url, base);
    if (url.pathname === "/_alder/events") {
      response.writeHead(200, {"content-type":"text/event-stream", "cache-control":"no-store", "connection":"keep-alive"});
      response.write(`data: ${JSON.stringify(diagnostic ? {kind:"error", message:diagnostic, revision} : {kind:"ready", revision})}\n\n`);
      streams.add(response);
      request.on("close", () => streams.delete(response));
      return;
    }
    await update;
    if (url.pathname === "/_alder/client.mjs" && url.searchParams.has("revision")) {
      response.writeHead(200, {"content-type":"text/javascript; charset=utf-8", "cache-control":"no-store"});
      response.end(request.method === "HEAD" ? undefined : clients.get(Number(url.searchParams.get("revision"))) ?? "location.reload();");
      return;
    }
    if (!runtime) {
      response.writeHead(503, {"content-type":"text/html; charset=utf-8", "retry-after":"1"});
      response.end(compiling);
      return;
    }
    const abort = new AbortController();
    response.on("close", () => {if (!response.writableFinished) abort.abort();});
    const init = {method:request.method, headers:request.headers, signal:abort.signal, redirect:"manual"};
    if (request.method !== "GET" && request.method !== "HEAD") {init.body=Readable.toWeb(request);init.duplex="half";}
    const servedRevision = revision;
    const result = await runtime.dispatchFetch(url, init);
    response.statusCode = result.status;
    response.statusMessage = result.statusText;
    for (const [name,value] of result.headers) if (name !== "set-cookie" && name !== "transfer-encoding") response.setHeader(name,value);
    response.setHeader("x-alder-dev-revision",String(servedRevision));
    response.setHeader("cache-control","no-store");
    const cookies = result.headers.getSetCookie();
    if (cookies.length) response.setHeader("set-cookie",cookies);
    if (request.method === "HEAD" || !result.body) {await result.body?.cancel();response.end();}
    else if (result.headers.get("content-type")?.startsWith("text/html")) {
      response.removeHeader("content-length");
      response.end((await result.text()).replace("</head>", `<meta name="alder-dev-revision" content="${servedRevision}"></head>`)
        .replace('src="/_alder/client.mjs"', `src="/_alder/client.mjs?revision=${servedRevision}"`));
    } else await pipeline(Readable.fromWeb(result.body),response);
  } catch (error) {
    if (response.destroyed) return;
    report(error);
    if (!response.headersSent) response.writeHead(500,{"content-type":"text/plain; charset=utf-8"});
    response.end("Alder local Worker request failed; see the terminal for details.");
  }
});
server.listen(port,host);
await once(server,"listening");
const address=server.address();
const urlHost=address.address.includes(":") ? `[${address.address}]` : address.address;
const listeningUrl=`http://${urlHost}:${address.port}`;
send({type:"listening",url:listeningUrl});

async function shutdown() {
  if (closing) return;
  closing=true;
  for (const response of streams) response.end();
  streams.clear();
  server.close();
  await update;
  await runtime?.dispose();
  server.closeAllConnections();
}
const input=createInterface({input:process.stdin,crlfDelay:Infinity});
const interrupted=()=>{input.close();process.stdin.destroy();void shutdown().catch(report);};
process.on("SIGINT",interrupted);
process.on("SIGTERM",interrupted);
try {
  for await (const line of input) {
    if (!line.trim()) continue;
    let message;
    try {message=JSON.parse(line);} catch(error) {failed(error);continue;}
    if (message.type==="shutdown") break;
    if (closing) break;
    if (message.type==="error") {diagnostic=String(message.message);broadcast({kind:"error",message:diagnostic,revision});send({type:"error",message:diagnostic});}
    else if (message.type==="build" || message.type==="render") {
      update=update.then(async()=>{
        await rebuild(message);
        if (message.type==="render") {
          const response=await runtime.dispatchFetch("http://alder-prerender/");
          if (!response.ok) throw new Error(`Prerender Worker returned HTTP ${response.status}`);
          send({type:"rendered",result:await response.json()});
        }
      }).catch(failed);
      await update;
    }
    else failed(new Error(`Unknown development event ${String(message.type)}`));
  }
} finally {input.close();await shutdown();}
