# M7: Data layer

`table` declarations with dialect column builders, SQL-shaped
`query { }` blocks type-checked against them with `^` pinned parameters,
diff-generated migrations, `schema` declarations for input validation,
and drivers for D1, Hyperdrive, and embedded SQLite. Drizzle's model,
built into the language.

## Starting state

- Parser: `table`, `schema`, `query { }` with clause-order checking and
  `^` pins, `sql` tagged templates as ordinary postfix templates.
- M2: tables and schemas are opaque types; queries are typed
  `Query[r]` with `r` fresh; pins are type-checked as expressions.
- M3: traits (`Driver`, dialect traits, `Json`). M4: `Task`, error rows,
  context (`use Db`).
- `docs/data.md` is the design.

## Exit criteria

- `pub table users { ... }` with builders from `@alder/sqlite`,
  `@alder/postgres`, or `@alder/mysql` yields `users.Row`,
  `users.Insert`, typed column references, and dialect-checked modifiers.
- `query { select { u.name, p.title } from users as u join posts as p on
... where ... }` infers the row type from the projection; unknown
  columns, ambiguous bare names, type mismatches in `where`, and misuse of
  `^` are compile errors; `insert`/`update`/`delete` type-check against
  `Insert`/`Row`.
- `db.run(q).await?` executes through the provided `Db` with bound
  parameters; no SQL text is ever built from pinned values.
- `alder db generate` diffs tables against the last snapshot and writes
  dialect SQL; `alder db migrate` applies; `alder db push` for dev;
  `alder deploy` applies pending D1/Hyperdrive migrations.
- `schema SignUp from users { pick email, name; name: min(3); ... }`
  yields a record type and a parser producing open `:tag` errors mapped
  to fields.

## Settled decisions

- Column builders are functions imported from dialect modules; the table
  body is `col: builder() modifier modifier(args)`.
- Queries are contextual-keyword blocks; bare identifiers are columns and
  aliases; host values are pinned with `^` and always become parameters.
- The block desugars to a chain API so packages can extend it.
- Migrations are diff-generated SQL files checked in (drizzle-kit model).
- Storage shape and input shape are separate; `schema` bridges with
  `pick`.

## Open decisions (recommendation in bold)

1. Relations for nested selects. **Out of scope for M7; flat joins only.
   A `relations` declaration comes later.**
2. Aggregates and grouping. **`count(*)`, `sum`, `avg`, `min`, `max` as
   dialect functions usable in projections with `groupBy`; typed as
   `Number`/`Option[Number]` per SQL nullability rules.**
3. Snapshot location. **`migrations/meta/` committed, like drizzle-kit,
   so CI can verify the diff is empty.**
4. Transactions. **`db.transaction(fn(tx) task)` where `tx` is a `Db`;
   nested transactions are savepoints where the dialect supports them.**
5. Quoting escape for SQL-word column names. **A column named `limit`,
   `set`, or `on` is written `` `limit` `` inside a query (backtick
   escape only in query mode).**
6. Query desugar target. **`Query[row]` value built from a small typed
   builder in `std/` (`Query.select(...)`), which the dialect driver
   renders to SQL; the compiler emits the builder calls, never SQL.**

## Work breakdown

### Wave 0: contract

Design panel producing `docs/data-internals.md`:

- Table declaration semantics: builder typing per dialect, modifier
  checking, generated types, primary/foreign keys, defaults, nullability.
- Query typing: name resolution for tables/aliases/columns, projection
  row inference, `where`/`on` expression typing (SQL operators over
  column types, `in` with pinned arrays, null handling), aggregates,
  ordering, limit/offset, insert/update/delete typing, `^` typing.
- Desugar to the builder; parameter binding order; dialect rendering
  rules (quoting, placeholders `?` vs `$1`).
- Migration engine: snapshot format, diff rules per dialect, SQL
  generation, apply/journal tables, `push`.
- `schema` semantics: `pick` from a table (nullability, lengths),
  rules (`min`, `max`, `matches`, `equals`, custom `fn(a) -> Result[()]`),
  generated record type and parser, error tags per field.
- Driver trait and the three drivers; `Db` context; embedded SQLite via
  a Rust-side extension in deno_core.

### Wave 1: front end (parallel)

- `alder-can` + `alder-constrain`: table and schema declarations,
  query name resolution and typing, aggregates, pins.
- `std/`: `@alder/sqlite`, `@alder/postgres`, `@alder/mysql` builder
  modules; `Query` builder; `Db`, `Driver` trait; `Schema` runtime.
- Error rendering for query errors (point at the column, suggest names).

### Wave 2: runtime and tooling (parallel)

- Dialect renderers and drivers (D1 binding, Hyperdrive/postgres via
  `fetch`-free TCP on standalone, embedded SQLite).
- `alder db generate/migrate/push/studio` (studio can be a stub that
  opens a generated page in M8).
- `alder deploy` migration step.
- e2e: a `standalone` app with SQLite running the docs' queries; the same
  app on miniflare with D1.

### Wave 3: sweep

- Docs, SPEC M7 ticked, changeset, critic pass over `docs/data.md`
  examples, fuzz the query type checker with permuted clauses.

## Tests to add (minimum)

- Tables: each builder per dialect, modifier rejection across dialects,
  generated `Row`/`Insert` snapshots.
- Queries: projection inference for joins with aliases, ambiguous bare
  column error, unknown column suggestion, `^` in every clause, `in ^ids`,
  aggregates with `groupBy`, insert with `values ^rows`, update/delete;
  rendered SQL snapshots per dialect with parameter order.
- Migrations: diff snapshots for add/drop/alter column, rename detection
  prompt, apply against embedded SQLite in `cargo test`.
- Schema: parser output for valid/invalid input with field-mapped tags;
  `pick` nullability derived correctly.

## Risks

- Query typing is a second type system (SQL's) embedded in the first;
  keep it a separate checker module invoked by the constraint generator
  rather than spreading SQL rules through the solver.
- Three dialects triple the surface; land SQLite fully first, then
  Postgres, then MySQL, each with its own owner.
- Embedded SQLite in deno_core needs a native extension; decide between
  `rusqlite` behind ops and a WASM SQLite early.
