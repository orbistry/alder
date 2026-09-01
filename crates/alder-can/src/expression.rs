use std::collections::BTreeMap;

use alder_ast::{
    Annotation, CaseBranch as CanCaseBranch, ConstructorName, CtorOpts, Def as CanDef,
    Expr as CanExpr, FieldUpdate as CanFieldUpdate, FieldValue as CanFieldValue, FreeVars,
    IfBranch as CanIfBranch, ModuleName, QualifiedName, Type as CanType,
    TypedPattern as CanTypedPattern,
};
use alder_region::{Located, Region};
use alder_source::{
    BinOpOperand, CaseArm, Def as SourceDef, Expr as SourceExpr, FieldAssign,
    IfBranch as SourceIfBranch, VarType,
};
use bumpalo::Bump;

use crate::Error;
use crate::environment::{self, Ctor as EnvCtor, Env, Info, Var};
use crate::error::DuplicatePatternContext;
use crate::pattern::{self, Bindings};
use crate::scc;
use crate::types;
use crate::warning::{Warning, WarningContext};

pub type FreeLocals<'a> = BTreeMap<&'a str, Uses>;

#[derive(Clone, Copy, Debug)]
pub struct Uses {
    pub direct: u32,
    pub delayed: u32,
}

fn log_var<'a>(free_locals: &mut FreeLocals<'a>, name: &'a str) {
    free_locals
        .entry(name)
        .and_modify(|u| u.direct += 1)
        .or_insert(Uses {
            direct: 1,
            delayed: 0,
        });
}

fn delay_use(uses: Uses) -> Uses {
    Uses {
        direct: 0,
        delayed: uses.direct + uses.delayed,
    }
}

fn merge_free_locals<'a>(target: &mut FreeLocals<'a>, source: FreeLocals<'a>, delay: bool) {
    for (name, uses) in source {
        let uses = if delay { delay_use(uses) } else { uses };
        target
            .entry(name)
            .and_modify(|u| {
                u.direct += uses.direct;
                u.delayed += uses.delayed;
            })
            .or_insert(uses);
    }
}

pub fn verify_bindings<'a>(
    context: WarningContext,
    bindings: &Bindings<'a>,
    body_free_locals: FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> FreeLocals<'a> {
    let mut outer_free = FreeLocals::new();

    for (name, uses) in &body_free_locals {
        if !bindings.contains_key(name) {
            outer_free.insert(name, *uses);
        }
    }

    for (&name, &region) in bindings {
        if !body_free_locals.contains_key(name) {
            warnings.push(Warning::UnusedVariable {
                region,
                context,
                name,
            });
        }
    }

    outer_free
}

pub fn canonicalize_expr<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    expr: &'a Located<SourceExpr<'a>>,
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    let region = expr.region;
    let can_expr = match &expr.value {
        SourceExpr::Str(s) => CanExpr::Str(s),
        SourceExpr::Int(n) => CanExpr::Int(*n),

        SourceExpr::Var {
            kind: VarType::LowVar,
            name,
        } => find_var(bump, env, region, name, free_locals)?,

        SourceExpr::Var {
            kind: VarType::CapVar,
            name,
        } => {
            let ctor = env.find_ctor(bump, region, name)?;
            to_var_ctor(bump, name, &ctor)
        }

        SourceExpr::VarQual {
            kind: VarType::LowVar,
            module,
            name,
        } => find_var_qual(bump, env, region, module, name)?,

        SourceExpr::VarQual {
            kind: VarType::CapVar,
            module,
            name,
        } => {
            let ctor = env.find_ctor_qual(bump, region, module, name)?;
            to_var_ctor(bump, name, &ctor)
        }

        SourceExpr::List(exprs) => {
            CanExpr::List(canonicalize_exprs(bump, env, exprs, free_locals, warnings)?)
        }

        SourceExpr::Op(symbol) => {
            let binop = env.find_binop(bump, region, symbol)?;
            CanExpr::VarOperator {
                symbol,
                reference: QualifiedName {
                    home: binop.home,
                    name: binop.function,
                },
                annotation: binop.annotation,
            }
        }

        SourceExpr::Negate(inner) => {
            CanExpr::Negate(canonicalize_expr(bump, env, inner, free_locals, warnings)?)
        }

        SourceExpr::BinOps { operands, last } => {
            return canonicalize_binops(bump, env, operands, last, region, free_locals, warnings);
        }

        SourceExpr::Lambda { parameters, body } => {
            return canonicalize_lambda(bump, env, parameters, body, region, free_locals, warnings);
        }

        SourceExpr::Call {
            function,
            arguments,
        } => {
            let can_func = canonicalize_expr(bump, env, function, free_locals, warnings)?;
            let can_args = canonicalize_exprs(bump, env, arguments, free_locals, warnings)?;
            CanExpr::Call {
                function: can_func,
                arguments: can_args,
            }
        }

        SourceExpr::If {
            branches,
            final_else,
        } => canonicalize_if(bump, env, branches, final_else, free_locals, warnings)?,

        SourceExpr::Let { defs, body } => {
            return canonicalize_let(bump, env, defs, body, region, free_locals, warnings);
        }

        SourceExpr::Case { scrutinee, arms } => {
            let can_scrutinee = canonicalize_expr(bump, env, scrutinee, free_locals, warnings)?;
            let can_branches = canonicalize_case_branches(bump, env, arms, free_locals, warnings)?;
            CanExpr::Case {
                scrutinee: can_scrutinee,
                branches: can_branches,
            }
        }

        SourceExpr::Accessor(field) => CanExpr::Accessor(field),

        SourceExpr::Access { record, field } => CanExpr::Access {
            record: canonicalize_expr(bump, env, record, free_locals, warnings)?,
            field,
        },

        SourceExpr::Update { record, fields } => {
            canonicalize_update(bump, env, record, fields, free_locals, warnings)?
        }

        SourceExpr::Record(fields) => {
            canonicalize_record(bump, env, fields, free_locals, warnings)?
        }

        SourceExpr::Unit => CanExpr::Unit,

        SourceExpr::Tuple {
            first,
            second,
            rest,
        } => {
            if rest.len() > 1 {
                return Err(vec![Error::TupleLargerThanThree { region }]);
            }
            CanExpr::Tuple {
                first: canonicalize_expr(bump, env, first, free_locals, warnings)?,
                second: canonicalize_expr(bump, env, second, free_locals, warnings)?,
                rest: canonicalize_exprs(bump, env, rest, free_locals, warnings)?,
            }
        }
    };
    Ok(bump.alloc(Located::at(region, can_expr)))
}

fn find_var<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    region: Region,
    name: &'a str,
    free_locals: &mut FreeLocals<'a>,
) -> Result<CanExpr<'a>, Vec<Error<'a>>> {
    match env.vars.get(name) {
        Some(Var::Local(_)) => {
            log_var(free_locals, name);
            Ok(CanExpr::VarLocal(name))
        }
        Some(Var::TopLevel(_)) => {
            log_var(free_locals, name);
            Ok(CanExpr::VarTopLevel(QualifiedName {
                home: env.home,
                name,
            }))
        }
        Some(Var::Foreign(home, annotation)) => Ok(CanExpr::VarForeign {
            reference: QualifiedName { home: *home, name },
            annotation,
        }),
        Some(Var::Foreigns(first, others)) => Err(vec![Error::AmbiguousVar {
            region,
            prefix: None,
            name,
            first_module: *first,
            other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
        }]),
        None => Err(vec![Error::NotFoundVar {
            region,
            prefix: None,
            name,
            suggestions: env.possible_var_names(bump),
        }]),
    }
}

fn find_var_qual<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    region: Region,
    prefix: &'a str,
    name: &'a str,
) -> Result<CanExpr<'a>, Vec<Error<'a>>> {
    let info = env
        .q_vars
        .get(prefix)
        .and_then(|m| m.get(name))
        .ok_or_else(|| {
            vec![Error::NotFoundVar {
                region,
                prefix: Some(prefix),
                name,
                suggestions: env.possible_var_names(bump),
            }]
        })?;
    match info {
        Info::Specific(home, annotation) => Ok(CanExpr::VarForeign {
            reference: QualifiedName { home: *home, name },
            annotation,
        }),
        Info::Ambiguous(first, others) => Err(vec![Error::AmbiguousVar {
            region,
            prefix: Some(prefix),
            name,
            first_module: *first,
            other_modules: bump.alloc_slice_fill_iter(others.iter().copied()),
        }]),
    }
}

fn to_var_ctor<'a>(bump: &'a Bump, name: &'a str, ctor: &EnvCtor<'a>) -> CanExpr<'a> {
    match ctor {
        EnvCtor::Union {
            home,
            type_name,
            type_vars,
            index,
            arguments,
            options,
            ..
        } => {
            // Build: a -> b -> ... -> TypeName a b
            // Elm keeps free vars in a Map, so they come out name-sorted.
            let mut sorted_vars: Vec<&'a str> = type_vars.to_vec();
            sorted_vars.sort_unstable();
            sorted_vars.dedup();
            let free_vars: FreeVars<'a> = bump.alloc_slice_fill_iter(sorted_vars);
            let result_type: &Located<CanType> = bump.alloc(Located::at(
                Region::zero(),
                CanType::Named {
                    reference: QualifiedName {
                        home: *home,
                        name: type_name,
                    },
                    args: bump.alloc_slice_fill_iter(
                        type_vars
                            .iter()
                            .map(|v| &*bump.alloc(Located::at(Region::zero(), CanType::Var(v)))),
                    ),
                },
            ));
            // foldr TLambda result args
            let mut typ: &Located<CanType> = result_type;
            for arg in arguments.iter().rev() {
                typ = bump.alloc(Located::at(
                    Region::zero(),
                    CanType::Lambda { from: arg, to: typ },
                ));
            }
            let annotation = bump.alloc(Annotation { free_vars, typ });

            CanExpr::VarConstructor {
                options: *options,
                reference: ConstructorName {
                    home: *home,
                    union: type_name,
                    name,
                },
                index: *index,
                annotation,
            }
        }
        EnvCtor::Bool { home, union, index } => {
            let annotation = bump.alloc(Annotation {
                free_vars: &[],
                typ: bump.alloc(Located::at(
                    Region::zero(),
                    CanType::Named {
                        reference: QualifiedName {
                            home: *home,
                            name: union.name.value,
                        },
                        args: &[],
                    },
                )),
            });
            CanExpr::VarConstructor {
                options: union.options,
                reference: ConstructorName {
                    home: *home,
                    union: union.name.value,
                    name,
                },
                index: *index,
                annotation,
            }
        }
        EnvCtor::RecordCtor {
            home,
            alias_name,
            type_vars,
            typ,
        } => {
            // Like Elm's `Env.RecordCtor home vars tipe`: the curried type
            // was built when the ctor entered the env; the free vars are
            // the alias's declared parameters (name-sorted, as a Map).
            let mut sorted_vars: Vec<&'a str> = type_vars.to_vec();
            sorted_vars.sort_unstable();
            sorted_vars.dedup();
            let free_vars: FreeVars<'a> = bump.alloc_slice_fill_iter(sorted_vars);
            let annotation = bump.alloc(Annotation { free_vars, typ });

            CanExpr::VarConstructor {
                options: CtorOpts::Normal,
                reference: ConstructorName {
                    home: *home,
                    union: alias_name,
                    name,
                },
                index: 0,
                annotation,
            }
        }
    }
}

fn canonicalize_exprs<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    exprs: &[&'a Located<SourceExpr<'a>>],
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a [&'a Located<CanExpr<'a>>], Vec<Error<'a>>> {
    let mut results = Vec::with_capacity(exprs.len());
    let mut errors = Vec::new();
    for expr in exprs {
        match canonicalize_expr(bump, env, expr, free_locals, warnings) {
            Ok(can) => results.push(can),
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(bump.alloc_slice_fill_iter(results))
}

fn canonicalize_lambda<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    parameters: &'a [&'a Located<alder_source::Pattern<'a>>],
    body: &'a Located<SourceExpr<'a>>,
    region: Region,
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    // One duplicate-detection scope across ALL parameters, so `\x x -> x`
    // is rejected like in Elm.
    let (can_params, all_bindings) =
        pattern::verify_all(bump, env, DuplicatePatternContext::LambdaArgs, parameters)?;

    let inner_env = env.add_locals(&all_bindings)?;
    let mut body_free_locals = FreeLocals::new();
    let can_body = canonicalize_expr(bump, &inner_env, body, &mut body_free_locals, warnings)?;

    let outer_free = verify_bindings(
        WarningContext::Pattern,
        &all_bindings,
        body_free_locals,
        warnings,
    );
    merge_free_locals(free_locals, outer_free, true);

    Ok(bump.alloc(Located::at(
        region,
        CanExpr::Lambda {
            parameters: bump.alloc_slice_fill_iter(can_params),
            body: can_body,
        },
    )))
}

fn canonicalize_case_branches<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    arms: &[&'a CaseArm<'a>],
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a [CanCaseBranch<'a>], Vec<Error<'a>>> {
    let mut results = Vec::with_capacity(arms.len());
    let mut errors = Vec::new();
    for arm in arms {
        match canonicalize_case_branch(bump, env, arm, free_locals, warnings) {
            Ok(branch) => results.push(branch),
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(bump.alloc_slice_fill_iter(results))
}

fn canonicalize_case_branch<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    arm: &'a CaseArm<'a>,
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<CanCaseBranch<'a>, Vec<Error<'a>>> {
    let (can_pattern, bindings) =
        pattern::verify(bump, env, DuplicatePatternContext::CaseBranch, arm.pattern)?;
    let inner_env = env.add_locals(&bindings)?;
    let mut body_free_locals = FreeLocals::new();
    let can_body = canonicalize_expr(bump, &inner_env, arm.body, &mut body_free_locals, warnings)?;
    let outer_free = verify_bindings(
        WarningContext::Pattern,
        &bindings,
        body_free_locals,
        warnings,
    );
    merge_free_locals(free_locals, outer_free, false);
    Ok(CanCaseBranch {
        pattern: can_pattern,
        body: can_body,
    })
}

fn canonicalize_if<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    branches: &[&'a SourceIfBranch<'a>],
    final_else: &'a Located<SourceExpr<'a>>,
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<CanExpr<'a>, Vec<Error<'a>>> {
    let mut can_branches = Vec::with_capacity(branches.len());
    let mut errors = Vec::new();
    for branch in branches {
        match (
            canonicalize_expr(bump, env, branch.condition, free_locals, warnings),
            canonicalize_expr(bump, env, branch.then_branch, free_locals, warnings),
        ) {
            (Ok(cond), Ok(then)) => can_branches.push(CanIfBranch {
                condition: cond,
                then_branch: then,
            }),
            (Err(e1), Err(e2)) => {
                errors.extend(e1);
                errors.extend(e2);
            }
            (Err(e), _) | (_, Err(e)) => errors.extend(e),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let can_else = canonicalize_expr(bump, env, final_else, free_locals, warnings)?;
    Ok(CanExpr::If {
        branches: bump.alloc_slice_fill_iter(can_branches),
        final_else: can_else,
    })
}

/// Mirrors Elm's `Dups.checkFields`: one `DuplicateField` per duplicated
/// name with its first two occurrences, in name order; on success the
/// fields come back keyed (hence sorted) by name, matching Elm's canonical
/// `Map Name ...` record representation.
fn check_field_assigns<'a>(
    fields: &[&'a FieldAssign<'a>],
) -> Result<BTreeMap<&'a str, &'a FieldAssign<'a>>, Vec<Error<'a>>> {
    let mut occurrences: BTreeMap<&'a str, Vec<&'a FieldAssign<'a>>> = BTreeMap::new();
    for field in fields {
        occurrences
            .entry(field.field.value)
            .or_default()
            .push(field);
    }

    let mut result = BTreeMap::new();
    let mut errors = Vec::new();
    for (name, entries) in occurrences {
        if entries.len() > 1 {
            errors.push(Error::DuplicateField {
                name,
                first: entries[0].field.region,
                second: entries[1].field.region,
            });
        } else {
            result.insert(name, entries[0]);
        }
    }

    if errors.is_empty() {
        Ok(result)
    } else {
        Err(errors)
    }
}

fn canonicalize_record<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    fields: &[&'a FieldAssign<'a>],
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<CanExpr<'a>, Vec<Error<'a>>> {
    let field_dict = check_field_assigns(fields)?;
    let mut can_fields = Vec::with_capacity(field_dict.len());
    let mut errors = Vec::new();
    for (_, field) in field_dict {
        match canonicalize_expr(bump, env, field.value, free_locals, warnings) {
            Ok(value) => can_fields.push(CanFieldValue {
                field: field.field,
                value,
            }),
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(CanExpr::Record(bump.alloc_slice_fill_iter(can_fields)))
}

fn canonicalize_update<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    record: &'a Located<&'a str>,
    fields: &[&'a FieldAssign<'a>],
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<CanExpr<'a>, Vec<Error<'a>>> {
    // Like Elm's `Can.Update name <$> findVar ... <*> makeCanFields`, the
    // base-variable error accumulates with the field errors.
    let base_result = find_var(bump, env, record.region, record.value, free_locals)
        .map(|base_expr| &*bump.alloc(Located::at(record.region, base_expr)));

    let fields_result = check_field_assigns(fields).and_then(|field_dict| {
        let mut can_fields = Vec::with_capacity(field_dict.len());
        let mut errors = Vec::new();
        for (_, field) in field_dict {
            match canonicalize_expr(bump, env, field.value, free_locals, warnings) {
                Ok(value) => can_fields.push(CanFieldUpdate {
                    field: field.field,
                    value,
                }),
                Err(errs) => errors.extend(errs),
            }
        }
        if errors.is_empty() {
            Ok(can_fields)
        } else {
            Err(errors)
        }
    });

    let (base, can_fields) = crate::accumulate::accumulate2(base_result, fields_result)?;
    Ok(CanExpr::Update {
        record: record.value,
        base,
        fields: bump.alloc_slice_fill_iter(can_fields),
    })
}

struct ResolvedOp<'a> {
    symbol: &'a str,
    home: ModuleName<'a>,
    function: &'a str,
    annotation: &'a Annotation<'a>,
    associativity: alder_ast::Associativity,
    precedence: alder_ast::Precedence,
}

fn canonicalize_binops<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    operands: &[&'a BinOpOperand<'a>],
    last: &'a Located<SourceExpr<'a>>,
    overall_region: Region,
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    let mut can_exprs = Vec::with_capacity(operands.len() + 1);
    let mut ops = Vec::with_capacity(operands.len());
    let mut errors = Vec::new();
    for operand in operands {
        match canonicalize_expr(bump, env, operand.expr, free_locals, warnings) {
            Ok(e) => can_exprs.push(e),
            Err(errs) => errors.extend(errs),
        }
        match env.find_binop(bump, operand.op.region, operand.op.value) {
            Ok(binop) => ops.push(ResolvedOp {
                symbol: binop.symbol,
                home: binop.home,
                function: binop.function,
                annotation: binop.annotation,
                associativity: binop.associativity,
                precedence: binop.precedence,
            }),
            Err(errs) => errors.extend(errs),
        }
    }
    match canonicalize_expr(bump, env, last, free_locals, warnings) {
        Ok(e) => can_exprs.push(e),
        Err(errs) => errors.extend(errs),
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    build_binop_tree(bump, &can_exprs, &ops, overall_region)
}

/// Precedence-climbing: find the root operator (lowest precedence),
/// recurse left and right. For left-assoc ties, rightmost is root.
/// For right-assoc ties, leftmost is root. Same-prec different-assoc = error.
fn build_binop_tree<'a>(
    bump: &'a Bump,
    exprs: &[&'a Located<CanExpr<'a>>],
    ops: &[ResolvedOp<'a>],
    overall_region: Region,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    if ops.is_empty() {
        return Ok(exprs[0]);
    }
    build_tree_rec(bump, exprs, ops, 0, exprs.len() - 1, overall_region)
}

fn build_tree_rec<'a>(
    bump: &'a Bump,
    exprs: &[&'a Located<CanExpr<'a>>],
    ops: &[ResolvedOp<'a>],
    start: usize,
    end: usize,
    overall_region: Region,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    if start == end {
        return Ok(exprs[start]);
    }

    // Find root: operator with lowest precedence in [start..end)
    let mut root_idx = start;
    for i in (start + 1)..end {
        if ops[i].precedence < ops[root_idx].precedence {
            root_idx = i;
        } else if ops[i].precedence == ops[root_idx].precedence {
            use alder_ast::Associativity::*;
            if ops[i].associativity != ops[root_idx].associativity {
                return Err(vec![Error::BinopConflict {
                    region: overall_region,
                    op1: ops[root_idx].symbol,
                    op2: ops[i].symbol,
                }]);
            }
            match ops[i].associativity {
                Left => root_idx = i,
                Right => {}
                None => {
                    return Err(vec![Error::BinopConflict {
                        region: overall_region,
                        op1: ops[root_idx].symbol,
                        op2: ops[i].symbol,
                    }]);
                }
            }
        }
    }

    let op = &ops[root_idx];
    let left = build_tree_rec(bump, exprs, ops, start, root_idx, overall_region)?;
    let right = build_tree_rec(bump, exprs, ops, root_idx + 1, end, overall_region)?;
    Ok(bump.alloc(Located::at(
        Region::span_across(&left.region, &right.region),
        CanExpr::Binop {
            symbol: op.symbol,
            reference: QualifiedName {
                home: op.home,
                name: op.function,
            },
            annotation: op.annotation,
            left,
            right,
        },
    )))
}

enum LetBinding<'a> {
    Define(&'a CanDef<'a>),
    Destruct(
        &'a Located<alder_ast::Pattern<'a>>,
        &'a Located<CanExpr<'a>>,
    ),
    Edge(&'a Located<&'a str>),
}

fn canonicalize_let<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    defs: &[&'a Located<SourceDef<'a>>],
    body: &'a Located<SourceExpr<'a>>,
    region: Region,
    free_locals: &mut FreeLocals<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    let mut name_regions: Vec<(&'a str, Region)> = Vec::new();
    for def in defs {
        collect_def_names(&def.value, &mut name_regions);
    }
    let bindings = environment::dups::detect(name_regions.into_iter(), |name, first, second| {
        Error::DuplicatePattern {
            context: DuplicatePatternContext::LetBinding,
            name,
            first,
            second,
        }
    })?;

    let inner_env = env.add_locals(&bindings)?;

    let mut nodes: Vec<(&'a str, LetBinding<'a>)> = Vec::new();
    let mut edges: BTreeMap<&'a str, Vec<&'a str>> = BTreeMap::new();
    let mut def_free_locals_list: Vec<(bool, FreeLocals<'a>)> = Vec::new();
    let mut errors = Vec::new();
    for def in defs {
        match canonicalize_let_def(bump, &inner_env, &def.value, &bindings, warnings) {
            Ok((def_nodes, has_args, def_free)) => {
                for (name, binding, deps) in def_nodes {
                    edges.insert(name, deps);
                    nodes.push((name, binding));
                }
                def_free_locals_list.push((has_args, def_free));
            }
            Err(errs) => errors.extend(errs),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut combined_free_locals = FreeLocals::new();
    let can_body = canonicalize_expr(bump, &inner_env, body, &mut combined_free_locals, warnings)?;

    for (has_args, def_free) in def_free_locals_list {
        merge_free_locals(&mut combined_free_locals, def_free, has_args);
    }

    let outer_free = verify_bindings(
        WarningContext::Def,
        &bindings,
        combined_free_locals,
        warnings,
    );
    merge_free_locals(free_locals, outer_free, false);

    let scc_nodes: Vec<scc::Node<'_, (&'a str, LetBinding<'a>)>> = nodes
        .into_iter()
        .map(|(name, binding)| {
            let deps = edges.remove(name).unwrap_or_default();
            scc::Node {
                key: name,
                value: (name, binding),
                deps,
            }
        })
        .collect();
    let sccs = scc::strongly_connected_components(scc_nodes);

    let mut result = can_body;
    for scc_group in sccs.into_iter().rev() {
        match scc_group {
            scc::Scc::Acyclic((_, binding)) => match binding {
                LetBinding::Define(def) => {
                    result = bump.alloc(Located::at(
                        region,
                        CanExpr::Let {
                            definition: def,
                            body: result,
                        },
                    ));
                }
                LetBinding::Destruct(pat, val) => {
                    result = bump.alloc(Located::at(
                        region,
                        CanExpr::LetDestruct {
                            pattern: pat,
                            value: val,
                            body: result,
                        },
                    ));
                }
                LetBinding::Edge(_) => {}
            },
            scc::Scc::Cyclic(pairs) => {
                let cycle_defs = check_let_cycle(bump, &pairs)?;
                result = bump.alloc(Located::at(
                    region,
                    CanExpr::LetRec {
                        definitions: bump.alloc_slice_fill_iter(cycle_defs),
                        body: result,
                    },
                ));
            }
        }
    }
    Ok(result)
}

fn collect_def_names<'a>(def: &SourceDef<'a>, out: &mut Vec<(&'a str, Region)>) {
    match def {
        SourceDef::Define { name, .. } => out.push((name.value, name.region)),
        SourceDef::Destruct { pattern, .. } => {
            collect_pattern_names(&pattern.value, pattern.region, out);
        }
    }
}

/// Mirrors Elm's `addBindingsHelp`: every binder in the pattern, in
/// traversal order, for duplicate detection across a `let` block.
fn collect_pattern_names<'a>(
    pat: &alder_source::Pattern<'a>,
    region: Region,
    out: &mut Vec<(&'a str, Region)>,
) {
    match pat {
        alder_source::Pattern::Var(name) => out.push((name, region)),
        alder_source::Pattern::Record(fields) => {
            for f in *fields {
                out.push((f.value, f.region));
            }
        }
        alder_source::Pattern::Alias { pattern, name } => {
            collect_pattern_names(&pattern.value, pattern.region, out);
            out.push((name.value, name.region));
        }
        alder_source::Pattern::Tuple {
            first,
            second,
            rest,
        } => {
            collect_pattern_names(&first.value, first.region, out);
            collect_pattern_names(&second.value, second.region, out);
            for r in *rest {
                collect_pattern_names(&r.value, r.region, out);
            }
        }
        alder_source::Pattern::Ctor { args, .. } | alder_source::Pattern::CtorQual { args, .. } => {
            for arg in *args {
                collect_pattern_names(&arg.value, arg.region, out);
            }
        }
        alder_source::Pattern::List(patterns) => {
            for p in *patterns {
                collect_pattern_names(&p.value, p.region, out);
            }
        }
        alder_source::Pattern::Cons { head, tail } => {
            collect_pattern_names(&head.value, head.region, out);
            collect_pattern_names(&tail.value, tail.region, out);
        }
        alder_source::Pattern::Anything
        | alder_source::Pattern::Unit
        | alder_source::Pattern::Str(_)
        | alder_source::Pattern::Int(_) => {}
    }
}

/// Mirrors Elm's `getPatternNames`, including its accumulation order
/// (names are PREPENDED as the pattern is traversed), because the list
/// head feeds `Name.fromManyNames` for the destructure node key.
fn get_pattern_names<'a>(
    mut names: Vec<(&'a str, Region)>,
    pat: &'a Located<alder_source::Pattern<'a>>,
) -> Vec<(&'a str, Region)> {
    match &pat.value {
        alder_source::Pattern::Var(name) => {
            names.insert(0, (name, pat.region));
            names
        }
        alder_source::Pattern::Record(fields) => {
            let mut out: Vec<(&'a str, Region)> =
                fields.iter().map(|f| (f.value, f.region)).collect();
            out.append(&mut names);
            out
        }
        alder_source::Pattern::Alias { pattern, name } => {
            names.insert(0, (name.value, name.region));
            get_pattern_names(names, pattern)
        }
        alder_source::Pattern::Tuple {
            first,
            second,
            rest,
        } => {
            let names = get_pattern_names(names, first);
            let names = get_pattern_names(names, second);
            rest.iter().fold(names, |acc, p| get_pattern_names(acc, p))
        }
        alder_source::Pattern::Ctor { args, .. } | alder_source::Pattern::CtorQual { args, .. } => {
            args.iter().fold(names, |acc, p| get_pattern_names(acc, p))
        }
        alder_source::Pattern::List(patterns) => patterns
            .iter()
            .fold(names, |acc, p| get_pattern_names(acc, p)),
        alder_source::Pattern::Cons { head, tail } => {
            let names = get_pattern_names(names, head);
            get_pattern_names(names, tail)
        }
        alder_source::Pattern::Anything
        | alder_source::Pattern::Unit
        | alder_source::Pattern::Str(_)
        | alder_source::Pattern::Int(_) => names,
    }
}

type LetDefResult<'a> = Result<
    (
        Vec<(&'a str, LetBinding<'a>, Vec<&'a str>)>,
        bool,
        FreeLocals<'a>,
    ),
    Vec<Error<'a>>,
>;

fn canonicalize_let_def<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    def: &SourceDef<'a>,
    let_bindings: &Bindings<'a>,
    warnings: &mut Vec<Warning<'a>>,
) -> LetDefResult<'a> {
    match def {
        SourceDef::Define {
            name,
            args,
            body,
            annotation,
        } => {
            // Mirrors Elm's `addDefNodes`: for typed defs the annotation is
            // resolved and matched against the arguments BEFORE the body is
            // canonicalized; either way one duplicate scope spans all args.
            let (can_def_builder, arg_bindings): (DefBuilder<'a>, Bindings<'a>) = if let Some(ann) =
                annotation
            {
                let annotation_val = types::to_annotation(bump, env, ann)?;
                let mut bound: Vec<(&'a str, Region)> = Vec::new();
                let (typed_args, result_type) =
                    gather_typed_args(bump, env, name.value, args, annotation_val.typ, &mut bound)?;
                let arg_bindings = pattern::detect_duplicates(
                    DuplicatePatternContext::FuncArgs(name.value),
                    bound,
                )?;
                (
                    DefBuilder::Typed {
                        free_vars: annotation_val.free_vars,
                        args: bump.alloc_slice_fill_iter(typed_args),
                        typ: result_type,
                    },
                    arg_bindings,
                )
            } else {
                let (can_args, arg_bindings) = pattern::verify_all(
                    bump,
                    env,
                    DuplicatePatternContext::FuncArgs(name.value),
                    args,
                )?;
                (
                    DefBuilder::Untyped {
                        args: bump.alloc_slice_fill_iter(can_args),
                    },
                    arg_bindings,
                )
            };

            let body_env = env.add_locals(&arg_bindings)?;
            let mut body_free_locals = FreeLocals::new();
            let can_body =
                canonicalize_expr(bump, &body_env, body, &mut body_free_locals, warnings)?;

            let def_free = verify_bindings(
                WarningContext::Pattern,
                &arg_bindings,
                body_free_locals,
                warnings,
            );

            let deps: Vec<&'a str> = def_free
                .keys()
                .filter(|k| let_bindings.contains_key(*k))
                .copied()
                .collect();

            let has_args = !args.is_empty();
            let can_def: &'a CanDef<'a> = match can_def_builder {
                DefBuilder::Typed {
                    free_vars,
                    args,
                    typ,
                } => bump.alloc(CanDef::TypedDef {
                    name,
                    free_vars,
                    args,
                    body: can_body,
                    typ,
                }),
                DefBuilder::Untyped { args } => bump.alloc(CanDef::Def {
                    name,
                    args,
                    body: can_body,
                }),
            };

            Ok((
                vec![(name.value, LetBinding::Define(can_def), deps)],
                has_args,
                def_free,
            ))
        }
        SourceDef::Destruct { pattern, body } => {
            let (can_pattern, _) =
                pattern::verify(bump, env, DuplicatePatternContext::Destruct, pattern)?;
            let mut body_free_locals = FreeLocals::new();
            let can_body = canonicalize_expr(bump, env, body, &mut body_free_locals, warnings)?;
            let deps: Vec<&'a str> = body_free_locals
                .keys()
                .filter(|k| let_bindings.contains_key(*k))
                .copied()
                .collect();

            // Elm keys the destructure node with `Name.fromManyNames`: the
            // head of `getPatternNames` prefixed by "_M$", which cannot
            // collide with a source-level name. Every bound name then gets
            // an Edge node pointing at the destructure, so a self-reference
            // like `let (a, b) = f a` forms a detectable cycle.
            let names = get_pattern_names(Vec::new(), pattern);
            let synthetic: &'a str = match names.first() {
                Some((n, _)) => bump.alloc_str(&format!("_M${n}")),
                None => "_M$",
            };

            let mut result = vec![(synthetic, LetBinding::Destruct(can_pattern, can_body), deps)];
            for (pname, pregion) in &names {
                result.push((
                    *pname,
                    LetBinding::Edge(bump.alloc(Located::at(*pregion, *pname))),
                    vec![synthetic],
                ));
            }
            Ok((result, false, body_free_locals))
        }
    }
}

enum DefBuilder<'a> {
    Typed {
        free_vars: alder_ast::FreeVars<'a>,
        args: &'a [CanTypedPattern<'a>],
        typ: &'a Located<CanType<'a>>,
    },
    Untyped {
        args: &'a [&'a Located<alder_ast::Pattern<'a>>],
    },
}

fn check_let_cycle<'a>(
    bump: &'a Bump,
    pairs: &[(&'a str, LetBinding<'a>)],
) -> Result<Vec<&'a CanDef<'a>>, Vec<Error<'a>>> {
    let mut defs: Vec<&'a CanDef<'a>> = Vec::new();
    for (position, (_, binding)) in pairs.iter().enumerate() {
        match binding {
            LetBinding::Define(def) => {
                let has_args = match def {
                    CanDef::Def { args, .. } => !args.is_empty(),
                    CanDef::TypedDef { args, .. } => !args.is_empty(),
                };
                if !has_args {
                    let def_name = match def {
                        CanDef::Def { name, .. } | CanDef::TypedDef { name, .. } => *name,
                    };
                    return Err(vec![Error::RecursiveLet {
                        name: def_name,
                        others: to_cycle_names(bump, &pairs[position + 1..], &defs),
                    }]);
                }
                defs.push(*def);
            }
            LetBinding::Edge(name_loc) => {
                return Err(vec![Error::RecursiveLet {
                    name: name_loc,
                    others: to_cycle_names(bump, &pairs[position + 1..], &defs),
                }]);
            }
            LetBinding::Destruct(..) => {}
        }
    }
    Ok(defs)
}

/// Mirrors Elm's `toNames`: the not-yet-visited bindings (skipping
/// destructure nodes, whose synthetic names are meaningless to users)
/// followed by the already-validated defs in source order.
fn to_cycle_names<'a>(
    bump: &'a Bump,
    rest: &[(&'a str, LetBinding<'a>)],
    defs: &[&'a CanDef<'a>],
) -> &'a [&'a str] {
    let mut names: Vec<&'a str> = Vec::new();
    for (_, binding) in rest {
        match binding {
            LetBinding::Define(def) => names.push(def_name(def)),
            LetBinding::Edge(name_loc) => names.push(name_loc.value),
            LetBinding::Destruct(..) => {}
        }
    }
    for def in defs {
        names.push(def_name(def));
    }
    bump.alloc_slice_fill_iter(names)
}

fn def_name<'a>(def: &CanDef<'a>) -> &'a str {
    match def {
        CanDef::Def { name, .. } | CanDef::TypedDef { name, .. } => name.value,
    }
}

/// Mirrors Elm's `gatherTypedArgs`: walk the annotation and the source
/// argument patterns together, canonicalizing each pattern against the
/// (iteratively dealiased) argument type. Binders land in `bound` so the
/// caller can run one duplicate check across all arguments.
pub fn gather_typed_args<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    func_name: &'a str,
    src_args: &'a [&'a Located<alder_source::Pattern<'a>>],
    annotation_typ: &'a Located<CanType<'a>>,
    bound: &mut Vec<(&'a str, Region)>,
) -> Result<(Vec<CanTypedPattern<'a>>, &'a Located<CanType<'a>>), Vec<Error<'a>>> {
    let mut typed_args = Vec::with_capacity(src_args.len());
    let mut current_type = annotation_typ;
    for (index, src_arg) in src_args.iter().enumerate() {
        let dealiased = types::iterated_dealias(bump, current_type);
        match &dealiased.value {
            CanType::Lambda { from, to } => {
                let pattern = pattern::canonicalize(bump, env, src_arg, bound)?;
                typed_args.push(CanTypedPattern { pattern, typ: from });
                current_type = to;
            }
            _ => {
                let start = src_args[index].region;
                let end = src_args
                    .last()
                    .expect("non-empty: inside for loop over src_args")
                    .region;
                return Err(vec![Error::AnnotationTooShort {
                    region: Region::span_across(&start, &end),
                    name: func_name,
                    index,
                    leftovers: src_args.len() - index,
                }]);
            }
        }
    }
    Ok((typed_args, current_type))
}
