// Opt-in manual check for the binding-free web-full production artifact.
// node tools/serve-built-worker.mjs /absolute/project/path [port]
import {readFile} from "node:fs/promises";
import {resolve} from "node:path";
import {Miniflare,convertV4MiniflareOptions} from "../crates/alder-cli/support/node_modules/miniflare/dist/src/index.js";
const root = resolve(process.argv[2] ?? "examples/web-full");
const port = Number(process.argv[3] ?? 3000);
const config = JSON.parse(await readFile(resolve(root,"dist/wrangler.jsonc"),"utf8"));
const runtime = new Miniflare(convertV4MiniflareOptions({
  name:config.name, rootPath:root, modules:true,
  script:await readFile(resolve(root,"dist/worker.mjs"),"utf8"),
  scriptPath:resolve(root,"dist/worker.mjs"),
  compatibilityDate:config.compatibility_date,
  compatibilityFlags:config.compatibility_flags,
  cf:false, telemetry:{enabled:false}, host:"127.0.0.1", port,
  assets:{directory:resolve(root,"dist/client"),binding:"ASSETS",run_worker_first:true,routerConfig:{has_user_worker:true}},
}));
try {
  console.log("Built Worker:",String(await runtime.ready));
  await new Promise(resolve => {
    process.once("SIGINT",resolve);
    process.once("SIGTERM",resolve);
  });
} finally {await runtime.dispose();}
