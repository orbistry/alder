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

Before hydrating interfaces or discovering headers, compilation groups source
URIs by canonical identity. Multiple distinct URIs for one identity stop the
build. A shared source-aware diagnostic shows the conflicting files, ordered
by URI. No interface or JavaScript artifacts are returned for that build.
Ambiguous identities are not assigned arbitrary graph edges.

The low-level source-only driver API retains a compatibility path inference
when project metadata is absent. It is not appropriate for ambiguous source
roots; project-aware callers should supply `BuildDependencies::module_paths`
and `module_packages` and use `build_graph_with_dependencies`.

Remaining audits are tracked in `plans/compiler-hardening.md`, including
workspace application identities, overlapping roots, cache consistency, and
public re-exports. Deterministic graph traversal alone does not establish
determinism of every downstream artifact or initialization order.
