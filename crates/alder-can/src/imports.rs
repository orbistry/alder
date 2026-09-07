//! Shared source-to-canonical import identity resolution.

use alder_ast::{
    ModuleId, PackageId, PackageName, ResolvedImport, ResolvedImportKind, ResolvedImportName,
    Visibility,
};
use bumpalo::Bump;

/// Resolve only the declared root. Availability and public-name checks belong
/// to interface loading; this operation never consults files or a registry.
pub fn resolve_imports<'a>(
    bump: &'a Bump,
    module: &alder_source::Module<'a>,
    home_package: PackageId<'a>,
) -> &'a [ResolvedImport<'a>] {
    bump.alloc_slice_fill_iter(
        module
            .import_entries()
            .map(|(visibility, region, import)| {
                let path = import.path.value;
                let (package, root_name) = match path.root {
                    alder_source::ModuleRoot::StandardLibrary => (PackageId::Builtin, None),
                    alder_source::ModuleRoot::Local(_) => (home_package, None),
                    alder_source::ModuleRoot::Package { author, package } => (
                        PackageId::Named(PackageName {
                            author: author.value,
                            project: package.value,
                        }),
                        Some(package),
                    ),
                };
                let module = ModuleId {
                    package,
                    path: bump
                        .alloc_slice_fill_iter(path.segments.iter().map(|segment| segment.value)),
                };
                let kind = match import.tail {
                    alder_source::ImportTail::Module => ResolvedImportKind::Module {
                        binding: path
                            .segments
                            .last()
                            .copied()
                            .or(root_name)
                            .expect("the parser rejects imports with no bindable segment"),
                    },
                    alder_source::ImportTail::Alias(binding) => {
                        ResolvedImportKind::Module { binding }
                    }
                    alder_source::ImportTail::Names(names) => ResolvedImportKind::Names(
                        bump.alloc_slice_fill_iter(names.iter().map(|name| ResolvedImportName {
                            source: name.name,
                            binding: name.alias.unwrap_or(name.name),
                        })),
                    ),
                    alder_source::ImportTail::All(_) => ResolvedImportKind::All,
                };
                ResolvedImport {
                    module,
                    region,
                    visibility: match visibility {
                        alder_source::Visibility::Private => Visibility::Private,
                        alder_source::Visibility::Pub(region) => Visibility::Public(region),
                    },
                    kind,
                }
            })
            .collect::<Vec<_>>(),
    )
}
