# Module identity and source roots

Project discovery supplies two maps keyed by source URI: the owning package and
the module path relative to that package's source directory. Removing `.ald`
and a final `mod` segment produces the canonical module path. The path of a
root `mod.ald` is empty; the package name is an import binding, not a path segment.

CLI check/build pass this metadata both to graph construction and compilation.
Local imports retain the importing module's package; package imports select the
named package. Graph edges use exact package/path lookup, not URI suffixes.
Consequently a `src` directory in a checkout ancestor or within a module path
cannot redefine the project's source root.

Workspace member roots are canonicalized before constructing source directories
or identities, then deduplicated and sorted by canonical path. Repeated glob
matches, `..` spellings, direct config-file matches, and symlink aliases of the
same member therefore discover one source tree. Canonicalization failures retain
the underlying filesystem error and offending member path.

Distinct canonical workspace roots cannot declare the same named package,
even if their module paths do not overlap. Loading rejects the conflict before
module discovery and identifies both roots in canonical path order. This avoids
silently merging separate source trees into one package and interface index;
repeated paths to the same physical package remain valid.

Applications checked together from a workspace use `ApplicationMember` identities
instead of sharing `Application`. The opaque member key is `w` followed by the
SHA-256 digest of the slash-separated workspace-relative member path. It is
stable under member reordering and workspace relocation, distinguishes equal
directory basenames, and occupies one fixed-length URL/cache path segment.
Standalone application builds retain the `Application` identity. Members outside
the workspace root currently use their full path as the key input; relocatability
for those remains an audit item. When source roots nest, the most specific
containing source root owns the file. Package identity and module paths use the
same ownership lookup; member ordering cannot move an inner member's files into
an enclosing package.

Before hydrating interfaces or discovering headers, compilation groups source
URIs by canonical identity. Multiple distinct URIs for one identity stop the
build. A shared source-aware diagnostic shows the conflicting files, ordered
by URI. No interface or JavaScript artifacts are returned for that build.
Ambiguous identities are not assigned arbitrary graph edges.

Emitted modules retain an optional physical Alder source path alongside the
owned Oxc AST. The driver supplies it from the source URI; generated entry
modules have no physical origin. The bundler preserves this map independently
of its consumable AST map and delegates foreign resolution to Rolldown using
the physical importer. A relative extern therefore resolves beside its Alder
declaration, including nested modules; a wrapper's own imports use ordinary
JavaScript resolution. Compiler-generated JavaScript is never serialized for
this handoff. The virtual ID registry also remains available after an AST has
been transferred into Rolldown.

The driver also retains source text and extern declaration regions in emitted
module metadata. For direct extern resolution failures the bundler constructs
an `alder-report` diagnostic with the relevant declaration labels, keeps the
underlying resolver explanation, and returns that structured report to the CLI.
Reports are sorted by source name and message before grouping; the CLI does not
flatten them to strings. Colorless rendering is selected only by tests.

The low-level source-only driver API retains a compatibility path inference
when project metadata is absent. It is not appropriate for ambiguous source
roots; project-aware callers should supply `BuildDependencies::module_paths`
and `module_packages` and use `build_graph_with_dependencies`.

Remaining audits are tracked in `plans/compiler-hardening.md`, including
external workspace members, cache consistency, and
public re-exports. Deterministic graph traversal alone does not establish
determinism of every downstream artifact or initialization order.
