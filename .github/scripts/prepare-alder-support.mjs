// Release-only support preparation. No npm operation runs in a user's project.
import { createRequire } from "node:module";
import { lstatSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";

const repository = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const support = join(repository, "crates/alder-cli/support");
export const targets = {
  "darwin/arm64": "aarch64-apple-darwin",
  "darwin/x64": "x86_64-apple-darwin",
  "linux/arm64": "aarch64-unknown-linux-gnu",
  "linux/x64": "x86_64-unknown-linux-gnu",
  "win32/x64": "x86_64-pc-windows-msvc",
};

export function checkHost(requested, platform = process.platform, arch = process.arch, node = process.versions.node) {
  assert(Number(node.split(".")[0]) >= 22, "Compiler support requires Node.js 22 or newer");
  const target = targets[`${platform}/${arch}`];
  assert(target, `Unsupported native compiler support platform: ${platform}/${arch}`);
  assert.equal(requested, target, `Release support must be installed natively for exactly one target (${target}); cross-target/merged node_modules are unsafe`);
  return target;
}

export function rejectLinks(directory) {
  for (const name of readdirSync(directory)) {
    const path = join(directory, name);
    const stat = lstatSync(path);
    assert(!stat.isSymbolicLink(), `Release support must contain regular files, not symlinks: ${path}; use npm ci --bin-links=false`);
    if (stat.isDirectory()) rejectLinks(path);
    else assert(stat.isFile(), `Unsupported support entry: ${path}`);
  }
}

function verify(target) {
  const manifest = JSON.parse(readFileSync(join(support, "package.json"), "utf8"));
  for (const [name, version] of Object.entries(manifest.dependencies)) {
    assert.match(version, /^\d+\.\d+\.\d+(?:-[\w.-]+)?$/, `${name} must be pinned, not a semver range`);
    const installed = JSON.parse(readFileSync(join(support, "node_modules", name, "package.json"), "utf8"));
    assert.equal(installed.version, version, `Installed ${name} differs from the pinned support manifest`);
  }
  const require = createRequire(join(support, "package.json"));
  const workerd = require("workerd");
  const nativePath = workerd.default;
  assert(nativePath.startsWith(join(support, "node_modules") + "/") || nativePath.startsWith(join(support, "node_modules") + "\\"), "workerd must resolve inside the vendored support tree");
  execFileSync(nativePath, ["--version"], { stdio: "inherit" });
  // This also verifies the pinned Miniflare entry point can load with the
  // shipped production dependencies. No Worker or network service is started.
  require("miniflare");
  rejectLinks(support);
  writeFileSync(join(support, "release-support.json"), JSON.stringify({ target, node: ">=22", dependencies: manifest.dependencies, workerd: workerd.version }, null, 2) + "\n");
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [mode, requested] = process.argv.slice(2);
  assert(["--check-host", "--verify"].includes(mode), "Expected --check-host or --verify TARGET");
  const target = checkHost(requested);
  if (mode === "--verify") verify(target);
}
