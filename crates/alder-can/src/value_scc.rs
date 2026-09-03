//! Deterministic top-level value dependency groups for inference.

use std::collections::BTreeSet;

use alder_ast::{
    AttrValue, Block, Child, ChildBlock, ChildItem, Expr, Item, ItemKind, Markup, ModuleId, Node,
    Pattern, Projection, QualifiedName, Query, RecordField, Stmt, Style, StyleValue, ValueRef,
    ValueScc,
};
use bumpalo::Bump;

use crate::scc;

struct ValueNode<'a> {
    name: QualifiedName<'a>,
    dependencies: BTreeSet<&'a str>,
}

pub fn build<'a>(
    bump: &'a Bump,
    home: ModuleId<'a>,
    items: &'a [Node<'a, Item<'a>>],
) -> &'a [ValueScc<'a>] {
    let mut nodes = Vec::new();
    for item in items {
        match &item.value.kind {
            ItemKind::Fn(function) => nodes.push(node(function.name, |dependencies| {
                block(home, function.body, dependencies);
            })),
            ItemKind::Let(declaration) => {
                let mut dependencies = BTreeSet::new();
                pattern(home, declaration.pattern, &mut dependencies);
                expr(home, declaration.value, &mut dependencies);
                for name in declaration.bindings {
                    nodes.push(ValueNode {
                        name: *name,
                        dependencies: dependencies.clone(),
                    });
                }
            }
            ItemKind::Component(component) => {
                nodes.push(node(component.name, |dependencies| {
                    block(home, component.body, dependencies);
                }));
            }
            ItemKind::Extern(alder_ast::ExternDecl::Fn { name, .. }) => {
                nodes.push(ValueNode {
                    name: *name,
                    dependencies: BTreeSet::new(),
                });
            }
            ItemKind::TypeAlias(_)
            | ItemKind::Enum(_)
            | ItemKind::Trait(_)
            | ItemKind::Impl(_)
            | ItemKind::ErrorGroup(_)
            | ItemKind::Table(_)
            | ItemKind::Schema(_)
            | ItemKind::Test(_)
            | ItemKind::Tests(_)
            | ItemKind::Macro(_)
            | ItemKind::Comptime(_)
            | ItemKind::Extern(alder_ast::ExternDecl::Type { .. }) => {}
        }
    }

    let graph = nodes
        .into_iter()
        .map(|node| scc::Node {
            key: node.name.name,
            deps: node.dependencies.into_iter().collect(),
            value: node.name,
        })
        .collect();
    bump.alloc_slice_fill_iter(scc::strongly_connected_components(graph).into_iter().map(
        |component| match component {
            scc::Scc::Acyclic(member) => ValueScc {
                recursive: false,
                members: bump.alloc_slice_fill_iter([member]),
            },
            scc::Scc::Cyclic(members) => ValueScc {
                recursive: true,
                members: bump.alloc_slice_copy(&members),
            },
        },
    ))
}

fn node<'a>(
    name: QualifiedName<'a>,
    collect: impl FnOnce(&mut BTreeSet<&'a str>),
) -> ValueNode<'a> {
    let mut dependencies = BTreeSet::new();
    collect(&mut dependencies);
    ValueNode { name, dependencies }
}

fn block<'a>(home: ModuleId<'a>, value: Node<'a, Block<'a>>, out: &mut BTreeSet<&'a str>) {
    for statement in value.value.statements {
        stmt(home, statement, out);
    }
    if let Some(tail) = value.value.tail {
        expr(home, tail, out);
    }
}

fn stmt<'a>(home: ModuleId<'a>, value: Node<'a, Stmt<'a>>, out: &mut BTreeSet<&'a str>) {
    match &value.value {
        Stmt::Let(declaration) => {
            pattern(home, declaration.pattern, out);
            expr(home, declaration.value, out);
        }
        Stmt::Use { .. } | Stmt::Continue => {}
        Stmt::Assign { place, value, .. } => {
            for step in place.steps {
                if let alder_ast::PlaceStep::Index(index) = step {
                    expr(home, index, out);
                }
            }
            expr(home, value, out);
        }
        Stmt::For {
            pattern: binding,
            iter,
            body,
        } => {
            pattern(home, binding, out);
            expr(home, iter, out);
            block(home, body, out);
        }
        Stmt::While { condition, body } => {
            expr(home, condition, out);
            block(home, body, out);
        }
        Stmt::Return(value) | Stmt::Break(value) => {
            if let Some(value) = value {
                expr(home, value, out);
            }
        }
        Stmt::Assert(value) | Stmt::Expr(value) => expr(home, value, out),
    }
}

fn expr<'a>(home: ModuleId<'a>, value: Node<'a, Expr<'a>>, out: &mut BTreeSet<&'a str>) {
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
                    expr(home, value, out);
                }
            }
        }
        Expr::TaggedTemplate { tag, parts } => {
            expr(home, tag, out);
            for part in *parts {
                if let alder_ast::TemplatePart::Expr(value) = part {
                    expr(home, value, out);
                }
            }
        }
        Expr::Var {
            reference: ValueRef::TopLevel(reference),
            ..
        } if reference.module == home => {
            out.insert(reference.name);
        }
        Expr::Var { .. } => {}
        Expr::Tag { args, .. } | Expr::Array(args) | Expr::Tuple(args) => {
            for argument in *args {
                expr(home, argument, out);
            }
        }
        Expr::Record(fields) | Expr::RecordConstructor { fields, .. } => {
            record_fields(home, fields, out);
        }
        Expr::Call {
            function,
            arguments,
            ..
        } => {
            expr(home, function, out);
            for argument in *arguments {
                expr(home, argument, out);
            }
        }
        Expr::Access { record, .. } => expr(home, record, out),
        Expr::TupleAccess { tuple, .. } => expr(home, tuple, out),
        Expr::Index { target, index } => {
            expr(home, target, out);
            expr(home, index, out);
        }
        Expr::Await(value)
        | Expr::Try(value)
        | Expr::Pin(value)
        | Expr::Not(value)
        | Expr::State(value) => expr(home, value, out),
        Expr::Negate { expr: value, .. } => expr(home, value, out),
        Expr::Binop { left, right, .. } => {
            expr(home, left, out);
            expr(home, right, out);
        }
        Expr::Block(value) | Expr::Loop(value) => block(home, value, out),
        Expr::Lambda { params, body, .. } => {
            for param in *params {
                pattern(home, param.pattern, out);
            }
            expr(home, body, out);
        }
        Expr::If {
            branches,
            final_else,
        } => {
            for branch in *branches {
                expr(home, branch.condition, out);
                block(home, branch.body, out);
            }
            if let Some(final_else) = final_else {
                block(home, final_else, out);
            }
        }
        Expr::Match { scrutinee, arms } => {
            expr(home, scrutinee, out);
            for arm in *arms {
                for alternative in arm.patterns {
                    pattern(home, alternative, out);
                }
                if let Some(guard) = arm.guard {
                    expr(home, guard, out);
                }
                expr(home, arm.body, out);
            }
        }
        Expr::Provide { value, body, .. } => {
            expr(home, value, out);
            block(home, body, out);
        }
        Expr::Style(style) => collect_style(home, style, out),
        Expr::Query(query) => collect_query(home, query, out),
        Expr::Markup(markup) => collect_markup(home, markup, out),
    }
}

fn pattern<'a>(home: ModuleId<'a>, value: Node<'a, Pattern<'a>>, out: &mut BTreeSet<&'a str>) {
    match &value.value {
        Pattern::Pin { value, .. } => expr(home, value, out),
        Pattern::Constructor { args, .. } | Pattern::Tag { args, .. } | Pattern::Tuple(args) => {
            for argument in *args {
                pattern(home, argument, out);
            }
        }
        Pattern::ConstructorRecord { fields, .. } | Pattern::Record { fields, .. } => {
            for field in *fields {
                pattern(home, field.pattern, out);
            }
        }
        Pattern::Array { elements, .. } => {
            for element in *elements {
                pattern(home, element, out);
            }
        }
        Pattern::Alias {
            pattern: nested, ..
        } => pattern(home, nested, out),
        Pattern::Anything
        | Pattern::Bind(_)
        | Pattern::Number { .. }
        | Pattern::BigInt(_)
        | Pattern::Str(_)
        | Pattern::Bool(_)
        | Pattern::Unit => {}
    }
}

fn record_fields<'a>(
    home: ModuleId<'a>,
    fields: &'a [RecordField<'a>],
    out: &mut BTreeSet<&'a str>,
) {
    for field in fields {
        match field {
            RecordField::Field { value, .. } | RecordField::Spread(value) => {
                expr(home, value, out);
            }
        }
    }
}

fn collect_style<'a>(home: ModuleId<'a>, style: &'a Style<'a>, out: &mut BTreeSet<&'a str>) {
    for entry in style.entries {
        match entry.value {
            StyleValue::Expr(value) => expr(home, value, out),
            StyleValue::Nested(nested) => collect_style(home, nested, out),
            StyleValue::Dimension { .. } => {}
        }
    }
}

fn collect_query<'a>(home: ModuleId<'a>, query: &'a Query<'a>, out: &mut BTreeSet<&'a str>) {
    match query {
        Query::Select(select) => {
            if let Projection::Fields(fields) = select.projection {
                for field in fields {
                    expr(home, field, out);
                }
            }
            for join in select.joins {
                expr(home, join.on, out);
            }
            if let Some(where_) = select.where_ {
                expr(home, where_, out);
            }
            for value in select.group_by {
                expr(home, value, out);
            }
            for order in select.order_by {
                expr(home, order.expr, out);
            }
            if let Some(limit) = select.limit {
                expr(home, limit, out);
            }
            if let Some(offset) = select.offset {
                expr(home, offset, out);
            }
        }
        Query::Insert { values, .. } => expr(home, values, out),
        Query::Update { set, where_, .. } => {
            record_fields(home, set, out);
            if let Some(where_) = where_ {
                expr(home, where_, out);
            }
        }
        Query::Delete { where_, .. } => {
            if let Some(where_) = where_ {
                expr(home, where_, out);
            }
        }
    }
}

fn collect_markup<'a>(home: ModuleId<'a>, markup: &'a Markup<'a>, out: &mut BTreeSet<&'a str>) {
    match markup {
        Markup::Element(element) => collect_element(home, element, out),
        Markup::Fragment(children) => {
            for child in *children {
                collect_child(home, child, out);
            }
        }
    }
}

fn collect_element<'a>(
    home: ModuleId<'a>,
    element: &'a alder_ast::Element<'a>,
    out: &mut BTreeSet<&'a str>,
) {
    if let alder_ast::ElementName::Component(reference) = element.name.value
        && reference.module == home
    {
        out.insert(reference.name);
    }
    for attr in element.attrs {
        if let Some(AttrValue::Expr(value)) = attr.value {
            expr(home, value, out);
        }
    }
    for child in element.children {
        collect_child(home, child, out);
    }
}

fn collect_child<'a>(home: ModuleId<'a>, child: Node<'a, Child<'a>>, out: &mut BTreeSet<&'a str>) {
    match &child.value {
        Child::Element(element) => collect_element(home, element, out),
        Child::Fragment(children) => {
            for child in *children {
                collect_child(home, child, out);
            }
        }
        Child::Text(_) => {}
        Child::Hole(value) => expr(home, value, out),
        Child::If {
            branches,
            final_else,
        } => {
            for branch in *branches {
                expr(home, branch.condition, out);
                collect_child_block(home, branch.body, out);
            }
            if let Some(final_else) = final_else {
                collect_child_block(home, final_else, out);
            }
        }
        Child::For {
            pattern: binding,
            iter,
            key,
            body,
            empty,
        } => {
            pattern(home, binding, out);
            expr(home, iter, out);
            if let Some(key) = key {
                expr(home, key, out);
            }
            collect_child_block(home, body, out);
            if let Some(empty) = empty {
                collect_child_block(home, empty, out);
            }
        }
        Child::Match { scrutinee, arms } => {
            expr(home, scrutinee, out);
            for arm in *arms {
                for alternative in arm.patterns {
                    pattern(home, alternative, out);
                }
                if let Some(guard) = arm.guard {
                    expr(home, guard, out);
                }
                collect_child_block(home, arm.body, out);
            }
        }
    }
}

fn collect_child_block<'a>(
    home: ModuleId<'a>,
    value: Node<'a, ChildBlock<'a>>,
    out: &mut BTreeSet<&'a str>,
) {
    for item in value.value.items {
        match item {
            ChildItem::Stmt(value) => stmt(home, value, out),
            ChildItem::Child(value) => collect_child(home, value, out),
        }
    }
}
