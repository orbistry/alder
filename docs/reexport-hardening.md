# Public re-export hardening

Status: named/wildcard value publication fixed; wider boundary audit remains open.

The driver accepted a facade containing `pub import ~/leaf.*` or
`pub import ~/leaf.{ answer }`, but a consumer calling `facade.answer()` failed
with unknown name. The facade's published interface contained no values.
Permanent driver regressions reproduced both failures before the fix.

The interface builders now receive the same dependency interfaces used during
canonicalization. Public named and wildcard imports copy selected public entries
for values, types, enums, traits, and module namespaces. Each entry keeps its
defining identity and checked scheme/body, changing only its exported name when
aliased. Each name selection is processed independently, so one source name can
be exported under multiple aliases. Private entries and dependency instance
definitions are not republished as facade-owned definitions.

`alder_can::from_module` and `headers_from_module` now require the dependency
interface slice. The driver supplies it for preliminary headers and final
publication; isolated callers use an empty slice only when there are no imports.
No owned-interface wire-format fields were added. Canonical value references
retain the original owner, so Alder codegen calls the defining module directly
rather than adding a facade wrapper with a second dictionary convention.

Coverage at this checkpoint:

- Named, wildcard, and repeated-source alias publication through a three-module
  driver build, including identity/scheme equality in the owned interfaces.
- Actual CLI execution of a generic Show/Eq function through a wildcard facade.
- Actual CLI execution across a named-renaming module and a wildcard facade for
  an enum, generic record alias, and trait. Both a generic trait-bound function
  and a qualified call through the renamed trait use the original instance.
- A package-root interface containing a generic alias/function is serialized
  with bincode, deserialized, and consumed without supplying the defining leaf
  interface or its arena. The consumer checks successfully.
- Explicitly re-exporting a private value fails at the imported name, with a
  reviewed colorless diagnostic; the failed facade publishes no interface or
  executable artifact.

Remaining required audit: broader alias/chain combinations and package runtime
execution, collision/wildcard-privacy diagnostics, mutable binding identity,
dictionary instance ownership, cyclic imports, and
emitted-module initialization/export behavior. The shared copying paths do not
by themselves prove those contracts. Do not mark the whole re-export audit done.
Source syntax currently requires public imports to have a names or wildcard
tail, so direct public module-namespace imports are rejected by the parser;
do not infer source-level support merely from the canonical Module import arm.
