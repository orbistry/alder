---
cargo/alder-cli: patch
---

Keep the production build identity stable across prerendering and final bundling,
preventing unnecessary navigation reloads and loss of browser state.
