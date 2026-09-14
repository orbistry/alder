// Manual dev-race check; make the two requested edits in a disposable app copy.
import assert from "node:assert/strict";
import {chromium} from "../crates/alder-cli/support/node_modules/playwright-core/index.mjs";
const origin = process.argv[2];
const browser = await chromium.launch({channel:"chrome",headless:true});
try {
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror",error => errors.push(error.message));
  await page.addInitScript(() => {
    const Native = EventSource;
    window.EventSource = class extends Native {
      constructor(...args) {super(...args); window.__alderEvents = this;}
      addEventListener(name,listener,...args) {
        super.addEventListener(name,event => {
          if (name === "message" && window.__dropBuild) {
            const message = JSON.parse(event.data);
            if (message.kind === "build") {window.__missedBuild = message;return;}
          }
          listener(event);
        },...args);
      }
    };
  });
  let release, reached;
  const intercepted = new Promise(resolve => {reached=resolve;});
  const resume = new Promise(resolve => {release=resolve;});
  let held = false;
  await page.route("**/_alder/client.mjs*",async route => {
    if (!held) {held=true;reached();await resume;}
    await route.continue();
  });
  const navigation = page.goto(origin,{timeout:180000});
  await intercepted;
  console.log("EDIT_BOOTSTRAP: change the layout heading to 'Alder boot race recovered'");
  const deadline = Date.now()+180000;
  while (!(await (await fetch(origin)).text()).includes("Alder boot race recovered")) {
    if (Date.now()>deadline) throw new Error("Timed out waiting for source edit");
    await new Promise(resolve => setTimeout(resolve,300));
  }
  release();
  await navigation;
  await page.getByRole("heading",{name:"Alder boot race recovered",exact:true}).waitFor();
  assert.deepEqual(errors,[]);
  console.log("PASS: document/client revision pinning survives a build before hydration; ready catches the newer build");
  const time = await page.evaluate(() => performance.timeOrigin);
  await page.locator("#increment").click();
  assert.equal(await page.locator("#layout-count").textContent(),"2");
  await page.evaluate(() => {window.__dropBuild=true;});
  console.log("EDIT_RECONNECT: change the layout heading to 'Alder reconnect recovered'");
  await page.waitForFunction(() => window.__missedBuild, undefined, {timeout:180000});
  assert.equal(await page.getByRole("heading",{name:"Alder boot race recovered",exact:true}).count(),1);
  await page.evaluate(() => {
    window.__dropBuild=false;
    window.__alderEvents.dispatchEvent(new MessageEvent("message",{data:JSON.stringify({...window.__missedBuild,kind:"ready"})}));
  });
  await page.getByRole("heading",{name:"Alder reconnect recovered",exact:true}).waitFor();
  assert.equal(await page.locator("#layout-count").textContent(),"2");
  assert.equal(await page.evaluate(() => performance.timeOrigin),time);
  assert.deepEqual(errors,[]);
  console.log("PASS: reconnect-ready catches a missed build with state intact and no document reload");
} finally {await browser.close();}
