use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{Interface, Module, QualifiedName, ResolvedImportKind};
use alder_constrain::{
    DiagnosticName, DiagnosticPackage, DiagnosticType, Error, ErrorKind, GenericRestriction,
};

/// Localize only at the reporting boundary. Inference keeps owned identities,
/// including identities reached through re-exported interfaces.
pub(super) fn localize(module: &Module<'_>, interfaces: &[Interface<'_>], error: &Error) -> Error {
    let mut error = error.clone();
    localize_types(module, interfaces, core_types(&mut error));
    error
}

/// Localize an entire comparison or obligation chain together so collisions
/// are resolved consistently in every part of one diagnostic.
pub(super) fn localize_types<'t>(
    module: &Module<'_>,
    interfaces: &[Interface<'_>],
    types: impl IntoIterator<Item = &'t mut DiagnosticType>,
) {
    let mut types = types.into_iter().collect::<Vec<_>>();
    let mut references = BTreeSet::new();
    let mut builtins = BTreeSet::new();
    for typ in &mut types {
        typ.visit_mut(&mut |typ| match typ {
            DiagnosticType::NamedReference(reference) => {
                references.insert(reference.clone());
            }
            DiagnosticType::Named(name) => {
                builtins.insert(name.clone());
            }
            _ => {}
        });
    }
    let mut names = references
        .into_iter()
        .map(|reference| {
            let name = preferred_name(module, interfaces, &reference)
                .unwrap_or_else(|| origin_name(&reference));
            (reference, name)
        })
        .collect::<BTreeMap<_, _>>();
    let mut counts = BTreeMap::new();
    for name in names.values().chain(&builtins) {
        *counts.entry(name.clone()).or_insert(0) += 1;
    }
    for (reference, name) in &mut names {
        if counts[name] > 1 {
            *name = origin_name(reference);
        }
    }
    for typ in types {
        typ.visit_mut(&mut |typ| {
            if let DiagnosticType::NamedReference(reference) = typ {
                *typ = DiagnosticType::Named(names[reference].clone());
            }
        });
    }
}

fn core_types(error: &mut Error) -> Vec<&mut DiagnosticType> {
    match &mut error.kind {
        ErrorKind::Mismatch { actual, expected }
        | ErrorKind::RecordFieldsMismatch { actual, expected } => vec![actual, expected],
        ErrorKind::AssocTypeMismatch {
            actual, expected, ..
        } => vec![actual, expected],
        ErrorKind::InvalidResultErrorType { actual: typ }
        | ErrorKind::MissingReturn { expected: typ } => vec![typ],
        ErrorKind::InfiniteType {
            equation: Some(equation),
        } => vec![&mut equation.0, &mut equation.1],
        ErrorKind::GenericSpecialization {
            restriction: GenericRestriction::Type(typ),
            ..
        } => vec![typ],
        _ => vec![],
    }
}

fn preferred_name(
    module: &Module<'_>,
    interfaces: &[Interface<'_>],
    name: &DiagnosticName,
) -> Option<String> {
    let home = DiagnosticName::from(QualifiedName {
        module: module.id,
        name: "",
    });
    if name.package == home.package && name.module == home.module {
        return Some(name.name.clone());
    }
    let mut candidates = BTreeSet::new();
    for import in module.imports {
        let Some(interface) = interfaces
            .iter()
            .find(|interface| interface.home == import.module)
        else {
            continue;
        };
        let exports = interface
            .types
            .iter()
            .map(|typ| (typ.exported_as, typ.reference))
            .chain(
                interface
                    .enums
                    .iter()
                    .map(|typ| (typ.exported_as, typ.reference)),
            )
            .chain(
                interface
                    .traits
                    .iter()
                    .map(|trait_| (trait_.exported_as, trait_.id.0)),
            );
        for (exported, reference) in exports {
            if DiagnosticName::from(reference) != *name {
                continue;
            }
            match import.kind {
                ResolvedImportKind::Names(imports) => {
                    for imported in imports
                        .iter()
                        .filter(|imported| imported.source.value == exported)
                    {
                        let binding = imported.binding.value;
                        candidates.insert((0, binding.len(), binding.to_owned()));
                    }
                }
                ResolvedImportKind::All => {
                    candidates.insert((1, exported.len(), exported.to_owned()));
                }
                ResolvedImportKind::Module { binding } => {
                    // Module bindings are lower-case and are not type paths.
                    // Describe provenance instead of inventing a type spelling.
                    let description = format!("{exported} (from {})", binding.value);
                    candidates.insert((2, description.len(), description));
                }
            }
        }
    }
    candidates.into_iter().next().map(|(_, _, name)| name)
}

fn origin_name(reference: &DiagnosticName) -> String {
    let path = reference.module.join("/");
    let origin = match &reference.package {
        DiagnosticPackage::Application => format!("~/{path}"),
        DiagnosticPackage::ApplicationMember(member) => format!("workspace {member}/{path}"),
        DiagnosticPackage::Named { author, project } if path.is_empty() => {
            format!("@{author}/{project}")
        }
        DiagnosticPackage::Named { author, project } => format!("@{author}/{project}/{path}"),
        DiagnosticPackage::Builtin => format!("standard library/{path}"),
    };
    format!("{} (from {origin})", reference.name)
}
