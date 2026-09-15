---
cargo/alder-cli: minor
---

Install shared Cloudflare tooling explicitly with `alder cloudflare setup` and
authenticate with `alder cloudflare login`. Keep Node user-managed, reuse pinned
tooling across compatible compiler versions, and ship stock binary-only releases
without custom installer patches. Preserve existing legacy support directories.
Keep cargo-dist's generated installation instructions and download links when
composing aggregated release notes.
Keep successful setup output in Alder's status format, showing npm diagnostics
only when installation fails; leave interactive login output unchanged.
