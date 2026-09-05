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

Remaining required audit: type/enum/trait/module aliases, chains, package-root
consumers, private-name/collision diagnostics, interface round-trips in isolation,
mutable binding identity, dictionary instance ownership, cyclic imports, and
emitted-module initialization/export behavior. The shared copying paths do not
by themselves prove those contracts. Do not mark the whole re-export audit done.
