# Module identity and source roots

Project discovery supplies two maps keyed by source URI: the owning package and
the module path relative to that package's source directory. Removing `.ald`
and a final `mod` segment produces the canonical module path. The path of a
root `mod.ald` is empty; the package name is an import binding, not a path segment.

CLI check/build pass this metadata both to graph construction and compilation.
Local imports retain the importing module's package; package imports select the
named package. Bare imports select only compiler-shipped `Builtin` modules,
with lowercase path segments; they never consult package sources or a registry.
`alder-can::resolve_imports` supplies canonical identities for individual and
grouped entries. Graph edges use exact package/path lookup, not URI suffixes.
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
the workspace root use a relative path with parent components when they share
a filesystem root, preserving identity when the workspace and its siblings move
together. Different filesystem prefixes (such as different Windows drives)
retain an absolute key input because no relative path exists. When source roots nest, the most specific
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

Interface caches separate package identity kinds into `application/`, `builtin/`,
`members/<key>/`, and `packages/<author>/<project>/` namespaces before appending
the module path. Package indexes use the same kind separation. A source module
named `builtin/value` therefore cannot overwrite builtin `value`, and a named
package `members/<key>` cannot overwrite a workspace member's index. Cache APIs
take owned canonical identities rather than unqualified dotted module names.
There is no fallback reader for the previous colliding layout.

Interface-only dependencies validate the package index and every listed module
interface before use. In addition to each file's version/fingerprint and owner
checks, indexed implementation headers must match the module's declarations,
independent of header ordering. A stale index cannot add a deleted implementation
or omit a current one. The error identifies the dependency package and module;
it does not fabricate an application-source label for inconsistent cache files.
Source-backed dependencies continue to rebuild from current sources without
loading these saved headers.

Every source build requires both `BuildDependencies::module_paths` and
`module_packages` entries for each source URI. `Project::build_dependencies`
supplies them for filesystem projects; embedders supply the identities of their
own sources, including in-memory URLs. Graph construction and compilation use
the same validated maps. Missing metadata is rejected deterministically before
interfaces or code are produced, without fabricated source labels. There are
no metadata-free graph/build entry points or URI-based identity guesses.

Public namespace imports preserve module identities in interface format 8;
selective and wildcard re-exports preserve declaration identities. Hydrated
interfaces retain namespace chains, and private names never enter their public
lookup tables. There is no reader for earlier interface formats.

Code generation sorts and deduplicates direct module initialization imports by
canonical ESM module ID. Dependencies initialize before importers, and shared
dependencies initialize once. Independent sibling order is canonical rather
than source-declaration order, so formatter sorting cannot alter execution.
The graph and bundler also retain deterministic identity-based traversal.
