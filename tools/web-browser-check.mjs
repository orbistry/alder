// Explicit opt-in real-browser check, never invoked from cargo test.
// npm ci --prefix crates/alder-cli/support
// node tools/web-browser-check.mjs http://127.0.0.1:4317 [--hmr]
import assert from "node:assert/strict";
import { chromium } from "../crates/alder-cli/support/node_modules/playwright-core/index.mjs";

const url = process.argv[2] ?? "http://127.0.0.1:4317";
const browser = await chromium.launch({channel:"chrome", headless:true});
try {
  const context = await browser.newContext();
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.route("**/_alder/client.mjs*", async route => {
    await page.evaluate(() => {
      window.__alderOriginalCounter = document.getElementById("counter");
      window.__alderOriginalDoubled = document.getElementById("doubled");
    });
    await route.continue();
  });
  const response = await page.goto(url);
  assert.equal(response.status(), 200);
  assert.match(await response.text(), /Count: .*0/s);
  const originalTime = await page.evaluate(() => performance.timeOrigin);
  await page.getByRole("button", {name:"Count: 0"}).click();
  await page.getByRole("button", {name:"Count: 1"}).click();
  assert.equal(await page.locator("#counter").textContent(), "Count: 2");
  assert.equal(await page.locator("#doubled").textContent(), "Doubled: 4");
  assert.equal(await page.evaluate(() => window.__alderOriginalCounter === document.getElementById("counter") && window.__alderOriginalDoubled === document.getElementById("doubled")), true);
  console.log("PASS: SSR content, hydration node identity, native click, direct state/derived updates");
  if (process.argv.includes("--hmr")) {
    console.log("HMR_READY: counter=2; change heading to Alder hot update now");
    await page.getByRole("heading", {name:"Alder hot update", exact:true}).waitFor({timeout:120000});
    assert.equal(await page.locator("#counter").textContent(), "Count: 2");
    assert.equal(await page.locator("#doubled").textContent(), "Doubled: 4");
    assert.equal(await page.evaluate(() => performance.timeOrigin), originalTime);
    await page.getByRole("button", {name:"Count: 2"}).click();
    assert.equal(await page.locator("#counter").textContent(), "Count: 3");
    console.log("PASS: HMR preserves writable state, recomputes derived state, attaches new handlers, no document reload");
  }
  await page.getByRole("link", {name:"About this application"}).click();
  await page.getByRole("heading", {name:"About Alder", exact:true}).waitFor();
  assert.equal(new URL(page.url()).pathname, "/about");
  assert.equal(await page.evaluate(() => performance.timeOrigin), originalTime);
  await page.getByRole("link", {name:"Back to the counter"}).click();
  await page.getByRole("button", {name:"Count: 0"}).waitFor();
  assert.equal(new URL(page.url()).pathname, "/");
  assert.equal(await page.evaluate(() => performance.timeOrigin), originalTime);
  await page.goBack();
  await page.getByRole("heading", {name:"About Alder", exact:true}).waitFor();
  assert.deepEqual(errors, []);
  console.log("PASS: client navigation, history, component cleanup/remount, zero page errors");
} finally {
  await browser.close();
}
