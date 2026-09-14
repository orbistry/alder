// A stable listener owns HTTP/SSE while compiler-produced application modules
// are replaced. Failed compilation/evaluation keeps the last good application.
let application = null, revision = 0, diagnostic = null;
const streams = new Set();
const clients = new Map();
const encoder = new TextEncoder();
const broadcast = value => {
  const bytes = encoder.encode(`data: ${JSON.stringify(value)}\n\n`);
  for (const stream of streams) {try {stream.enqueue(bytes);} catch {streams.delete(stream);}}
};
const server = __alderHost.serve(async request => {
  const url = new URL(request.url);
  if (url.pathname === "/_alder/events") {
    let current;
    return new Response(new ReadableStream({
      start(controller) {
        current = controller;
        streams.add(controller);
        controller.enqueue(encoder.encode(`data: ${JSON.stringify(diagnostic ? {kind:"error", message:diagnostic, revision} : {kind:"ready", revision})}\n\n`));
      },
      cancel() {streams.delete(current);},
    }), {headers:{"content-type":"text/event-stream", "cache-control":"no-store", "connection":"keep-alive"}});
  }
  if (url.pathname === "/_alder/client.mjs" && url.searchParams.has("revision")) {
    const source = clients.get(Number(url.searchParams.get("revision"))) ?? "location.reload();";
    return new Response(request.method === "HEAD" ? null : source, {headers:{"content-type":"text/javascript; charset=utf-8", "cache-control":"no-store"}});
  }
  if (!application) return new Response(`<!doctype html><meta charset="utf-8"><title>Alder dev</title><pre id="status">Alder is compiling…</pre><script>const events=new EventSource('/_alder/events');events.onmessage=event=>{const message=JSON.parse(event.data);if(message.kind==='build'||(message.kind==='ready'&&message.revision>0))location.reload();if(message.kind==='error')document.getElementById('status').textContent=message.message;};</script>`, {status:503, headers:{"content-type":"text/html; charset=utf-8", "retry-after":"1"}});
  const servedRevision = revision;
  const response = await application.fetch(request, {}, {waitUntil(promise) {Promise.resolve(promise).catch(console.error);}});
  const headers = new Headers(response.headers);
  headers.set("x-alder-dev-revision", String(servedRevision));
  headers.set("cache-control", "no-store");
  if (request.method !== "HEAD" && response.headers.get("content-type")?.startsWith("text/html")) {
    headers.delete("content-length");
    const html = (await response.text()).replace("</head>", `<meta name="alder-dev-revision" content="${servedRevision}"></head>`)
      .replace('src="/_alder/client.mjs"', `src="/_alder/client.mjs?revision=${servedRevision}"`);
    return new Response(html, {status:response.status, statusText:response.statusText, headers});
  }
  return new Response(response.body, {status:response.status, statusText:response.statusText, headers});
}, {hostname:__alderHost.args[0], port:Number(__alderHost.args[1]), onListen:address => console.log(`Alder dev: http://${address.hostname}:${address.port}`)});
try {
  while (true) {
    const event = await __alderHost.devEvent();
    if (event.kind === "shutdown") break;
    if (event.kind === "error") {
      diagnostic = event.message;
      broadcast({kind:"error", message:diagnostic, revision});
    } else if (event.kind === "build") {
      try {
        const next = (await import(event.module)).default;
        if (typeof next?.fetch !== "function") throw new TypeError("Compiled development application has no fetch handler");
        const client = await next.fetch(new Request("http://alder-dev/_alder/client.mjs"), {}, {});
        if (!client.ok || !client.headers.get("content-type")?.includes("javascript")) throw new TypeError("Compiled development application has no browser entry");
        clients.set(event.revision, await client.text());
        if (clients.size > 8) clients.delete(clients.keys().next().value);
        application = next; revision = event.revision; diagnostic = null;
        broadcast({kind:"build", revision});
      } catch (error) {
        diagnostic = error.stack ?? String(error);
        console.error(diagnostic);
        broadcast({kind:"error", message:diagnostic, revision});
      }
    }
  }
} finally {
  for (const stream of streams) {try {stream.close();} catch {}}
  streams.clear();
  await server.shutdown();
}
