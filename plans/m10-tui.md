# M10: TUI

Terminal applications on the `standalone` target using the same signal
graph, stores, and markup grammar as the web, with their own element
vocabulary and a Rust-side renderer, layout engine, and input loop inside
the embedded runtime.

## Starting state

- M6: signals, stores, markup checking, reactive regions.
- M2b: `standalone` runtime with deno_core; `Tui` is not yet a module.
- Parser: markup elements are keyword-insensitive dashed names, so
  `<box>` and `<text>` already parse.
- `docs/web.md` TUI section; `docs/runtime.md` targets table.

## Exit criteria

- `fn main() { Tui.run(App) }` renders a component tree of TUI elements
  to the terminal with flexbox layout, redraws on signal changes, handles
  keyboard and resize events, and restores the terminal on exit and on
  panic.
- The TUI element vocabulary is checked by the same markup checker as
  HTML, from a separate schema; using an HTML element in a TUI component
  or the reverse is a compile error.
- Stores and `resource` work unchanged.

## Settled decisions

- Separate element vocabulary, shared reactivity (`docs/web.md`).
- Layout and I/O in Rust behind deno_core ops; the JS side holds the
  element tree and signals.

## Open decisions (recommendation in bold)

1. Element set. **`box` (flex container: direction, gap, padding, border,
   width/height constraints), `text` (spans with bold/dim/italic/color,
   wrapping), `input` (single-line editor), `list` (virtualized, keyed),
   `spacer`, `scroll`.** Keep it small; everything else is components.
2. Layout engine. **`taffy` in Rust, one layout pass per frame on the
   dirty subtree.**
3. Rendering. **Diffed cell buffer (like ratatui's double buffer) written
   through `crossterm`; the JS side sends element-tree patches, Rust
   owns layout and painting.**
4. Focus and input. **A focus ring computed from the tree order with
   `tabIndex`; key events dispatched to the focused element's handlers,
   then bubbled; global keymaps via a `Tui.keys` store.**
5. Web preview. **Out of scope.**

## Work breakdown

### Wave 0: contract

Design panel producing `docs/tui-internals.md`: element schema, the
tree-patch protocol between JS and Rust, layout and paint pipeline,
event model, terminal lifecycle, testing approach (a headless terminal
backend that records frames as text).

### Wave 1 (parallel)

- TUI schema in the markup checker with target gating.
- Kernel `tui` module: element tree, patches, event dispatch, focus.
- Rust: `alder-tui` crate with taffy layout, cell buffer, crossterm
  backend, headless backend; deno_core ops.
- `std/` `Tui` module and components (`List`, `TextInput`, `Table`).

### Wave 2: sweep

- e2e: the docs' task list example rendered headlessly with frame
  snapshots; resize and key events.
- Docs, SPEC M10 ticked, changeset, critic pass.

## Tests to add (minimum)

- Frame snapshots per element and per layout property; focus traversal;
  key handling; resize reflow; terminal restore on panic (integration
  test with a pty).

## Risks

- A JS/Rust boundary per frame can be slow; batch patches and measure
  with a 10k-row list.
- Terminal restore on panic must be bulletproof or users lose their
  shell; test it explicitly.
