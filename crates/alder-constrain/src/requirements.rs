use alder_ast::{
    AttrValue, BinOp, Block, Child, ChildBlock, ChildItem, Expr, Item, ItemKind, Markup, Module,
    Node, Pattern, Projection, Query, RecordField, Stmt, Style, StyleValue, ValueRef,
};
use bumpalo::Bump;

use crate::{RequirementKind, RequirementSeed};

pub fn collect<'a>(bump: &'a Bump, module: &'a Module<'a>) -> &'a [RequirementSeed<'a>] {
    let mut seeds = Vec::new();
    for item in module.items {
        collect_item(item, &mut seeds);
    }
    seeds.sort_by_key(|seed| seed.use_id);
    bump.alloc_slice_copy(&seeds)
}

fn collect_item<'a>(item: Node<'a, Item<'a>>, out: &mut Vec<RequirementSeed<'a>>) {
    match &item.value.kind {
        ItemKind::Fn(function) => block(function.body, out),
        ItemKind::Let(declaration) => {
            pattern(declaration.pattern, out);
            expr(declaration.value, out);
        }
        ItemKind::Trait(trait_) => {
            for item in trait_.items {
                if let alder_ast::TraitItem::Fn(function) = item
                    && let Some(body) = function.body
                {
                    block(body, out);
                }
            }
        }
        ItemKind::Impl(impl_) => {
            for item in impl_.items {
                if let alder_ast::ImplItem::Fn(function) = item {
                    block(function.body, out);
                }
            }
        }
        ItemKind::Component(component) => block(component.body, out),
        ItemKind::Table(table) => {
            for column in table.columns {
                expr(column.builder, out);
                for modifier in column.modifiers {
                    expressions(modifier.args, out);
                }
            }
        }
        ItemKind::Schema(schema) => {
            for item in schema.items {
                if let alder_ast::SchemaItem::Field { rules, .. } = item {
                    for rule in *rules {
                        expressions(rule.args, out);
                    }
                }
            }
        }
        ItemKind::Test(test) => block(test.body, out),
        ItemKind::Tests(items) => {
            for item in *items {
                collect_item(item, out);
            }
        }
        ItemKind::Comptime(value) => block(value, out),
        ItemKind::TypeAlias(_)
        | ItemKind::Enum(_)
        | ItemKind::ErrorGroup(_)
        | ItemKind::Macro(_)
        | ItemKind::Extern(_) => {}
    }
}

fn block<'a>(value: Node<'a, Block<'a>>, out: &mut Vec<RequirementSeed<'a>>) {
    for statement in value.value.statements {
        stmt(statement, out);
    }
    if let Some(tail) = value.value.tail {
        expr(tail, out);
    }
}

fn stmt<'a>(value: Node<'a, Stmt<'a>>, out: &mut Vec<RequirementSeed<'a>>) {
    match &value.value {
        Stmt::Let(declaration) => {
            pattern(declaration.pattern, out);
            expr(declaration.value, out);
        }
        Stmt::Use { .. } | Stmt::Continue => {}
        Stmt::Assign {
            use_id,
            place,
            op,
            value,
        } => {
            for step in place.steps {
                if let alder_ast::PlaceStep::Index(index) = step {
                    expr(index, out);
                }
            }
            if let Some(use_id) = use_id {
                out.push(RequirementSeed {
                    use_id: *use_id,
                    kind: RequirementKind::Num,
                    region: op.region,
                });
            }
            expr(value, out);
        }
        Stmt::For {
            pattern: binding,
            iter,
            body,
        } => {
            pattern(binding, out);
            expr(iter, out);
            block(body, out);
        }
        Stmt::While { condition, body } => {
            expr(condition, out);
            block(body, out);
        }
        Stmt::Return(value) | Stmt::Break(value) => {
            if let Some(value) = value {
                expr(value, out);
            }
        }
        Stmt::Assert(value) | Stmt::Expr(value) => expr(value, out),
    }
}

fn expr<'a>(value: Node<'a, Expr<'a>>, out: &mut Vec<RequirementSeed<'a>>) {
    match &value.value {
        Expr::Number { .. }
        | Expr::BigInt(_)
        | Expr::Str(_)
        | Expr::Bool(_)
        | Expr::Unit
        | Expr::Constructor(_)
        | Expr::MacroCall { .. } => {}
        Expr::Template(parts) => {
            for part in *parts {
                if let alder_ast::TemplatePart::Expr(value) = part {
                    expr(value, out);
                }
            }
        }
        Expr::TaggedTemplate { tag, parts } => {
            expr(tag, out);
            for part in *parts {
                if let alder_ast::TemplatePart::Expr(value) = part {
                    expr(value, out);
                }
            }
        }
        Expr::Var {
            use_id,
            reference: ValueRef::TraitMethod { method, .. },
        } => out.push(RequirementSeed {
            use_id: *use_id,
            kind: RequirementKind::TraitMethod(*method),
            region: value.region,
        }),
        Expr::Var { .. } => {}
        Expr::Tag { args, .. } | Expr::Array(args) | Expr::Tuple(args) => expressions(args, out),
        Expr::Record(fields) | Expr::RecordConstructor { fields, .. } => {
            record_fields(fields, out);
        }
        Expr::Call {
            function,
            arguments,
            ..
        } => {
            expr(function, out);
            expressions(arguments, out);
        }
        Expr::Access { record, .. } => expr(record, out),
        Expr::TupleAccess { tuple, .. } => expr(tuple, out),
        Expr::Index { target, index } => {
            expr(target, out);
            expr(index, out);
        }
        Expr::Await(value)
        | Expr::Try(value)
        | Expr::Pin(value)
        | Expr::Not(value)
        | Expr::State(value) => expr(value, out),
        Expr::Negate {
            use_id,
            expr: operand,
        } => {
            out.push(RequirementSeed {
                use_id: *use_id,
                kind: RequirementKind::Num,
                region: value.region,
            });
            expr(operand, out);
        }
        Expr::Binop {
            use_id,
            op,
            left,
            right,
        } => {
            let kind = match op.value {
                BinOp::Eq | BinOp::NotEq => Some(RequirementKind::Eq),
                BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => Some(RequirementKind::Ord),
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                    Some(RequirementKind::Num)
                }
                BinOp::And | BinOp::Or | BinOp::Coalesce | BinOp::Pipe | BinOp::In => None,
            };
            if let Some(kind) = kind {
                out.push(RequirementSeed {
                    use_id: *use_id,
                    kind,
                    region: op.region,
                });
            }
            expr(left, out);
            expr(right, out);
        }
        Expr::Block(value) | Expr::Loop(value) => block(value, out),
        Expr::Lambda { params, body, .. } => {
            for param in *params {
                pattern(param.pattern, out);
            }
            expr(body, out);
        }
        Expr::If {
            branches,
            final_else,
        } => {
            for branch in *branches {
                expr(branch.condition, out);
                block(branch.body, out);
            }
            if let Some(final_else) = final_else {
                block(final_else, out);
            }
        }
        Expr::Match { scrutinee, arms } => {
            expr(scrutinee, out);
            for arm in *arms {
                for alternative in arm.patterns {
                    pattern(alternative, out);
                }
                if let Some(guard) = arm.guard {
                    expr(guard, out);
                }
                expr(arm.body, out);
            }
        }
        Expr::Provide { value, body, .. } => {
            expr(value, out);
            block(body, out);
        }
        Expr::Style(style) => collect_style(style, out),
        Expr::Query(query) => collect_query(query, out),
        Expr::Markup(markup) => collect_markup(markup, out),
    }
}

fn expressions<'a>(values: &'a [Node<'a, Expr<'a>>], out: &mut Vec<RequirementSeed<'a>>) {
    for value in values {
        expr(value, out);
    }
}

fn pattern<'a>(value: Node<'a, Pattern<'a>>, out: &mut Vec<RequirementSeed<'a>>) {
    match &value.value {
        Pattern::Pin { use_id, value: pin } => {
            out.push(RequirementSeed {
                use_id: *use_id,
                kind: RequirementKind::Eq,
                region: value.region,
            });
            expr(pin, out);
        }
        Pattern::Constructor { args, .. } | Pattern::Tag { args, .. } | Pattern::Tuple(args) => {
            for argument in *args {
                pattern(argument, out);
            }
        }
        Pattern::ConstructorRecord { fields, .. } | Pattern::Record { fields, .. } => {
            for field in *fields {
                pattern(field.pattern, out);
            }
        }
        Pattern::Array { elements, .. } => {
            for element in *elements {
                pattern(element, out);
            }
        }
        Pattern::Alias {
            pattern: nested, ..
        } => pattern(nested, out),
        Pattern::Anything
        | Pattern::Bind(_)
        | Pattern::Number { .. }
        | Pattern::BigInt(_)
        | Pattern::Str(_)
        | Pattern::Bool(_)
        | Pattern::Unit => {}
    }
}

fn record_fields<'a>(fields: &'a [RecordField<'a>], out: &mut Vec<RequirementSeed<'a>>) {
    for field in fields {
        match field {
            RecordField::Field { value, .. } | RecordField::Spread(value) => expr(value, out),
        }
    }
}

fn collect_style<'a>(style: &'a Style<'a>, out: &mut Vec<RequirementSeed<'a>>) {
    for entry in style.entries {
        match entry.value {
            StyleValue::Expr(value) => expr(value, out),
            StyleValue::Nested(nested) => collect_style(nested, out),
            StyleValue::Dimension { .. } => {}
        }
    }
}

fn collect_query<'a>(query: &'a Query<'a>, out: &mut Vec<RequirementSeed<'a>>) {
    match query {
        Query::Select(select) => {
            if let Projection::Fields(fields) = select.projection {
                expressions(fields, out);
            }
            for join in select.joins {
                expr(join.on, out);
            }
            if let Some(where_) = select.where_ {
                expr(where_, out);
            }
            expressions(select.group_by, out);
            for order in select.order_by {
                expr(order.expr, out);
            }
            if let Some(limit) = select.limit {
                expr(limit, out);
            }
            if let Some(offset) = select.offset {
                expr(offset, out);
            }
        }
        Query::Insert { values, .. } => expr(values, out),
        Query::Update { set, where_, .. } => {
            record_fields(set, out);
            if let Some(where_) = where_ {
                expr(where_, out);
            }
        }
        Query::Delete { where_, .. } => {
            if let Some(where_) = where_ {
                expr(where_, out);
            }
        }
    }
}

fn collect_markup<'a>(markup: &'a Markup<'a>, out: &mut Vec<RequirementSeed<'a>>) {
    match markup {
        Markup::Element(element) => collect_element(element, out),
        Markup::Fragment(children) => {
            for child in *children {
                collect_child(child, out);
            }
        }
    }
}

fn collect_element<'a>(element: &'a alder_ast::Element<'a>, out: &mut Vec<RequirementSeed<'a>>) {
    for attr in element.attrs {
        if let Some(AttrValue::Expr(value)) = attr.value {
            expr(value, out);
        }
    }
    for child in element.children {
        collect_child(child, out);
    }
}

fn collect_child<'a>(child: Node<'a, Child<'a>>, out: &mut Vec<RequirementSeed<'a>>) {
    match &child.value {
        Child::Element(element) => collect_element(element, out),
        Child::Fragment(children) => {
            for child in *children {
                collect_child(child, out);
            }
        }
        Child::Text(_) => {}
        Child::Hole(value) => expr(value, out),
        Child::If {
            branches,
            final_else,
        } => {
            for branch in *branches {
                expr(branch.condition, out);
                collect_child_block(branch.body, out);
            }
            if let Some(final_else) = final_else {
                collect_child_block(final_else, out);
            }
        }
        Child::For {
            pattern: binding,
            iter,
            key,
            body,
            empty,
        } => {
            pattern(binding, out);
            expr(iter, out);
            if let Some(key) = key {
                expr(key, out);
            }
            collect_child_block(body, out);
            if let Some(empty) = empty {
                collect_child_block(empty, out);
            }
        }
        Child::Match { scrutinee, arms } => {
            expr(scrutinee, out);
            for arm in *arms {
                for alternative in arm.patterns {
                    pattern(alternative, out);
                }
                if let Some(guard) = arm.guard {
                    expr(guard, out);
                }
                collect_child_block(arm.body, out);
            }
        }
    }
}

fn collect_child_block<'a>(value: Node<'a, ChildBlock<'a>>, out: &mut Vec<RequirementSeed<'a>>) {
    for item in value.value.items {
        match item {
            ChildItem::Stmt(value) => stmt(value, out),
            ChildItem::Child(value) => collect_child(value, out),
        }
    }
}

#[cfg(test)]
mod tests {
    use alder_ast::{ModuleId, PackageId, UseId};
    use alder_can::Context;
    use bumpalo::Bump;
    use indoc::indoc;

    use super::*;

    #[test]
    fn evidence_requirements_follow_stable_use_ids() {
        let bump = Bump::new();
        let source_text = bump.alloc_str(indoc! {r#"
            fn requirements(mut x, y) {
                x = y
                x += -y
                match x { ^y => x == y, _ => x < y }
            }
        "#});
        let source = alder_parse::parse_module(&bump, source_text).unwrap();
        let module = alder_can::canonicalize(
            &bump,
            Context {
                home: ModuleId {
                    package: PackageId::Application,
                    path: &["Main"],
                },
                imports: &[],
                interfaces: &[],
            },
            &source,
        )
        .unwrap()
        .module;

        let seeds = collect(&bump, module)
            .iter()
            .map(|seed| (seed.use_id, seed.kind))
            .collect::<Vec<_>>();
        assert_eq!(
            seeds,
            vec![
                (UseId(1), RequirementKind::Num),
                (UseId(2), RequirementKind::Num),
                (UseId(5), RequirementKind::Eq),
                (UseId(8), RequirementKind::Eq),
                (UseId(11), RequirementKind::Ord),
            ]
        );
    }
}
