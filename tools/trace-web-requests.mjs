// Local-only manual browser verification; no browser automation.
// node tools/trace-web-requests.mjs <upstream-port> [listen-port] [blocked-asset-path ...]
import http from "node:http";

const upstreamPort = Number(process.argv[2]);
const listenPort = Number(process.argv[3] ?? 3005);
const blockedAssets = new Set(process.argv.slice(4));
for (const path of blockedAssets) {
  if (!path.startsWith("/_alder/") || path.includes("?") || path.includes("#")) {
    throw new Error("Fault injection must name an exact /_alder/ asset path");
  }
}
for (const port of [upstreamPort, listenPort]) {
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error("Expected TCP ports between 1 and 65535");
  }
}
if (upstreamPort === listenPort) throw new Error("Proxy and upstream ports must differ");

const server = http.createServer((request, response) => {
  if (blockedAssets.has(request.url)) {
    console.log(`${request.method} ${request.url} -> 503 (injected)`);
    response.writeHead(503, {"cache-control": "no-store", "content-type": "text/plain"});
    response.end("Deliberate local asset failure");
    return;
  }
  const upstream = http.request({
    hostname: "127.0.0.1", port: upstreamPort,
    path: request.url, method: request.method, headers: request.headers,
  }, result => {
    console.log(`${request.method} ${request.url} -> ${result.statusCode}`);
    response.writeHead(result.statusCode, result.headers);
    result.pipe(response);
    result.on("error", () => response.destroy());
  });
  upstream.on("error", error => {
    console.error(`Upstream request failed: ${error.message}`);
    if (!response.headersSent) response.writeHead(502);
    response.end();
  });
  request.on("aborted", () => upstream.destroy());
  request.pipe(upstream);
});
server.listen(listenPort, "127.0.0.1", () => {
  console.log(`Tracing http://127.0.0.1:${listenPort} -> 127.0.0.1:${upstreamPort}`);
});
