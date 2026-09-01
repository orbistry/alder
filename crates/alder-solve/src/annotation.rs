//! Port of the second half of Elm's `Type.Type`: reading a solved variable
//! back out as a canonical annotation (`toAnnotation`) or as an error type
//! (`toErrorType`), inventing pretty names for anonymous variables.

use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{AliasArgument, AliasType, Annotation, FieldType, QualifiedName, Type as CanType};
use alder_constrain::error_type::{self, ErrorType, Extension};
use alder_constrain::type_::{OCCURS_MARK, SuperType};
use alder_constrain::{Content, FlatType, Super, UnionFind, Variable};
use alder_region::Located;
use bumpalo::Bump;

// TO TYPE ANNOTATION

pub fn to_annotation<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    variable: Variable,
) -> &'a Annotation<'a> {
    let mut seen = BTreeSet::new();
    let user_names = get_var_names(bump, uf, &mut seen, variable, BTreeMap::new());
    let mut state = NameState::new(&user_names);
    let tipe = variable_to_can_type(bump, uf, &mut state, variable);
    bump.alloc(Annotation {
        free_vars: bump.alloc_slice_fill_iter(state.taken.keys().copied()),
        typ: tipe,
    })
}

fn variable_to_can_type<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    state: &mut NameState<'a>,
    variable: Variable,
) -> &'a Located<CanType<'a>> {
    let content = uf.get(variable).content.clone();
    match content {
        Content::Structure(term) => term_to_can_type(bump, uf, state, term),

        Content::FlexVar(maybe_name) => {
            let name = match maybe_name {
                Some(name) => name,
                None => {
                    let name = state.fresh_var_name(bump);
                    uf.modify(variable, |desc| desc.content = Content::FlexVar(Some(name)));
                    name
                }
            };
            bump.alloc(Located::at_zero(CanType::Var(name)))
        }

        Content::FlexSuper(super_type, maybe_name) => {
            let name = match maybe_name {
                Some(name) => name,
                None => {
                    let name = state.fresh_super_name(bump, super_type);
                    uf.modify(variable, |desc| {
                        desc.content = Content::FlexSuper(super_type, Some(name));
                    });
                    name
                }
            };
            bump.alloc(Located::at_zero(CanType::Var(name)))
        }

        Content::RigidVar(name) | Content::RigidSuper(_, name) => {
            bump.alloc(Located::at_zero(CanType::Var(name)))
        }

        Content::Alias {
            home,
            name,
            args,
            real,
        } => {
            let can_args =
                bump.alloc_slice_fill_iter(args.iter().map(|(arg_name, arg_var)| AliasArgument {
                    name: arg_name,
                    typ: variable_to_can_type(bump, uf, state, *arg_var),
                }));
            let can_type = variable_to_can_type(bump, uf, state, real);
            bump.alloc(Located::at_zero(CanType::Alias {
                reference: QualifiedName { home, name },
                arguments: can_args,
                target: AliasType::Filled(can_type),
            }))
        }

        Content::Error => panic!("cannot handle Error types in variable_to_can_type"),
    }
}

fn term_to_can_type<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    state: &mut NameState<'a>,
    term: FlatType<'a>,
) -> &'a Located<CanType<'a>> {
    match term {
        FlatType::App1(home, name, args) => bump.alloc(Located::at_zero(CanType::Named {
            reference: QualifiedName { home, name },
            args: bump.alloc_slice_fill_iter(
                args.iter()
                    .map(|arg| variable_to_can_type(bump, uf, state, *arg)),
            ),
        })),

        FlatType::Fun1(a, b) => bump.alloc(Located::at_zero(CanType::Lambda {
            from: variable_to_can_type(bump, uf, state, a),
            to: variable_to_can_type(bump, uf, state, b),
        })),

        FlatType::EmptyRecord1 => bump.alloc(Located::at_zero(CanType::Record {
            fields: &[],
            ext: None,
        })),

        FlatType::Record1(fields, extension) => {
            let can_fields: Vec<FieldType<'a>> = fields
                .iter()
                .map(|(field, field_var)| FieldType {
                    index: 0,
                    field,
                    typ: variable_to_can_type(bump, uf, state, *field_var),
                })
                .collect();
            let can_ext = alder_can::types::iterated_dealias(
                bump,
                variable_to_can_type(bump, uf, state, extension),
            );
            match &can_ext.value {
                CanType::Record {
                    fields: sub_fields,
                    ext: sub_ext,
                } => bump.alloc(Located::at_zero(CanType::Record {
                    fields: union_fields(bump, sub_fields, &can_fields),
                    ext: *sub_ext,
                })),

                CanType::Var(ext_name) => bump.alloc(Located::at_zero(CanType::Record {
                    fields: bump.alloc_slice_fill_iter(can_fields),
                    ext: Some(ext_name),
                })),

                _ => panic!("used to_annotation on a type that is not well-formed"),
            }
        }

        FlatType::Unit1 => bump.alloc(Located::at_zero(CanType::Unit)),

        FlatType::Tuple1(a, b, maybe_c) => {
            let first = variable_to_can_type(bump, uf, state, a);
            let second = variable_to_can_type(bump, uf, state, b);
            let rest: &'a [_] = match maybe_c {
                None => &[],
                Some(c) => bump.alloc_slice_copy(&[variable_to_can_type(bump, uf, state, c)]),
            };
            bump.alloc(Located::at_zero(CanType::Tuple {
                first,
                second,
                rest,
            }))
        }
    }
}

/// Elm's `Map.union subFields canFields`: left-biased merge of two
/// name-sorted field slices.
fn union_fields<'a>(
    bump: &'a Bump,
    sub_fields: &[FieldType<'a>],
    can_fields: &[FieldType<'a>],
) -> &'a [FieldType<'a>] {
    let mut merged: BTreeMap<&'a str, FieldType<'a>> = BTreeMap::new();
    for field in can_fields {
        merged.insert(field.field, *field);
    }
    for field in sub_fields {
        merged.insert(field.field, *field);
    }
    bump.alloc_slice_fill_iter(merged.into_values())
}

// TO ERROR TYPE

pub fn to_error_type<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    variable: Variable,
) -> &'a ErrorType<'a> {
    let mut seen = BTreeSet::new();
    let user_names = get_var_names(bump, uf, &mut seen, variable, BTreeMap::new());
    let mut state = NameState::new(&user_names);
    variable_to_error_type(bump, uf, &mut state, variable)
}

fn variable_to_error_type<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    state: &mut NameState<'a>,
    variable: Variable,
) -> &'a ErrorType<'a> {
    let mark = uf.get(variable).mark;
    if mark == OCCURS_MARK {
        bump.alloc(ErrorType::Infinite)
    } else {
        uf.modify(variable, |desc| desc.mark = OCCURS_MARK);
        let content = uf.get(variable).content.clone();
        let err_type = content_to_error_type(bump, uf, state, variable, content);
        uf.modify(variable, |desc| desc.mark = mark);
        err_type
    }
}

fn content_to_error_type<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    state: &mut NameState<'a>,
    variable: Variable,
    content: Content<'a>,
) -> &'a ErrorType<'a> {
    match content {
        Content::Structure(term) => term_to_error_type(bump, uf, state, term),

        Content::FlexVar(maybe_name) => {
            let name = match maybe_name {
                Some(name) => name,
                None => {
                    let name = state.fresh_var_name(bump);
                    uf.modify(variable, |desc| desc.content = Content::FlexVar(Some(name)));
                    name
                }
            };
            bump.alloc(ErrorType::FlexVar(name))
        }

        Content::FlexSuper(super_type, maybe_name) => {
            let name = match maybe_name {
                Some(name) => name,
                None => {
                    let name = state.fresh_super_name(bump, super_type);
                    uf.modify(variable, |desc| {
                        desc.content = Content::FlexSuper(super_type, Some(name));
                    });
                    name
                }
            };
            bump.alloc(ErrorType::FlexSuper(super_to_super(super_type), name))
        }

        Content::RigidVar(name) => bump.alloc(ErrorType::RigidVar(name)),

        Content::RigidSuper(super_type, name) => {
            bump.alloc(ErrorType::RigidSuper(super_to_super(super_type), name))
        }

        Content::Alias {
            home,
            name,
            args,
            real,
        } => {
            let err_args = bump.alloc_slice_fill_iter(args.iter().map(|(arg_name, arg_var)| {
                (*arg_name, variable_to_error_type(bump, uf, state, *arg_var))
            }));
            let err_type = variable_to_error_type(bump, uf, state, real);
            bump.alloc(ErrorType::Alias {
                home,
                name,
                args: err_args,
                real: err_type,
            })
        }

        Content::Error => bump.alloc(ErrorType::Error),
    }
}

fn super_to_super(super_type: SuperType) -> Super {
    match super_type {
        SuperType::Number => Super::Number,
        SuperType::Comparable => Super::Comparable,
        SuperType::Appendable => Super::Appendable,
        SuperType::CompAppend => Super::CompAppend,
    }
}

fn term_to_error_type<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    state: &mut NameState<'a>,
    term: FlatType<'a>,
) -> &'a ErrorType<'a> {
    match term {
        FlatType::App1(home, name, args) => bump.alloc(ErrorType::Type {
            home,
            name,
            args: bump.alloc_slice_fill_iter(
                args.iter()
                    .map(|arg| variable_to_error_type(bump, uf, state, *arg)),
            ),
        }),

        FlatType::Fun1(a, b) => {
            let arg = variable_to_error_type(bump, uf, state, a);
            let result = variable_to_error_type(bump, uf, state, b);
            match result {
                ErrorType::Lambda(arg1, arg2, others) => {
                    let mut rest = vec![*arg2];
                    rest.extend(others.iter().copied());
                    bump.alloc(ErrorType::Lambda(
                        arg,
                        arg1,
                        bump.alloc_slice_fill_iter(rest),
                    ))
                }
                _ => bump.alloc(ErrorType::Lambda(arg, result, &[])),
            }
        }

        FlatType::EmptyRecord1 => bump.alloc(ErrorType::Record {
            fields: &[],
            ext: Extension::Closed,
        }),

        FlatType::Record1(fields, extension) => {
            let err_fields: Vec<(&'a str, &'a ErrorType<'a>)> = fields
                .iter()
                .map(|(field, field_var)| {
                    (*field, variable_to_error_type(bump, uf, state, *field_var))
                })
                .collect();
            let err_ext =
                error_type::iterated_dealias(variable_to_error_type(bump, uf, state, extension));
            match err_ext {
                ErrorType::Record {
                    fields: sub_fields,
                    ext: sub_ext,
                } => bump.alloc(ErrorType::Record {
                    fields: union_error_fields(bump, sub_fields, &err_fields),
                    ext: *sub_ext,
                }),

                ErrorType::FlexVar(ext) => bump.alloc(ErrorType::Record {
                    fields: bump.alloc_slice_fill_iter(err_fields),
                    ext: Extension::FlexOpen(ext),
                }),

                ErrorType::RigidVar(ext) => bump.alloc(ErrorType::Record {
                    fields: bump.alloc_slice_fill_iter(err_fields),
                    ext: Extension::RigidOpen(ext),
                }),

                _ => panic!("used to_error_type on a type that is not well-formed"),
            }
        }

        FlatType::Unit1 => bump.alloc(ErrorType::Unit),

        FlatType::Tuple1(a, b, maybe_c) => {
            let first = variable_to_error_type(bump, uf, state, a);
            let second = variable_to_error_type(bump, uf, state, b);
            let third = maybe_c.map(|c| variable_to_error_type(bump, uf, state, c));
            bump.alloc(ErrorType::Tuple(first, second, third))
        }
    }
}

/// Elm's `Map.union subFields errFields`: left-biased merge of two
/// name-sorted field slices.
fn union_error_fields<'a>(
    bump: &'a Bump,
    sub_fields: &[(&'a str, &'a ErrorType<'a>)],
    err_fields: &[(&'a str, &'a ErrorType<'a>)],
) -> &'a [(&'a str, &'a ErrorType<'a>)] {
    let mut merged: BTreeMap<&'a str, &'a ErrorType<'a>> = BTreeMap::new();
    for (field, tipe) in err_fields {
        merged.insert(field, tipe);
    }
    for (field, tipe) in sub_fields {
        merged.insert(field, tipe);
    }
    bump.alloc_slice_fill_iter(merged)
}

// MANAGE FRESH VARIABLE NAMES

struct NameState<'a> {
    taken: BTreeMap<&'a str, ()>,
    normals: usize,
    numbers: usize,
    comparables: usize,
    appendables: usize,
    comp_appends: usize,
}

impl<'a> NameState<'a> {
    fn new(taken: &BTreeMap<&'a str, Variable>) -> NameState<'a> {
        NameState {
            taken: taken.keys().map(|name| (*name, ())).collect(),
            normals: 0,
            numbers: 0,
            comparables: 0,
            appendables: 0,
            comp_appends: 0,
        }
    }

    fn fresh_var_name(&mut self, bump: &'a Bump) -> &'a str {
        let mut index = self.normals;
        loop {
            let name = from_type_variable_scheme(bump, index);
            if !self.taken.contains_key(name) {
                self.taken.insert(name, ());
                self.normals = index + 1;
                return name;
            }
            index += 1;
        }
    }

    fn fresh_super_name(&mut self, bump: &'a Bump, super_type: SuperType) -> &'a str {
        let (prefix, counter): (&'static str, &mut usize) = match super_type {
            SuperType::Number => ("number", &mut self.numbers),
            SuperType::Comparable => ("comparable", &mut self.comparables),
            SuperType::Appendable => ("appendable", &mut self.appendables),
            SuperType::CompAppend => ("compappend", &mut self.comp_appends),
        };
        let mut index = *counter;
        loop {
            let name = from_type_variable(bump, prefix, index);
            if !self.taken.contains_key(name) {
                self.taken.insert(name, ());
                *counter = index + 1;
                return name;
            }
            index += 1;
        }
    }
}

// FRESH VAR NAMES

/// Elm's `Name.fromTypeVariableScheme`: `a`..`z`, then `a1`, `b1`, ...
fn from_type_variable_scheme(bump: &Bump, scheme: usize) -> &str {
    let letter = (b'a' + (scheme % 26) as u8) as char;
    if scheme < 26 {
        bump.alloc_str(&letter.to_string())
    } else {
        let extra = scheme / 26;
        bump.alloc_str(&format!("{letter}{extra}"))
    }
}

/// Elm's `Name.fromTypeVariable`: append the index, separated by `_` when
/// the name already ends in a digit.
fn from_type_variable<'a>(bump: &'a Bump, name: &'a str, index: usize) -> &'a str {
    if index == 0 {
        name
    } else if name.ends_with(|c: char| c.is_ascii_digit()) {
        bump.alloc_str(&format!("{name}_{index}"))
    } else {
        bump.alloc_str(&format!("{name}{index}"))
    }
}

// GET ALL VARIABLE NAMES

// DEVIATION: Elm tracks visited variables by stamping `getVarNamesMark`
// into their descriptors, and the stamps persist across the `toAnnotation`
// calls of one solver run. Top-level values that share generalized
// variables (e.g. unannotated mutually recursive functions) then get
// annotations whose `Forall` is missing the shared variables, and Elm
// 0.19.1 crashes with "Map.!: given key is not an element in the map" when
// another module instantiates such an export. Alder tracks visits with a
// per-call `seen` set (keyed by representative) instead, so every
// annotation lists its actual free variables.
fn get_var_names<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    seen: &mut BTreeSet<Variable>,
    var: Variable,
    taken_names: BTreeMap<&'a str, Variable>,
) -> BTreeMap<&'a str, Variable> {
    if !seen.insert(uf.find(var)) {
        return taken_names;
    }
    let content = uf.get(var).content.clone();

    match content {
        Content::Error => taken_names,

        Content::FlexVar(maybe_name) => match maybe_name {
            None => taken_names,
            Some(name) => add_name(
                bump,
                uf,
                name,
                var,
                |n| Content::FlexVar(Some(n)),
                taken_names,
            ),
        },

        Content::FlexSuper(super_type, maybe_name) => match maybe_name {
            None => taken_names,
            Some(name) => add_name(
                bump,
                uf,
                name,
                var,
                move |n| Content::FlexSuper(super_type, Some(n)),
                taken_names,
            ),
        },

        Content::RigidVar(name) => add_name(bump, uf, name, var, Content::RigidVar, taken_names),

        Content::RigidSuper(super_type, name) => add_name(
            bump,
            uf,
            name,
            var,
            move |n| Content::RigidSuper(super_type, n),
            taken_names,
        ),

        // Elm folds with `foldrM`, so children are visited right-to-left.
        Content::Alias { args, .. } => args.iter().rev().fold(taken_names, |taken, (_, arg)| {
            get_var_names(bump, uf, seen, *arg, taken)
        }),

        Content::Structure(flat_type) => match flat_type {
            FlatType::App1(_, _, args) => args.iter().rev().fold(taken_names, |taken, arg| {
                get_var_names(bump, uf, seen, *arg, taken)
            }),

            FlatType::Fun1(arg, body) => {
                let taken = get_var_names(bump, uf, seen, body, taken_names);
                get_var_names(bump, uf, seen, arg, taken)
            }

            FlatType::EmptyRecord1 => taken_names,

            FlatType::Record1(fields, extension) => {
                let taken = fields.values().rev().fold(taken_names, |taken, field| {
                    get_var_names(bump, uf, seen, *field, taken)
                });
                get_var_names(bump, uf, seen, extension, taken)
            }

            FlatType::Unit1 => taken_names,

            FlatType::Tuple1(a, b, None) => {
                let taken = get_var_names(bump, uf, seen, b, taken_names);
                get_var_names(bump, uf, seen, a, taken)
            }

            FlatType::Tuple1(a, b, Some(c)) => {
                let taken = get_var_names(bump, uf, seen, c, taken_names);
                let taken = get_var_names(bump, uf, seen, b, taken);
                get_var_names(bump, uf, seen, a, taken)
            }
        },
    }
}

// REGISTER NAME / RENAME DUPLICATES

fn add_name<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    given_name: &'a str,
    var: Variable,
    make_content: impl Fn(&'a str) -> Content<'a>,
    mut taken_names: BTreeMap<&'a str, Variable>,
) -> BTreeMap<&'a str, Variable> {
    let mut index = 0;
    loop {
        let indexed_name = from_type_variable(bump, given_name, index);
        match taken_names.get(indexed_name) {
            None => {
                if indexed_name != given_name {
                    let content = make_content(indexed_name);
                    uf.modify(var, |desc| desc.content = content);
                }
                taken_names.insert(indexed_name, var);
                return taken_names;
            }
            Some(other_var) => {
                if uf.equivalent(var, *other_var) {
                    return taken_names;
                }
                index += 1;
            }
        }
    }
}
