//! Deterministic top-level value dependency groups for inference.

use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{
    AttrValue, Block, Child, ChildBlock, ChildItem, Expr, Item, ItemKind, Markup, ModuleId, Node,
    Pattern, Projection, QualifiedName, Query, RecordField, Stmt, Style, StyleValue, ValueRef,
    ValueScc,
};
use bumpalo::Bump;

use crate::scc;

// Share the resolved expression walk with unused-local analysis. SCCs only
// care about module dependencies; warnings additionally inspect local IDs.
trait References<'a> {
    fn insert(&mut self, name: &'a str);
    fn bind(
        &mut self,
        _name: alder_ast::BindingName<'a>,
        _region: alder_region::Region,
        _form: crate::BindingForm,
    ) {
    }
    fn use_local(&mut self, _name: alder_ast::LocalName<'a>) {}
    fn use_value(&mut self, _name: QualifiedName<'a>) {}
}

impl<'a> References<'a> for BTreeSet<&'a str> {
    fn insert(&mut self, name: &'a str) {
        BTreeSet::insert(self, name);
    }
}

#[derive(Default)]
struct Locals<'a> {
    bindings: BTreeMap<
        alder_ast::LocalId,
        (
            alder_ast::LocalName<'a>,
            alder_region::Region,
            crate::BindingForm,
        ),
    >,
    uses: BTreeSet<alder_ast::LocalId>,
}

impl<'a> References<'a> for Locals<'a> {
    fn insert(&mut self, _name: &'a str) {}

    fn bind(
        &mut self,
        name: alder_ast::BindingName<'a>,
        region: alder_region::Region,
        form: crate::BindingForm,
    ) {
        if let alder_ast::BindingName::Local(name) = name {
            // Alternatives share IDs. Warn once at the first binding site.
            self.bindings.entry(name.id).or_insert((name, region, form));
        }
    }

    fn use_local(&mut self, name: alder_ast::LocalName<'a>) {
        self.uses.insert(name.id);
    }
}

pub(crate) fn unused_locals<'a>(
    home: ModuleId<'a>,
    items: &[Node<'a, Item<'a>>],
) -> Vec<crate::Warning<'a>> {
    let mut locals = Locals::default();
    for item in items {
        if matches!(&item.value.kind, ItemKind::Impl(implementation) if implementation.synthetic.is_some())
        {
            continue;
        }
        collect_item(home, &item.value.kind, &mut locals);
    }
    let mut warnings = locals
        .bindings
        .into_iter()
        .filter_map(|(id, (name, region, form))| {
            (!locals.uses.contains(&id)).then_some(crate::Warning {
                region,
                kind: crate::WarningKind::UnusedBinding {
                    name: name.text,
                    form,
                },
            })
        })
        .collect::<Vec<_>>();
    warnings.sort_by_key(|warning| warning.region);
    warnings
}

struct ValueNode<'a> {
    name: QualifiedName<'a>,
    dependencies: BTreeSet<&'a str>,
}

#[derive(Default)]
struct ModuleBindings<'a>(BTreeMap<&'a str, (alder_region::Region, crate::BindingForm)>);

impl<'a> References<'a> for ModuleBindings<'a> {
    fn insert(&mut self, _: &'a str) {}
    fn bind(
        &mut self,
        name: alder_ast::BindingName<'a>,
        region: alder_region::Region,
        form: crate::BindingForm,
    ) {
        if let alder_ast::BindingName::TopLevel(name) = name {
            self.0.entry(name.name).or_insert((region, form));
        }
    }
}

pub(crate) fn unused_module_bindings<'a>(
    home: ModuleId<'a>,
    items: &[Node<'a, Item<'a>>],
    env: &crate::environment::Env<'a>,
) -> Vec<crate::Warning<'a>> {
    let mut candidates = ModuleBindings::default();
    let mut edges = BTreeMap::new();
    let mut roots = BTreeSet::new();
    for item in items {
        let dependencies = dependencies(home, &item.value.kind);
        let names = match &item.value.kind {
            ItemKind::Fn(function) => vec![function.name.name],
            ItemKind::Component(component) => vec![component.name.name],
            ItemKind::Extern(alder_ast::ExternDecl::Fn { name, .. }) => vec![name.name],
            ItemKind::Let(declaration) => {
                pattern(home, declaration.pattern, &mut candidates);
                // Initializers and refutable/pinned patterns are evaluated even
                // when none of their bound names are subsequently read.
                roots.extend(dependencies.iter().copied());
                declaration.bindings.iter().map(|name| name.name).collect()
            }
            _ => {
                // Test bodies and trait/impl methods can be entered without a
                // direct local value reference, so retain their dependencies.
                roots.extend(dependencies);
                continue;
            }
        };
        for name in names {
            let region = env.scopes[0]
                .values
                .get(name)
                .map_or(item.region, |binding| binding.region);
            candidates
                .0
                .entry(name)
                .or_insert((region, crate::BindingForm::Declaration));
            edges.insert(name, dependencies.clone());
            if matches!(item.value.visibility, alder_ast::Visibility::Public(_)) || name == "main" {
                roots.insert(name);
            }
        }
    }
    let mut reachable = BTreeSet::new();
    let mut pending = roots.into_iter().collect::<Vec<_>>();
    while let Some(name) = pending.pop() {
        if reachable.insert(name)
            && let Some(dependencies) = edges.get(name)
        {
            pending.extend(dependencies);
        }
    }
    candidates
        .0
        .into_iter()
        .filter_map(|(name, (region, form))| {
            (!reachable.contains(name)).then_some(crate::Warning {
                region,
                kind: crate::WarningKind::UnusedBinding { name, form },
            })
        })
        .collect()
}

/// Local module values read or written by this declaration, including nested
/// bodies and pins. Recovery uses the same resolved traversal as value SCCs so
/// a failed binding cannot be mistaken for an independent dependency.
pub fn dependencies<'a>(home: ModuleId<'a>, item: &ItemKind<'a>) -> BTreeSet<&'a str> {
    let mut out = BTreeSet::new();
    collect_item(home, item, &mut out);
    out
}

/// The resolved value dependencies of one method or default body. Keeping this
/// traversal shared with SCCs includes parameter pins and nested writes.
pub fn callable_dependencies<'a>(
    home: ModuleId<'a>,
    parameters: &[alder_ast::Param<'a>],
    body: Node<'a, Block<'a>>,
) -> BTreeSet<&'a str> {
    let mut out = BTreeSet::new();
    params(home, parameters, &mut out);
    block(home, body, &mut out);
    out
}

/// Resolved local identities read or introduced by a statement, including
/// captures, pattern pins and assignment targets. Recovery uses these same
/// identities as unused-binding analysis rather than comparing source names.
pub struct StatementLocalDependencies {
    pub bindings: BTreeSet<alder_ast::LocalId>,
    pub uses: BTreeSet<alder_ast::LocalId>,
}

pub fn statement_local_dependencies<'a>(
    home: ModuleId<'a>,
    statement: Node<'a, Stmt<'a>>,
) -> StatementLocalDependencies {
    let mut locals = Locals::default();
    stmt(home, statement, &mut locals);
    StatementLocalDependencies {
        bindings: locals.bindings.into_keys().collect(),
        uses: locals.uses,
    }
}

pub fn expression_local_dependencies<'a>(
    home: ModuleId<'a>,
    expression: Node<'a, Expr<'a>>,
) -> BTreeSet<alder_ast::LocalId> {
    let mut locals = Locals::default();
    expr(home, expression, &mut locals);
    locals.uses
}

/// All resolved module value references, including foreign stores and captures.
pub fn expression_value_dependencies<'a>(
    home: ModuleId<'a>,
    expression: Node<'a, Expr<'a>>,
) -> BTreeSet<QualifiedName<'a>> {
    #[derive(Default)]
    struct Values<'a>(BTreeSet<QualifiedName<'a>>);
    impl<'a> References<'a> for Values<'a> {
        fn insert(&mut self, _name: &'a str) {}
        fn use_value(&mut self, name: QualifiedName<'a>) {
            self.0.insert(name);
        }
    }
    let mut values = Values::default();
    expr(home, expression, &mut values);
    values.0
}

pub fn callable_value_dependencies<'a>(
    home: ModuleId<'a>,
    parameters: &[alder_ast::Param<'a>],
    body: Node<'a, Block<'a>>,
) -> BTreeSet<QualifiedName<'a>> {
    #[derive(Default)]
    struct Values<'a>(BTreeSet<QualifiedName<'a>>);
    impl<'a> References<'a> for Values<'a> {
        fn insert(&mut self, _name: &'a str) {}
        fn use_value(&mut self, name: QualifiedName<'a>) {
            self.0.insert(name);
        }
    }
    let mut values = Values::default();
    params(home, parameters, &mut values);
    block(home, body, &mut values);
    values.0
}

/// Local storage introduced by a pattern, without treating nested expression
/// reads (for example pins) as declarations.
pub fn pattern_local_bindings<'a>(
    home: ModuleId<'a>,
    binding: Node<'a, Pattern<'a>>,
) -> BTreeSet<alder_ast::LocalId> {
    let mut locals = Locals::default();
    pattern(home, binding, &mut locals);
    locals.bindings.into_keys().collect()
}

fn collect_item<'a>(home: ModuleId<'a>, item: &ItemKind<'a>, out: &mut impl References<'a>) {
    match item {
        ItemKind::Fn(function) => {
            params(home, function.params, out);
            block(home, function.body, out);
        }
        ItemKind::Let(declaration) => {
            pattern(home, declaration.pattern, out);
            expr(home, declaration.value, out);
        }
        ItemKind::Component(component) => {
            params(home, component.params, out);
            block(home, component.body, out);
        }
        ItemKind::Test(test) => block(home, test.body, out),
        ItemKind::Tests(items) => {
            for item in *items {
                collect_item(home, &item.value.kind, out);
            }
        }
        ItemKind::Impl(implementation) => {
            for item in implementation.items {
                if let alder_ast::ImplItem::Fn(function) = item {
                    params(home, function.params, out);
                    block(home, function.body, out);
                }
            }
        }
        ItemKind::Trait(trait_) => {
            for item in trait_.items {
                if let alder_ast::TraitItem::Fn(function) = item
                    && let Some(body) = function.body
                {
                    params(home, function.params, out);
                    block(home, body, out);
                }
            }
        }
        ItemKind::TypeAlias(_)
        | ItemKind::Enum(_)
        | ItemKind::ErrorGroup(_)
        | ItemKind::Table(_)
        | ItemKind::Schema(_)
        | ItemKind::Macro(_)
        | ItemKind::Comptime(_)
        | ItemKind::Extern(_) => {}
    }
}

fn params<'a>(home: ModuleId<'a>, params: &[alder_ast::Param<'a>], out: &mut impl References<'a>) {
    for param in params {
        pattern(home, param.pattern, out);
    }
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

fn block<'a>(home: ModuleId<'a>, value: Node<'a, Block<'a>>, out: &mut impl References<'a>) {
    for statement in value.value.statements {
        stmt(home, statement, out);
    }
    if let Some(tail) = value.value.tail {
        expr(home, tail, out);
    }
}

fn stmt<'a>(home: ModuleId<'a>, value: Node<'a, Stmt<'a>>, out: &mut impl References<'a>) {
    match &value.value {
        Stmt::Let(declaration) => {
            pattern(home, declaration.pattern, out);
            expr(home, declaration.value, out);
        }
        Stmt::Use { .. } | Stmt::Continue => {}
        Stmt::Assign { place, value, .. } => {
            // Conservatively count writes as uses: replacing a written binding
            // with `_` would leave an invalid assignment target.
            if let alder_ast::BindingName::Local(local) = place.root {
                out.use_local(local);
            }
            // A write constrains the target's type even without a read. Keep
            // writers in the same dependency analysis as ordinary references.
            if let alder_ast::BindingName::TopLevel(reference) = place.root {
                out.use_value(reference);
                if reference.module == home {
                    out.insert(reference.name);
                }
            }
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

fn expr<'a>(home: ModuleId<'a>, value: Node<'a, Expr<'a>>, out: &mut impl References<'a>) {
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
            reference: ValueRef::TopLevel(reference) | ValueRef::Foreign { reference, .. },
            ..
        } => {
            out.use_value(*reference);
            if reference.module == home {
                out.insert(reference.name);
            }
        }
        Expr::Var {
            reference: ValueRef::Local(local),
            ..
        } => out.use_local(*local),
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
        Expr::Block(value) | Expr::Async(value) | Expr::Loop(value) => block(home, value, out),
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

fn pattern<'a>(home: ModuleId<'a>, value: Node<'a, Pattern<'a>>, out: &mut impl References<'a>) {
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
        Pattern::Array { elements, rest } => {
            for element in *elements {
                pattern(home, element, out);
            }
            if let Some(rest) = rest
                && let Some(name) = rest.name
            {
                out.bind(name, rest.region, crate::BindingForm::ArrayRest);
            }
        }
        Pattern::Alias {
            pattern: nested,
            name,
        } => {
            pattern(home, nested, out);
            out.bind(*name, value.region, crate::BindingForm::Alias);
        }
        Pattern::Bind(name) => out.bind(*name, value.region, crate::BindingForm::Pattern),
        Pattern::Anything
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
    out: &mut impl References<'a>,
) {
    for field in fields {
        match field {
            RecordField::Field { value, .. } | RecordField::Spread(value) => {
                expr(home, value, out);
            }
        }
    }
}

fn collect_style<'a>(home: ModuleId<'a>, style: &'a Style<'a>, out: &mut impl References<'a>) {
    for entry in style.entries {
        match entry.value {
            StyleValue::Expr(value) => expr(home, value, out),
            StyleValue::Nested(nested) => collect_style(home, nested, out),
            StyleValue::Dimension { .. } => {}
        }
    }
}

fn collect_query<'a>(home: ModuleId<'a>, query: &'a Query<'a>, out: &mut impl References<'a>) {
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

fn collect_markup<'a>(home: ModuleId<'a>, markup: &'a Markup<'a>, out: &mut impl References<'a>) {
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
    out: &mut impl References<'a>,
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

fn collect_child<'a>(
    home: ModuleId<'a>,
    child: Node<'a, Child<'a>>,
    out: &mut impl References<'a>,
) {
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
    out: &mut impl References<'a>,
) {
    for item in value.value.items {
        match item {
            ChildItem::Stmt(value) => stmt(home, value, out),
            ChildItem::Child(value) => collect_child(home, value, out),
        }
    }
}
