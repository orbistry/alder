use alder_ast::{
    Alias as CanAlias, AliasArgument, AliasType, Annotation, Associativity, Binop as CanBinop,
    Ctor as CanCtor, CtorOpts, Decls, Def, Export, Exports, FieldType, Module as CanModule,
    ModuleName, PackageName, Precedence, QualifiedName, Type as CanType, Union as CanUnion,
};
use alder_region::Located;
use bumpalo::Bump;

/// An exported value with its type, mirroring Elm's `I.Interface` values
/// map (`Map Name Can.Annotation`). Every entry comes from the type
/// solver, so interfaces can only be produced for solved modules.
#[derive(Clone, Copy, Debug)]
pub struct InterfaceValue<'a> {
    pub name: &'a str,
    pub annotation: &'a Annotation<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnionVisibility {
    Open,
    Closed,
    Private,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AliasVisibility {
    Public,
    Private,
}

#[derive(Clone, Copy, Debug)]
pub struct Interface<'a> {
    pub home: ModuleName<'a>,
    pub values: &'a [InterfaceValue<'a>],
    pub aliases: &'a [InterfaceAlias<'a>],
    pub unions: &'a [InterfaceUnion<'a>],
    pub binops: &'a [InterfaceBinop<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct InterfaceUnion<'a> {
    pub name: &'a str,
    pub parameters: &'a [&'a str],
    pub ctors: &'a [&'a CanCtor<'a>],
    pub alternatives: u16,
    pub options: CtorOpts,
    pub visibility: UnionVisibility,
}

#[derive(Clone, Copy, Debug)]
pub struct InterfaceAlias<'a> {
    pub name: &'a str,
    pub parameters: &'a [&'a str],
    pub typ: &'a Located<CanType<'a>>,
    pub visibility: AliasVisibility,
}

/// Mirrors Elm's `I.Binop op annotation associativity precedence`: the
/// annotation is the underlying function's, from the solver.
#[derive(Clone, Copy, Debug)]
pub struct InterfaceBinop<'a> {
    pub symbol: &'a str,
    pub annotation: &'a Annotation<'a>,
    pub associativity: Associativity,
    pub precedence: Precedence,
    pub function: &'a str,
}

/// Type annotations for every top-level value of a module, as produced by
/// the type solver (Elm's `Map Name Can.Annotation`).
pub type Annotations<'a> = std::collections::BTreeMap<&'a str, &'a Annotation<'a>>;

/// Mirrors Elm's `I.fromModule home canModule annotations`. Like Elm's
/// partial `annotations ! name` lookup, a top-level value or operator
/// function missing from `annotations` is a bug in the caller: interfaces
/// exist only for fully solved modules.
pub fn from_module<'a>(
    bump: &'a Bump,
    module: &CanModule<'a>,
    annotations: &Annotations<'a>,
) -> Interface<'a> {
    Interface {
        home: module.name,
        values: extract_values(bump, &module.exports, module.decls, annotations),
        unions: extract_unions(bump, &module.exports, module.unions),
        aliases: extract_aliases(bump, &module.exports, module.aliases),
        binops: extract_binops(bump, &module.exports, module.binops, annotations),
    }
}

impl<'a> InterfaceUnion<'a> {
    pub fn to_public(&self) -> Option<InterfaceUnion<'a>> {
        match self.visibility {
            UnionVisibility::Open => Some(*self),
            UnionVisibility::Closed => Some(InterfaceUnion {
                ctors: &[],
                alternatives: 0,
                ..*self
            }),
            UnionVisibility::Private => None,
        }
    }
}

impl<'a> InterfaceAlias<'a> {
    pub fn to_public(&self) -> Option<InterfaceAlias<'a>> {
        match self.visibility {
            AliasVisibility::Public => Some(*self),
            AliasVisibility::Private => None,
        }
    }
}

/// Mirrors Elm's `restrict exports annotations`: every top-level value's
/// annotation, filtered by the export list.
fn extract_values<'a>(
    bump: &'a Bump,
    exports: &Exports<'a>,
    decls: &Decls<'a>,
    annotations: &Annotations<'a>,
) -> &'a [InterfaceValue<'a>] {
    let mut names = Vec::new();
    collect_decl_names(decls, &mut names);

    let to_value = |name: &'a str| InterfaceValue {
        name,
        annotation: annotations
            .get(name)
            .copied()
            .expect("solver annotations cover every top-level value"),
    };

    match exports {
        Exports::Everything(_) => bump.alloc_slice_fill_iter(names.into_iter().map(to_value)),
        Exports::Explicit(exports) => bump.alloc_slice_fill_iter(
            names
                .into_iter()
                .filter(|name| is_exported_value(exports, name))
                .map(to_value)
                .collect::<Vec<_>>(),
        ),
    }
}

fn collect_decl_names<'a>(decls: &Decls<'a>, names: &mut Vec<&'a str>) {
    match decls {
        Decls::Declare { definition, next } => {
            names.push(def_name(definition));
            collect_decl_names(next, names);
        }
        Decls::DeclareRec {
            definition,
            following,
            next,
        } => {
            names.push(def_name(definition));
            for def in *following {
                names.push(def_name(def));
            }
            collect_decl_names(next, names);
        }
        Decls::Empty => {}
    }
}

fn def_name<'a>(def: &Def<'a>) -> &'a str {
    match def {
        Def::Def { name, .. } | Def::TypedDef { name, .. } => name.value,
    }
}

fn is_exported_value(exports: &[&Located<Export<'_>>], name: &str) -> bool {
    exports
        .iter()
        .any(|export| matches!(&export.value, Export::Value(n) if *n == name))
}

fn extract_unions<'a>(
    bump: &'a Bump,
    exports: &Exports<'a>,
    unions: &'a [&'a Located<CanUnion<'a>>],
) -> &'a [InterfaceUnion<'a>] {
    bump.alloc_slice_fill_iter(unions.iter().map(|union| {
        let name = union.value.name.value;
        InterfaceUnion {
            name,
            parameters: union.value.parameters,
            ctors: union.value.ctors,
            alternatives: union.value.alternatives,
            options: union.value.options,
            visibility: union_visibility(exports, name),
        }
    }))
}

fn extract_aliases<'a>(
    bump: &'a Bump,
    exports: &Exports<'a>,
    aliases: &'a [&'a Located<CanAlias<'a>>],
) -> &'a [InterfaceAlias<'a>] {
    bump.alloc_slice_fill_iter(aliases.iter().map(|alias| {
        let name = alias.value.name.value;
        InterfaceAlias {
            name,
            parameters: alias.value.parameters,
            typ: alias.value.typ,
            visibility: alias_visibility(exports, name),
        }
    }))
}

fn extract_binops<'a>(
    bump: &'a Bump,
    exports: &Exports<'a>,
    binops: &'a [&'a Located<CanBinop<'a>>],
    annotations: &Annotations<'a>,
) -> &'a [InterfaceBinop<'a>] {
    bump.alloc_slice_fill_iter(
        binops
            .iter()
            .filter_map(|binop| {
                if is_exported_binop(exports, binop.value.symbol) {
                    // Elm's `toOp` uses `annotations ! name`: the operator's
                    // function must be a solved top-level value.
                    let annotation = annotations
                        .get(binop.value.function)
                        .copied()
                        .expect("solver annotations cover every operator function");
                    Some(InterfaceBinop {
                        symbol: binop.value.symbol,
                        annotation,
                        associativity: binop.value.associativity,
                        precedence: binop.value.precedence,
                        function: binop.value.function,
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
    )
}

fn union_visibility(exports: &Exports<'_>, name: &str) -> UnionVisibility {
    match exports {
        Exports::Everything(_) => UnionVisibility::Open,
        Exports::Explicit(exports) => {
            for export in exports.iter() {
                match &export.value {
                    Export::UnionOpen(n) if *n == name => return UnionVisibility::Open,
                    Export::UnionClosed(n) if *n == name => return UnionVisibility::Closed,
                    _ => {}
                }
            }
            UnionVisibility::Private
        }
    }
}

fn alias_visibility(exports: &Exports<'_>, name: &str) -> AliasVisibility {
    match exports {
        Exports::Everything(_) => AliasVisibility::Public,
        Exports::Explicit(exports) => {
            if exports
                .iter()
                .any(|e| matches!(&e.value, Export::Alias(n) if *n == name))
            {
                AliasVisibility::Public
            } else {
                AliasVisibility::Private
            }
        }
    }
}

fn is_exported_binop(exports: &Exports<'_>, symbol: &str) -> bool {
    match exports {
        Exports::Everything(_) => true,
        Exports::Explicit(exports) => exports
            .iter()
            .any(|export| matches!(&export.value, Export::Binop(s) if *s == symbol)),
    }
}

// ---- deep copy helpers ----

fn copy_str<'d>(dst: &'d Bump, s: &str) -> &'d str {
    dst.alloc_str(s)
}

fn copy_module_name<'d>(dst: &'d Bump, m: &ModuleName<'_>) -> ModuleName<'d> {
    ModuleName {
        package: m.package.map(|p| PackageName {
            author: copy_str(dst, p.author),
            project: copy_str(dst, p.project),
        }),
        name: copy_str(dst, m.name),
    }
}

fn copy_qualified_name<'d>(dst: &'d Bump, q: &QualifiedName<'_>) -> QualifiedName<'d> {
    QualifiedName {
        home: copy_module_name(dst, &q.home),
        name: copy_str(dst, q.name),
    }
}

fn copy_located_type<'d>(dst: &'d Bump, lt: &Located<CanType<'_>>) -> &'d Located<CanType<'d>> {
    dst.alloc(Located::at(lt.region, copy_type(dst, &lt.value)))
}

fn copy_annotation<'d>(dst: &'d Bump, a: &Annotation<'_>) -> &'d Annotation<'d> {
    dst.alloc(Annotation {
        free_vars: dst.alloc_slice_fill_iter(a.free_vars.iter().map(|v| copy_str(dst, v))),
        typ: copy_located_type(dst, a.typ),
    })
}

fn copy_type<'d>(dst: &'d Bump, t: &CanType<'_>) -> CanType<'d> {
    match t {
        CanType::Lambda { from, to } => CanType::Lambda {
            from: copy_located_type(dst, from),
            to: copy_located_type(dst, to),
        },
        CanType::Var(name) => CanType::Var(copy_str(dst, name)),
        CanType::Named { reference, args } => CanType::Named {
            reference: copy_qualified_name(dst, reference),
            args: dst.alloc_slice_fill_iter(args.iter().map(|a| copy_located_type(dst, a))),
        },
        CanType::Record { fields, ext } => CanType::Record {
            fields: dst.alloc_slice_fill_iter(fields.iter().map(|f| FieldType {
                index: f.index,
                field: copy_str(dst, f.field),
                typ: copy_located_type(dst, f.typ),
            })),
            ext: ext.map(|e| copy_str(dst, e)),
        },
        CanType::Unit => CanType::Unit,
        CanType::Tuple {
            first,
            second,
            rest,
        } => CanType::Tuple {
            first: copy_located_type(dst, first),
            second: copy_located_type(dst, second),
            rest: dst.alloc_slice_fill_iter(rest.iter().map(|r| copy_located_type(dst, r))),
        },
        CanType::Alias {
            reference,
            arguments,
            target,
        } => CanType::Alias {
            reference: copy_qualified_name(dst, reference),
            arguments: dst.alloc_slice_fill_iter(arguments.iter().map(|a| AliasArgument {
                name: copy_str(dst, a.name),
                typ: copy_located_type(dst, a.typ),
            })),
            target: match target {
                AliasType::Open(t) => AliasType::Open(copy_located_type(dst, t)),
                AliasType::Filled(t) => AliasType::Filled(copy_located_type(dst, t)),
            },
        },
    }
}

fn copy_ctor<'d>(dst: &'d Bump, c: &CanCtor<'_>) -> &'d CanCtor<'d> {
    dst.alloc(CanCtor {
        name: copy_str(dst, c.name),
        index: c.index,
        arity: c.arity,
        arguments: dst.alloc_slice_fill_iter(c.arguments.iter().map(|a| copy_located_type(dst, a))),
    })
}

/// Deep-copy an `Interface` into a different bump arena.
pub fn deep_copy<'d>(dst: &'d Bump, src: &Interface<'_>) -> Interface<'d> {
    Interface {
        home: copy_module_name(dst, &src.home),
        values: dst.alloc_slice_fill_iter(src.values.iter().map(|v| InterfaceValue {
            name: copy_str(dst, v.name),
            annotation: copy_annotation(dst, v.annotation),
        })),
        aliases: dst.alloc_slice_fill_iter(src.aliases.iter().map(|a| InterfaceAlias {
            name: copy_str(dst, a.name),
            parameters: dst.alloc_slice_fill_iter(a.parameters.iter().map(|p| copy_str(dst, p))),
            typ: copy_located_type(dst, a.typ),
            visibility: a.visibility,
        })),
        unions: dst.alloc_slice_fill_iter(src.unions.iter().map(|u| InterfaceUnion {
            name: copy_str(dst, u.name),
            parameters: dst.alloc_slice_fill_iter(u.parameters.iter().map(|p| copy_str(dst, p))),
            ctors: dst.alloc_slice_fill_iter(u.ctors.iter().map(|c| copy_ctor(dst, c))),
            alternatives: u.alternatives,
            options: u.options,
            visibility: u.visibility,
        })),
        binops: dst.alloc_slice_fill_iter(src.binops.iter().map(|b| InterfaceBinop {
            symbol: copy_str(dst, b.symbol),
            annotation: copy_annotation(dst, b.annotation),
            associativity: b.associativity,
            precedence: b.precedence,
            function: copy_str(dst, b.function),
        })),
    }
}
