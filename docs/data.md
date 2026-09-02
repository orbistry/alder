# Alder Data Layer

**Status: current direction, everything provisional.**

A Drizzle-like layer built into the language: `table` declarations,
SQL-shaped query expressions checked against them, diff-generated
migrations, and `schema` declarations for input validation. MySQL,
PostgreSQL, and SQLite (including D1) are the initial dialects.

## Tables

`table` is a declaration form. Column builders come from a dialect
module, so the schema respects the target database exactly as Drizzle
does.

```alder
import @alder/sqlite.{ text, integer, timestamp, primaryKey }

pub table users {
    id: integer() primaryKey autoIncrement
    email: text() notNull unique
    name: text() notNull
    created: timestamp() notNull default(now)
}

pub table posts {
    id: integer() primaryKey autoIncrement
    author: integer() notNull references(users.id)
    title: text() notNull
    body: text()
}
```

- Each table yields a row type (`users.Row`), an insert type
  (`users.Insert`, honoring defaults and nullability), and typed column
  references for queries.
- Column modifiers are dialect-checked: a Postgres-only modifier in an
  `@alder/sqlite` table is a compile error.
- **Open:** indexes and composite keys syntax, relations declaration for
  nested selects, and whether column modifiers are attributes or the
  bare-word form shown here.

## Queries

Queries are SQL-shaped expressions inside a `query { ... }` block,
type-checked against table declarations. The result type is inferred from
the projection. SQL words are contextual keywords only inside `query`, so
`update`, `select`, and `from` remain ordinary identifiers elsewhere.

Inside the block, bare identifiers are columns and table aliases. Runtime
values from the surrounding code are pinned with `^`, as in Ecto:

```alder
let recent = query {
    select { u.name, p.title, p.created }
    from users as u
    join posts as p on p.author == u.id
    where u.active && p.created > ^since && u.id in ^ids
    orderBy p.created desc
    limit ^pageSize
}

let rows = db.run(recent).await?      // Array[{ name: String, title: String, created: Timestamp }]

db.run(query { insert into users values ^{ email, name } }).await?
db.run(query { update users set { name: ^newName } where users.id == ^user.id }).await?
db.run(query { delete from posts where posts.author == ^user.id }).await?
```

- `^` binds looser than `.`, calls and indexing but tighter than every
  binary operator: `^user.id` pins `user.id`, `^f(x)` pins the call, and
  `^(a + b)` pins an arbitrary expression. A pinned value is always a
  bound parameter, never SQL text, so injection is impossible by
  construction. Arrays pin as parameter lists for `in`.
- Bare identifiers resolve against the tables and aliases in scope;
  ambiguity between two tables is a compile error and `unknown column
since` suggests `^since`.
- `values` takes a pinned record or array of records
  (`values ^rows`); `set` takes a record whose fields are columns and
  whose values are query expressions.
- A `query` block produces a `Query[row]` value; `db.run` executes it
  against the provided `Db` context.
- The block desugars to a chain API (`Query.select(...).from(...)`) so
  packages can extend and compose queries programmatically.
- Escape hatch: `` sql`select ... ${id}` `` tagged template, checked
  against the schema where possible.
- **Open:** aggregates and grouping syntax, subqueries, transactions API,
  and how dialect-specific functions are exposed.

## Migrations

Diff-generated SQL files checked into the repository (drizzle-kit model).

```
alder db generate     # diff tables against the last snapshot, write SQL per dialect
alder db migrate      # apply pending migrations
alder db push         # dev only: make the database match the schema
alder db studio       # open: browse data
```

- Generated SQL is editable before commit.
- `alder deploy` applies pending migrations for D1/Hyperdrive as part of
  deployment.
- Snapshots live under `.alder/` or a committed `migrations/meta/`.
  **Open:** which.

## Validation schemas

Input validation is separate from storage. See `web.md` for the `schema`
declaration; it can `pick` columns from a table to derive nullability and
lengths, then add rules (`min`, `equals`, custom functions returning
`Result`) that never touch the database.

## Drivers

- SQLite: D1 on Cloudflare, embedded SQLite under deno_core for
  `standalone`.
- PostgreSQL and MySQL: through Hyperdrive on Cloudflare, direct drivers
  elsewhere.
- Drivers are `extern` wrappers in first-party packages implementing a
  `Driver` trait; `Db` is provided via context.
