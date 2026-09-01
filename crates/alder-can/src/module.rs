use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{
    Alias as CanAlias, Binop as CanBinop, Ctor as CanCtor, CtorOpts, Decls, Export, Exports,
    Module as CanModule, ModuleName, PackageName, Union as CanUnion,
};
use alder_region::{Located, Region};
use alder_source::{
    Alias as SourceAlias, Ctor as SourceCtor, Exposed, Exposing, Infix, Module as SourceModule,
    Privacy, Type as SourceType, Union as SourceUnion, Value as SourceValue,
};
use bumpalo::Bump;

use crate::accumulate;
use crate::environment::{self, Env, dups};
use crate::error::{DuplicatePatternContext, VarKind};
use crate::expression;
use crate::pattern;
use crate::scc;
use crate::types;
use crate::warning::{Warning, WarningContext};
use crate::{Error, Interface};

#[derive(Clone, Copy, Debug, Default)]
pub struct Context<'a> {
    pub package: Option<PackageName<'a>>,
    pub interfaces: Option<&'a BTreeMap<&'a str, Interface<'a>>>,
}

#[derive(Debug)]
pub struct CanResult<'a> {
    pub module: CanModule<'a>,
    pub warnings: Vec<Warning<'a>>,
}

fn canonicalize_header<'a>(
    context: Context<'a>,
    module: &SourceModule<'a>,
) -> Result<ModuleName<'a>, Error<'a>> {
    let name = module.name.ok_or(Error::MissingModuleHeader)?;

    Ok(ModuleName {
        package: context.package,
        name: name.value,
    })
}

pub fn canonicalize<'a>(
    bump: &'a Bump,
    context: Context<'a>,
    module: &SourceModule<'a>,
) -> Result<CanResult<'a>, Vec<Error<'a>>> {
    let home = canonicalize_header(context, module).map_err(|e| vec![e])?;

    let mut env =
        environment::foreign::create_initial_env(bump, home, context.interfaces, module.imports)?;

    // Phase order mirrors Elm's `Local.add`: addTypes (type dups, union
    // free-var checks, alias SCC + canonicalization), then addVars, then
    // addCtors (which canonicalizes constructor argument types).
    environment::local::add_union_types(&mut env, module.unions, module.aliases)?;
    for union in module.unions {
        check_union_free_vars(bump, union)?;
    }
    let aliases = canonicalize_aliases(bump, &mut env, module.aliases)?;
    environment::local::add_vars(&mut env, module.values)?;
    let unions = canonicalize_unions(bump, &env, module.unions)?;
    environment::local::add_ctors(bump, &mut env, module.unions, unions, aliases)?;
    environment::local::check_binops(&env, module.binops)?;

    let mut warnings = Vec::new();
    let decls = canonicalize_decls(bump, &env, module.values, &mut warnings)?;
    let binops = canonicalize_binops(bump, module.binops);
    let exports = canonicalize_exports(bump, module)?;

    let can_module = CanModule {
        name: env.home,
        exports,
        docs: module.docs,
        decls,
        unions,
        aliases,
        binops,
    };

    let used_modules = collect_used_modules(&can_module);
    for import in module.imports {
        let module_name = import.import.value;
        if !used_modules.contains(module_name) {
            warnings.push(Warning::UnusedImport {
                region: import.import.region,
                module_name,
            });
        }
    }

    Ok(CanResult {
        module: can_module,
        warnings,
    })
}

fn canonicalize_decls<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    values: &'a [&'a Located<SourceValue<'a>>],
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a Decls<'a>, Vec<Error<'a>>> {
    if values.is_empty() {
        return Ok(bump.alloc(Decls::Empty));
    }

    let mut errors = Vec::new();
    let mut nodes: Vec<NodeOne<'a>> = Vec::with_capacity(values.len());
    for value in values {
        match to_node_one(bump, env, value, warnings) {
            Ok(node) => nodes.push(node),
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let top_level_names: BTreeSet<&str> = nodes.iter().map(|n| n.name).collect();

    // Phase 1: SCC on ALL dependencies
    let scc_nodes: Vec<scc::Node<'_, NodeOne<'a>>> = nodes
        .into_iter()
        .map(|node| {
            let deps: Vec<&str> = node
                .free_locals
                .keys()
                .filter(|k| top_level_names.contains(*k))
                .copied()
                .collect();
            scc::Node {
                key: node.name,
                value: node,
                deps,
            }
        })
        .collect();
    let phase1_sccs = scc::strongly_connected_components(scc_nodes);

    let mut decls: &'a Decls<'a> = bump.alloc(Decls::Empty);
    for scc_group in phase1_sccs.into_iter().rev() {
        match scc_group {
            scc::Scc::Acyclic(node) => {
                decls = bump.alloc(Decls::Declare {
                    definition: node.def,
                    next: decls,
                });
            }
            scc::Scc::Cyclic(group) => {
                // Phase 2: SCC on DIRECT deps within the cyclic group,
                // preserving the group's own node order like Elm's
                // `Graph.stronglyConnComp subNodes`.
                let group_names: BTreeSet<&str> = group.iter().map(|n| n.name).collect();

                let phase2_nodes: Vec<scc::Node<'_, &NodeOne<'a>>> = group
                    .iter()
                    .map(|node| {
                        let deps = if node.has_args {
                            vec![] // functions: body is delayed
                        } else {
                            node.free_locals
                                .iter()
                                .filter(|(k, uses)| group_names.contains(*k) && uses.direct > 0)
                                .map(|(k, _)| *k)
                                .collect()
                        };
                        scc::Node {
                            key: node.name,
                            value: node,
                            deps,
                        }
                    })
                    .collect();

                let phase2_sccs = scc::strongly_connected_components(phase2_nodes);

                // Elm's `traverse detectBadCycles` accumulates every bad
                // cycle in this group before giving up.
                let mut rec_defs: Vec<&'a alder_ast::Def<'a>> = Vec::new();
                let mut cycle_errors: Vec<Error<'a>> = Vec::new();
                for sub_scc in phase2_sccs {
                    match sub_scc {
                        scc::Scc::Acyclic(node) => {
                            rec_defs.push(node.def);
                        }
                        scc::Scc::Cyclic(bad_nodes) => {
                            let def_name = match bad_nodes[0].def {
                                alder_ast::Def::Def { name, .. }
                                | alder_ast::Def::TypedDef { name, .. } => name,
                            };
                            cycle_errors.push(Error::RecursiveDecl {
                                name: def_name,
                                others: bump
                                    .alloc_slice_fill_iter(bad_nodes[1..].iter().map(|n| n.name)),
                            });
                        }
                    }
                }
                if !cycle_errors.is_empty() {
                    return Err(cycle_errors);
                }

                if let Some((first, rest)) = rec_defs.split_first() {
                    decls = bump.alloc(Decls::DeclareRec {
                        definition: first,
                        following: bump.alloc_slice_fill_iter(rest.iter().copied()),
                        next: decls,
                    });
                }
            }
        }
    }
    Ok(decls)
}

struct NodeOne<'a> {
    def: &'a alder_ast::Def<'a>,
    name: &'a str,
    has_args: bool,
    free_locals: expression::FreeLocals<'a>,
}

enum TopLevelDefBuilder<'a> {
    Typed {
        free_vars: alder_ast::FreeVars<'a>,
        args: &'a [alder_ast::TypedPattern<'a>],
        typ: &'a Located<alder_ast::Type<'a>>,
    },
    Untyped {
        args: &'a [&'a Located<alder_ast::Pattern<'a>>],
    },
}

fn to_node_one<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    value: &'a Located<SourceValue<'a>>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<NodeOne<'a>, Vec<Error<'a>>> {
    let src = &value.value;

    // Mirrors Elm's `toNodeOne`: typed definitions resolve the annotation
    // and match it against the arguments before the body is touched, and
    // one duplicate scope spans all arguments either way.
    let (builder, arg_bindings) = if let Some(ann) = src.annotation {
        let annotation = types::to_annotation(bump, env, ann)?;
        let mut bound: Vec<(&'a str, Region)> = Vec::new();
        let (typed_args, result_type) = expression::gather_typed_args(
            bump,
            env,
            src.name.value,
            src.arguments,
            annotation.typ,
            &mut bound,
        )?;
        let arg_bindings =
            pattern::detect_duplicates(DuplicatePatternContext::FuncArgs(src.name.value), bound)?;
        (
            TopLevelDefBuilder::Typed {
                free_vars: annotation.free_vars,
                args: bump.alloc_slice_fill_iter(typed_args),
                typ: result_type,
            },
            arg_bindings,
        )
    } else {
        let (can_args, arg_bindings) = pattern::verify_all(
            bump,
            env,
            DuplicatePatternContext::FuncArgs(src.name.value),
            src.arguments,
        )?;
        (
            TopLevelDefBuilder::Untyped {
                args: bump.alloc_slice_fill_iter(can_args),
            },
            arg_bindings,
        )
    };

    let body_env = env.add_locals(&arg_bindings)?;
    let mut free_locals = expression::FreeLocals::new();
    let can_body =
        expression::canonicalize_expr(bump, &body_env, src.body, &mut free_locals, warnings)?;

    let outer_free = expression::verify_bindings(
        WarningContext::Pattern,
        &arg_bindings,
        free_locals,
        warnings,
    );

    let def = match builder {
        TopLevelDefBuilder::Typed {
            free_vars,
            args,
            typ,
        } => bump.alloc(alder_ast::Def::TypedDef {
            name: src.name,
            free_vars,
            args,
            body: can_body,
            typ,
        }),
        TopLevelDefBuilder::Untyped { args } => bump.alloc(alder_ast::Def::Def {
            name: src.name,
            args,
            body: can_body,
        }),
    };

    Ok(NodeOne {
        def,
        name: src.name.value,
        has_args: !src.arguments.is_empty(),
        free_locals: outer_free,
    })
}

fn canonicalize_unions<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    unions: &'a [&'a Located<SourceUnion<'a>>],
) -> Result<&'a [&'a Located<CanUnion<'a>>], Vec<Error<'a>>> {
    accumulate::try_all_alloc_ref(
        bump,
        unions.iter().copied().map(|union| {
            let can = canonicalize_union(bump, env, union)?;
            Ok(&*bump.alloc(Located::at(union.region, can)))
        }),
    )
}

fn canonicalize_union<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_union: &'a Located<SourceUnion<'a>>,
) -> Result<CanUnion<'a>, Vec<Error<'a>>> {
    let union = &source_union.value;
    let parameters =
        bump.alloc_slice_fill_iter(union.arguments.iter().copied().map(|arg| arg.value));
    let ctors = canonicalize_ctors(bump, env, union.ctors)?;
    let alternatives = union
        .ctors
        .len()
        .try_into()
        .expect("union alternatives exceed u16");
    let options = if union.ctors.len() == 1 && union.ctors[0].arguments.len() == 1 {
        CtorOpts::Unbox
    } else if union.ctors.iter().all(|ctor| ctor.arguments.is_empty()) {
        CtorOpts::Enum
    } else {
        CtorOpts::Normal
    };

    Ok(CanUnion {
        name: union.name,
        parameters,
        ctors,
        alternatives,
        options,
    })
}

fn canonicalize_ctors<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    ctors: &'a [&'a SourceCtor<'a>],
) -> Result<&'a [&'a CanCtor<'a>], Vec<Error<'a>>> {
    accumulate::try_all_alloc_ref(
        bump,
        ctors.iter().copied().enumerate().map(|(index, ctor)| {
            let arguments = types::canonicalize_type_arguments(bump, env, ctor.arguments)?;
            Ok(&*bump.alloc(CanCtor {
                name: ctor.name.value,
                index: index.try_into().expect("constructor index exceeds u16"),
                arity: ctor
                    .arguments
                    .len()
                    .try_into()
                    .expect("constructor arity exceeds u16"),
                arguments,
            }))
        }),
    )
}

fn canonicalize_aliases<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source_aliases: &'a [&'a Located<SourceAlias<'a>>],
) -> Result<&'a [&'a Located<CanAlias<'a>>], Vec<Error<'a>>> {
    let alias_names: BTreeSet<&str> = source_aliases.iter().map(|a| a.value.name.value).collect();

    let scc_nodes: Vec<scc::Node<'_, &'a Located<SourceAlias<'a>>>> = source_aliases
        .iter()
        .map(|&alias| {
            let mut deps = Vec::new();
            collect_type_edges(&alias.value.typ.value, &alias_names, &mut deps);
            deps.reverse();
            scc::Node {
                key: alias.value.name.value,
                value: alias,
                deps,
            }
        })
        .collect();
    let sccs = scc::strongly_connected_components(scc_nodes);

    let mut results: BTreeMap<&str, &Located<CanAlias>> = BTreeMap::new();
    for component in sccs {
        match component {
            scc::Scc::Acyclic(source) => {
                check_alias_free_vars(bump, source)?;
                let alias = canonicalize_single_alias(bump, env, source)?;
                environment::local::add_alias_type(env, &alias.value);
                results.insert(source.value.name.value, alias);
            }
            scc::Scc::Cyclic(cycle) => {
                // Elm checks the head alias's type variables before
                // reporting the cycle, so a messed-up cyclic alias gets
                // the variable error first.
                let first = &cycle[0];
                check_alias_free_vars(bump, first)?;
                return Err(vec![Error::RecursiveAlias {
                    region: first.value.name.region,
                    name: first.value.name.value,
                    args: bump.alloc_slice_fill_iter(first.value.arguments.iter().map(|a| a.value)),
                    typ: first.value.typ,
                    others: bump
                        .alloc_slice_fill_iter(cycle[1..].iter().map(|a| a.value.name.value)),
                }]);
            }
        }
    }

    Ok(bump.alloc_slice_fill_iter(source_aliases.iter().map(|a| {
        *results
            .get(a.value.name.value)
            .expect("all acyclic aliases inserted into results")
    })))
}

fn canonicalize_single_alias<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source_alias: &'a Located<SourceAlias<'a>>,
) -> Result<&'a Located<CanAlias<'a>>, Vec<Error<'a>>> {
    let alias = &source_alias.value;
    let parameters =
        bump.alloc_slice_fill_iter(alias.arguments.iter().copied().map(|arg| arg.value));
    let typ = types::canonicalize_type(bump, env, alias.typ)?;

    let can_alias = CanAlias {
        name: alias.name,
        parameters,
        typ,
    };

    Ok(&*bump.alloc(Located::at(source_alias.region, can_alias)))
}

fn check_union_free_vars<'a>(
    bump: &'a Bump,
    union: &'a Located<SourceUnion<'a>>,
) -> Result<(), Vec<Error<'a>>> {
    let u = &union.value;

    // Elm builds the argument dups dict with foldr, so occurrences are
    // inserted in reverse source order; replicated for identical regions.
    dups::detect(
        u.arguments.iter().rev().map(|a| (a.value, a.region)),
        |arg_name, first, second| Error::DuplicateUnionArg {
            type_name: u.name.value,
            arg_name,
            first,
            second,
        },
    )?;

    let bound: BTreeSet<&str> = u.arguments.iter().map(|a| a.value).collect();

    // Elm folds ctors with foldr and overwriting inserts: later ctors are
    // processed first, so earlier ctors win region conflicts.
    let mut free_vars: BTreeMap<&str, Region> = BTreeMap::new();
    for ctor in u.ctors.iter().rev() {
        for arg in ctor.arguments {
            collect_free_type_vars(arg, &mut free_vars);
        }
    }

    let unbound: Vec<(&str, Region)> = free_vars
        .into_iter()
        .filter(|(name, _)| !bound.contains(name))
        .collect();

    if unbound.is_empty() {
        Ok(())
    } else {
        let args = bump.alloc_slice_fill_iter(u.arguments.iter().map(|a| a.value));
        let (first_unbound, rest_unbound) = unbound
            .split_first()
            .expect("unbound is non-empty: guarded by is_empty check");
        Err(vec![Error::TypeVarsUnboundInUnion {
            region: union.region,
            name: u.name.value,
            args,
            unbound: *first_unbound,
            more_unbound: bump.alloc_slice_fill_iter(rest_unbound.iter().copied()),
        }])
    }
}

fn check_alias_free_vars<'a>(
    bump: &'a Bump,
    alias: &'a Located<SourceAlias<'a>>,
) -> Result<(), Vec<Error<'a>>> {
    let a = &alias.value;

    // Reverse source order, matching Elm's foldr-built dups dict.
    dups::detect(
        a.arguments.iter().rev().map(|arg| (arg.value, arg.region)),
        |arg_name, first, second| Error::DuplicateAliasArg {
            type_name: a.name.value,
            arg_name,
            first,
            second,
        },
    )?;

    let bound: BTreeSet<&str> = a.arguments.iter().map(|arg| arg.value).collect();

    let mut free_vars: BTreeMap<&str, Region> = BTreeMap::new();
    collect_free_type_vars(a.typ, &mut free_vars);

    // Name-sorted, like Elm's `Map.toList (Map.difference bound free)`.
    let unused: BTreeMap<&str, Region> = a
        .arguments
        .iter()
        .filter(|arg| !free_vars.contains_key(arg.value))
        .map(|arg| (arg.value, arg.region))
        .collect();

    let unbound: Vec<(&str, Region)> = free_vars
        .into_iter()
        .filter(|(name, _)| !bound.contains(name))
        .collect();

    if unused.is_empty() && unbound.is_empty() {
        Ok(())
    } else {
        let args = bump.alloc_slice_fill_iter(a.arguments.iter().map(|arg| arg.value));
        Err(vec![Error::TypeVarsMessedUpInAlias {
            region: alias.region,
            name: a.name.value,
            args,
            unused: bump.alloc_slice_fill_iter(unused),
            unbound: bump.alloc_slice_fill_iter(unbound),
        }])
    }
}

fn collect_type_edges<'a>(
    typ: &SourceType<'a>,
    alias_names: &BTreeSet<&'a str>,
    edges: &mut Vec<&'a str>,
) {
    match typ {
        SourceType::Lambda { from, to } => {
            collect_type_edges(&from.value, alias_names, edges);
            collect_type_edges(&to.value, alias_names, edges);
        }
        SourceType::Var(_) => {}
        SourceType::Type { name, args, .. } => {
            // Elm's `getEdges` keeps duplicates; the caller reverses the
            // final list to match its prepend accumulation.
            if alias_names.contains(name) {
                edges.push(name);
            }
            for arg in *args {
                collect_type_edges(&arg.value, alias_names, edges);
            }
        }
        SourceType::TypeQual { args, .. } => {
            // Qualified refs are external, not local alias deps
            for arg in *args {
                collect_type_edges(&arg.value, alias_names, edges);
            }
        }
        SourceType::Record { fields, ext: _ } => {
            for field in *fields {
                collect_type_edges(&field.typ.value, alias_names, edges);
            }
        }
        SourceType::Unit => {}
        SourceType::Tuple {
            first,
            second,
            rest,
        } => {
            collect_type_edges(&first.value, alias_names, edges);
            collect_type_edges(&second.value, alias_names, edges);
            for r in *rest {
                collect_type_edges(&r.value, alias_names, edges);
            }
        }
    }
}

/// Mirrors Elm's `addFreeVars`: overwriting inserts (the last occurrence
/// wins the region), and the record extension variable counts as free.
fn collect_free_type_vars<'a>(typ: &Located<SourceType<'a>>, vars: &mut BTreeMap<&'a str, Region>) {
    match &typ.value {
        SourceType::Var(name) => {
            vars.insert(name, typ.region);
        }
        SourceType::Lambda { from, to } => {
            collect_free_type_vars(from, vars);
            collect_free_type_vars(to, vars);
        }
        SourceType::Type { args, .. } | SourceType::TypeQual { args, .. } => {
            for arg in *args {
                collect_free_type_vars(arg, vars);
            }
        }
        SourceType::Record { fields, ext } => {
            if let Some(ext_var) = ext {
                vars.insert(ext_var.value, ext_var.region);
            }
            for field in *fields {
                collect_free_type_vars(field.typ, vars);
            }
        }
        SourceType::Unit => {}
        SourceType::Tuple {
            first,
            second,
            rest,
        } => {
            collect_free_type_vars(first, vars);
            collect_free_type_vars(second, vars);
            for r in *rest {
                collect_free_type_vars(r, vars);
            }
        }
    }
}

/// Mirrors Elm's `canonicalizeExports`: each exposed item is resolved
/// against the module's own values/types/binops first (accumulating all
/// resolution errors), and only then are duplicates detected. The result
/// is name-keyed, hence name-sorted, like Elm's `Map Name Export`.
fn canonicalize_exports<'a>(
    bump: &'a Bump,
    module: &SourceModule<'a>,
) -> Result<Exports<'a>, Vec<Error<'a>>> {
    match module.exports.value {
        Exposing::Open => Ok(Exports::Everything(module.exports.region)),
        Exposing::Explicit(exposed) => {
            let value_names: BTreeSet<&str> =
                module.values.iter().map(|v| v.value.name.value).collect();
            let union_names: BTreeSet<&str> =
                module.unions.iter().map(|u| u.value.name.value).collect();
            let alias_names: BTreeSet<&str> =
                module.aliases.iter().map(|a| a.value.name.value).collect();
            let binop_names: BTreeSet<&str> = module.binops.iter().map(|b| b.value.op).collect();

            let mut resolved: Vec<(&'a str, Region, Export<'a>)> = Vec::new();
            let mut errors: Vec<Error<'a>> = Vec::new();

            for item in exposed {
                match item {
                    Exposed::Lower(name) => {
                        if value_names.contains(name.value) {
                            resolved.push((name.value, name.region, Export::Value(name.value)));
                        } else {
                            errors.push(Error::ExportNotFound {
                                region: name.region,
                                kind: VarKind::BadVar,
                                name: name.value,
                                suggestions: bump
                                    .alloc_slice_fill_iter(value_names.iter().copied()),
                            });
                        }
                    }
                    Exposed::Operator { region, op } => {
                        if binop_names.contains(*op) {
                            resolved.push((op, *region, Export::Binop(op)));
                        } else {
                            errors.push(Error::ExportNotFound {
                                region: *region,
                                kind: VarKind::BadOp,
                                name: op,
                                suggestions: bump
                                    .alloc_slice_fill_iter(binop_names.iter().copied()),
                            });
                        }
                    }
                    Exposed::Upper { name, privacy } => match privacy {
                        Privacy::Public(dot_dot_region) => {
                            if union_names.contains(name.value) {
                                resolved.push((
                                    name.value,
                                    name.region,
                                    Export::UnionOpen(name.value),
                                ));
                            } else if alias_names.contains(name.value) {
                                errors.push(Error::ExportOpenAlias {
                                    region: *dot_dot_region,
                                    name: name.value,
                                });
                            } else {
                                errors.push(Error::ExportNotFound {
                                    region: name.region,
                                    kind: VarKind::BadType,
                                    name: name.value,
                                    suggestions: type_suggestions(bump, &union_names, &alias_names),
                                });
                            }
                        }
                        Privacy::Private => {
                            if union_names.contains(name.value) {
                                resolved.push((
                                    name.value,
                                    name.region,
                                    Export::UnionClosed(name.value),
                                ));
                            } else if alias_names.contains(name.value) {
                                resolved.push((name.value, name.region, Export::Alias(name.value)));
                            } else {
                                errors.push(Error::ExportNotFound {
                                    region: name.region,
                                    kind: VarKind::BadType,
                                    name: name.value,
                                    suggestions: type_suggestions(bump, &union_names, &alias_names),
                                });
                            }
                        }
                    },
                }
            }

            if !errors.is_empty() {
                return Err(errors);
            }

            let mut occurrences: BTreeMap<&'a str, Vec<(Region, Export<'a>)>> = BTreeMap::new();
            for (name, region, export) in resolved {
                occurrences.entry(name).or_default().push((region, export));
            }

            let mut exports: Vec<&'a Located<Export<'a>>> = Vec::new();
            let mut dup_errors: Vec<Error<'a>> = Vec::new();
            for (name, entries) in occurrences {
                if entries.len() > 1 {
                    dup_errors.push(Error::ExportDuplicate {
                        name,
                        first: entries[0].0,
                        second: entries[1].0,
                    });
                } else {
                    let (region, export) = entries.into_iter().next().expect("one entry");
                    exports.push(bump.alloc(Located::at(region, export)));
                }
            }

            if !dup_errors.is_empty() {
                return Err(dup_errors);
            }

            Ok(Exports::Explicit(bump.alloc_slice_fill_iter(exports)))
        }
    }
}

/// Elm suggests `Map.keys unions ++ Map.keys aliases` for a bad type export.
fn type_suggestions<'a>(
    bump: &'a Bump,
    union_names: &BTreeSet<&'a str>,
    alias_names: &BTreeSet<&'a str>,
) -> &'a [&'a str] {
    let names: Vec<&'a str> = union_names
        .iter()
        .chain(alias_names.iter())
        .copied()
        .collect();
    bump.alloc_slice_fill_iter(names)
}

fn canonicalize_binops<'a>(
    bump: &'a Bump,
    binops: &'a [&'a Located<Infix<'a>>],
) -> &'a [&'a Located<CanBinop<'a>>] {
    bump.alloc_slice_fill_iter(binops.iter().copied().map(|binop| {
        &*bump.alloc(Located::at(
            binop.region,
            CanBinop {
                symbol: binop.value.op,
                associativity: binop.value.associativity,
                precedence: binop.value.precedence,
                function: binop.value.name,
            },
        ))
    }))
}

// --- Unused import detection ---

fn collect_used_modules<'a>(module: &CanModule<'a>) -> BTreeSet<&'a str> {
    let mut used = BTreeSet::new();
    let home = module.name;
    collect_from_decls(module.decls, home, &mut used);
    for union in module.unions {
        for ctor in union.value.ctors {
            for arg in ctor.arguments {
                collect_from_type(&arg.value, home, &mut used);
            }
        }
    }
    for alias in module.aliases {
        collect_from_type(&alias.value.typ.value, home, &mut used);
    }
    used
}

fn add_if_foreign<'a>(
    home: ModuleName<'a>,
    reference_home: ModuleName<'a>,
    used: &mut BTreeSet<&'a str>,
) {
    if reference_home != home {
        used.insert(reference_home.name);
    }
}

fn collect_from_decls<'a>(decls: &Decls<'a>, home: ModuleName<'a>, used: &mut BTreeSet<&'a str>) {
    match decls {
        Decls::Declare { definition, next } => {
            collect_from_def(definition, home, used);
            collect_from_decls(next, home, used);
        }
        Decls::DeclareRec {
            definition,
            following,
            next,
        } => {
            collect_from_def(definition, home, used);
            for def in *following {
                collect_from_def(def, home, used);
            }
            collect_from_decls(next, home, used);
        }
        Decls::Empty => {}
    }
}

fn collect_from_def<'a>(
    def: &alder_ast::Def<'a>,
    home: ModuleName<'a>,
    used: &mut BTreeSet<&'a str>,
) {
    match def {
        alder_ast::Def::Def { body, args, .. } => {
            for arg in *args {
                collect_from_pattern(&arg.value, home, used);
            }
            collect_from_expr(&body.value, home, used);
        }
        alder_ast::Def::TypedDef {
            args, body, typ, ..
        } => {
            for arg in *args {
                collect_from_pattern(&arg.pattern.value, home, used);
                collect_from_type(&arg.typ.value, home, used);
            }
            collect_from_expr(&body.value, home, used);
            collect_from_type(&typ.value, home, used);
        }
    }
}

fn collect_from_expr<'a>(
    expr: &alder_ast::Expr<'a>,
    home: ModuleName<'a>,
    used: &mut BTreeSet<&'a str>,
) {
    use alder_ast::Expr::*;
    match expr {
        VarLocal(_) | Str(_) | Int(_) | Accessor(_) | Unit => {}
        VarTopLevel(q) => add_if_foreign(home, q.home, used),
        // Only the reference counts as a use: the annotation is data from
        // the origin module's solver, not something written here.
        VarForeign { reference, .. } => add_if_foreign(home, reference.home, used),
        VarConstructor {
            reference,
            annotation,
            ..
        } => {
            add_if_foreign(home, reference.home, used);
            collect_from_type(&annotation.typ.value, home, used);
        }
        VarOperator { reference, .. } => {
            add_if_foreign(home, reference.home, used);
        }
        Binop {
            reference,
            left,
            right,
            ..
        } => {
            add_if_foreign(home, reference.home, used);
            collect_from_expr(&left.value, home, used);
            collect_from_expr(&right.value, home, used);
        }
        List(items) => {
            for item in *items {
                collect_from_expr(&item.value, home, used);
            }
        }
        Negate(e) => collect_from_expr(&e.value, home, used),
        Lambda { parameters, body } => {
            for p in *parameters {
                collect_from_pattern(&p.value, home, used);
            }
            collect_from_expr(&body.value, home, used);
        }
        Call {
            function,
            arguments,
        } => {
            collect_from_expr(&function.value, home, used);
            for a in *arguments {
                collect_from_expr(&a.value, home, used);
            }
        }
        If {
            branches,
            final_else,
        } => {
            for b in *branches {
                collect_from_expr(&b.condition.value, home, used);
                collect_from_expr(&b.then_branch.value, home, used);
            }
            collect_from_expr(&final_else.value, home, used);
        }
        Let { definition, body } => {
            collect_from_def(definition, home, used);
            collect_from_expr(&body.value, home, used);
        }
        LetRec { definitions, body } => {
            for d in *definitions {
                collect_from_def(d, home, used);
            }
            collect_from_expr(&body.value, home, used);
        }
        LetDestruct {
            pattern,
            value,
            body,
        } => {
            collect_from_pattern(&pattern.value, home, used);
            collect_from_expr(&value.value, home, used);
            collect_from_expr(&body.value, home, used);
        }
        Case {
            scrutinee,
            branches,
        } => {
            collect_from_expr(&scrutinee.value, home, used);
            for b in *branches {
                collect_from_pattern(&b.pattern.value, home, used);
                collect_from_expr(&b.body.value, home, used);
            }
        }
        Access { record, .. } => collect_from_expr(&record.value, home, used),
        Update { base, fields, .. } => {
            collect_from_expr(&base.value, home, used);
            for f in *fields {
                collect_from_expr(&f.value.value, home, used);
            }
        }
        Record(fields) => {
            for f in *fields {
                collect_from_expr(&f.value.value, home, used);
            }
        }
        Tuple {
            first,
            second,
            rest,
        } => {
            collect_from_expr(&first.value, home, used);
            collect_from_expr(&second.value, home, used);
            for r in *rest {
                collect_from_expr(&r.value, home, used);
            }
        }
    }
}

fn collect_from_pattern<'a>(
    pat: &alder_ast::Pattern<'a>,
    home: ModuleName<'a>,
    used: &mut BTreeSet<&'a str>,
) {
    use alder_ast::Pattern::*;
    match pat {
        Anything | Var(_) | Str(_) | Int(_) | Unit | Record(_) => {}
        // `True`/`False` patterns only ever come from the module named
        // Basics (see `environment::Ctor::Bool`), so count it as used.
        Bool { .. } => {
            used.insert("Basics");
        }
        Constructor(ctor) => {
            add_if_foreign(home, ctor.reference.home, used);
            for arg in ctor.arguments {
                collect_from_type(&arg.typ.value, home, used);
                collect_from_pattern(&arg.pattern.value, home, used);
            }
        }
        Alias { pattern, .. } => collect_from_pattern(&pattern.value, home, used),
        Tuple {
            first,
            second,
            rest,
        } => {
            collect_from_pattern(&first.value, home, used);
            collect_from_pattern(&second.value, home, used);
            for r in *rest {
                collect_from_pattern(&r.value, home, used);
            }
        }
        List(items) => {
            for item in *items {
                collect_from_pattern(&item.value, home, used);
            }
        }
        Cons { head, tail } => {
            collect_from_pattern(&head.value, home, used);
            collect_from_pattern(&tail.value, home, used);
        }
    }
}

fn collect_from_type<'a>(
    typ: &alder_ast::Type<'a>,
    home: ModuleName<'a>,
    used: &mut BTreeSet<&'a str>,
) {
    use alder_ast::Type::*;
    match typ {
        Var(_) | Unit => {}
        Lambda { from, to } => {
            collect_from_type(&from.value, home, used);
            collect_from_type(&to.value, home, used);
        }
        Named { reference, args } => {
            add_if_foreign(home, reference.home, used);
            for a in *args {
                collect_from_type(&a.value, home, used);
            }
        }
        Record { fields, .. } => {
            for f in *fields {
                collect_from_type(&f.typ.value, home, used);
            }
        }
        Tuple {
            first,
            second,
            rest,
        } => {
            collect_from_type(&first.value, home, used);
            collect_from_type(&second.value, home, used);
            for r in *rest {
                collect_from_type(&r.value, home, used);
            }
        }
        Alias {
            reference,
            arguments,
            target,
        } => {
            add_if_foreign(home, reference.home, used);
            for a in *arguments {
                collect_from_type(&a.typ.value, home, used);
            }
            match target {
                alder_ast::AliasType::Open(t) | alder_ast::AliasType::Filled(t) => {
                    collect_from_type(&t.value, home, used);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use alder_ast::{
        Associativity, Ctor as CanCtor, CtorOpts, Module as CanModule, ModuleName, PackageName,
        Precedence, Type as CanType,
    };
    use alder_region::{Located, Region};
    use bumpalo::Bump;
    use indoc::indoc;

    use super::{Context, canonicalize};
    use crate::interface::{
        self, AliasVisibility, InterfaceAlias, InterfaceBinop, InterfaceUnion, InterfaceValue,
        UnionVisibility,
    };
    use crate::{Error, Interface};

    fn parse_and_canonicalize<'a>(
        bump: &'a Bump,
        input: &str,
        context: Context<'a>,
    ) -> Result<CanModule<'a>, Vec<Error<'a>>> {
        let src = bump.alloc_str(input);
        let mut parser = alder_parse::Parser::new(bump, src.as_bytes());
        let module = parser.module().expect("expected successful parse");
        canonicalize(bump, context, &module).map(|r| r.module)
    }

    macro_rules! assert_module_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let result = parse_and_canonicalize(&bump, input, Context::default())
                .expect("expected successful canonicalization");

            insta::with_settings!({
                description => format!("Code:\n\n{}", input),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result);
            });
        }};
    }

    macro_rules! assert_module_error_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let result = parse_and_canonicalize(&bump, input, Context::default())
                .expect_err("expected canonicalization error");

            insta::with_settings!({
                description => format!("Code:\n\n{}", input),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result);
            });
        }};
    }

    macro_rules! assert_interface_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let can_module = parse_and_canonicalize(&bump, input, Context::default())
                .expect("expected successful canonicalization");
            let annotations = mock_annotations(&bump, &can_module);
            let result = interface::from_module(&bump, &can_module, &annotations);
            insta::with_settings!({
                description => format!("Code:\n\n{}", input),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result);
            });
        }};
    }

    fn var_type<'a>(bump: &'a Bump, name: &'a str) -> &'a Located<CanType<'a>> {
        bump.alloc(Located::at(Region::zero(), CanType::Var(name)))
    }

    /// Stand-in for a solver-produced annotation in tests: `Forall [a] a`.
    fn test_annotation<'a>(bump: &'a Bump) -> &'a alder_ast::Annotation<'a> {
        bump.alloc(alder_ast::Annotation {
            free_vars: bump.alloc_slice_fill_iter(["a"]),
            typ: var_type(bump, "a"),
        })
    }

    /// Solver stand-in for interface extraction tests: give every
    /// top-level value a `Forall [a] a` annotation, mimicking the map
    /// Elm's `Interface.fromModule` receives from the solver.
    fn mock_annotations<'a>(
        bump: &'a Bump,
        module: &CanModule<'a>,
    ) -> crate::interface::Annotations<'a> {
        fn walk<'a>(
            decls: &alder_ast::Decls<'a>,
            bump: &'a Bump,
            out: &mut crate::interface::Annotations<'a>,
        ) {
            match decls {
                alder_ast::Decls::Declare { definition, next } => {
                    add(definition, bump, out);
                    walk(next, bump, out);
                }
                alder_ast::Decls::DeclareRec {
                    definition,
                    following,
                    next,
                } => {
                    add(definition, bump, out);
                    for def in *following {
                        add(def, bump, out);
                    }
                    walk(next, bump, out);
                }
                alder_ast::Decls::Empty => {}
            }
        }
        fn add<'a>(
            def: &alder_ast::Def<'a>,
            bump: &'a Bump,
            out: &mut crate::interface::Annotations<'a>,
        ) {
            let name = match def {
                alder_ast::Def::Def { name, .. } | alder_ast::Def::TypedDef { name, .. } => {
                    name.value
                }
            };
            out.insert(name, test_annotation(bump));
        }
        let mut annotations = crate::interface::Annotations::new();
        walk(module.decls, bump, &mut annotations);
        annotations
    }

    fn union_interface<'a>(
        bump: &'a Bump,
        module_name: &'a str,
        union_name: &'a str,
        parameters: &'a [&'a str],
    ) -> Interface<'a> {
        Interface {
            home: ModuleName {
                package: None,
                name: module_name,
            },
            values: &[],
            aliases: &[],
            unions: bump.alloc_slice_fill_iter([InterfaceUnion {
                name: union_name,
                parameters,
                ctors: &[],
                alternatives: 0,
                options: CtorOpts::Normal,
                visibility: UnionVisibility::Open,
            }]),
            binops: &[],
        }
    }

    fn alias_interface<'a>(
        bump: &'a Bump,
        module_name: &'a str,
        alias_name: &'a str,
        parameters: &'a [&'a str],
        typ: &'a Located<CanType<'a>>,
    ) -> Interface<'a> {
        Interface {
            home: ModuleName {
                package: None,
                name: module_name,
            },
            values: &[],
            aliases: bump.alloc_slice_fill_iter([InterfaceAlias {
                name: alias_name,
                parameters,
                typ,
                visibility: AliasVisibility::Public,
            }]),
            unions: &[],
            binops: &[],
        }
    }

    // === Module tests ===

    #[test]
    fn module_shell_header_only() {
        assert_module_snapshot!("module Main exposing (..)\n");
    }

    #[test]
    fn module_shell_with_infix_metadata() {
        assert_module_snapshot!(
            r#"
            module Main exposing ((|>))

            infix left 6 (|>) = apR

            apR x f = f x
        "#
        );
    }

    #[test]
    fn module_shell_with_enum_union() {
        assert_module_snapshot!(
            r#"
            module Main exposing (Bool(..))

            type Bool
                = True
                | False
        "#
        );
    }

    #[test]
    fn module_shell_with_aliases_and_unions() {
        assert_module_snapshot!(
            r#"
            module Main exposing (Pair, Maybe(..))

            type alias Pair a b = (a, b)

            type Maybe a
                = Just a
                | Nothing
        "#
        );
    }

    #[test]
    fn module_shell_with_local_named_types() {
        assert_module_snapshot!(
            r#"
            module Main exposing (Pair, WrappedPair, WrappedMaybe, Maybe(..))

            type alias Pair a b = (a, b)

            type alias WrappedPair a b = Pair a b

            type alias WrappedMaybe a = Maybe a

            type Maybe a
                = Just a
                | Nothing
        "#
        );
    }

    #[test]
    fn module_shell_with_self_qualified_named_types() {
        assert_module_snapshot!(
            r#"
            module Main exposing (Maybe(..), Wrapped)

            type alias Wrapped a = Main.Maybe a

            type Maybe a
                = Just a
                | Nothing
        "#
        );
    }

    #[test]
    fn module_shell_reports_unresolved_named_types() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (Wrapped)

            type alias Wrapped = Missing
        "#
        );
    }

    #[test]
    fn module_shell_reports_unresolved_qualified_named_types() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (Wrapped)

            type alias Wrapped = Missing.Maybe
        "#
        );
    }

    #[test]
    fn module_shell_reports_missing_imported_interfaces() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (Wrapped)

            import Result

            type alias Wrapped e a = Result.Result e a
        "#
        );
    }

    #[test]
    fn module_shell_reports_ambiguous_open_imported_types() {
        let input = indoc!(
            r#"
            module Main exposing (Wrapped)

            import Maybe exposing (..)

            import Option exposing (..)

            type alias Wrapped a = Maybe a
        "#
        );
        let bump = Bump::new();
        let parameters = bump.alloc_slice_fill_iter(["a"]);
        let interfaces = BTreeMap::from([
            (
                "Maybe",
                union_interface(&bump, "Maybe", "Maybe", parameters),
            ),
            (
                "Option",
                union_interface(&bump, "Option", "Maybe", parameters),
            ),
        ]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn module_shell_reports_ambiguous_explicit_and_open_imported_types() {
        let input = indoc!(
            r#"
            module Main exposing (Wrapped)

            import Maybe exposing (Maybe)

            import Option exposing (..)

            type alias Wrapped a = Maybe a
        "#
        );
        let bump = Bump::new();
        let parameters = bump.alloc_slice_fill_iter(["a"]);
        let interfaces = BTreeMap::from([
            (
                "Maybe",
                union_interface(&bump, "Maybe", "Maybe", parameters),
            ),
            (
                "Option",
                union_interface(&bump, "Option", "Maybe", parameters),
            ),
        ]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn module_shell_reports_ambiguous_qualified_imported_types() {
        let input = indoc!(
            r#"
            module Main exposing (Wrapped)

            import Json.Decode as Decode

            import Html.Decode as Decode

            type alias Wrapped msg = Decode.Decoder msg
        "#
        );
        let bump = Bump::new();
        let parameters = bump.alloc_slice_fill_iter(["msg"]);
        let interfaces = BTreeMap::from([
            (
                "Json.Decode",
                alias_interface(
                    &bump,
                    "Json.Decode",
                    "Decoder",
                    parameters,
                    var_type(&bump, "msg"),
                ),
            ),
            (
                "Html.Decode",
                alias_interface(
                    &bump,
                    "Html.Decode",
                    "Decoder",
                    parameters,
                    var_type(&bump, "msg"),
                ),
            ),
        ]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn module_shell_reports_bad_type_arity() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (Wrapped, Maybe(..))

            type alias Wrapped = Maybe

            type Maybe a
                = Just a
                | Nothing
        "#
        );
    }

    #[test]
    fn module_shell_reports_missing_exported_values() {
        assert_module_error_snapshot!("module Main exposing (main)\n");
    }

    #[test]
    fn module_shell_reports_missing_exported_operators() {
        assert_module_error_snapshot!("module Main exposing ((|>))\n");
    }

    #[test]
    fn module_shell_reports_ambiguous_imported_types_with_multiple_modules() {
        let input = indoc!(
            r#"
            module Main exposing (Wrapped)

            import Maybe exposing (..)

            import Option exposing (..)

            import Choice exposing (..)

            type alias Wrapped a = Maybe a
        "#
        );
        let bump = Bump::new();
        let parameters = bump.alloc_slice_fill_iter(["a"]);
        let interfaces = BTreeMap::from([
            (
                "Maybe",
                union_interface(&bump, "Maybe", "Maybe", parameters),
            ),
            (
                "Option",
                union_interface(&bump, "Option", "Maybe", parameters),
            ),
            (
                "Choice",
                union_interface(&bump, "Choice", "Maybe", parameters),
            ),
        ]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn module_shell_with_imported_exposed_union_types() {
        let input = indoc!(
            r#"
            module Main exposing (Wrapped)

            import Maybe exposing (Maybe)

            type alias Wrapped a = Maybe a
        "#
        );
        let bump = Bump::new();
        let parameters = bump.alloc_slice_fill_iter(["a"]);
        let interfaces = BTreeMap::from([(
            "Maybe",
            union_interface(&bump, "Maybe", "Maybe", parameters),
        )]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn module_shell_with_imported_qualified_union_types() {
        let input = indoc!(
            r#"
            module Main exposing (Wrapped)

            import Result

            type alias Wrapped e a = Result.Result e a
        "#
        );
        let bump = Bump::new();
        let parameters = bump.alloc_slice_fill_iter(["e", "a"]);
        let interfaces = BTreeMap::from([(
            "Result",
            union_interface(&bump, "Result", "Result", parameters),
        )]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn module_shell_with_imported_aliased_alias_types() {
        let input = indoc!(
            r#"
            module Main exposing (Decoder)

            import Json.Decode as Decode

            type alias Decoder msg = Decode.Decoder msg
        "#
        );
        let bump = Bump::new();
        let parameters = bump.alloc_slice_fill_iter(["msg"]);
        let interfaces = BTreeMap::from([(
            "Json.Decode",
            alias_interface(
                &bump,
                "Json.Decode",
                "Decoder",
                parameters,
                var_type(&bump, "msg"),
            ),
        )]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    // === Converted tests ===

    #[test]
    fn module_shell_requires_explicit_header() {
        assert_module_error_snapshot!("main = 42\n");
    }

    #[test]
    fn module_shell_keeps_package_context() {
        let input = indoc!("module Json.Decode exposing (..)\n");
        let bump = Bump::new();
        let context = Context {
            package: Some(PackageName {
                author: "alder",
                project: "compiler",
            }),
            interfaces: None,
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");

        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn module_shell_reports_unresolved_exported_upper_names() {
        assert_module_error_snapshot!("module Main exposing (Missing)\n");
    }

    #[test]
    fn module_shell_reports_export_open_alias() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (Pair(..))

            type alias Pair a b = (a, b)
        "#
        );
    }

    // === Interface tests ===

    #[test]
    fn interface_from_module_empty() {
        assert_interface_snapshot!("module Main exposing (..)\n");
    }

    #[test]
    fn interface_from_module_open_exports() {
        assert_interface_snapshot!(
            r#"
            module Main exposing (..)

            type alias Pair a b = (a, b)

            type Maybe a
                = Just a
                | Nothing
        "#
        );
    }

    #[test]
    fn interface_from_module_open_union() {
        assert_interface_snapshot!(
            r#"
            module Main exposing (Bool(..))

            type Bool
                = True
                | False
        "#
        );
    }

    #[test]
    fn interface_from_module_closed_union() {
        assert_interface_snapshot!(
            r#"
            module Main exposing (Bool)

            type Bool
                = True
                | False
        "#
        );
    }

    #[test]
    fn interface_from_module_mixed_visibility() {
        assert_interface_snapshot!(
            r#"
            module Main exposing (PublicAlias)

            type alias PublicAlias a = a

            type alias PrivateAlias a = a

            type PrivateUnion
                = Foo
                | Bar
        "#
        );
    }

    #[test]
    fn interface_from_module_with_binops() {
        assert_interface_snapshot!(
            r#"
            module Main exposing ((|>))

            infix left 6 (|>) = apR

            apR x f = f x
        "#
        );
    }

    // === to_public tests ===

    #[test]
    fn to_public_union_open_passes_through() {
        let union = InterfaceUnion {
            name: "Bool",
            parameters: &[],
            ctors: &[],
            alternatives: 2,
            options: CtorOpts::Enum,
            visibility: UnionVisibility::Open,
        };
        insta::with_settings!({
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(union.to_public());
        });
    }

    #[test]
    fn to_public_union_closed_strips_ctors() {
        let bump = Bump::new();
        let ctor: &CanCtor = bump.alloc(CanCtor {
            name: "True",
            index: 0,
            arity: 0,
            arguments: &[],
        });
        let union = InterfaceUnion {
            name: "Bool",
            parameters: &[],
            ctors: bump.alloc_slice_fill_iter([ctor]),
            alternatives: 2,
            options: CtorOpts::Enum,
            visibility: UnionVisibility::Closed,
        };
        insta::with_settings!({
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(union.to_public());
        });
    }

    #[test]
    fn to_public_union_private_returns_none() {
        let union = InterfaceUnion {
            name: "Internal",
            parameters: &[],
            ctors: &[],
            alternatives: 0,
            options: CtorOpts::Normal,
            visibility: UnionVisibility::Private,
        };
        insta::with_settings!({
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(union.to_public());
        });
    }

    #[test]
    fn to_public_alias_public_passes_through() {
        let bump = Bump::new();
        let typ = bump.alloc(Located::at(Region::zero(), CanType::Unit));
        let alias = InterfaceAlias {
            name: "Pair",
            parameters: &["a", "b"],
            typ,
            visibility: AliasVisibility::Public,
        };
        insta::with_settings!({
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(alias.to_public());
        });
    }

    #[test]
    fn to_public_alias_private_returns_none() {
        let bump = Bump::new();
        let typ = bump.alloc(Located::at(Region::zero(), CanType::Unit));
        let alias = InterfaceAlias {
            name: "Internal",
            parameters: &[],
            typ,
            visibility: AliasVisibility::Private,
        };
        insta::with_settings!({
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(alias.to_public());
        });
    }

    // === Validation tests ===

    #[test]
    fn duplicate_type_alias_and_union() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Foo = Int

            type Foo
                = Bar
        "#
        );
    }

    #[test]
    fn duplicate_type_two_aliases() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Foo = Int

            type alias Foo = String
        "#
        );
    }

    #[test]
    fn duplicate_value_decl() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            foo = 1

            foo = 2
        "#
        );
    }

    #[test]
    fn duplicate_binop() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            infix left 6 (|>) = apR

            infix left 6 (|>) = apR2

            apR x f = f x

            apR2 x f = f x
        "#
        );
    }

    #[test]
    fn record_type_duplicate_field() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias R = { x : Int, x : String }
        "#
        );
    }

    #[test]
    fn recursive_alias_self() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Loop = Loop
        "#
        );
    }

    #[test]
    fn recursive_alias_cycle() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias A = B

            type alias B = A
        "#
        );
    }

    #[test]
    fn union_unbound_type_var() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type MyType a
                = MyTag b
        "#
        );
    }

    #[test]
    fn alias_unused_type_param() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Phantom a = Int
        "#
        );
    }

    #[test]
    fn alias_unbound_type_var() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Bad = List a
        "#
        );
    }

    #[test]
    fn union_duplicate_type_param() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type Bad a a
                = Foo
        "#
        );
    }

    #[test]
    fn alias_duplicate_type_param() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Bad a a = a
        "#
        );
    }

    #[test]
    fn multiple_duplicate_types_all_reported() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Foo = Int

            type alias Foo = String

            type alias Bar = Int

            type alias Bar = String
        "#
        );
    }

    #[test]
    fn record_multiple_duplicate_fields_all_reported() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias R = { x : Int, x : String, y : Int, y : String }
        "#
        );
    }

    #[test]
    fn duplicate_export() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (Foo, Foo)

            type alias Foo = Int
        "#
        );
    }

    // === Expression canonicalization tests ===

    fn parse_and_canonicalize_with_warnings<'a>(
        bump: &'a Bump,
        input: &str,
        context: Context<'a>,
    ) -> Result<(CanModule<'a>, Vec<crate::Warning<'a>>), Vec<Error<'a>>> {
        let src = bump.alloc_str(input);
        let mut parser = alder_parse::Parser::new(bump, src.as_bytes());
        let module = parser.module().expect("expected successful parse");
        canonicalize(bump, context, &module).map(|r| (r.module, r.warnings))
    }

    macro_rules! assert_module_warning_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let (_, warnings) =
                parse_and_canonicalize_with_warnings(&bump, input, Context::default())
                    .expect("expected successful canonicalization");
            assert!(!warnings.is_empty(), "expected warnings but got none");
            insta::with_settings!({
                description => format!("Code:\n\n{}", input),
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(warnings);
            });
        }};
    }

    // -- Positive expression tests --

    #[test]
    fn simple_value() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            x = 42
        "#
        );
    }

    #[test]
    fn function_def() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f a b = a
        "#
        );
    }

    #[test]
    fn lambda_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = \x -> x
        "#
        );
    }

    #[test]
    fn let_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    x = 1
                in
                x
        "#
        );
    }

    #[test]
    fn if_then_else() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x = if x then 1 else 2
        "#
        );
    }

    #[test]
    fn record_literal() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = { x = 1, y = 2 }
        "#
        );
    }

    #[test]
    fn record_update() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f r = { r | x = 1 }
        "#
        );
    }

    #[test]
    fn list_literal() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = [ 1, 2, 3 ]
        "#
        );
    }

    #[test]
    fn accessor_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = .name
        "#
        );
    }

    #[test]
    fn field_access() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f r = r.name
        "#
        );
    }

    #[test]
    fn case_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x =
                case x of
                    1 -> "one"
                    _ -> "other"
        "#
        );
    }

    #[test]
    fn string_literal() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = "hello"
        "#
        );
    }

    #[test]
    fn negate_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x = -x
        "#
        );
    }

    #[test]
    fn function_call() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x = g x

            g y = y
        "#
        );
    }

    #[test]
    fn tuple_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = ( 1, 2, 3 )
        "#
        );
    }

    #[test]
    fn unit_expr() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = ()
        "#
        );
    }

    #[test]
    fn let_recursive_function() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    go x = go x
                in
                go 1
        "#
        );
    }

    #[test]
    fn nested_let() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    x = 1
                    y = 2
                in
                x
        "#
        );
    }

    // -- Error expression tests --

    #[test]
    fn not_found_var() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f = nonexistent
        "#
        );
    }

    #[test]
    fn recursive_let_value() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    x = x
                in
                x
        "#
        );
    }

    #[test]
    fn shadowing_local() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f x =
                let
                    x = 1
                in
                x
        "#
        );
    }

    #[test]
    fn duplicate_let_bindings() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    x = 1
                    x = 2
                in
                x
        "#
        );
    }

    #[test]
    fn tuple_four_in_expr() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f = ( 1, 2, 3, 4 )
        "#
        );
    }

    #[test]
    fn duplicate_record_fields_in_expr() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f = { x = 1, x = 2 }
        "#
        );
    }

    // -- Warning tests --

    #[test]
    fn unused_lambda_arg() {
        assert_module_warning_snapshot!(
            r#"
            module Main exposing (..)

            f = \x -> 1
        "#
        );
    }

    #[test]
    fn unused_let_binding() {
        assert_module_warning_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    x = 1
                in
                2
        "#
        );
    }

    // === SCC tests ===

    #[test]
    fn mutual_recursion_functions() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x = g x

            g x = f x
        "#
        );
    }

    #[test]
    fn self_recursive_function() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x = f x
        "#
        );
    }

    #[test]
    fn mixed_recursive_and_non_recursive() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            x = 1

            f a = g a

            g a = f a

            y = 2
        "#
        );
    }

    #[test]
    fn dependency_ordering() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            b = a

            a = 1
        "#
        );
    }

    #[test]
    fn value_and_function_in_cycle() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            x = f 1

            f a = x
        "#
        );
    }

    // -- SCC error tests --

    #[test]
    fn recursive_decl_self_reference() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            x = x
        "#
        );
    }

    #[test]
    fn recursive_decl_mutual_values() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            x = y

            y = x
        "#
        );
    }

    // === Unused import warning tests ===

    fn value_interface<'a>(
        bump: &'a Bump,
        module_name: &'a str,
        val_name: &'a str,
    ) -> Interface<'a> {
        Interface {
            home: ModuleName {
                package: None,
                name: module_name,
            },
            values: bump.alloc_slice_fill_iter([crate::interface::InterfaceValue {
                name: val_name,
                annotation: test_annotation(bump),
            }]),
            aliases: &[],
            unions: &[],
            binops: &[],
        }
    }

    #[test]
    fn unused_import_warning() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo

            x = 1
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let (_, warnings) = parse_and_canonicalize_with_warnings(&bump, input, context)
            .expect("expected successful canonicalization");
        assert!(!warnings.is_empty(), "expected warnings but got none");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(warnings);
        });
    }

    #[test]
    fn used_import_no_warning() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo

            x = Foo.bar
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let (_, warnings) = parse_and_canonicalize_with_warnings(&bump, input, context)
            .expect("expected successful canonicalization");
        assert!(
            warnings.is_empty(),
            "expected no warnings but got: {warnings:?}"
        );
    }

    // === Helpers for interface-dependent tests ===

    fn maybe_with_ctors_interface<'a>(bump: &'a Bump) -> Interface<'a> {
        let just_arg = bump.alloc(Located::at(Region::zero(), CanType::Var("a")));
        let just_ctor: &CanCtor = bump.alloc(CanCtor {
            name: "Just",
            index: 0,
            arity: 1,
            arguments: bump.alloc_slice_fill_iter([&*just_arg]),
        });
        let nothing_ctor: &CanCtor = bump.alloc(CanCtor {
            name: "Nothing",
            index: 1,
            arity: 0,
            arguments: &[],
        });
        Interface {
            home: ModuleName {
                package: None,
                name: "Maybe",
            },
            values: &[],
            aliases: &[],
            unions: bump.alloc_slice_fill_iter([InterfaceUnion {
                name: "Maybe",
                parameters: bump.alloc_slice_fill_iter(["a"]),
                ctors: bump.alloc_slice_fill_iter([just_ctor, nothing_ctor]),
                alternatives: 2,
                options: CtorOpts::Normal,
                visibility: UnionVisibility::Open,
            }]),
            binops: &[],
        }
    }

    fn basics_with_binops_interface<'a>(bump: &'a Bump) -> Interface<'a> {
        Interface {
            home: ModuleName {
                package: None,
                name: "Basics",
            },
            values: bump.alloc_slice_fill_iter([
                InterfaceValue {
                    name: "add",
                    annotation: test_annotation(bump),
                },
                InterfaceValue {
                    name: "sub",
                    annotation: test_annotation(bump),
                },
                InterfaceValue {
                    name: "mul",
                    annotation: test_annotation(bump),
                },
                InterfaceValue {
                    name: "apR",
                    annotation: test_annotation(bump),
                },
                InterfaceValue {
                    name: "apL",
                    annotation: test_annotation(bump),
                },
            ]),
            aliases: &[],
            unions: &[],
            binops: bump.alloc_slice_fill_iter([
                InterfaceBinop {
                    symbol: "+",
                    annotation: test_annotation(bump),
                    associativity: Associativity::Left,
                    precedence: Precedence(6),
                    function: "add",
                },
                InterfaceBinop {
                    symbol: "-",
                    annotation: test_annotation(bump),
                    associativity: Associativity::Left,
                    precedence: Precedence(6),
                    function: "sub",
                },
                InterfaceBinop {
                    symbol: "*",
                    annotation: test_annotation(bump),
                    associativity: Associativity::Left,
                    precedence: Precedence(7),
                    function: "mul",
                },
                InterfaceBinop {
                    symbol: "|>",
                    annotation: test_annotation(bump),
                    associativity: Associativity::Left,
                    precedence: Precedence(0),
                    function: "apR",
                },
                InterfaceBinop {
                    symbol: "<|",
                    annotation: test_annotation(bump),
                    associativity: Associativity::Right,
                    precedence: Precedence(0),
                    function: "apL",
                },
            ]),
        }
    }

    // === Constructor / operator expression tests ===

    #[test]
    fn constructor_in_expression() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Maybe(..))

            x = Just
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Maybe", maybe_with_ctors_interface(&bump))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn record_ctor_in_expression() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            type alias Pair a b = { first : a, second : b }

            p = Pair
        "#
        );
    }

    // qualified_ctor_in_expression skipped: parser doesn't yet produce
    // VarQual { kind: CapVar } for `Module.Ctor` syntax.

    #[test]
    fn op_as_value() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Basics exposing (..)

            x = (+)
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Basics", basics_with_binops_interface(&bump))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn binop_expression() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Basics exposing (..)

            x a b = a + b
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Basics", basics_with_binops_interface(&bump))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn binop_multi_precedence() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Basics exposing (..)

            x a b c = a + b * c
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Basics", basics_with_binops_interface(&bump))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn binop_right_assoc() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Basics exposing (..)

            x a b c = a <| b <| c
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Basics", basics_with_binops_interface(&bump))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    // === Typed def tests ===

    #[test]
    fn typed_def_top_level() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f : a -> a
            f x = x
        "#
        );
    }

    #[test]
    fn typed_def_in_let() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            g =
                let
                    f : a -> a
                    f x = x
                in
                f 1
        "#
        );
    }

    // === Let destruct ===

    #[test]
    fn let_destruct() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    (a, b) = (1, 2)
                in
                a
        "#
        );
    }

    #[test]
    fn let_mutual_recursion() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    go x = stop x
                    stop x = go x
                in
                go 1
        "#
        );
    }

    // === find_var gaps ===

    #[test]
    fn foreign_var_unqualified() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo exposing (bar)

            x = bar
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn not_found_var_qualified() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo

            x = Foo.nonexistent
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    // === verify_bindings ===

    #[test]
    fn underscore_prefix_no_warning() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f = \_ -> 1
        "#
        );
    }

    // === canonicalize_if ===

    #[test]
    fn chained_if_else_if() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f x =
                if x then
                    1
                else if x then
                    2
                else
                    3
        "#
        );
    }

    // === canonicalize_update ===

    #[test]
    fn update_missing_record_var() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f = { nonexistent | x = 1 }
        "#
        );
    }

    // === case branches ===

    #[test]
    fn case_with_ctor_patterns() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Maybe(..))

            f x =
                case x of
                    Just y -> y
                    Nothing -> 0
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Maybe", maybe_with_ctors_interface(&bump))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    // === Expression error tests ===

    #[test]
    fn not_found_binop() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            x = (+)
        "#
        );
    }

    #[test]
    fn ambiguous_var() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo exposing (..)

            import Bar exposing (..)

            x = baz
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([
            ("Foo", value_interface(&bump, "Foo", "baz")),
            ("Bar", value_interface(&bump, "Bar", "baz")),
        ]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn ambiguous_ctor_in_expr() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Maybe(..))

            import Option exposing (Option(..))

            x = Just
        "#
        );
        let bump = Bump::new();
        // Build two interfaces that both expose "Just"
        let just_arg = bump.alloc(Located::at(Region::zero(), CanType::Var("a")));
        let just_ctor: &CanCtor = bump.alloc(CanCtor {
            name: "Just",
            index: 0,
            arity: 1,
            arguments: bump.alloc_slice_fill_iter([&*just_arg]),
        });
        let nothing_ctor: &CanCtor = bump.alloc(CanCtor {
            name: "Nothing",
            index: 1,
            arity: 0,
            arguments: &[],
        });
        let maybe_interface = Interface {
            home: ModuleName {
                package: None,
                name: "Maybe",
            },
            values: &[],
            aliases: &[],
            unions: bump.alloc_slice_fill_iter([InterfaceUnion {
                name: "Maybe",
                parameters: bump.alloc_slice_fill_iter(["a"]),
                ctors: bump.alloc_slice_fill_iter([just_ctor, nothing_ctor]),
                alternatives: 2,
                options: CtorOpts::Normal,
                visibility: UnionVisibility::Open,
            }]),
            binops: &[],
        };

        let just_arg2 = bump.alloc(Located::at(Region::zero(), CanType::Var("a")));
        let just_ctor2: &CanCtor = bump.alloc(CanCtor {
            name: "Just",
            index: 0,
            arity: 1,
            arguments: bump.alloc_slice_fill_iter([&*just_arg2]),
        });
        let none_ctor: &CanCtor = bump.alloc(CanCtor {
            name: "None",
            index: 1,
            arity: 0,
            arguments: &[],
        });
        let option_interface = Interface {
            home: ModuleName {
                package: None,
                name: "Option",
            },
            values: &[],
            aliases: &[],
            unions: bump.alloc_slice_fill_iter([InterfaceUnion {
                name: "Option",
                parameters: bump.alloc_slice_fill_iter(["a"]),
                ctors: bump.alloc_slice_fill_iter([just_ctor2, none_ctor]),
                alternatives: 2,
                options: CtorOpts::Normal,
                visibility: UnionVisibility::Open,
            }]),
            binops: &[],
        };

        let interfaces = BTreeMap::from([("Maybe", maybe_interface), ("Option", option_interface)]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn ambiguous_binop() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Basics exposing (..)

            import MyMath exposing (..)

            x a b = a + b
        "#
        );
        let bump = Bump::new();
        let basics = basics_with_binops_interface(&bump);
        let mymath = Interface {
            home: ModuleName {
                package: None,
                name: "MyMath",
            },
            values: &[],
            aliases: &[],
            unions: &[],
            binops: bump.alloc_slice_fill_iter([InterfaceBinop {
                symbol: "+",
                annotation: test_annotation(&bump),
                associativity: Associativity::Left,
                precedence: Precedence(6),
                function: "myAdd",
            }]),
        };
        let interfaces = BTreeMap::from([("Basics", basics), ("MyMath", mymath)]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn binop_non_assoc_conflict() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Basics exposing (..)

            x a b c = a == b == c
        "#
        );
        let bump = Bump::new();
        let basics = Interface {
            home: ModuleName {
                package: None,
                name: "Basics",
            },
            values: bump.alloc_slice_fill_iter([InterfaceValue {
                name: "eq",
                annotation: test_annotation(&bump),
            }]),
            aliases: &[],
            unions: &[],
            binops: bump.alloc_slice_fill_iter([InterfaceBinop {
                symbol: "==",
                annotation: test_annotation(&bump),
                associativity: Associativity::None,
                precedence: Precedence(4),
                function: "eq",
            }]),
        };
        let interfaces = BTreeMap::from([("Basics", basics)]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn annotation_too_short() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f : a
            f x = x
        "#
        );
    }

    #[test]
    fn duplicate_field_in_update() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f r = { r | x = 1, x = 2 }
        "#
        );
    }

    #[test]
    fn duplicate_pattern_lambda_args() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f = \x x -> x
        "#
        );
    }

    #[test]
    fn duplicate_pattern_func_args() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f x x = x
        "#
        );
    }

    #[test]
    fn duplicate_pattern_destruct() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    (x, x) = (1, 2)
                in
                x
        "#
        );
    }

    #[test]
    fn let_mutual_recursion_values_error() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            f =
                let
                    a = b
                    b = a
                in
                a
        "#
        );
    }

    #[test]
    fn shadowing_toplevel() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            x = 1

            f =
                let
                    x = 2
                in
                x
        "#
        );
    }

    // === Expression warning tests ===

    #[test]
    fn unused_func_arg_warning() {
        assert_module_warning_snapshot!(
            r#"
            module Main exposing (..)

            f x = 42
        "#
        );
    }

    #[test]
    fn unused_case_branch_var() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Maybe(..))

            f x =
                case x of
                    Just y -> 1
                    Nothing -> 0
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Maybe", maybe_with_ctors_interface(&bump))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let (_, warnings) = parse_and_canonicalize_with_warnings(&bump, input, context)
            .expect("expected successful canonicalization");
        assert!(!warnings.is_empty(), "expected warnings but got none");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(warnings);
        });
    }

    // === Module-level tests ===

    #[test]
    fn duplicate_ctor_error() {
        // A record alias and union both produce a ctor named "Point"
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Point a = { x : a, y : a }

            type Shape a
                = Point a a
                | Circle a
        "#
        );
    }

    #[test]
    fn ctor_opts_unbox() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            type Wrap a
                = Wrap a
        "#
        );
    }

    #[test]
    fn multiple_unbound_union_vars() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type Foo
                = Bar a b
        "#
        );
    }

    #[test]
    fn alias_both_unused_and_unbound() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Bad a = b
        "#
        );
    }

    #[test]
    fn export_explicit_value() {
        assert_module_snapshot!(
            r#"
            module Main exposing (foo)

            foo = 42
        "#
        );
    }

    #[test]
    fn infix_right_associativity() {
        assert_module_snapshot!(
            r#"
            module Main exposing ((<|))

            infix right 0 (<|) = apL

            apL f x = f x
        "#
        );
    }

    #[test]
    fn infix_non_associativity() {
        assert_module_snapshot!(
            r#"
            module Main exposing ((==))

            infix non 4 (==) = eq

            eq a b = a
        "#
        );
    }

    #[test]
    fn local_record_alias_ctor() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            type alias Point a = { x : a, y : a }

            p = Point
        "#
        );
    }

    // === Interface tests: non-exported binop ===

    #[test]
    fn interface_non_exported_binop() {
        assert_interface_snapshot!(
            r#"
            module Main exposing (foo)

            infix left 6 (|>) = apR

            apR x f = f x

            foo = 42
        "#
        );
    }

    // === Import validation tests ===

    #[test]
    fn import_exposing_not_found_value() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo exposing (nonexistent)

            x = 1
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn import_exposing_not_found_type() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo exposing (Nonexistent)

            x = 1
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn import_exposing_not_found_op() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo exposing ((+))

            x = 1
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Foo", value_interface(&bump, "Foo", "bar"))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn import_ctor_by_name() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Just)

            x = 1
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Maybe", maybe_with_ctors_interface(&bump))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn import_open_alias() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Foo exposing (MyAlias(..))

            x = 1
        "#
        );
        let bump = Bump::new();
        let alias_type = bump.alloc(Located::at(Region::zero(), CanType::Var("a")));
        let foo = Interface {
            home: ModuleName {
                package: None,
                name: "Foo",
            },
            values: &[],
            aliases: bump.alloc_slice_fill_iter([InterfaceAlias {
                name: "MyAlias",
                parameters: bump.alloc_slice_fill_iter(["a"]),
                typ: alias_type,
                visibility: AliasVisibility::Public,
            }]),
            unions: &[],
            binops: &[],
        };
        let interfaces = BTreeMap::from([("Foo", foo)]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    // === collect_used_modules coverage ===

    #[test]
    fn collect_from_def_typed() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Maybe)

            f : Maybe a -> Maybe a
            f x = x
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Maybe", maybe_with_ctors_interface(&bump))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let (_, warnings) = parse_and_canonicalize_with_warnings(&bump, input, context)
            .expect("expected successful canonicalization");
        // Should not have an "unused import" warning for Maybe because
        // it is used in the type annotation.
        assert!(
            warnings.is_empty(),
            "expected no warnings but got: {warnings:?}"
        );
    }

    // === Import privacy (toPublicUnion / toPublicAlias) ===

    fn maybe_interface_with_visibility<'a>(
        bump: &'a Bump,
        visibility: UnionVisibility,
    ) -> Interface<'a> {
        let base = maybe_with_ctors_interface(bump);
        Interface {
            unions: bump.alloc_slice_fill_iter([InterfaceUnion {
                visibility,
                ..base.unions[0]
            }]),
            ..base
        }
    }

    #[test]
    fn closed_union_does_not_leak_ctors() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Maybe(..))

            x = Just
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([(
            "Maybe",
            maybe_interface_with_visibility(&bump, UnionVisibility::Closed),
        )]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn private_union_not_importable() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Maybe)

            x = 1
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([(
            "Maybe",
            maybe_interface_with_visibility(&bump, UnionVisibility::Private),
        )]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect_err("expected canonicalization error");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn aliased_import_exposes_ctors_unqualified() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe as M exposing (Maybe(..))

            x = Just
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Maybe", maybe_with_ctors_interface(&bump))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    // === Let destructuring ===

    #[test]
    fn let_destruct_self_reference_is_recursive() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            main = let (a, b) = (a, 1) in b
        "#
        );
    }

    #[test]
    fn let_destruct_ctor_pattern_binds_names() {
        let input = indoc!(
            r#"
            module Main exposing (..)

            import Maybe exposing (Maybe(..))

            f w = let (Just x) = w in x
        "#
        );
        let bump = Bump::new();
        let interfaces = BTreeMap::from([("Maybe", maybe_with_ctors_interface(&bump))]);
        let context = Context {
            package: None,
            interfaces: Some(&interfaces),
        };
        let result = parse_and_canonicalize(&bump, input, context)
            .expect("expected successful canonicalization");
        insta::with_settings!({
            description => format!("Code:\n\n{}", input),
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn let_destruct_list_pattern_binds_names() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            f w = let [a, b] = w in a
        "#
        );
    }

    // === Record extension variables in type declarations ===

    #[test]
    fn extensible_record_alias_allowed() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            type alias Extend a b = { a | items : b }
        "#
        );
    }

    #[test]
    fn unbound_record_ext_var_in_alias() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type alias Bad = { r | items : List r }
        "#
        );
    }

    #[test]
    fn unbound_record_ext_var_in_union() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing (..)

            type Foo = Bar { r | items : List r }
        "#
        );
    }

    #[test]
    fn let_destruct_local_ctor_pattern_binds_names() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            type Wrap a
                = Wrap a

            f w = let (Wrap x) = w in x
        "#
        );
    }

    #[test]
    fn binop_function_must_be_top_level() {
        assert_module_error_snapshot!(
            r#"
            module Main exposing ((|>))

            infix left 6 (|>) = missing
        "#
        );
    }

    // === Typed defs through parameterized aliases ===

    #[test]
    fn typed_def_through_parameterized_alias() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            type alias Transform a = a -> a

            f : Transform (List b)
            f x = x
        "#
        );
    }
}
