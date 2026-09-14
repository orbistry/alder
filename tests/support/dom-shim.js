// Deterministic test-only DOM. The parser handles the first-slice SSR grammar,
// not arbitrary HTML; browser parsing rules are enforced by the markup checker.
export class TestDocument {
  constructor() { this.mutations = 0; this.allocations = 0; }
  createElement(tag) { return new TestElement(this, tag); }
  createTextNode(text) { return new TestNode(this, 3, text); }
  createComment(text) { return new TestNode(this, 8, text); }
}

class TestNode {
  constructor(document, type, data = "") {
    this.ownerDocument = document;
    this.nodeType = type;
    this._data = data;
    this.parentNode = null;
    this.childNodes = [];
    document.allocations++;
  }
  get data() { return this._data; }
  set data(value) { this.ownerDocument.mutations++; this._data = String(value); }
  get firstChild() { return this.childNodes[0] ?? null; }
  get nextSibling() {
    if (!this.parentNode) return null;
    const siblings = this.parentNode.childNodes;
    return siblings[siblings.indexOf(this) + 1] ?? null;
  }
  get textContent() {
    return this.nodeType === 3 ? this.data : this.childNodes.map(node => node.textContent).join("");
  }
  appendChild(node) { return this.insertBefore(node, null); }
  insertBefore(node, anchor) {
    if (node.parentNode) node.parentNode.removeChild(node);
    const index = anchor === null ? this.childNodes.length : this.childNodes.indexOf(anchor);
    if (index < 0) throw new Error("invalid insertBefore anchor");
    this.ownerDocument.mutations++;
    this.childNodes.splice(index, 0, node);
    node.parentNode = this;
    return node;
  }
  removeChild(node) {
    const index = this.childNodes.indexOf(node);
    if (index < 0) throw new Error("invalid removeChild target");
    this.ownerDocument.mutations++;
    this.childNodes.splice(index, 1);
    node.parentNode = null;
    return node;
  }
}

class TestElement extends TestNode {
  constructor(document, tag) {
    super(document, 1);
    this.localName = tag;
    this.namespaceURI = "http://www.w3.org/1999/xhtml";
    this.attributeMap = new Map();
    this.listeners = new Map();
    if (tag === "template") this.content = new TestNode(document, 11);
  }
  get attributes() { return [...this.attributeMap].map(([name, value]) => ({ name, value })); }
  getAttribute(name) { return this.attributeMap.get(name) ?? null; }
  setAttribute(name, value) { this.ownerDocument.mutations++; this.attributeMap.set(name, String(value)); }
  removeAttribute(name) { this.ownerDocument.mutations++; this.attributeMap.delete(name); }
  addEventListener(name, listener) {
    if (!this.listeners.has(name)) this.listeners.set(name, new Set());
    this.listeners.get(name).add(listener);
  }
  removeEventListener(name, listener) { this.listeners.get(name)?.delete(listener); }
  dispatchEvent(event) {
    for (const listener of [...(this.listeners.get(event.type) ?? [])]) listener(event);
  }
}

function decode(text) {
  const entities = { amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'" };
  return text.replace(/&(amp|lt|gt|quot|#39);/g, (_, name) => entities[name]);
}

export function parseSsr(document, html) {
  const target = document.createElement("main");
  const stack = [target];
  for (const token of html.match(/<!--[\s\S]*?-->|<\/?[^>]+>|[^<]+/g) ?? []) {
    const frame = stack[stack.length - 1];
    const parent = frame.content ?? frame;
    if (token.startsWith("<!--")) {
      parent.appendChild(document.createComment(token.slice(4, -3)));
    } else if (token.startsWith("</")) {
      if (frame.localName !== token.slice(2, -1)) throw new Error("unbalanced SSR close tag");
      stack.pop();
    } else if (token.startsWith("<")) {
      const name = /^<([a-z][a-z0-9-]*)/.exec(token)?.[1];
      if (!name) throw new Error("invalid SSR tag");
      const node = document.createElement(name);
      for (const attr of token.matchAll(/\s+([^\s=]+)="([^"]*)"/g)) node.setAttribute(attr[1], decode(attr[2]));
      parent.appendChild(node);
      if (!["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"].includes(name)) stack.push(node);
    } else if (token !== "") {
      let text = decode(token);
      if (["textarea", "pre", "listing"].includes(frame.localName) && !parent.firstChild && text.startsWith("\n")) text = text.slice(1);
      if (text === "") continue;
      const previous = parent.childNodes[parent.childNodes.length - 1];
      if (previous?.nodeType === 3) previous.data += text;
      else parent.appendChild(document.createTextNode(text));
    }
  }
  if (stack.length !== 1) throw new Error("unclosed SSR tag");
  return target;
}

export function nodes(root) { return [root, ...(root.content ? nodes(root.content) : []), ...root.childNodes.flatMap(nodes)]; }
export function element(root, tag) { return nodes(root).find(node => node.localName === tag); }
export function listenerCount(root) {
  return nodes(root).reduce((total, node) => total + [...(node.listeners?.values() ?? [])].reduce((sum, set) => sum + set.size, 0), 0);
}
export function assert(condition, message) { if (!condition) throw new Error(message); }
