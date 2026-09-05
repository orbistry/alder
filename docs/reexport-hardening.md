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
- Wildcard publication has exact interface assertions for public values, aliases,
  enums, and traits. A consumer supplied only the facade interface can use the
  public declarations but cannot import private functions, aliases, enums,
  traits, or methods. Private diagnostic names and dependency instances are not
  copied into the facade. Conflicting wildcard value exports reject facade
  publication with a reviewed diagnostic labeling both source imports.
- Shared array exports preserve reference identity through repeated aliases and
  a named/wildcard chain. Mutations through each route affect the original.
- Renamed trait methods retain their method identity but bind under the selected
  local name. Previously import binding ignored `as`, making distinct aliases
  collide under the original name and missing actual local-name collisions.
  CLI execution covers two aliases through a named/wildcard chain and another
  consumer-side rename. Negative driver tests verify that the original name is
  not introduced and that a real alias collision rejects interface/artifact
  publication; colorless snapshots label the relevant source names.
- Facade initialization is retained: codegen emits explicit imports as bare
  AST import declarations in source order, before generated value imports, and
  includes them in dependency metadata even without local value uses. Previously
  owner-directed references bypassed facades entirely, dropping their top-level
  effects. The CLI regression failed before the fix; it now checks ordered,
  exactly-once mutations from both intermediate modules despite multiple routes
  to their shared dependency. Two unused sibling imports deliberately reverse
  filename order and verify source-ordered effects rather than sorted graph
  order. Driver tests check the retained metadata too.

Remaining required audit: broader alias/chain combinations and package runtime
execution, cross-namespace collisions and broader privacy cases, mutable binding and
dictionary instance ownership, cyclic imports, and more complex
emitted-module initialization/export behavior. The shared copying paths do not
by themselves prove those contracts. Do not mark the whole re-export audit done.
Source syntax currently requires public imports to have a names or wildcard
tail, so direct public module-namespace imports are rejected by the parser;
do not infer source-level support merely from the canonical Module import arm.
