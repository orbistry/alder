// Explicit manual acceptance check. Not invoked by Cargo tests.
// node tools/web-full-browser-check.mjs http://127.0.0.1:4340
import assert from "node:assert/strict";
import { chromium } from "../crates/alder-cli/support/node_modules/playwright-core/index.mjs";

const origin = process.argv[2] ?? "http://127.0.0.1:4340";
const browser = await chromium.launch({channel:"chrome", headless:true});
try {
  const context = await browser.newContext();
  const page = await context.newPage();
  const errors = [], remotes = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("request", request => {if (request.url().includes("/_alder/remote/")) remotes.push(request.url());});
  await page.route(/\/_alder\/(?:client|entry-[^/]+)\.mjs(?:\?.*)?$/, async route => {
    await page.evaluate(() => {window.__originalGreeting=document.getElementById("greeting");window.__originalCount=document.getElementById("layout-count");});
    await route.continue();
  });
  const initial = await page.goto(origin);
  // The production entry loads route chunks asynchronously after document load.
  await page.waitForLoadState("networkidle");
  assert.equal(initial.status(),200);
  assert.match(await initial.text(),/Hello, /);
  await page.locator("#greeting").waitFor();
  const time = await page.evaluate(() => performance.timeOrigin);
  assert.equal(await page.locator("#layout-count").textContent(),"1");
  await page.locator("#increment").click();
  assert.equal(await page.locator("#layout-count").textContent(),"2");
  assert.equal(await page.locator("#page-count").textContent(),"2");
  assert.equal(await page.evaluate(() => window.__originalGreeting===document.getElementById("greeting") && window.__originalCount===document.getElementById("layout-count")),true);
  assert.equal(remotes.length,0,"hydration and unrelated browser store changes must not refetch the server resource");
  console.log("PASS: SSR resource hydration reuses nodes and data; shared store updates without a remote fetch");

  for (let index=0;index<2;index++) {
    const response=page.waitForResponse(response=>response.url().includes("/_alder/remote/") && response.url().endsWith("/greet"));
    await page.locator("#refresh").click();
    assert.equal((await response).status(),200);
    await page.locator("#greeting").waitFor();
  }
  await page.locator("#command").click();
  await page.waitForFunction(()=>document.getElementById("command-count")?.textContent==="2");
  await page.locator("#greeting").waitFor();
  assert.match(await page.locator("#greeting").textContent(),/server request count: 1/);
  assert.equal(await page.locator("#layout-count").textContent(),"2");
  console.log("PASS: explicit refresh crosses HTTP; command invalidates queries without changing browser stores");

  await page.getByRole("link",{name:"Ada",exact:true}).click();
  await page.locator("#name-form").waitFor();
  assert.equal(new URL(page.url()).pathname,"/users/ada");
  assert.equal(await page.locator("#layout-count").textContent(),"2");
  assert.match(await page.locator("#viewer").textContent(),/Alder visitor/);
  assert.equal(await page.locator("#request-count").textContent(),"1");
  await page.locator("#name").fill("A");
  await page.getByRole("button",{name:"Submit typed action"}).click();
  await page.waitForFunction(()=>document.getElementById("form-status")?.textContent==="Use at least two characters.");
  await page.locator("#name").fill("Ada Lovelace");
  await page.getByRole("button",{name:"Submit typed action"}).click();
  await page.waitForFunction(()=>document.getElementById("form-status")?.textContent==="Alder visitor submitted: Ada Lovelace");
  assert.equal(await page.evaluate(()=>performance.timeOrigin),time);
  console.log("PASS: client navigation preserves stores; native form submission uses typed server validation and hook context");

  for (const name of ["Grace", "Ada", "Grace", "Ada"]) {
    await page.getByRole("link",{name,exact:true}).click();
    await page.waitForFunction(id => document.getElementById("user-id")?.textContent === `User: ${id}`,name.toLowerCase());
    assert.equal(await page.locator("#layout-count").textContent(),"2");
    assert.equal(await page.evaluate(()=>performance.timeOrigin),time);
  }
  console.log("PASS: repeated Ada/Grace prerender navigation keeps the same document and browser store");

  await page.getByRole("link",{name:"Try the typed error boundary"}).click();
  await page.locator("#page-error").waitFor();
  assert.match(await page.locator("#page-error").textContent(),/That user does not exist/);
  await page.getByRole("link",{name:"Return home",exact:true}).click();
  await page.locator("#greeting").waitFor();
  assert.equal(await page.locator("#layout-count").textContent(),"2");
  assert.equal(await page.evaluate(()=>performance.timeOrigin),time);
  assert.deepEqual(errors,[]);
  console.log("PASS: typed error boundary, navigation cleanup, and zero browser exceptions");
} finally {await browser.close();}
