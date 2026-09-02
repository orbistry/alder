# Plans

One file per remaining milestone of the Alder roadmap (`SPEC.md`). Each
plan is written so that a fresh agent, human or otherwise, can execute it
without this conversation: it states where the repo is when the milestone
starts, what "done" means, every decision that is already settled, every
decision still open (with a recommendation), the work broken into steps
with file ownership, the tests each step must add, and the risks.

| Plan                     | Milestone                                     | Depends on |
| ------------------------ | --------------------------------------------- | ---------- |
| `m2-core-language.md`    | Front end green on the new AST, then JS + run | M1 (done)  |
| `m3-traits.md`           | Type classes, HKT, dictionaries               | M2         |
| `m4-errors-and-async.md` | Error rows, `Task`, fibers, context           | M2, M3     |
| `m5-macros.md`           | Compile-time Alder, derives                   | M2b, M3    |
| `m6-web.md`              | Components, SSR, routing, Cloudflare          | M4, M5     |
| `m7-data.md`             | `table`, `query`, migrations, `schema`        | M3, M4     |
| `m8-styles-forms-api.md` | `style`, forms, typed API clients             | M6, M7     |
| `m9-tooling.md`          | Tests, LSP, docs, publishing                  | M2b, M5    |
| `m10-tui.md`             | Terminal renderer                             | M6         |

## Ground rules that apply to every plan

- **Authority.** `docs/*.md` describe the language and framework;
  `SPEC.md` holds the grammar and the checklists; `docs/parser-internals.md`
  is the parser contract. When a plan and a doc disagree, fix the doc in
  the same change and say so in the commit.
- **Conventions.** `CLAUDE.md` is binding: bumpalo arenas and the AST type
  rules, insta snapshot macros per module, `cargo fmt`, `cargo clippy
--all-targets --all-features -- -D warnings`, `cargo test`. Changesets go
  in `.sampo/changesets/` (one per milestone is fine, list every crate
  touched).
- **Gate.** A milestone is not done until the whole workspace builds and
  CI is green. There is no `default-members` trick and no `#[ignore]`
  left behind. If a crate is knowingly red for a while inside a milestone,
  say so in the plan's step list and in the commit messages.
- **How M1 was run, and how to run the rest.** Each milestone starts with
  a design contract written by a judged panel (three designers from
  different angles, judges, a synthesizer, a completeness critic) and
  committed to `docs/<area>-internals.md`. The contract fixes every shared
  type and signature so implementers never edit shared files. Then a
  serial "wave 0" lands the shared skeleton with `todo!()` stubs, and
  parallel waves of owners work in git worktrees created from the branch
  (`git worktree add <scratch>/<name> -b w<N>/<name> <branch>`), each
  owning disjoint files, each followed by an adversarial reviewer who
  reads the snapshot files rather than trusting them, then a fix pass, then
  one merge agent that merges, un-ignores tests whose dependencies landed
  (before running `cargo insta test --unreferenced delete`, never after
  ignoring), and runs the gate. A final wave sweeps `#[allow(unused)]`,
  updates docs and SPEC, and runs a crate-wide critic pass (grammar or
  contract completeness, fuzzing with every docs example, quality review).
  Plans below name their waves in that vocabulary.
- **Branches.** Work happens on a branch per milestone (`m2-core`, ...)
  merged into `main` by the user. `main` may be red between M1 and the end
  of M2a; that is accepted.
- **Docs examples are tests.** Every code block in `docs/` that is a full
  module must keep parsing and, once M2b exists, keep compiling. Add a
  module test when you add an example.
