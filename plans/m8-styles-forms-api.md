# M8: Styles, forms, API

Typed `style` blocks compiled to atomic CSS, `Form`/`Field` components
typed from `schema`, typed clients generated from `+server.ald` with
`.d.ts` emission for TypeScript consumers, and the hono-like router
builder for API-only packages.

## Starting state

- Parser: `style { }` with dimensions and nested blocks; markup and
  components (M6); `schema` (M7).
- M6: components, SSR, routing, remote functions. M7: schemas with
  parsers and field-mapped errors.
- `docs/web.md` Styles, Forms, API sections; `docs/tooling.md`.

## Exit criteria

- `let card = style { padding: 16px, color: theme.text, ":hover": {...},
"@media (...)": {...} }` type-checks property names and value types,
  compiles to atomic classes with deterministic merging, and ships a
  single CSS asset per build with SSR-critical extraction.
- A `theme` declaration defines tokens with types; `theme.text` is
  checked.
- `<Form action={signUp}>` with `<Field name="email" />` is typed from
  the action's `schema` argument: unknown field names and wrong input
  types are compile errors; validation errors render per field; the
  action runs as a remote function or a `+page.server.ald` action.
- `+server.ald` handlers with typed bodies generate an Alder client module
  (`Api.users.get`) and a `.d.ts` for TypeScript; `alder build` emits
  `.d.ts` for every `pub` item of a package when asked.
- `Router.new().get("/users/:id", handler)` types path params from the
  string literal and shares handler and middleware types with file routes.

## Settled decisions

- StyleX model: atomic CSS, typed properties, last style wins per
  property.
- Forms are typed from `schema`; storage and input shapes stay separate.
- Both file routes and a router builder, sharing types.
- Alder emits `.d.ts` for its `pub` items (two-way interop).

## Open decisions (recommendation in bold)

1. CSS property schema source. **Generate from `@webref/css` data into a
   Rust table; units and keywords typed per property; `--custom`
   properties typed as `String`.**
2. Dynamic values. **Static values compile to atomic classes; a value
   that reads a signal compiles to a CSS variable set inline on the
   element, so the class set is stable.**
3. Keyframes and global styles. **`keyframes name { from: {...}, to: {...} }`
   as an expression form producing a typed animation name; a `global`
   style block escape hatch for resets.**
4. Middleware model. **`fn(Event, Next) Task[Response]` composed with
   `Router.use`; the same signature as `hooks.server.ald` `handle`.**
5. OpenAPI. **Generated from `+server.ald` and router types on `alder
build --openapi`; not required for the milestone's exit.**
6. `.d.ts` mapping. **Records to interfaces, enums to discriminated unions
   over the M2 representation, `Option` to `T | null`, `Result` to a
   tagged union, `Task` to `Promise`, functions to functions; traits are
   not exported.**

## Work breakdown

### Wave 0: contract

Design panel producing `docs/styles-forms-api-internals.md`: style
checking and atomic compilation, theme tokens, CSS asset pipeline and SSR
extraction, form typing and error mapping, client generation and `.d.ts`
mapping rules, router builder types and path-param parsing at the type
level, middleware.

### Wave 1 (parallel)

- Style checker and compiler (`alder-can`, `alder-codegen`), CSS asset
  emission in `alder build`, `theme`.
- `Form`/`Field`/`Errors` components in `std/` with schema-driven typing
  in the checker.
- Router builder in `std/` with path-param typing (a compiler-known
  literal parser or a type-level trick; the contract decides).
- `.d.ts` emitter and typed client generator in `alder-codegen`/driver.

### Wave 2: sweep

- e2e: the docs' sign-up form end to end; an API-only package consumed
  from a TypeScript test via the emitted `.d.ts` (tsc in CI).
- Docs, SPEC M8 ticked, changeset, critic pass.

## Tests to add (minimum)

- Styles: unknown property, wrong unit, nested pseudo/media, merge order,
  dynamic value to CSS variable, SSR-critical extraction snapshot.
- Forms: unknown field error, type mismatch, validation error rendering,
  action wiring to remote and to `+page.server.ald`.
- API: client module snapshot, `.d.ts` snapshot per type shape, router
  path-param typing success and failure.

## Risks

- Property schemas are large; generate, do not hand-write.
- `.d.ts` emission exposes the runtime representation; it is a public
  contract once shipped, so it must match the M2 decisions exactly.
