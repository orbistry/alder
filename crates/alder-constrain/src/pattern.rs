//! Port of Elm's `Type.Constrain.Pattern`: turn a canonical pattern into
//! binding headers plus the constraints its structure implies.

use std::collections::BTreeMap;

use alder_ast::{Pattern as CanPattern, PatternCtor};
use alder_region::{Located, Region};
use bumpalo::Bump;

use crate::error::{PCategory, PContext, PExpected};
use crate::instantiate;
use crate::type_::{self, Constraint, Type, mk_flex_var, name_to_flex};
use crate::union_find::{UnionFind, Variable};

/// Elm's `Pattern.State`. Constraints are stored in reverse order so that
/// adding one is O(1); callers reverse when building the final `CLet`.
pub struct State<'a> {
    pub headers: Header<'a>,
    pub vars: Vec<Variable>,
    pub rev_cons: Vec<Constraint<'a>>,
}

pub type Header<'a> = BTreeMap<&'a str, Located<&'a Type<'a>>>;

pub fn empty_state<'a>() -> State<'a> {
    State {
        headers: BTreeMap::new(),
        vars: Vec::new(),
        rev_cons: Vec::new(),
    }
}

pub fn add<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    pattern: &Located<CanPattern<'a>>,
    expectation: PExpected<'a, &'a Type<'a>>,
    state: State<'a>,
) -> State<'a> {
    let region = pattern.region;
    match &pattern.value {
        CanPattern::Anything => state,

        CanPattern::Var(name) => add_to_headers(region, name, expectation, state),

        CanPattern::Alias {
            pattern: real_pattern,
            name,
        } => {
            let state = add_to_headers(region, name, expectation, state);
            add(bump, uf, real_pattern, expectation, state)
        }

        CanPattern::Unit => {
            let mut state = state;
            let unit_con = Constraint::Pattern(
                region,
                PCategory::Unit,
                bump.alloc(Type::UnitN),
                expectation,
            );
            state.rev_cons.push(unit_con);
            state
        }

        CanPattern::Tuple {
            first,
            second,
            rest,
        } => add_tuple(
            bump,
            uf,
            region,
            first,
            second,
            rest.first().copied(),
            expectation,
            state,
        ),

        CanPattern::Constructor(ctor) => add_ctor(bump, uf, region, ctor, expectation, state),

        CanPattern::List(patterns) => {
            let entry_var = mk_flex_var(uf);
            let entry_type: &'a Type<'a> = bump.alloc(Type::VarN(entry_var));
            let list_type: &'a Type<'a> = bump.alloc(Type::AppN {
                home: type_::list_home(),
                name: "List",
                args: bump.alloc_slice_copy(&[entry_type]),
            });

            let mut state =
                patterns
                    .iter()
                    .enumerate()
                    .fold(state, |state, (index, entry_pattern)| {
                        let expectation =
                            PExpected::FromContext(region, PContext::ListEntry(index), entry_type);
                        add(bump, uf, entry_pattern, expectation, state)
                    });

            let list_con = Constraint::Pattern(region, PCategory::List, list_type, expectation);
            state.vars.push(entry_var);
            state.rev_cons.push(list_con);
            state
        }

        CanPattern::Cons { head, tail } => {
            let entry_var = mk_flex_var(uf);
            let entry_type: &'a Type<'a> = bump.alloc(Type::VarN(entry_var));
            let list_type: &'a Type<'a> = bump.alloc(Type::AppN {
                home: type_::list_home(),
                name: "List",
                args: bump.alloc_slice_copy(&[entry_type]),
            });

            let head_expectation = PExpected::NoExpectation(entry_type);
            let tail_expectation = PExpected::FromContext(region, PContext::Tail, list_type);

            let state = add(bump, uf, tail, tail_expectation, state);
            let mut state = add(bump, uf, head, head_expectation, state);

            let list_con = Constraint::Pattern(region, PCategory::List, list_type, expectation);
            state.vars.push(entry_var);
            state.rev_cons.push(list_con);
            state
        }

        CanPattern::Record(fields) => {
            let ext_var = mk_flex_var(uf);
            let ext_type: &'a Type<'a> = bump.alloc(Type::VarN(ext_var));

            let field_vars: Vec<(&'a str, Variable)> = fields
                .iter()
                .map(|field| (*field, mk_flex_var(uf)))
                .collect();
            let field_types: BTreeMap<&'a str, &'a Type<'a>> = field_vars
                .iter()
                .map(|(field, var)| (*field, &*bump.alloc(Type::VarN(*var))))
                .collect();
            let record_type: &'a Type<'a> = bump.alloc(Type::RecordN {
                fields: bump
                    .alloc_slice_fill_iter(field_types.iter().map(|(field, typ)| (*field, *typ))),
                ext: ext_type,
            });

            let mut state = state;
            let record_con =
                Constraint::Pattern(region, PCategory::Record, record_type, expectation);
            // Elm: `Map.union headers (Map.map (A.At region) fieldTypes)`
            // is left-biased, so existing headers win.
            for (field, typ) in &field_types {
                state
                    .headers
                    .entry(field)
                    .or_insert_with(|| Located::at(region, *typ));
            }
            state.vars.extend(field_vars.iter().map(|(_, var)| *var));
            state.vars.push(ext_var);
            state.rev_cons.push(record_con);
            state
        }

        CanPattern::Int(_) => {
            let mut state = state;
            let int_con = Constraint::Pattern(
                region,
                PCategory::Int,
                bump.alloc(type_::int()),
                expectation,
            );
            state.rev_cons.push(int_con);
            state
        }

        CanPattern::Str(_) => {
            let mut state = state;
            let str_con = Constraint::Pattern(
                region,
                PCategory::Str,
                bump.alloc(type_::string()),
                expectation,
            );
            state.rev_cons.push(str_con);
            state
        }

        CanPattern::Bool { .. } => {
            let mut state = state;
            let bool_con = Constraint::Pattern(
                region,
                PCategory::Bool,
                bump.alloc(type_::bool()),
                expectation,
            );
            state.rev_cons.push(bool_con);
            state
        }
    }
}

// STATE HELPERS

fn add_to_headers<'a>(
    region: Region,
    name: &'a str,
    expectation: PExpected<'a, &'a Type<'a>>,
    mut state: State<'a>,
) -> State<'a> {
    let tipe = get_type(expectation);
    state.headers.insert(name, Located::at(region, tipe));
    state
}

fn get_type<'a>(expectation: PExpected<'a, &'a Type<'a>>) -> &'a Type<'a> {
    match expectation {
        PExpected::NoExpectation(tipe) => tipe,
        PExpected::FromContext(_, _, tipe) => tipe,
    }
}

// CONSTRAIN TUPLE

#[allow(clippy::too_many_arguments)]
fn add_tuple<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    region: Region,
    a: &Located<CanPattern<'a>>,
    b: &Located<CanPattern<'a>>,
    maybe_c: Option<&Located<CanPattern<'a>>>,
    expectation: PExpected<'a, &'a Type<'a>>,
    state: State<'a>,
) -> State<'a> {
    let a_var = mk_flex_var(uf);
    let b_var = mk_flex_var(uf);
    let a_type: &'a Type<'a> = bump.alloc(Type::VarN(a_var));
    let b_type: &'a Type<'a> = bump.alloc(Type::VarN(b_var));

    match maybe_c {
        None => {
            let state = simple_add(bump, uf, a, a_type, state);
            let mut state = simple_add(bump, uf, b, b_type, state);

            let tuple_con = Constraint::Pattern(
                region,
                PCategory::Tuple,
                bump.alloc(Type::TupleN(a_type, b_type, None)),
                expectation,
            );

            state.vars.push(a_var);
            state.vars.push(b_var);
            state.rev_cons.push(tuple_con);
            state
        }

        Some(c) => {
            let c_var = mk_flex_var(uf);
            let c_type: &'a Type<'a> = bump.alloc(Type::VarN(c_var));

            let state = simple_add(bump, uf, a, a_type, state);
            let state = simple_add(bump, uf, b, b_type, state);
            let mut state = simple_add(bump, uf, c, c_type, state);

            let tuple_con = Constraint::Pattern(
                region,
                PCategory::Tuple,
                bump.alloc(Type::TupleN(a_type, b_type, Some(c_type))),
                expectation,
            );

            state.vars.push(a_var);
            state.vars.push(b_var);
            state.vars.push(c_var);
            state.rev_cons.push(tuple_con);
            state
        }
    }
}

fn simple_add<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    pattern: &Located<CanPattern<'a>>,
    pattern_type: &'a Type<'a>,
    state: State<'a>,
) -> State<'a> {
    add(
        bump,
        uf,
        pattern,
        PExpected::NoExpectation(pattern_type),
        state,
    )
}

// CONSTRAIN CONSTRUCTORS

fn add_ctor<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    region: Region,
    ctor: &PatternCtor<'a>,
    expectation: PExpected<'a, &'a Type<'a>>,
    state: State<'a>,
) -> State<'a> {
    let home = ctor.reference.home;
    let type_name = ctor.reference.union;
    let ctor_name = ctor.reference.name;

    let var_pairs: Vec<(&'a str, Variable)> = ctor
        .union
        .parameters
        .iter()
        .map(|var| (*var, name_to_flex(uf, var)))
        .collect();
    let type_pairs: Vec<(&'a str, &'a Type<'a>)> = var_pairs
        .iter()
        .map(|(name, var)| (*name, &*bump.alloc(Type::VarN(*var))))
        .collect();
    let free_var_dict: instantiate::FreeVars<'a> = type_pairs.iter().copied().collect();

    let mut state = ctor.arguments.iter().fold(state, |state, arg| {
        let tipe = instantiate::from_src_type(bump, &free_var_dict, arg.typ);
        let arg_expectation = PExpected::FromContext(
            region,
            PContext::CtorArg(ctor_name, arg.index as usize),
            tipe,
        );
        add(bump, uf, arg.pattern, arg_expectation, state)
    });

    let ctor_type: &'a Type<'a> = bump.alloc(Type::AppN {
        home,
        name: type_name,
        args: bump.alloc_slice_fill_iter(type_pairs.iter().map(|(_, typ)| *typ)),
    });
    let ctor_con = Constraint::Pattern(region, PCategory::Ctor(ctor_name), ctor_type, expectation);

    state.vars.extend(var_pairs.iter().map(|(_, var)| *var));
    state.rev_cons.push(ctor_con);
    state
}
