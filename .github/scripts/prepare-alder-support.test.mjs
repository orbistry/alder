import { test } from "node:test";
import assert from "node:assert/strict";
import { checkHost, targets } from "./prepare-alder-support.mjs";

test("every advertised target has a native support host", () => {
  for (const [host, target] of Object.entries(targets)) {
    const [platform, arch] = host.split("/");
    assert.equal(checkHost(target, platform, arch, "22.0.0"), target);
  }
});
test("reject cross-target bundles, merged tasks, and obsolete Node", () => {
  assert.throws(() => checkHost(targets["linux/arm64"], "linux", "x64", "22.0.0"));
  assert.throws(() => checkHost(Object.values(targets).join(" "), "linux", "x64", "22.0.0"));
  assert.throws(() => checkHost(targets["linux/x64"], "linux", "x64", "20.0.0"));
});
