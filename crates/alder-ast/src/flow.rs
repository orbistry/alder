//! Structural control flow of the active canonical AST.
//!
//! No normal continuation is distinct from producing unit. Loops consume their
//! own breaks/continues; lambda creation does not execute its body. Calls and
//! deferred DSL constructs conservatively retain a normal continuation.
//!
//! Match evaluation distinguishes pattern rejection from entering an arm.
//! Sibling patterns execute after matching; alternatives execute after rejection.
//! Pin expressions can exit before either outcome, including from nested patterns.

use crate::{BinOp, Block, Expr, Pattern, PlaceStep, RecordField, Stmt, TemplatePart};
use alder_region::Located;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flow {
    pub falls_through: bool,
    pub returns: bool,
    pub breaks: bool,
    pub continues: bool,
}

impl Flow {
    const NEXT: Self = Self {
        falls_through: true,
        returns: false,
        breaks: false,
        continues: false,
    };
    const RETURN: Self = Self {
        falls_through: false,
        returns: true,
        breaks: false,
        continues: false,
    };
    const BREAK: Self = Self {
        falls_through: false,
        returns: false,
        breaks: true,
        continues: false,
    };
    const CONTINUE: Self = Self {
        falls_through: false,
        returns: false,
        breaks: false,
        continues: true,
    };

    fn either(self, other: Self) -> Self {
        Self {
            falls_through: self.falls_through || other.falls_through,
            returns: self.returns || other.returns,
            breaks: self.breaks || other.breaks,
            continues: self.continues || other.continues,
        }
    }

    fn then(self, other: Self) -> Self {
        if !self.falls_through {
            return self;
        }
        Self {
            falls_through: false,
            ..self
        }
        .either(other)
    }

    fn loop_body(self, may_skip: bool) -> Self {
        Self {
            falls_through: may_skip || self.breaks,
            returns: self.returns,
            ..Self::default()
        }
    }
}

pub fn block(block: &Located<Block<'_>>) -> Flow {
    let statements = block
        .value
        .statements
        .iter()
        .fold(Flow::NEXT, |flow, stmt| flow.then(statement(stmt)));
    statements.then(block.value.tail.map_or(Flow::NEXT, expression))
}

/// Pattern evaluation can match, reject and try another alternative, or exit
/// through a pin expression. Rejection is not normal execution of the arm body.
#[derive(Clone, Copy, Debug)]
pub struct PatternFlow {
    pub matches: bool,
    pub rejects: bool,
    pub exits: Flow,
}

impl PatternFlow {
    fn test(rejects: bool) -> Self {
        Self {
            matches: true,
            rejects,
            exits: Flow::default(),
        }
    }

    fn then(self, other: Self) -> Self {
        Self {
            matches: self.matches && other.matches,
            rejects: self.rejects || (self.matches && other.rejects),
            exits: if self.matches {
                self.exits.either(other.exits)
            } else {
                self.exits
            },
        }
    }

    fn or(self, other: Self) -> Self {
        Self {
            matches: self.matches || (self.rejects && other.matches),
            rejects: self.rejects && other.rejects,
            exits: if self.rejects {
                self.exits.either(other.exits)
            } else {
                self.exits
            },
        }
    }
}

pub fn pattern(pattern: &Located<Pattern<'_>>) -> PatternFlow {
    match &pattern.value {
        Pattern::Anything | Pattern::Bind(_) | Pattern::Unit => PatternFlow::test(false),
        Pattern::Pin { value, .. } => {
            let flow = expression(value);
            PatternFlow {
                matches: flow.falls_through,
                rejects: flow.falls_through,
                exits: Flow {
                    falls_through: false,
                    ..flow
                },
            }
        }
        Pattern::Alias { pattern: inner, .. } => self::pattern(inner),
        Pattern::Tuple(items) => items.iter().fold(PatternFlow::test(false), |flow, item| {
            flow.then(self::pattern(item))
        }),
        Pattern::Array { elements, rest } => elements.iter().fold(
            PatternFlow::test(rest.is_none() || !elements.is_empty()),
            |flow, item| flow.then(self::pattern(item)),
        ),
        Pattern::Record { fields, .. } => {
            fields.iter().fold(PatternFlow::test(false), |flow, field| {
                flow.then(self::pattern(field.pattern))
            })
        }
        Pattern::Constructor { constructor, args } => args.iter().fold(
            PatternFlow::test(constructor.alternatives > 1),
            |flow, arg| flow.then(self::pattern(arg)),
        ),
        Pattern::ConstructorRecord {
            constructor,
            fields,
            ..
        } => fields.iter().fold(
            PatternFlow::test(constructor.alternatives > 1),
            |flow, field| flow.then(self::pattern(field.pattern)),
        ),
        Pattern::Tag { args, .. } => args.iter().fold(PatternFlow::test(true), |flow, arg| {
            flow.then(self::pattern(arg))
        }),
        Pattern::Number { .. } | Pattern::BigInt(_) | Pattern::Str(_) | Pattern::Bool(_) => {
            PatternFlow::test(true)
        }
    }
}

pub fn patterns(alternatives: &[&Located<Pattern<'_>>]) -> PatternFlow {
    alternatives.iter().fold(
        PatternFlow {
            matches: false,
            rejects: true,
            exits: Flow::default(),
        },
        |flow, alternative| flow.or(pattern(alternative)),
    )
}

/// Whether evaluation can reach the right operand. Unknown values are
/// conservative; Boolean literals expose the two definite short circuits.
pub fn binary_rhs_reachable(op: BinOp, left: &Located<Expr<'_>>) -> bool {
    expression(left).falls_through && !binary_rhs_skipped(op, left)
}

fn binary_rhs_skipped(op: BinOp, left: &Located<Expr<'_>>) -> bool {
    matches!(
        (op, &left.value),
        (BinOp::And, Expr::Bool(false)) | (BinOp::Or, Expr::Bool(true))
    )
}

pub fn statement(stmt: &Located<Stmt<'_>>) -> Flow {
    match &stmt.value {
        Stmt::Return(value) => value.map_or(Flow::NEXT, expression).then(Flow::RETURN),
        Stmt::Break(value) => value.map_or(Flow::NEXT, expression).then(Flow::BREAK),
        Stmt::Continue => Flow::CONTINUE,
        Stmt::Let(decl) => expression(decl.value),
        Stmt::Use { .. } => Flow::NEXT,
        Stmt::Assign { place, value, .. } => {
            let target = place
                .steps
                .iter()
                .fold(Flow::NEXT, |flow, step| match step {
                    PlaceStep::Index(index) => flow.then(expression(index)),
                    _ => flow,
                });
            target.then(expression(value))
        }
        Stmt::Expr(expr) => expression(expr),
        Stmt::Assert(expr) => expression(expr).then(if matches!(expr.value, Expr::Bool(false)) {
            Flow::default()
        } else {
            Flow::NEXT
        }),
        Stmt::For { iter, body, .. } => expression(iter).then(block(body).loop_body(true)),
        Stmt::While { condition, body } => {
            expression(condition).then(if matches!(condition.value, Expr::Bool(false)) {
                Flow::NEXT
            } else {
                block(body).loop_body(!matches!(condition.value, Expr::Bool(true)))
            })
        }
    }
}

pub fn expression(expr: &Located<Expr<'_>>) -> Flow {
    match &expr.value {
        Expr::Block(body) => block(body),
        Expr::Loop(body) => block(body).loop_body(false),
        Expr::If {
            branches,
            final_else,
        } => {
            let mut flow = final_else.map_or(Flow::NEXT, block);
            for branch in branches.iter().rev() {
                let paths = match branch.condition.value {
                    Expr::Bool(true) => block(branch.body),
                    Expr::Bool(false) => flow,
                    _ => block(branch.body).either(flow),
                };
                flow = expression(branch.condition).then(paths);
            }
            flow
        }
        Expr::Match { scrutinee, arms } => {
            let mut branches = Flow::default();
            let mut remaining = true;
            for arm in *arms {
                if !remaining {
                    break;
                }
                let pattern = patterns(arm.patterns);
                branches = branches.either(pattern.exits);
                remaining = pattern.rejects;
                if pattern.matches {
                    let guard = arm.guard.map_or(Flow::NEXT, expression);
                    branches = branches.either(Flow {
                        falls_through: false,
                        ..guard
                    });
                    if guard.falls_through {
                        if !arm
                            .guard
                            .is_some_and(|guard| matches!(guard.value, Expr::Bool(false)))
                        {
                            branches = branches.either(expression(arm.body));
                        }
                        remaining |= arm
                            .guard
                            .is_some_and(|guard| !matches!(guard.value, Expr::Bool(true)));
                    }
                }
            }
            expression(scrutinee).then(branches)
        }
        Expr::Provide { value, body, .. } => expression(value).then(block(body)),
        Expr::Call {
            function,
            arguments,
            ..
        } => arguments
            .iter()
            .fold(expression(function), |flow, arg| flow.then(expression(arg))),
        Expr::Array(items) | Expr::Tuple(items) | Expr::Tag { args: items, .. } => items
            .iter()
            .fold(Flow::NEXT, |flow, item| flow.then(expression(item))),
        Expr::Record(fields) | Expr::RecordConstructor { fields, .. } => {
            fields.iter().fold(Flow::NEXT, |flow, field| {
                flow.then(expression(match field {
                    RecordField::Field { value, .. } | RecordField::Spread(value) => value,
                }))
            })
        }
        Expr::Template(parts) | Expr::TaggedTemplate { parts, .. } => {
            let initial = if let Expr::TaggedTemplate { tag, .. } = &expr.value {
                expression(tag)
            } else {
                Flow::NEXT
            };
            parts.iter().fold(initial, |flow, part| match part {
                TemplatePart::Expr(expr) => flow.then(expression(expr)),
                TemplatePart::Text(_) => flow,
            })
        }
        Expr::Access { record, .. } => expression(record),
        Expr::TupleAccess { tuple, .. } => expression(tuple),
        Expr::Index { target, index } => expression(target).then(expression(index)),
        Expr::Await(value)
        | Expr::Pin(value)
        | Expr::Not(value)
        | Expr::State(value)
        | Expr::Negate { expr: value, .. } => expression(value),
        Expr::Try(value) => expression(value).then(Flow::NEXT.either(Flow::RETURN)),
        Expr::Binop {
            op, left, right, ..
        } => expression(left).then(if binary_rhs_skipped(op.value, left) {
            Flow::NEXT
        } else if matches!(
            (op.value, &left.value),
            (BinOp::And, Expr::Bool(true)) | (BinOp::Or, Expr::Bool(false))
        ) {
            expression(right)
        } else if matches!(op.value, BinOp::And | BinOp::Or | BinOp::Coalesce) {
            Flow::NEXT.either(expression(right))
        } else {
            expression(right)
        }),
        Expr::Number { .. }
        | Expr::BigInt(_)
        | Expr::Str(_)
        | Expr::Bool(_)
        | Expr::Unit
        | Expr::Var { .. }
        | Expr::Constructor(_)
        | Expr::Lambda { .. }
        | Expr::Async(_)
        | Expr::Style(_)
        | Expr::Query(_)
        | Expr::Markup(_)
        | Expr::MacroCall { .. } => Flow::NEXT,
    }
}

#[cfg(test)]
mod tests {
    use super::{Flow, PatternFlow};

    #[test]
    fn pattern_children_run_only_after_a_match() {
        let exit = PatternFlow {
            matches: false,
            rejects: false,
            exits: Flow::RETURN,
        };
        let later = PatternFlow {
            matches: true,
            rejects: true,
            exits: Flow::BREAK,
        };
        let sequence = exit.then(later);
        assert!(!sequence.matches);
        assert!(!sequence.rejects);
        assert_eq!(sequence.exits, Flow::RETURN);

        let sequence = PatternFlow::test(true).then(exit);
        assert!(!sequence.matches);
        assert!(sequence.rejects);
        assert_eq!(sequence.exits, Flow::RETURN);
    }

    #[test]
    fn pattern_alternatives_run_only_after_rejection() {
        let exit = PatternFlow {
            matches: false,
            rejects: false,
            exits: Flow::BREAK,
        };
        let selected = PatternFlow::test(false).or(exit);
        assert!(selected.matches);
        assert!(!selected.rejects);
        assert_eq!(selected.exits, Flow::default());

        let conditional = PatternFlow::test(true).or(exit);
        assert!(conditional.matches);
        assert!(!conditional.rejects);
        assert_eq!(conditional.exits, Flow::BREAK);

        let exited = exit.or(PatternFlow::test(false));
        assert!(!exited.matches);
        assert!(!exited.rejects);
        assert_eq!(exited.exits, Flow::BREAK);
    }

    #[test]
    fn unreachable_successors_cannot_add_exits() {
        assert_eq!(Flow::RETURN.then(Flow::BREAK), Flow::RETURN);
        assert_eq!(Flow::CONTINUE.then(Flow::NEXT), Flow::CONTINUE);
        assert_eq!(Flow::default().then(Flow::RETURN), Flow::default());
    }

    #[test]
    fn nested_loops_consume_only_their_own_exits() {
        let inner = Flow::BREAK.loop_body(false);
        assert_eq!(inner, Flow::NEXT);
        assert!(!inner.loop_body(false).falls_through);
        assert!(Flow::RETURN.loop_body(true).falls_through);
        assert!(Flow::RETURN.loop_body(true).returns);
    }
}
