use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{
    Attr as CanAttr, AttrValue as CanAttrValue, Block as CanBlock, Child as CanChild,
    ChildBlock as CanChildBlock, ChildIfBranch as CanChildIfBranch, ChildItem as CanChildItem,
    ChildMatchArm as CanChildMatchArm, Element as CanElement, ElementName as CanElementName,
    Expr as CanExpr, IfBranch as CanIfBranch, Join as CanJoin, LocalLet, Markup as CanMarkup,
    MatchArm as CanMatchArm, Order as CanOrder, Param as CanParam, Place as CanPlace,
    PlaceStep as CanPlaceStep, Projection as CanProjection, Query as CanQuery,
    RecordField as CanRecordField, Select as CanSelect, Stmt as CanStmt, Style as CanStyle,
    StyleEntry as CanStyleEntry, StyleKey as CanStyleKey, StyleValue as CanStyleValue,
    TableRef as CanTableRef, TemplatePart as CanTemplatePart, ValueRef,
};
use alder_region::{Located, Region};
use alder_source::{Expr as SourceExpr, Stmt as SourceStmt};
use bumpalo::Bump;

use crate::environment::Env;
use crate::pattern::{BindingMode, canonicalize_pattern};
use crate::types::{canonicalize_type, is_task_type};
use crate::{Error, ErrorKind, ExprError, NameError, StmtError};

pub fn canonicalize_expr<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a Located<SourceExpr<'a>>,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    let expr = match source.value {
        SourceExpr::Number(number) => CanExpr::Number {
            value: number.value,
            text: number.text,
        },
        SourceExpr::BigInt(value) => CanExpr::BigInt(value),
        SourceExpr::Str(value) => CanExpr::Str(value),
        SourceExpr::Bool(value) => CanExpr::Bool(value),
        SourceExpr::Template(parts) => {
            CanExpr::Template(canonicalize_template_parts(bump, env, parts)?)
        }
        SourceExpr::TaggedTemplate { tag, parts } => CanExpr::TaggedTemplate {
            tag: canonicalize_expr(bump, env, tag)?,
            parts: canonicalize_template_parts(bump, env, parts)?,
        },
        SourceExpr::Unit => CanExpr::Unit,
        SourceExpr::Var(name) => {
            if env.control.query_depth > 0 {
                return Ok(bump.alloc(Located::at(
                    source.region,
                    CanExpr::Var(ValueRef::QueryName(name)),
                )));
            }
            if let Some(module) = env.find_module(name) {
                return Ok(bump.alloc(Located::at(
                    source.region,
                    CanExpr::Var(ValueRef::Module(module.module)),
                )));
            }
            if env.control.opaque_names_depth > 0 {
                return Ok(bump.alloc(Located::at(
                    source.region,
                    CanExpr::Var(ValueRef::Opaque(name)),
                )));
            }
            let Some(binding) = env.find_value(name) else {
                return Err(vec![unknown_value(source.region, name)]);
            };
            CanExpr::Var(binding.reference)
        }
        SourceExpr::Path(path) => canonicalize_path_expr(bump, env, source.region, path)?,
        SourceExpr::PathVar { path, name } => {
            let Some(module) = path
                .segments
                .first()
                .and_then(|segment| env.find_module(segment.value))
            else {
                return Err(vec![unknown_value(path.region(), name.value)]);
            };
            if let Some(interface) = module.interface {
                if let Some(value) = interface
                    .values
                    .iter()
                    .find(|value| value.exported_as == name.value)
                {
                    CanExpr::Var(ValueRef::Foreign {
                        reference: value.reference,
                        annotation: value.annotation,
                    })
                } else {
                    return Err(vec![unknown_value(name.region, name.value)]);
                }
            } else if module.module.package == alder_ast::PackageId::Builtin {
                CanExpr::Var(ValueRef::Builtin(alder_ast::QualifiedName {
                    module: module.module,
                    name: name.value,
                }))
            } else {
                CanExpr::Access {
                    record: bump.alloc(Located::at(
                        path.region(),
                        CanExpr::Var(ValueRef::Module(module.module)),
                    )),
                    field: name,
                }
            }
        }
        SourceExpr::Tag { name, args } => CanExpr::Tag {
            group: None,
            name,
            args: canonicalize_exprs(bump, env, args)?,
        },
        SourceExpr::Placeholder => {
            return Err(vec![Error::new(
                source.region,
                ErrorKind::Expr(ExprError::PlaceholderOutsideCall),
            )]);
        }
        SourceExpr::Array(items) => CanExpr::Array(canonicalize_exprs(bump, env, items)?),
        SourceExpr::Tuple {
            first,
            second,
            rest,
        } => {
            let mut items = Vec::with_capacity(2 + rest.len());
            items.push(canonicalize_expr(bump, env, first)?);
            items.push(canonicalize_expr(bump, env, second)?);
            for item in rest {
                items.push(canonicalize_expr(bump, env, item)?);
            }
            CanExpr::Tuple(bump.alloc_slice_copy(&items))
        }
        SourceExpr::Record(fields) => {
            CanExpr::Record(canonicalize_record_fields(bump, env, fields)?)
        }
        SourceExpr::RecordCtor { path, fields } => {
            let constructor = env
                .find_constructor(bump, source.region, path.segments, false)
                .map_err(|error| vec![error])?;
            CanExpr::RecordConstructor {
                constructor,
                fields: canonicalize_record_fields(bump, env, fields)?,
            }
        }
        SourceExpr::Call {
            function,
            arguments,
        } => return canonicalize_call(bump, env, source.region, function, arguments),
        SourceExpr::Access { record, field } => {
            let module_name = match record.value {
                SourceExpr::Var(name) => Some(name),
                SourceExpr::Path(path) if path.segments.len() == 1 => Some(path.segments[0].value),
                _ => None,
            };
            if let Some(module_name) = module_name
                && let Some(module) = env.find_module(module_name)
            {
                if let Some(interface) = module.interface {
                    if let Some(value) = interface
                        .values
                        .iter()
                        .find(|value| value.exported_as == field.value)
                    {
                        CanExpr::Var(ValueRef::Foreign {
                            reference: value.reference,
                            annotation: value.annotation,
                        })
                    } else {
                        return Err(vec![unknown_value(field.region, field.value)]);
                    }
                } else if module.module.package == alder_ast::PackageId::Builtin {
                    CanExpr::Var(ValueRef::Builtin(alder_ast::QualifiedName {
                        module: module.module,
                        name: field.value,
                    }))
                } else {
                    CanExpr::Access {
                        record: canonicalize_expr(bump, env, record)?,
                        field,
                    }
                }
            } else {
                CanExpr::Access {
                    record: canonicalize_expr(bump, env, record)?,
                    field,
                }
            }
        }
        SourceExpr::TupleAccess { tuple, index } => CanExpr::TupleAccess {
            tuple: canonicalize_expr(bump, env, tuple)?,
            index,
        },
        SourceExpr::Index { target, index } => CanExpr::Index {
            target: canonicalize_expr(bump, env, target)?,
            index: canonicalize_expr(bump, env, index)?,
        },
        SourceExpr::Await(expr) => {
            if !env.control.task_return {
                return Err(vec![Error::new(
                    source.region,
                    ErrorKind::Expr(ExprError::AwaitRequiresTaskReturn),
                )]);
            }
            CanExpr::Await(canonicalize_expr(bump, env, expr)?)
        }
        SourceExpr::Try(expr) => CanExpr::Try(canonicalize_expr(bump, env, expr)?),
        SourceExpr::Negate(expr) => CanExpr::Negate(canonicalize_expr(bump, env, expr)?),
        SourceExpr::Not(expr) => CanExpr::Not(canonicalize_expr(bump, env, expr)?),
        SourceExpr::Pin(expr) => {
            if env.control.query_depth == 0 {
                return Err(vec![Error::new(
                    source.region,
                    ErrorKind::Expr(ExprError::PinOutsideQuery),
                )]);
            }
            env.control.query_depth -= 1;
            let result = canonicalize_expr(bump, env, expr);
            env.control.query_depth += 1;
            let expr = result?;
            CanExpr::Pin(expr)
        }
        SourceExpr::BinOps { operands, last } => {
            return canonicalize_binops(bump, env, source.region, operands, last);
        }
        SourceExpr::Block(block) => CanExpr::Block(canonicalize_block(bump, env, block)?),
        SourceExpr::Lambda(lambda) => return canonicalize_lambda(bump, env, source.region, lambda),
        SourceExpr::If {
            branches,
            final_else,
        } => {
            let mut canonical = Vec::with_capacity(branches.len());
            for branch in branches {
                canonical.push(CanIfBranch {
                    condition: canonicalize_expr(bump, env, branch.condition)?,
                    body: canonicalize_block(bump, env, branch.body)?,
                });
            }
            CanExpr::If {
                branches: bump.alloc_slice_copy(&canonical),
                final_else: match final_else {
                    Some(block) => Some(canonicalize_block(bump, env, block)?),
                    None => None,
                },
            }
        }
        SourceExpr::Match { scrutinee, arms } => {
            let scrutinee = canonicalize_expr(bump, env, scrutinee)?;
            let mut canonical = Vec::with_capacity(arms.len());
            for arm in arms {
                env.push_scope();
                env.control.match_depth += 1;
                let mut patterns = Vec::with_capacity(arm.patterns.len());
                if let Some((first, rest)) = arm.patterns.split_first() {
                    let base = env.clone();
                    patterns.push(canonicalize_pattern(
                        bump,
                        env,
                        first,
                        BindingMode::Local { mutable: false },
                    )?);
                    for pattern in rest {
                        let mut alternative_env = base.clone();
                        patterns.push(canonicalize_pattern(
                            bump,
                            &mut alternative_env,
                            pattern,
                            BindingMode::Local { mutable: false },
                        )?);
                    }
                }
                let guard = match arm.guard {
                    Some(guard) => Some(canonicalize_expr(bump, env, guard)?),
                    None => None,
                };
                let body = canonicalize_expr(bump, env, arm.body)?;
                env.control.match_depth -= 1;
                env.pop_scope();
                canonical.push(CanMatchArm {
                    patterns: bump.alloc_slice_copy(&patterns),
                    guard,
                    body,
                });
            }
            CanExpr::Match {
                scrutinee,
                arms: bump.alloc_slice_copy(&canonical),
            }
        }
        SourceExpr::Loop(block) => {
            env.control.loop_depth += 1;
            let block = canonicalize_block(bump, env, block)?;
            env.control.loop_depth -= 1;
            CanExpr::Loop(block)
        }
        SourceExpr::Provide { name, value, body } => {
            let provider_name = name.segments.last().expect("provider path is nonempty");
            let provider_type = env
                .find_type(
                    bump,
                    name.region(),
                    (name.segments.len() > 1).then(|| name.segments[0].value),
                    provider_name.value,
                )
                .map_err(|error| vec![error])?;
            let value = canonicalize_expr(bump, env, value)?;
            env.providers.push(BTreeMap::from([(
                provider_name.value,
                provider_type.reference,
            )]));
            let body = canonicalize_block(bump, env, body)?;
            env.providers.pop();
            CanExpr::Provide {
                provider: provider_type.reference,
                value,
                body,
            }
        }
        SourceExpr::State(expr) => CanExpr::State(canonicalize_expr(bump, env, expr)?),
        SourceExpr::Style(style) => CanExpr::Style(canonicalize_style(bump, env, style)?),
        SourceExpr::Query(query) => CanExpr::Query(canonicalize_query(bump, env, query)?),
        SourceExpr::Markup(markup) => CanExpr::Markup(canonicalize_markup(bump, env, markup)?),
        SourceExpr::MacroCall { name, .. } => {
            return Err(vec![Error::new(
                source.region,
                ErrorKind::Expr(ExprError::MacroUnavailable { name: name.value }),
            )]);
        }
    };
    Ok(bump.alloc(Located::at(source.region, expr)))
}

pub fn canonicalize_block<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a Located<alder_source::Block<'a>>,
) -> Result<&'a Located<CanBlock<'a>>, Vec<Error<'a>>> {
    env.push_scope();
    let mut statements = Vec::with_capacity(source.value.stmts.len());
    for statement in source.value.stmts {
        statements.push(canonicalize_stmt(bump, env, statement)?);
    }
    let tail = match source.value.tail {
        Some(tail) => Some(canonicalize_expr(bump, env, tail)?),
        None => None,
    };
    env.pop_scope();
    Ok(bump.alloc(Located::at(
        source.region,
        CanBlock {
            statements: bump.alloc_slice_copy(&statements),
            tail,
        },
    )))
}

pub(crate) fn canonicalize_stmt<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a Located<SourceStmt<'a>>,
) -> Result<&'a Located<CanStmt<'a>>, Vec<Error<'a>>> {
    let statement = match source.value {
        SourceStmt::Let(decl) => {
            let value = canonicalize_expr(bump, env, decl.value)?;
            let annotation = match decl.annotation {
                Some(typ) => Some(canonicalize_type(bump, env, &BTreeSet::new(), typ)?),
                None => None,
            };
            let pattern = canonicalize_pattern(
                bump,
                env,
                decl.pattern,
                BindingMode::Local {
                    mutable: decl.mutable.is_some(),
                },
            )?;
            CanStmt::Let(bump.alloc(LocalLet {
                mutable: decl.mutable.is_some(),
                pattern,
                annotation,
                value,
            }))
        }
        SourceStmt::Use(path) => {
            let name = path.segments.last().expect("provider path is nonempty");
            let provider = env
                .find_type(
                    bump,
                    path.region(),
                    (path.segments.len() > 1).then(|| path.segments[0].value),
                    name.value,
                )
                .map_err(|error| vec![error])?;
            CanStmt::Use {
                provider: provider.reference,
            }
        }
        SourceStmt::Assign { place, op, value } => {
            let Some(binding) = env.find_value(place.value.root.value) else {
                return Err(vec![unknown_statement_name(
                    place.value.root.region,
                    place.value.root.value,
                )]);
            };
            if !binding.mutable {
                return Err(vec![Error::new(
                    place.value.root.region,
                    ErrorKind::Stmt(StmtError::ImmutableAssignment {
                        name: place.value.root.value,
                        binding: binding.region,
                    }),
                )]);
            }
            let root = match binding.reference {
                ValueRef::Local(local) => alder_ast::BindingName::Local(local),
                ValueRef::TopLevel(top) => alder_ast::BindingName::TopLevel(top),
                _ => {
                    return Err(vec![Error::new(
                        place.value.root.region,
                        ErrorKind::Stmt(StmtError::InvalidAssignmentTarget),
                    )]);
                }
            };
            let mut steps = Vec::with_capacity(place.value.steps.len());
            for step in place.value.steps {
                steps.push(match step {
                    alder_source::PlaceStep::Field(name) => CanPlaceStep::Field(*name),
                    alder_source::PlaceStep::TupleIndex(index) => CanPlaceStep::TupleIndex(*index),
                    alder_source::PlaceStep::Index(index) => {
                        CanPlaceStep::Index(canonicalize_expr(bump, env, index)?)
                    }
                });
            }
            CanStmt::Assign {
                place: bump.alloc(CanPlace {
                    root,
                    root_region: place.value.root.region,
                    mutable: true,
                    steps: bump.alloc_slice_copy(&steps),
                }),
                op,
                value: canonicalize_expr(bump, env, value)?,
            }
        }
        SourceStmt::For {
            pattern,
            iter,
            body,
        } => {
            let iter = canonicalize_expr(bump, env, iter)?;
            env.push_scope();
            let pattern =
                canonicalize_pattern(bump, env, pattern, BindingMode::Local { mutable: false })?;
            env.control.loop_depth += 1;
            let body = canonicalize_block(bump, env, body)?;
            env.control.loop_depth -= 1;
            env.pop_scope();
            CanStmt::For {
                pattern,
                iter,
                body,
            }
        }
        SourceStmt::While { condition, body } => {
            let condition = canonicalize_expr(bump, env, condition)?;
            env.control.loop_depth += 1;
            let body = canonicalize_block(bump, env, body)?;
            env.control.loop_depth -= 1;
            CanStmt::While { condition, body }
        }
        SourceStmt::Return(value) => {
            if env.control.function_depth == 0 {
                return Err(vec![Error::new(
                    source.region,
                    ErrorKind::Stmt(StmtError::ReturnOutsideFunction),
                )]);
            }
            CanStmt::Return(match value {
                Some(value) => Some(canonicalize_expr(bump, env, value)?),
                None => None,
            })
        }
        SourceStmt::Break(value) => {
            if env.control.loop_depth == 0 {
                return Err(vec![Error::new(
                    source.region,
                    ErrorKind::Stmt(StmtError::BreakOutsideLoop),
                )]);
            }
            CanStmt::Break(match value {
                Some(value) => Some(canonicalize_expr(bump, env, value)?),
                None => None,
            })
        }
        SourceStmt::Continue => {
            if env.control.loop_depth == 0 {
                return Err(vec![Error::new(
                    source.region,
                    ErrorKind::Stmt(StmtError::ContinueOutsideLoop),
                )]);
            }
            CanStmt::Continue
        }
        SourceStmt::Assert(expr) => CanStmt::Assert(canonicalize_expr(bump, env, expr)?),
        SourceStmt::Expr(expr) => CanStmt::Expr(canonicalize_expr(bump, env, expr)?),
    };
    Ok(bump.alloc(Located::at(source.region, statement)))
}

fn canonicalize_path_expr<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    region: Region,
    path: alder_source::Path<'a>,
) -> Result<CanExpr<'a>, Vec<Error<'a>>> {
    if path.segments.len() == 1 {
        let name = path.segments[0].value;
        if let Some(provider) = env.find_provider(name) {
            return Ok(CanExpr::Var(ValueRef::Provider(provider)));
        }
        if let Some(module) = env.find_module(name) {
            return Ok(CanExpr::Var(ValueRef::Module(module.module)));
        }
        if let Some((enum_name, variant)) = env.enums.values().find_map(|candidate| {
            let crate::environment::Candidate::Unique(enum_) = candidate else {
                return None;
            };
            enum_
                .variants
                .iter()
                .find(|variant| variant.name.variant == name)
                .map(|variant| (enum_.reference.name, variant.name.variant))
        }) {
            return Err(vec![Error::new(
                region,
                ErrorKind::Expr(ExprError::UnqualifiedConstructor { enum_name, variant }),
            )]);
        }
    }
    env.find_constructor(bump, region, path.segments, false)
        .map(CanExpr::Constructor)
        .map_err(|error| vec![error])
}

fn canonicalize_call<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    region: Region,
    function: &'a Located<SourceExpr<'a>>,
    arguments: &'a [&'a Located<SourceExpr<'a>>],
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    let function = canonicalize_expr(bump, env, function)?;
    let mut params = Vec::new();
    let mut args: Vec<&'a Located<CanExpr<'a>>> = Vec::with_capacity(arguments.len());
    for argument in arguments {
        if matches!(argument.value, SourceExpr::Placeholder) {
            let text = bump.alloc_str(&format!("_{}", params.len()));
            let local = env.fresh_local(text);
            let pattern = bump.alloc(Located::at(
                argument.region,
                alder_ast::Pattern::Bind(alder_ast::BindingName::Local(local)),
            ));
            params.push(CanParam {
                mutable: false,
                pattern,
                annotation: None,
            });
            let argument: &'a Located<CanExpr<'a>> = bump.alloc(Located::at(
                argument.region,
                CanExpr::Var(ValueRef::Local(local)),
            ));
            args.push(argument);
        } else {
            args.push(canonicalize_expr(bump, env, argument)?);
        }
    }
    let call = bump.alloc(Located::at(
        region,
        CanExpr::Call {
            function,
            arguments: bump.alloc_slice_copy(&args),
        },
    ));
    if params.is_empty() {
        Ok(call)
    } else {
        Ok(bump.alloc(Located::at(
            region,
            CanExpr::Lambda {
                params: bump.alloc_slice_copy(&params),
                ret: None,
                body: call,
            },
        )))
    }
}

fn canonicalize_lambda<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    region: Region,
    lambda: &'a alder_source::Lambda<'a>,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    env.push_scope();
    let saved_control = env.control;
    env.control.function_depth += 1;
    env.control.loop_depth = 0;
    let mut params = Vec::with_capacity(lambda.params.len());
    for param in lambda.params {
        let annotation = match param.annotation {
            Some(typ) => Some(canonicalize_type(bump, env, &BTreeSet::new(), typ)?),
            None => None,
        };
        let pattern = canonicalize_pattern(
            bump,
            env,
            param.pattern,
            BindingMode::Local {
                mutable: param.mutable.is_some(),
            },
        )?;
        params.push(CanParam {
            mutable: param.mutable.is_some(),
            pattern,
            annotation,
        });
    }
    let ret = match lambda.ret {
        Some(ret) => Some(canonicalize_type(bump, env, &BTreeSet::new(), ret)?),
        None => None,
    };
    env.control.task_return = ret.is_some_and(is_task_type);
    let body = canonicalize_expr(bump, env, lambda.body)?;
    env.control = saved_control;
    env.pop_scope();
    Ok(bump.alloc(Located::at(
        region,
        CanExpr::Lambda {
            params: bump.alloc_slice_copy(&params),
            ret,
            body,
        },
    )))
}

fn canonicalize_exprs<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a [&'a Located<SourceExpr<'a>>],
) -> Result<&'a [&'a Located<CanExpr<'a>>], Vec<Error<'a>>> {
    let mut result = Vec::with_capacity(source.len());
    for expr in source {
        result.push(canonicalize_expr(bump, env, expr)?);
    }
    Ok(bump.alloc_slice_copy(&result))
}

fn canonicalize_template_parts<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a [alder_source::TemplatePart<'a>],
) -> Result<&'a [CanTemplatePart<'a>], Vec<Error<'a>>> {
    let mut result = Vec::with_capacity(source.len());
    for part in source {
        result.push(match part {
            alder_source::TemplatePart::Text(text) => CanTemplatePart::Text(text),
            alder_source::TemplatePart::Expr(expr) => {
                CanTemplatePart::Expr(canonicalize_expr(bump, env, expr)?)
            }
        });
    }
    Ok(bump.alloc_slice_copy(&result))
}

fn canonicalize_record_fields<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a [alder_source::RecordField<'a>],
) -> Result<&'a [CanRecordField<'a>], Vec<Error<'a>>> {
    let mut result = Vec::with_capacity(source.len());
    let mut seen = BTreeMap::new();
    for field in source {
        result.push(match field {
            alder_source::RecordField::Field { name, value } => {
                if let Some(first) = seen.insert(name.value, name.region) {
                    return Err(vec![Error::new(
                        name.region,
                        ErrorKind::Expr(ExprError::DuplicateField {
                            name: name.value,
                            first,
                        }),
                    )]);
                }
                let value = match value {
                    Some(value) => canonicalize_expr(bump, env, value)?,
                    None => {
                        let Some(binding) = env.find_value(name.value) else {
                            return Err(vec![unknown_value(name.region, name.value)]);
                        };
                        bump.alloc(Located::at(name.region, CanExpr::Var(binding.reference)))
                    }
                };
                CanRecordField::Field { name: *name, value }
            }
            alder_source::RecordField::Spread(expr) => {
                CanRecordField::Spread(canonicalize_expr(bump, env, expr)?)
            }
        });
    }
    Ok(bump.alloc_slice_copy(&result))
}

fn canonicalize_style<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::Style<'a>,
) -> Result<&'a CanStyle<'a>, Vec<Error<'a>>> {
    let mut entries = Vec::with_capacity(source.entries.len());
    for entry in source.entries {
        let key = Located::at(
            entry.key.region,
            match entry.key.value {
                alder_source::StyleKey::Ident(name) => CanStyleKey::Ident(name),
                alder_source::StyleKey::Str(value) => CanStyleKey::Str(value),
            },
        );
        let value = match entry.value {
            alder_source::StyleValue::Dimension { number, unit } => CanStyleValue::Dimension {
                value: number.value,
                text: number.text,
                unit,
            },
            alder_source::StyleValue::Expr(expr) => {
                CanStyleValue::Expr(canonicalize_expr(bump, env, expr)?)
            }
            alder_source::StyleValue::Nested(style) => {
                CanStyleValue::Nested(canonicalize_style(bump, env, style)?)
            }
        };
        entries.push(CanStyleEntry { key, value });
    }
    Ok(bump.alloc(CanStyle {
        entries: bump.alloc_slice_copy(&entries),
    }))
}

fn canonicalize_query<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::Query<'a>,
) -> Result<&'a CanQuery<'a>, Vec<Error<'a>>> {
    env.control.query_depth += 1;
    let result: Result<&'a CanQuery<'a>, Vec<Error<'a>>> = (|| {
        let query = match source {
            alder_source::Query::Select(select) => {
                let projection = match select.projection {
                    alder_source::Projection::Star(region) => CanProjection::Star(region),
                    alder_source::Projection::Fields(fields) => {
                        CanProjection::Fields(canonicalize_exprs(bump, env, fields)?)
                    }
                };
                let from = canonicalize_table_ref(bump, env, select.from)?;
                let mut joins = Vec::with_capacity(select.joins.len());
                for join in select.joins {
                    joins.push(CanJoin {
                        kind: join.kind,
                        table: canonicalize_table_ref(bump, env, join.table)?,
                        on: canonicalize_expr(bump, env, join.on)?,
                    });
                }
                let where_ = canonicalize_optional_expr(bump, env, select.where_)?;
                let group_by = canonicalize_exprs(bump, env, select.group_by)?;
                let mut order_by = Vec::with_capacity(select.order_by.len());
                for order in select.order_by {
                    order_by.push(CanOrder {
                        expr: canonicalize_expr(bump, env, order.expr)?,
                        direction: order.direction,
                    });
                }
                CanQuery::Select(bump.alloc(CanSelect {
                    projection,
                    from,
                    joins: bump.alloc_slice_copy(&joins),
                    where_,
                    group_by,
                    order_by: bump.alloc_slice_copy(&order_by),
                    limit: canonicalize_optional_expr(bump, env, select.limit)?,
                    offset: canonicalize_optional_expr(bump, env, select.offset)?,
                }))
            }
            alder_source::Query::Insert { table, values } => CanQuery::Insert {
                table: canonicalize_table_name(bump, env, *table)?,
                values: canonicalize_expr(bump, env, values)?,
            },
            alder_source::Query::Update { table, set, where_ } => CanQuery::Update {
                table: canonicalize_table_name(bump, env, *table)?,
                set: canonicalize_record_fields(bump, env, set)?,
                where_: canonicalize_optional_expr(bump, env, *where_)?,
            },
            alder_source::Query::Delete { table, where_ } => CanQuery::Delete {
                table: canonicalize_table_name(bump, env, *table)?,
                where_: canonicalize_optional_expr(bump, env, *where_)?,
            },
        };
        Ok(&*bump.alloc(query))
    })();
    env.control.query_depth -= 1;
    result
}

fn canonicalize_table_name<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    name: alder_source::Name<'a>,
) -> Result<alder_ast::QualifiedName<'a>, Vec<Error<'a>>> {
    env.find_type(bump, name.region, None, name.value)
        .map(|binding| binding.reference)
        .map_err(|error| vec![error])
}

fn canonicalize_table_ref<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    source: alder_source::TableRef<'a>,
) -> Result<CanTableRef<'a>, Vec<Error<'a>>> {
    Ok(CanTableRef {
        table: canonicalize_table_name(bump, env, source.name)?,
        alias: source.alias,
    })
}

fn canonicalize_optional_expr<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: Option<&'a Located<SourceExpr<'a>>>,
) -> Result<Option<&'a Located<CanExpr<'a>>>, Vec<Error<'a>>> {
    source
        .map(|expr| canonicalize_expr(bump, env, expr))
        .transpose()
}

fn canonicalize_markup<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::Markup<'a>,
) -> Result<&'a CanMarkup<'a>, Vec<Error<'a>>> {
    let markup = match source {
        alder_source::Markup::Element(element) => {
            CanMarkup::Element(canonicalize_element(bump, env, element)?)
        }
        alder_source::Markup::Fragment(children) => {
            CanMarkup::Fragment(canonicalize_children(bump, env, children)?)
        }
    };
    Ok(bump.alloc(markup))
}

fn canonicalize_element<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a alder_source::Element<'a>,
) -> Result<&'a CanElement<'a>, Vec<Error<'a>>> {
    let name = Located::at(
        source.name.region,
        match source.name.value {
            alder_source::ElementName::Tag(name) => CanElementName::Tag(name),
            alder_source::ElementName::Component(path) => {
                CanElementName::Component(resolve_component(bump, env, path)?)
            }
        },
    );
    let mut attrs = Vec::with_capacity(source.attrs.len());
    for attr in source.attrs {
        attrs.push(CanAttr {
            name: attr.name,
            value: match attr.value {
                None => None,
                Some(alder_source::AttrValue::Str(value)) => Some(CanAttrValue::Str(value)),
                Some(alder_source::AttrValue::Expr(expr)) => {
                    Some(CanAttrValue::Expr(canonicalize_expr(bump, env, expr)?))
                }
            },
        });
    }
    Ok(bump.alloc(CanElement {
        name,
        attrs: bump.alloc_slice_copy(&attrs),
        children: canonicalize_children(bump, env, source.children)?,
        self_closing: source.self_closing,
    }))
}

fn resolve_component<'a>(
    bump: &'a Bump,
    env: &Env<'a>,
    path: alder_source::Path<'a>,
) -> Result<alder_ast::QualifiedName<'a>, Vec<Error<'a>>> {
    let name = path.segments.last().expect("component path is nonempty");
    if path.segments.len() == 1 {
        if let Some(binding) = env.find_value(name.value) {
            return match binding.reference {
                ValueRef::TopLevel(reference) | ValueRef::Foreign { reference, .. } => {
                    Ok(reference)
                }
                _ => Err(vec![unknown_value(name.region, name.value)]),
            };
        }
    } else if let Some(module) = env.find_module(path.segments[0].value) {
        return Ok(alder_ast::QualifiedName {
            module: module.module,
            name: name.value,
        });
    }
    Err(vec![
        env.find_type(
            bump,
            path.region(),
            (path.segments.len() > 1).then(|| path.segments[0].value),
            name.value,
        )
        .err()
        .unwrap_or_else(|| unknown_value(name.region, name.value)),
    ])
}

fn canonicalize_children<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a [&'a Located<alder_source::Child<'a>>],
) -> Result<&'a [&'a Located<CanChild<'a>>], Vec<Error<'a>>> {
    let mut children = Vec::with_capacity(source.len());
    for child in source {
        children.push(canonicalize_child(bump, env, child)?);
    }
    Ok(bump.alloc_slice_copy(&children))
}

fn canonicalize_child<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a Located<alder_source::Child<'a>>,
) -> Result<&'a Located<CanChild<'a>>, Vec<Error<'a>>> {
    let child = match source.value {
        alder_source::Child::Element(element) => {
            CanChild::Element(canonicalize_element(bump, env, element)?)
        }
        alder_source::Child::Fragment(children) => {
            CanChild::Fragment(canonicalize_children(bump, env, children)?)
        }
        alder_source::Child::Text(text) => CanChild::Text(text),
        alder_source::Child::Hole(expr) => CanChild::Hole(canonicalize_expr(bump, env, expr)?),
        alder_source::Child::If {
            branches,
            final_else,
        } => {
            let mut canonical = Vec::with_capacity(branches.len());
            for branch in branches {
                canonical.push(CanChildIfBranch {
                    condition: canonicalize_expr(bump, env, branch.condition)?,
                    body: canonicalize_child_block(bump, env, branch.body)?,
                });
            }
            CanChild::If {
                branches: bump.alloc_slice_copy(&canonical),
                final_else: final_else
                    .map(|block| canonicalize_child_block(bump, env, block))
                    .transpose()?,
            }
        }
        alder_source::Child::For {
            pattern,
            iter,
            key,
            body,
            empty,
        } => {
            let iter = canonicalize_expr(bump, env, iter)?;
            env.push_scope();
            let pattern =
                canonicalize_pattern(bump, env, pattern, BindingMode::Local { mutable: false })?;
            let key = canonicalize_optional_expr(bump, env, key)?;
            let body = canonicalize_child_block(bump, env, body)?;
            env.pop_scope();
            CanChild::For {
                pattern,
                iter,
                key,
                body,
                empty: empty
                    .map(|block| canonicalize_child_block(bump, env, block))
                    .transpose()?,
            }
        }
        alder_source::Child::Match { scrutinee, arms } => {
            let scrutinee = canonicalize_expr(bump, env, scrutinee)?;
            let mut canonical = Vec::with_capacity(arms.len());
            for arm in arms {
                env.push_scope();
                env.control.match_depth += 1;
                let base = env.clone();
                let mut patterns = Vec::with_capacity(arm.patterns.len());
                if let Some((first, rest)) = arm.patterns.split_first() {
                    patterns.push(canonicalize_pattern(
                        bump,
                        env,
                        first,
                        BindingMode::Local { mutable: false },
                    )?);
                    for pattern in rest {
                        let mut alternative_env = base.clone();
                        patterns.push(canonicalize_pattern(
                            bump,
                            &mut alternative_env,
                            pattern,
                            BindingMode::Local { mutable: false },
                        )?);
                    }
                }
                let guard = canonicalize_optional_expr(bump, env, arm.guard)?;
                let body = canonicalize_child_block(bump, env, arm.body)?;
                env.control.match_depth -= 1;
                env.pop_scope();
                canonical.push(CanChildMatchArm {
                    patterns: bump.alloc_slice_copy(&patterns),
                    guard,
                    body,
                });
            }
            CanChild::Match {
                scrutinee,
                arms: bump.alloc_slice_copy(&canonical),
            }
        }
    };
    Ok(bump.alloc(Located::at(source.region, child)))
}

fn canonicalize_child_block<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    source: &'a Located<alder_source::ChildBlock<'a>>,
) -> Result<&'a Located<CanChildBlock<'a>>, Vec<Error<'a>>> {
    env.push_scope();
    let mut items = Vec::with_capacity(source.value.items.len());
    for item in source.value.items {
        items.push(match item {
            alder_source::ChildItem::Stmt(stmt) => {
                CanChildItem::Stmt(canonicalize_stmt(bump, env, stmt)?)
            }
            alder_source::ChildItem::Child(child) => {
                CanChildItem::Child(canonicalize_child(bump, env, child)?)
            }
        });
    }
    env.pop_scope();
    Ok(bump.alloc(Located::at(
        source.region,
        CanChildBlock {
            items: bump.alloc_slice_copy(&items),
        },
    )))
}

fn canonicalize_binops<'a>(
    bump: &'a Bump,
    env: &mut Env<'a>,
    region: Region,
    operands: &'a [alder_source::BinOpOperand<'a>],
    last: &'a Located<SourceExpr<'a>>,
) -> Result<&'a Located<CanExpr<'a>>, Vec<Error<'a>>> {
    let mut expressions = Vec::with_capacity(operands.len() + 1);
    let mut operators: Vec<Located<alder_source::BinOp>> = Vec::new();
    for operand in operands {
        expressions.push(canonicalize_expr(bump, env, operand.expr)?);
        let (precedence, associativity) = operand.op.value.precedence();
        while let Some(top) = operators.last().copied() {
            let (top_precedence, top_associativity) = top.value.precedence();
            if top_precedence == precedence
                && (matches!(associativity, alder_source::Associativity::None)
                    || matches!(top_associativity, alder_source::Associativity::None))
            {
                return Err(vec![Error::new(
                    operand.op.region,
                    ErrorKind::Expr(ExprError::NonAssociativeOperators {
                        left: top.value.as_str(),
                        right: operand.op.value.as_str(),
                    }),
                )]);
            }
            let reduce = top_precedence > precedence
                || (top_precedence == precedence
                    && matches!(associativity, alder_source::Associativity::Left));
            if !reduce {
                break;
            }
            reduce_binop(bump, &mut expressions, operators.pop().unwrap());
        }
        operators.push(operand.op);
    }
    expressions.push(canonicalize_expr(bump, env, last)?);
    while let Some(operator) = operators.pop() {
        reduce_binop(bump, &mut expressions, operator);
    }
    let expression = expressions.pop().expect("binop has an expression");
    debug_assert_eq!(expression.region, region);
    Ok(expression)
}

fn reduce_binop<'a>(
    bump: &'a Bump,
    expressions: &mut Vec<&'a Located<CanExpr<'a>>>,
    op: Located<alder_source::BinOp>,
) {
    let right = expressions.pop().expect("operator has right operand");
    let left = expressions.pop().expect("operator has left operand");
    expressions.push(bump.alloc(Located::at(
        Region::span_across(&left.region, &right.region),
        CanExpr::Binop { op, left, right },
    )));
}

fn unknown_value<'a>(region: Region, name: &'a str) -> Error<'a> {
    Error::new(
        region,
        ErrorKind::Expr(ExprError::Name(NameError::Unknown {
            namespace: alder_ast::Namespace::Value,
            qualifier: None,
            name,
            suggestions: &[],
        })),
    )
}

fn unknown_statement_name<'a>(region: Region, name: &'a str) -> Error<'a> {
    Error::new(
        region,
        ErrorKind::Stmt(StmtError::Name(NameError::Unknown {
            namespace: alder_ast::Namespace::Value,
            qualifier: None,
            name,
            suggestions: &[],
        })),
    )
}
