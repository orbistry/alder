//! Structural control flow of the active canonical AST.
//!
//! No normal continuation is distinct from producing unit. Loops consume their
//! own breaks/continues; lambda creation does not execute its body. Calls and
//! deferred DSL constructs conservatively retain a normal continuation.

use crate::{BinOp, Block, Expr, PlaceStep, RecordField, Stmt, TemplatePart};
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
            let branches = arms.iter().fold(Flow::default(), |flow, arm| {
                flow.either(
                    arm.guard.map_or(Flow::NEXT, expression).then(
                        if arm
                            .guard
                            .is_some_and(|guard| matches!(guard.value, Expr::Bool(false)))
                        {
                            Flow::default()
                        } else {
                            expression(arm.body)
                        },
                    ),
                )
            });
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
        | Expr::Style(_)
        | Expr::Query(_)
        | Expr::Markup(_)
        | Expr::MacroCall { .. } => Flow::NEXT,
    }
}

#[cfg(test)]
mod tests {
    use super::Flow;

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
