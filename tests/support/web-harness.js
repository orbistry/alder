import { $webSsr, $webSsrAsync, $webMount, $webHydrate, $webCreateStoreScope, $webStoreRun, $webDisposeStoreScope, $webRestoreStores } from "alder:kernel";
import { TestDocument, parseSsr, nodes, element, listenerCount, assert } from "./dom-shim.js";

let setups = 0;
let derives = 0;
export async function checkWhitespace(factory) {
  const document = new TestDocument();
  const html = $webSsr(factory());
  assert((await $webSsrAsync(factory())).html === html, 'sync and async SSR agree on literal whitespace');
  const expected = {
    adjacent: 'AB', prose: 'Hello world', inline: 'Hello Ada!', joined: 'HelloAda!',
    explicit: 'Hello Ada!', expression: 'Hello Ada!', punctuation: 'Ada!', spaces: 'A  B',
    runtime: '  A\n B  ', pre: '\n  A\n\n B  ', textarea: '\n  A\n B  ',
  };
  function check(target) {
    for (const [id, text] of Object.entries(expected)) {
      const node = nodes(target).find(node => node.getAttribute?.('id') === id);
      assert(node?.textContent === text, `${id}: expected ${JSON.stringify(text)}, got ${JSON.stringify(node?.textContent)}`);
    }
  }
  const target = parseSsr(document, html);
  check(target);
  const before = nodes(target), allocations = document.allocations;
  const instance = $webHydrate(factory(), target);
  check(target);
  assert(allocations === document.allocations && before.every((node, i) => nodes(target)[i] === node), 'whitespace hydration reuses every SSR node');
  element(target, 'button').dispatchEvent({type:'click'});
  assert(nodes(target).find(node => node.getAttribute?.('id') === 'expression').textContent === 'Hello Grace!', 'reactive expression preserves surrounding literal spaces');
  instance.dispose();
  const direct = document.createElement('div');
  const mounted = $webMount(factory(), direct);
  check(direct);
  mounted.dispose();
}
export async function checkNamedHandlers(factory) {
  const document = new TestDocument();
  const target = parseSsr(document, $webSsr(factory()));
  const instance = $webHydrate(factory(), target);
  const button = element(target, 'button');
  assert(target.textContent === '0', 'named handlers are not executed during setup or hydration');
  for (let count = 1; count <= 2; count++) {
    button.dispatchEvent({type: 'click'});
    await new Promise(resolve => setTimeout(resolve, 1));
    assert(target.textContent === String(count) && listenerCount(target) === 1, 'named Task handler writes live state and named read closure updates reactively');
  }
  instance.dispose();
  assert(listenerCount(target) === 0, 'named callbacks release their subscriptions on disposal');
}
export async function checkStores(factory, read, increment) {
  async function request(count) {
    const storeScope = $webCreateStoreScope();
    $webStoreRun(storeScope, () => { assert(read() === 1, 'request gets fresh imported store'); for (let index = 0; index < count; index++) increment(); });
    await Promise.resolve();
    const result = await $webSsrAsync(factory(), {storeScope});
    $webDisposeStoreScope(storeScope);
    return result;
  }
  const [first, second] = await Promise.all([request(1), request(10)]);
  assert(first.stores[0].value === 2 && second.stores[0].value === 11, 'compiled external functions dereference independent request stores');
  const document = new TestDocument();
  const target = parseSsr(document, first.html);
  const before = nodes(target);
  $webRestoreStores(first.stores);
  const instance = $webHydrate(factory(), target);
  assert(target.textContent === '24' && before.every((node, index) => nodes(target)[index] === node), 'imported store hydrates request state without replacing nodes');
  element(target, 'button').dispatchEvent({type: 'click'});
  assert(target.textContent === '36' && read() === 3, 'imported store writes update direct and derived subscriptions');
  instance.dispose();
}
let resourceRequests = 0;
let resourceSetups = 0;
const resourcePending = [];
export function resourceSetup() { resourceSetups++; }
export function resourceLoad(value) {
  resourceRequests++;
  return new Promise(resolve => resourcePending.push(() => resolve(value < 3 ? {$: 'Ok', _0: value * 2} : {$: 'Err', _0: {$: ':missing', _0: 'missing'}})));
}
export async function checkResources(factory) {
  async function until(check) {
    for (let attempt = 0; attempt < 100; attempt++) {
      if (check()) return;
      await new Promise(resolve => setTimeout(resolve, 0));
    }
    throw new Error('resource did not settle');
  }
  const result = $webSsrAsync(factory());
  await until(() => resourcePending.length > 0);
  resourcePending.shift()();
  const rendered = await result;
  assert(resourceSetups === 1 && resourceRequests === 1 && rendered.resources.length === 1, 'compiled SSR sets up and loads once');
  const document = new TestDocument();
  const target = parseSsr(document, rendered.html);
  const before = nodes(target);
  const instance = $webHydrate(factory(), target, {resources: rendered.resources});
  const id = name => nodes(target).find(node => node.getAttribute?.('id') === name);
  assert(resourceRequests === 1 && resourceSetups === 2 && id('ready').textContent === '2', 'compiled hydration uses serialized Ready state');
  assert(before.every((node, index) => nodes(target)[index] === node), 'resource hydration preserves SSR nodes');
  id('next').dispatchEvent({type: 'click'});
  assert(id('loading'), 'resource dependency publishes Loading');
  await until(() => resourcePending.length > 0);
  resourcePending.shift()();
  await until(() => id('ready'));
  assert(id('ready').textContent === '4' && resourceSetups === 2 && resourceRequests === 2, 'reactive loader refreshes without component setup');
  id('refresh').dispatchEvent({type: 'click'});
  await until(() => resourcePending.length > 0);
  resourcePending.shift()();
  await until(() => id('ready'));
  assert(resourceRequests === 3, 'compiled refresh retains resource handle');
  id('next').dispatchEvent({type: 'click'});
  await until(() => resourcePending.length > 0);
  resourcePending.shift()();
  await until(() => id('failed'));
  assert(id('failed').textContent === 'missing', 'Result Err reaches the typed Failed arm');
  instance.dispose();
  assert(target.childNodes.length === 0 && listenerCount(target) === 0, 'compiled resource instance disposes');
}
export function checkComposition(factory) {
  const view = factory();
  const document = new TestDocument();
  const target = parseSsr(document, $webSsr(view));
  const before = nodes(target);
  const allocations = document.allocations;
  const mounted = $webHydrate(view, target);
  assert(allocations === document.allocations && before.every((node, index) => nodes(target)[index] === node), "compiled composition hydrates existing nodes");
  const id = name => nodes(target).find(node => node.getAttribute?.("id") === name);
  const count = id("count");
  const prop = id("prop");
  id("increment").dispatchEvent({ type: "click" });
  assert(id("count") === count && count.textContent === "1" && id("match").textContent === "1", "children track outer cells and match switches arms");
  assert(id("prop") === prop && prop.textContent === "2", "component props update derived cells without replacing child nodes");
  const matched = id("match");
  id("increment").dispatchEvent({ type: "click" });
  assert(id("match") === matched && matched.textContent === "2", "same match arm refreshes pattern input without replacing nodes");
  const rows = nodes(target).filter(node => node.getAttribute?.("class") === "row");
  id("reorder").dispatchEvent({ type: "click" });
  const reordered = nodes(target).filter(node => node.getAttribute?.("class") === "row");
  assert(reordered[0] === rows[1] && reordered[1] === rows[0] && reordered.map(node => node.textContent).join("") === "BA", "compiled keyed rows preserve nodes and refresh fields");
  id("toggle").dispatchEvent({ type: "click" });
  assert(!id("branch"), "if switches owned branch");
  mounted.dispose();
  assert(target.childNodes.length === 0 && listenerCount(target) === 0, "compiled composition disposes all descendants");
}
export function recordSetup(value) { setups++; return value; }
export function recordDerived(value) { derives++; return value * 2; }

export function checkReactivity(factory) {
  const view = factory(1);
  assert(setups === 0 && derives === 0, "component call defers setup until rendering");
  const html = $webSsr(view);
  assert(setups === 1 && derives === 1, "SSR runs setup and initial derivation once");
  const document = new TestDocument();
  const target = parseSsr(document, html);
  const instance = $webHydrate(view, target);
  assert(setups === 2 && derives === 2, "hydrate reconstructs its own owner exactly once");
  const button = id => nodes(target).find(node => node.getAttribute?.("id") === id);
  button("other").dispatchEvent({ type: "click" });
  assert(button("other").textContent === "1" && derives === 2 && setups === 2, "unrelated state does not re-run setup or derivation");
  button("same").dispatchEvent({ type: "click" });
  assert(derives === 2 && setups === 2, "equal writes do not re-run setup or derivation");
  button("count").dispatchEvent({ type: "click" });
  assert(button("count").textContent === "2" && element(target, "span").textContent === "4", "tracked dependency updates");
  assert(derives === 3 && setups === 2, "dependency change re-runs only derivation once");
  instance.dispose();
}

export function checkCounter(factory) {
  const props = { initial: 2, label: '<img src=x onerror="bad"> & \'quote\'' };
  const text = '&lt;img src=x onerror="bad"&gt; &amp; \'quote\'';
  const attr = '&lt;img src=x onerror=&quot;bad&quot;&gt; &amp; &#39;quote&#39;';
  const html = $webSsr(factory(props));
  assert(html === '<section title="' + attr + '"><button><!--alder:text-->' + text + '<!--/alder:text--></button><span title="Count 2"><!--alder:text-->2<!--/alder:text--></span><strong><!--alder:text-->4<!--/alder:text--></strong></section>', "escaped SSR output");
  assert(!html.includes("onClick") && !html.includes("disabled="), "SSR excludes handlers and false boolean attrs");
  const document = new TestDocument();
  const target = parseSsr(document, html);
  const before = nodes(target);
  const mutations = document.mutations;
  const allocations = document.allocations;
  const instance = $webHydrate(factory(props), target);
  assert(nodes(target).every((node, index) => node === before[index]), "hydration node identity");
  assert(document.mutations === mutations && document.allocations === allocations, "hydration does not mutate or allocate DOM");
  assert(listenerCount(target) === 1, "hydration attaches one event listener");
  const button = element(target, "button");
  const span = element(target, "span");
  const strong = element(target, "strong");
  button.dispatchEvent({ type: "click", ignored: true });
  assert(span.textContent === "3" && strong.textContent === "6", "state and derived text update");
  assert(span.getAttribute("title") === "Count 3", "dynamic attribute update");
  assert(nodes(target).every((node, index) => node === before[index]), "updates preserve nodes");

  const secondTarget = document.createElement("main");
  const second = $webMount(factory({ initial: 10, label: "other" }), secondTarget);
  assert(element(secondTarget, "span").textContent === "10", "mount initial state");
  assert(element(secondTarget, "button").getAttribute("disabled") === "", "mount true boolean attribute");
  button.dispatchEvent({ type: "click" });
  button.dispatchEvent({ type: "click" });
  assert(span.textContent === "5" && strong.textContent === "10", "repeated updates");
  assert(button.getAttribute("disabled") === "", "reactive boolean attribute");
  assert(element(secondTarget, "span").textContent === "10", "instances are independent");

  instance.dispose();
  instance.dispose();
  assert(target.childNodes.length === 0 && listenerCount(button) === 0, "disposal unmounts and detaches events");
  const afterDispose = document.mutations;
  button.dispatchEvent({ type: "click" });
  assert(document.mutations === afterDispose && span.textContent === "5", "detached event cannot update disposed state");
  element(secondTarget, "button").dispatchEvent({ type: "click" });
  assert(element(secondTarget, "span").textContent === "11", "disposing one instance leaves others alive");
  second.dispose();

  const mismatched = parseSsr(document, html);
  element(mismatched, "strong").childNodes[1].data = "wrong";
  const mismatchNodes = nodes(mismatched);
  const beforeMismatch = document.mutations;
  let error;
  try { $webHydrate(factory(props), mismatched); } catch (caught) { error = caught; }
  assert(error?.message.startsWith("Alder hydration mismatch:"), "mismatch is explicit");
  assert(listenerCount(mismatched) === 0, "failed hydration detaches earlier listeners");
  assert(nodes(mismatched).every((node, index) => node === mismatchNodes[index]), "failed hydration preserves existing nodes");
  assert(document.mutations === beforeMismatch, "failed hydration never repairs DOM");
  assert($webSsr(factory(props)) === html, "SSR has fresh state after client updates");
}
