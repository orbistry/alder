# alder

## Development

### Validation

Before pushing, run:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

### Changesets

We use [sampo](https://github.com/bruits/sampo) for versioning and releases. When you make a notable change, add a changeset:

```sh
sampo add
```

Only list crates you actually changed -- sampo automatically bumps dependents.

### Release flow

1. Push to `main` with a changeset file
2. Sampo CI creates/updates a release PR that bumps versions and generates changelogs
3. Merge the release PR
4. Sampo publishes crates to crates.io and pushes version tags
5. Tags trigger cargo-dist to build and publish:
   - GitHub Releases with platform binaries
   - Homebrew formula (`brew install utxo-company/tap/alder`)
   - npm package (`npx @orbistry/cli`)

## Docs

### Imports

```
import { someFunc } from @microproofs/mpf
import tree from @microproofs/mpf/tree

import (
    { someFunc } from @microproofs/mpf
    thing from @microproofs/mpf/tree
)

// Without from
import @microproofs/mpf.{someFunc}
import @microproofs/mpf/tree

import (
    @microproofs/mpf.{someFunc}
    @microproofs/mpf/tree as thing
)
```

modules defined with `~` at the start of the name behave like `index.ts` in typescript modules

```
export {someFunc} from ~/leaf

export {someFunc} from @/tree
```

### Defining types

```
interface Thing {
  cmp(a, a) Ordering
}

type Wow = {
  thing: Int,
  next: Int,
  fiilll: Int,
}

type Something {
  Thing Wow
  Who (Wow)
  Me { thing: Int, next: Int }
}

when something is {
  Thing wow -> {
    wow.thing
  }
  Anal wow -> {
    wow.0.thing
  }
}
```
