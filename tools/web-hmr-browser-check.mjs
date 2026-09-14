// Manual check: start dev on a disposable web-full copy, then follow stdout.
// Source edits are made separately; this harness never changes project files.
import assert from "node:assert/strict";
import {chromium} from "../crates/alder-cli/support/node_modules/playwright-core/index.mjs";
const browser = await chromium.launch({channel:"chrome", headless:true});
try {
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(process.argv[2]);
  await page.locator("#greeting").waitFor();
  const time = await page.evaluate(() => performance.timeOrigin);
  await page.locator("#increment").click();
  await page.locator("#command").click();
  await page.waitForFunction(() => document.getElementById("command-count")?.textContent === "2");
  console.log("EDIT_LAYOUT: change the layout heading to 'Alder edited layout'");
  await page.getByRole("heading", {name:"Alder edited layout",exact:true}).waitFor({timeout:180000});
  assert.equal(await page.locator("#layout-count").textContent(),"2");
  assert.equal(await page.locator("#command-count").textContent(),"2");
  assert.equal(await page.evaluate(() => performance.timeOrigin),time);
  console.log("BREAK_SOURCE: introduce an undefined name in the page");
  await page.locator("#alder-dev-error").waitFor({timeout:180000});
  assert.match(await page.locator("#alder-dev-error").textContent(), /page|name|scope/i);
  assert.equal(await page.locator("#command-count").textContent(),"2");
  console.log("RECOVER_SOURCE: fix the error and change page heading to 'Recovered page'");
  await page.getByRole("heading",{name:"Recovered page",exact:true}).waitFor({timeout:180000});
  await page.locator("#alder-dev-error").waitFor({state:"detached"});
  assert.equal(await page.locator("#layout-count").textContent(),"2");
  assert.equal(await page.locator("#command-count").textContent(),"2");
  await page.locator("#increment").click();
  assert.equal(await page.locator("#layout-count").textContent(),"3");
  console.log("ADD_ROUTE: add /added page and an 'Added route' link in the layout");
  await page.getByRole("link",{name:"Added route",exact:true}).waitFor({timeout:180000});
  await page.getByRole("link",{name:"Added route",exact:true}).click();
  await page.getByRole("heading",{name:"Discovered new route",exact:true}).waitFor();
  assert.equal(new URL(page.url()).pathname,"/added");
  assert.equal(await page.locator("#layout-count").textContent(),"3");
  assert.equal(await page.evaluate(() => performance.timeOrigin),time);
  await page.goBack();
  await page.getByRole("heading",{name:"Recovered page",exact:true}).waitFor();
  assert.equal(await page.locator("#layout-count").textContent(),"3");
  console.log("REMOVE_ROUTE: remove /added page and its layout link");
  await page.getByRole("link",{name:"Added route",exact:true}).waitFor({state:"detached",timeout:180000});
  assert.equal((await page.request.get(new URL("/added",page.url()).href)).status(),404);
  assert.equal(await page.locator("#layout-count").textContent(),"3");
  assert.equal(await page.evaluate(() => performance.timeOrigin),time);
  assert.deepEqual(errors,[]);
  console.log("PASS: layout HMR, component/store state, compile diagnostics/recovery, route addition/deletion, history, no reload or browser errors");
} finally {await browser.close();}
