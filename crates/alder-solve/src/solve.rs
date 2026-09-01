//! Port of Elm's `Type.Solve`: solve a constraint tree with rank-based
//! generalization, producing an annotation per top-level value.

use std::collections::BTreeMap;

use alder_ast::Type as CanType;
use alder_can::Annotations;
use alder_constrain::error::{Category, Error, Expected, PExpected};
use alder_constrain::type_::{
    self, Constraint, Content, Descriptor, FlatType, Mark, NO_MARK, NO_RANK, OUTERMOST_RANK, Type,
};
use alder_constrain::{UnionFind, Variable};
use alder_region::Located;
use bumpalo::Bump;

use crate::annotation::{to_annotation, to_error_type};
use crate::occurs;
use crate::unify;

// RUN SOLVER

pub fn run<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    constraint: &Constraint<'a>,
) -> Result<Annotations<'a>, Vec<Error<'a>>> {
    let mut solver = Solver {
        bump,
        pools: vec![Vec::new(); 8],
    };

    let state = solver.solve(
        uf,
        &Env::new(),
        OUTERMOST_RANK,
        State {
            env: Env::new(),
            mark: NO_MARK.next(),
            errors: Vec::new(),
        },
        constraint,
    );

    if state.errors.is_empty() {
        Ok(state
            .env
            .iter()
            .map(|(name, var)| (*name, to_annotation(bump, uf, *var)))
            .collect())
    } else {
        // Elm accumulates errors by prepending; match its final order.
        let mut errors = state.errors;
        errors.reverse();
        Err(errors)
    }
}

// SOLVER

type Env<'a> = BTreeMap<&'a str, Variable>;

struct State<'a> {
    env: Env<'a>,
    mark: Mark,
    errors: Vec<Error<'a>>,
}

fn add_error<'a>(mut state: State<'a>, error: Error<'a>) -> State<'a> {
    state.errors.push(error);
    state
}

struct Solver<'a> {
    bump: &'a Bump,
    pools: Vec<Vec<Variable>>,
}

impl<'a> Solver<'a> {
    fn solve(
        &mut self,
        uf: &mut UnionFind<'a>,
        env: &Env<'a>,
        rank: usize,
        state: State<'a>,
        constraint: &Constraint<'a>,
    ) -> State<'a> {
        match constraint {
            Constraint::True => state,

            Constraint::SaveTheEnvironment => State {
                env: env.clone(),
                ..state
            },

            Constraint::Equal(region, category, tipe, expectation) => {
                let actual = self.type_to_variable(uf, rank, tipe);
                let expected = self.expected_to_variable(uf, rank, expectation);
                match unify::unify(self.bump, uf, actual, expected) {
                    unify::Answer::Ok(vars) => {
                        self.introduce(uf, rank, &vars);
                        state
                    }
                    unify::Answer::Err(vars, actual_type, expected_type) => {
                        self.introduce(uf, rank, &vars);
                        add_error(
                            state,
                            Error::BadExpr(
                                *region,
                                *category,
                                actual_type,
                                expectation.type_replace(expected_type),
                            ),
                        )
                    }
                }
            }

            Constraint::Local(region, name, expectation) => {
                let local_var = *env
                    .get(name)
                    .expect("constraint generator only references bound locals");
                let actual = self.make_copy(uf, rank, local_var);
                let expected = self.expected_to_variable(uf, rank, expectation);
                match unify::unify(self.bump, uf, actual, expected) {
                    unify::Answer::Ok(vars) => {
                        self.introduce(uf, rank, &vars);
                        state
                    }
                    unify::Answer::Err(vars, actual_type, expected_type) => {
                        self.introduce(uf, rank, &vars);
                        add_error(
                            state,
                            Error::BadExpr(
                                *region,
                                Category::Local(name),
                                actual_type,
                                expectation.type_replace(expected_type),
                            ),
                        )
                    }
                }
            }

            Constraint::Foreign(region, name, annotation, expectation) => {
                let actual =
                    self.src_type_to_variable(uf, rank, annotation.free_vars, annotation.typ);
                let expected = self.expected_to_variable(uf, rank, expectation);
                match unify::unify(self.bump, uf, actual, expected) {
                    unify::Answer::Ok(vars) => {
                        self.introduce(uf, rank, &vars);
                        state
                    }
                    unify::Answer::Err(vars, actual_type, expected_type) => {
                        self.introduce(uf, rank, &vars);
                        add_error(
                            state,
                            Error::BadExpr(
                                *region,
                                Category::Foreign(name),
                                actual_type,
                                expectation.type_replace(expected_type),
                            ),
                        )
                    }
                }
            }

            Constraint::Pattern(region, category, tipe, expectation) => {
                let actual = self.type_to_variable(uf, rank, tipe);
                let expected = self.pattern_expectation_to_variable(uf, rank, expectation);
                match unify::unify(self.bump, uf, actual, expected) {
                    unify::Answer::Ok(vars) => {
                        self.introduce(uf, rank, &vars);
                        state
                    }
                    unify::Answer::Err(vars, actual_type, expected_type) => {
                        self.introduce(uf, rank, &vars);
                        add_error(
                            state,
                            Error::BadPattern(
                                *region,
                                *category,
                                actual_type,
                                expectation.type_replace(expected_type),
                            ),
                        )
                    }
                }
            }

            Constraint::And(constraints) => constraints
                .iter()
                .fold(state, |state, sub| self.solve(uf, env, rank, state, sub)),

            Constraint::Let {
                rigid_vars,
                flex_vars,
                header,
                header_con,
                body_con,
            } => {
                if rigid_vars.is_empty() && matches!(body_con, Constraint::True) {
                    self.introduce(uf, rank, flex_vars);
                    self.solve(uf, env, rank, state, header_con)
                } else if rigid_vars.is_empty() && flex_vars.is_empty() {
                    let state1 = self.solve(uf, env, rank, state, header_con);
                    let locals: Vec<(&'a str, Located<Variable>)> = header
                        .iter()
                        .map(|(name, loc_type)| {
                            let var = self.type_to_variable(uf, rank, loc_type.value);
                            (*name, Located::at(loc_type.region, var))
                        })
                        .collect();
                    let mut new_env = env.clone();
                    for (name, loc) in &locals {
                        new_env.entry(name).or_insert(loc.value);
                    }
                    let state2 = self.solve(uf, &new_env, rank, state1, body_con);
                    locals.into_iter().fold(state2, |state, (name, loc)| {
                        self.check_occurs(uf, state, name, loc)
                    })
                } else {
                    // work in the next pool to localize header
                    let next_rank = rank + 1;
                    if next_rank >= self.pools.len() {
                        let pools_length = self.pools.len();
                        self.pools.resize(pools_length * 2, Vec::new());
                    }

                    // introduce variables
                    let vars: Vec<Variable> =
                        rigid_vars.iter().chain(flex_vars.iter()).copied().collect();
                    for var in &vars {
                        uf.modify(*var, |desc| desc.rank = next_rank);
                    }
                    self.pools[next_rank] = vars;

                    // run solver in next pool
                    let locals: Vec<(&'a str, Located<Variable>)> = header
                        .iter()
                        .map(|(name, loc_type)| {
                            let var = self.type_to_variable(uf, next_rank, loc_type.value);
                            (*name, Located::at(loc_type.region, var))
                        })
                        .collect();
                    let state1 = self.solve(uf, env, next_rank, state, header_con);

                    let young_mark = state1.mark;
                    let visit_mark = young_mark.next();
                    let final_mark = visit_mark.next();

                    // pop pool
                    self.generalize(uf, young_mark, visit_mark, next_rank);
                    self.pools[next_rank] = Vec::new();

                    // check that things went well
                    for rigid in rigid_vars.iter() {
                        self.is_generic(uf, *rigid);
                    }

                    let mut new_env = env.clone();
                    for (name, loc) in &locals {
                        new_env.entry(name).or_insert(loc.value);
                    }
                    let temp_state = State {
                        env: state1.env,
                        mark: final_mark,
                        errors: state1.errors,
                    };
                    let new_state = self.solve(uf, &new_env, rank, temp_state, body_con);

                    locals.into_iter().fold(new_state, |state, (name, loc)| {
                        self.check_occurs(uf, state, name, loc)
                    })
                }
            }
        }
    }

    /// Check that a variable has rank `NO_RANK`, meaning it generalized.
    fn is_generic(&mut self, uf: &mut UnionFind<'a>, var: Variable) {
        let rank = uf.get(var).rank;
        if rank != NO_RANK {
            let tipe = to_error_type(self.bump, uf, var);
            panic!(
                "You ran into a compiler bug. Here are some details for the developers:\n\n    \
                 {tipe:?} [rank = {rank}]\n\nPlease create a minimal example and report it."
            );
        }
    }

    // EXPECTATIONS TO VARIABLE

    fn expected_to_variable(
        &mut self,
        uf: &mut UnionFind<'a>,
        rank: usize,
        expectation: &Expected<'a, &'a Type<'a>>,
    ) -> Variable {
        let tipe = match expectation {
            Expected::NoExpectation(tipe) => tipe,
            Expected::FromContext(_, _, tipe) => tipe,
            Expected::FromAnnotation(_, _, _, tipe) => tipe,
        };
        self.type_to_variable(uf, rank, tipe)
    }

    fn pattern_expectation_to_variable(
        &mut self,
        uf: &mut UnionFind<'a>,
        rank: usize,
        expectation: &PExpected<'a, &'a Type<'a>>,
    ) -> Variable {
        let tipe = match expectation {
            PExpected::NoExpectation(tipe) => tipe,
            PExpected::FromContext(_, _, tipe) => tipe,
        };
        self.type_to_variable(uf, rank, tipe)
    }

    // OCCURS CHECK

    fn check_occurs(
        &mut self,
        uf: &mut UnionFind<'a>,
        state: State<'a>,
        name: &'a str,
        located_variable: Located<Variable>,
    ) -> State<'a> {
        let variable = located_variable.value;
        if occurs::occurs(uf, variable) {
            let error_type = to_error_type(self.bump, uf, variable);
            uf.modify(variable, |desc| desc.content = Content::Error);
            add_error(
                state,
                Error::InfiniteType {
                    region: located_variable.region,
                    name,
                    overall_type: error_type,
                },
            )
        } else {
            state
        }
    }

    // GENERALIZE

    /// Every variable has rank less than or equal to the maxRank of the
    /// pool. This sorts variables into the young and old pools accordingly.
    fn generalize(
        &mut self,
        uf: &mut UnionFind<'a>,
        young_mark: Mark,
        visit_mark: Mark,
        young_rank: usize,
    ) {
        let young_vars = self.pools[young_rank].clone();
        let rank_table = pool_to_rank_table(uf, young_mark, young_rank, young_vars);

        // get the ranks right for each entry.
        // start at low ranks so that we only have to pass
        // over the information once.
        for (rank, table) in rank_table.iter().enumerate() {
            for var in table {
                adjust_rank(uf, young_mark, visit_mark, rank, *var);
            }
        }

        // For variables that have rank lower than youngRank, register them
        // in the appropriate old pool if they are not redundant.
        for vars in &rank_table[..young_rank] {
            for var in vars {
                if !uf.redundant(*var) {
                    let rank = uf.get(*var).rank;
                    self.pools[rank].push(*var);
                }
            }
        }

        // For variables with rank youngRank
        //   If rank < youngRank: register in oldPool
        //   otherwise generalize
        for var in &rank_table[young_rank] {
            if !uf.redundant(*var) {
                let rank = uf.get(*var).rank;
                if rank < young_rank {
                    self.pools[rank].push(*var);
                } else {
                    uf.modify(*var, |desc| desc.rank = NO_RANK);
                }
            }
        }
    }

    // REGISTER VARIABLES

    fn introduce(&mut self, uf: &mut UnionFind<'a>, rank: usize, variables: &[Variable]) {
        self.pools[rank].extend_from_slice(variables);
        for var in variables {
            uf.modify(*var, |desc| desc.rank = rank);
        }
    }

    // TYPE TO VARIABLE

    fn type_to_variable(
        &mut self,
        uf: &mut UnionFind<'a>,
        rank: usize,
        tipe: &Type<'a>,
    ) -> Variable {
        self.type_to_var(uf, rank, &BTreeMap::new(), tipe)
    }

    fn type_to_var(
        &mut self,
        uf: &mut UnionFind<'a>,
        rank: usize,
        alias_dict: &BTreeMap<&'a str, Variable>,
        tipe: &Type<'a>,
    ) -> Variable {
        match tipe {
            Type::VarN(var) => *var,

            Type::AppN { home, name, args } => {
                let arg_vars: Vec<Variable> = args
                    .iter()
                    .map(|arg| self.type_to_var(uf, rank, alias_dict, arg))
                    .collect();
                self.register(
                    uf,
                    rank,
                    Content::Structure(FlatType::App1(*home, name, arg_vars)),
                )
            }

            Type::FunN(a, b) => {
                let a_var = self.type_to_var(uf, rank, alias_dict, a);
                let b_var = self.type_to_var(uf, rank, alias_dict, b);
                self.register(uf, rank, Content::Structure(FlatType::Fun1(a_var, b_var)))
            }

            Type::AliasN {
                home,
                name,
                args,
                real,
            } => {
                let arg_vars: Vec<(&'a str, Variable)> = args
                    .iter()
                    .map(|(arg_name, arg_type)| {
                        (*arg_name, self.type_to_var(uf, rank, alias_dict, arg_type))
                    })
                    .collect();
                let new_dict: BTreeMap<&'a str, Variable> = arg_vars.iter().copied().collect();
                let alias_var = self.type_to_var(uf, rank, &new_dict, real);
                self.register(
                    uf,
                    rank,
                    Content::Alias {
                        home: *home,
                        name,
                        args: arg_vars,
                        real: alias_var,
                    },
                )
            }

            Type::PlaceHolder(name) => *alias_dict
                .get(name)
                .expect("alias placeholders only reference alias arguments"),

            Type::RecordN { fields, ext } => {
                let field_vars: BTreeMap<&'a str, Variable> = fields
                    .iter()
                    .map(|(name, field_type)| {
                        (*name, self.type_to_var(uf, rank, alias_dict, field_type))
                    })
                    .collect();
                let ext_var = self.type_to_var(uf, rank, alias_dict, ext);
                self.register(
                    uf,
                    rank,
                    Content::Structure(FlatType::Record1(field_vars, ext_var)),
                )
            }

            Type::EmptyRecordN => {
                self.register(uf, rank, Content::Structure(FlatType::EmptyRecord1))
            }

            Type::UnitN => self.register(uf, rank, Content::Structure(FlatType::Unit1)),

            Type::TupleN(a, b, maybe_c) => {
                let a_var = self.type_to_var(uf, rank, alias_dict, a);
                let b_var = self.type_to_var(uf, rank, alias_dict, b);
                let c_var = maybe_c.map(|c| self.type_to_var(uf, rank, alias_dict, c));
                self.register(
                    uf,
                    rank,
                    Content::Structure(FlatType::Tuple1(a_var, b_var, c_var)),
                )
            }
        }
    }

    fn register(&mut self, uf: &mut UnionFind<'a>, rank: usize, content: Content<'a>) -> Variable {
        let var = uf.fresh(Descriptor {
            content,
            rank,
            mark: NO_MARK,
            copy: None,
        });
        self.pools[rank].push(var);
        var
    }

    // SOURCE TYPE TO VARIABLE

    fn src_type_to_variable(
        &mut self,
        uf: &mut UnionFind<'a>,
        rank: usize,
        free_vars: &[&'a str],
        src_type: &Located<CanType<'a>>,
    ) -> Variable {
        // Elm's freeVars is a `Map Name ()`, so creation is name-sorted.
        let mut sorted_names: Vec<&'a str> = free_vars.to_vec();
        sorted_names.sort_unstable();

        let flex_vars: BTreeMap<&'a str, Variable> = sorted_names
            .into_iter()
            .map(|name| {
                let content = match type_::to_super(name) {
                    Some(super_type) => Content::FlexSuper(super_type, Some(name)),
                    None => Content::FlexVar(Some(name)),
                };
                let var = uf.fresh(Descriptor {
                    content,
                    rank,
                    mark: NO_MARK,
                    copy: None,
                });
                (name, var)
            })
            .collect();
        self.pools[rank].extend(flex_vars.values().copied());

        self.src_type_to_var(uf, rank, &flex_vars, src_type)
    }

    fn src_type_to_var(
        &mut self,
        uf: &mut UnionFind<'a>,
        rank: usize,
        flex_vars: &BTreeMap<&'a str, Variable>,
        src_type: &Located<CanType<'a>>,
    ) -> Variable {
        match &src_type.value {
            CanType::Lambda { from, to } => {
                let arg_var = self.src_type_to_var(uf, rank, flex_vars, from);
                let result_var = self.src_type_to_var(uf, rank, flex_vars, to);
                self.register(
                    uf,
                    rank,
                    Content::Structure(FlatType::Fun1(arg_var, result_var)),
                )
            }

            CanType::Var(name) => *flex_vars
                .get(name)
                .expect("annotations only mention their free variables"),

            CanType::Named { reference, args } => {
                let arg_vars: Vec<Variable> = args
                    .iter()
                    .map(|arg| self.src_type_to_var(uf, rank, flex_vars, arg))
                    .collect();
                self.register(
                    uf,
                    rank,
                    Content::Structure(FlatType::App1(reference.home, reference.name, arg_vars)),
                )
            }

            CanType::Record { fields, ext } => {
                let field_vars: BTreeMap<&'a str, Variable> = fields
                    .iter()
                    .map(|field| {
                        (
                            field.field,
                            self.src_type_to_var(uf, rank, flex_vars, field.typ),
                        )
                    })
                    .collect();
                let ext_var = match ext {
                    None => self.register(uf, rank, Content::Structure(FlatType::EmptyRecord1)),
                    Some(ext_name) => *flex_vars
                        .get(ext_name)
                        .expect("annotations only mention their free variables"),
                };
                self.register(
                    uf,
                    rank,
                    Content::Structure(FlatType::Record1(field_vars, ext_var)),
                )
            }

            CanType::Unit => self.register(uf, rank, Content::Structure(FlatType::Unit1)),

            CanType::Tuple {
                first,
                second,
                rest,
            } => {
                let a_var = self.src_type_to_var(uf, rank, flex_vars, first);
                let b_var = self.src_type_to_var(uf, rank, flex_vars, second);
                let c_var = rest
                    .first()
                    .map(|third| self.src_type_to_var(uf, rank, flex_vars, third));
                self.register(
                    uf,
                    rank,
                    Content::Structure(FlatType::Tuple1(a_var, b_var, c_var)),
                )
            }

            CanType::Alias {
                reference,
                arguments,
                target,
            } => {
                let arg_vars: Vec<(&'a str, Variable)> = arguments
                    .iter()
                    .map(|arg| (arg.name, self.src_type_to_var(uf, rank, flex_vars, arg.typ)))
                    .collect();
                let alias_var = match target {
                    alder_ast::AliasType::Open(real_type) => {
                        let arg_dict: BTreeMap<&'a str, Variable> =
                            arg_vars.iter().copied().collect();
                        self.src_type_to_var(uf, rank, &arg_dict, real_type)
                    }
                    alder_ast::AliasType::Filled(real_type) => {
                        self.src_type_to_var(uf, rank, flex_vars, real_type)
                    }
                };
                self.register(
                    uf,
                    rank,
                    Content::Alias {
                        home: reference.home,
                        name: reference.name,
                        args: arg_vars,
                        real: alias_var,
                    },
                )
            }
        }
    }

    // COPY

    fn make_copy(&mut self, uf: &mut UnionFind<'a>, rank: usize, var: Variable) -> Variable {
        let copy = self.make_copy_help(uf, rank, var);
        restore(uf, var);
        copy
    }

    fn make_copy_help(
        &mut self,
        uf: &mut UnionFind<'a>,
        max_rank: usize,
        variable: Variable,
    ) -> Variable {
        let desc = uf.get(variable).clone();

        if let Some(copy) = desc.copy {
            return copy;
        }

        if desc.rank != NO_RANK {
            return variable;
        }

        let make_descriptor = |content: Content<'a>| Descriptor {
            content,
            rank: max_rank,
            mark: NO_MARK,
            copy: None,
        };

        let copy = uf.fresh(make_descriptor(desc.content.clone()));
        self.pools[max_rank].push(copy);

        // Link the original variable to the new variable. This lets us
        // avoid making multiple copies of the variable we are instantiating.
        //
        // Need to do this before recursively copying to avoid looping.
        uf.set(
            variable,
            Descriptor {
                content: desc.content.clone(),
                rank: desc.rank,
                mark: NO_MARK,
                copy: Some(copy),
            },
        );

        // Now we recursively copy the content of the variable. We have
        // already marked the variable as copied, so we will not repeat this
        // work or crawl this variable again.
        match desc.content {
            Content::Structure(term) => {
                let new_term = self.copy_flat_type(uf, max_rank, term);
                uf.set(copy, make_descriptor(Content::Structure(new_term)));
                copy
            }

            Content::FlexVar(_) | Content::FlexSuper(_, _) => copy,

            Content::RigidVar(name) => {
                uf.set(copy, make_descriptor(Content::FlexVar(Some(name))));
                copy
            }

            Content::RigidSuper(super_type, name) => {
                uf.set(
                    copy,
                    make_descriptor(Content::FlexSuper(super_type, Some(name))),
                );
                copy
            }

            Content::Alias {
                home,
                name,
                args,
                real,
            } => {
                let new_args: Vec<(&'a str, Variable)> = args
                    .iter()
                    .map(|(arg_name, arg_var)| {
                        (*arg_name, self.make_copy_help(uf, max_rank, *arg_var))
                    })
                    .collect();
                let new_real = self.make_copy_help(uf, max_rank, real);
                uf.set(
                    copy,
                    make_descriptor(Content::Alias {
                        home,
                        name,
                        args: new_args,
                        real: new_real,
                    }),
                );
                copy
            }

            Content::Error => copy,
        }
    }

    fn copy_flat_type(
        &mut self,
        uf: &mut UnionFind<'a>,
        max_rank: usize,
        flat_type: FlatType<'a>,
    ) -> FlatType<'a> {
        match flat_type {
            FlatType::App1(home, name, args) => FlatType::App1(
                home,
                name,
                args.iter()
                    .map(|arg| self.make_copy_help(uf, max_rank, *arg))
                    .collect(),
            ),

            FlatType::Fun1(a, b) => {
                let a_copy = self.make_copy_help(uf, max_rank, a);
                let b_copy = self.make_copy_help(uf, max_rank, b);
                FlatType::Fun1(a_copy, b_copy)
            }

            FlatType::EmptyRecord1 => FlatType::EmptyRecord1,

            FlatType::Record1(fields, ext) => {
                let field_copies: BTreeMap<&'a str, Variable> = fields
                    .iter()
                    .map(|(name, var)| (*name, self.make_copy_help(uf, max_rank, *var)))
                    .collect();
                let ext_copy = self.make_copy_help(uf, max_rank, ext);
                FlatType::Record1(field_copies, ext_copy)
            }

            FlatType::Unit1 => FlatType::Unit1,

            FlatType::Tuple1(a, b, maybe_c) => {
                let a_copy = self.make_copy_help(uf, max_rank, a);
                let b_copy = self.make_copy_help(uf, max_rank, b);
                let c_copy = maybe_c.map(|c| self.make_copy_help(uf, max_rank, c));
                FlatType::Tuple1(a_copy, b_copy, c_copy)
            }
        }
    }
}

// GENERALIZE HELPERS

fn pool_to_rank_table<'a>(
    uf: &mut UnionFind<'a>,
    young_mark: Mark,
    young_rank: usize,
    young_inhabitants: Vec<Variable>,
) -> Vec<Vec<Variable>> {
    let mut table = vec![Vec::new(); young_rank + 1];

    // Sort the youngPool variables into buckets by rank.
    for var in young_inhabitants {
        let rank = uf.get(var).rank;
        uf.modify(var, |desc| desc.mark = young_mark);
        table[rank].push(var);
    }

    table
}

// ADJUST RANK

// Adjust variable ranks such that ranks never increase as you move deeper.
// This way the outermost rank is representative of the entire structure.
fn adjust_rank<'a>(
    uf: &mut UnionFind<'a>,
    young_mark: Mark,
    visit_mark: Mark,
    group_rank: usize,
    var: Variable,
) -> usize {
    let desc = uf.get(var);
    let rank = desc.rank;
    let mark = desc.mark;

    if mark == young_mark {
        // Set the variable as marked first because it may be cyclic.
        uf.modify(var, |desc| desc.mark = visit_mark);
        let content = uf.get(var).content.clone();
        let max_rank = adjust_rank_content(uf, young_mark, visit_mark, group_rank, &content);
        uf.modify(var, |desc| {
            desc.rank = max_rank;
            desc.mark = visit_mark;
        });
        max_rank
    } else if mark == visit_mark {
        rank
    } else {
        let min_rank = group_rank.min(rank);
        // TODO how can minRank ever be groupRank?
        uf.modify(var, |desc| {
            desc.rank = min_rank;
            desc.mark = visit_mark;
        });
        min_rank
    }
}

fn adjust_rank_content<'a>(
    uf: &mut UnionFind<'a>,
    young_mark: Mark,
    visit_mark: Mark,
    group_rank: usize,
    content: &Content<'a>,
) -> usize {
    match content {
        Content::FlexVar(_)
        | Content::FlexSuper(_, _)
        | Content::RigidVar(_)
        | Content::RigidSuper(_, _)
        | Content::Error => group_rank,

        Content::Structure(flat_type) => match flat_type {
            FlatType::App1(_, _, args) => args.iter().fold(OUTERMOST_RANK, |rank, arg| {
                rank.max(adjust_rank(uf, young_mark, visit_mark, group_rank, *arg))
            }),

            FlatType::Fun1(arg, result) => {
                let arg_rank = adjust_rank(uf, young_mark, visit_mark, group_rank, *arg);
                let result_rank = adjust_rank(uf, young_mark, visit_mark, group_rank, *result);
                arg_rank.max(result_rank)
            }

            // THEORY: an empty record never needs to get generalized
            FlatType::EmptyRecord1 => OUTERMOST_RANK,

            FlatType::Record1(fields, extension) => {
                let ext_rank = adjust_rank(uf, young_mark, visit_mark, group_rank, *extension);
                fields.values().fold(ext_rank, |rank, field| {
                    rank.max(adjust_rank(uf, young_mark, visit_mark, group_rank, *field))
                })
            }

            // THEORY: a unit never needs to get generalized
            FlatType::Unit1 => OUTERMOST_RANK,

            FlatType::Tuple1(a, b, maybe_c) => {
                let a_rank = adjust_rank(uf, young_mark, visit_mark, group_rank, *a);
                let b_rank = adjust_rank(uf, young_mark, visit_mark, group_rank, *b);
                let ab_rank = a_rank.max(b_rank);
                match maybe_c {
                    None => ab_rank,
                    Some(c) => ab_rank.max(adjust_rank(uf, young_mark, visit_mark, group_rank, *c)),
                }
            }
        },

        // THEORY: anything in the realVar would be outermostRank
        Content::Alias { args, .. } => args.iter().fold(OUTERMOST_RANK, |rank, (_, arg_var)| {
            rank.max(adjust_rank(
                uf, young_mark, visit_mark, group_rank, *arg_var,
            ))
        }),
    }
}

// RESTORE

fn restore<'a>(uf: &mut UnionFind<'a>, variable: Variable) {
    let desc = uf.get(variable).clone();
    if desc.copy.is_some() {
        uf.set(
            variable,
            Descriptor {
                content: desc.content.clone(),
                rank: NO_RANK,
                mark: NO_MARK,
                copy: None,
            },
        );
        restore_content(uf, &desc.content);
    }
}

fn restore_content<'a>(uf: &mut UnionFind<'a>, content: &Content<'a>) {
    match content {
        Content::FlexVar(_)
        | Content::FlexSuper(_, _)
        | Content::RigidVar(_)
        | Content::RigidSuper(_, _)
        | Content::Error => {}

        Content::Structure(term) => match term {
            FlatType::App1(_, _, args) => {
                for arg in args {
                    restore(uf, *arg);
                }
            }

            FlatType::Fun1(arg, result) => {
                restore(uf, *arg);
                restore(uf, *result);
            }

            FlatType::EmptyRecord1 => {}

            FlatType::Record1(fields, ext) => {
                for field in fields.values() {
                    restore(uf, *field);
                }
                restore(uf, *ext);
            }

            FlatType::Unit1 => {}

            FlatType::Tuple1(a, b, maybe_c) => {
                restore(uf, *a);
                restore(uf, *b);
                if let Some(c) = maybe_c {
                    restore(uf, *c);
                }
            }
        },

        Content::Alias { args, real, .. } => {
            for (_, arg) in args {
                restore(uf, *arg);
            }
            restore(uf, *real);
        }
    }
}
