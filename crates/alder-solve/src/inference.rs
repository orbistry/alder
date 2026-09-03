use std::collections::{BTreeMap, BTreeSet};

use alder_ast::{
    Annotation, BinOp, BindingName, Block, Child, ChildBlock, ChildItem, Expr, FieldPresence,
    ItemKind, Module, ModuleId, PackageId, Pattern, QualifiedName, RecordField, RowExtension, Stmt,
    Type, TypeSlot, ValueRef,
};
use alder_can::Annotations;
use alder_constrain::{Constraints, Error, ErrorKind};
use alder_region::{Located, Region};
use bumpalo::Bump;

#[derive(Clone, Debug)]
enum Ty<'a> {
    Var(usize),
    Con(QualifiedName<'a>),
    App(Box<Ty<'a>>, Vec<Ty<'a>>),
    Partial(QualifiedName<'a>, Vec<TySlot<'a>>),
    Projection(
        alder_ast::TraitId<'a>,
        Vec<Ty<'a>>,
        alder_ast::AssocTypeId<'a>,
    ),
    Fn(Vec<Ty<'a>>, Box<Ty<'a>>),
    Unit,
    Tuple(Vec<Ty<'a>>),
    Record(BTreeMap<&'a str, (FieldPresence, Ty<'a>)>, bool),
    ErrorRow,
    Any,
}

#[derive(Clone, Debug)]
enum TySlot<'a> {
    Hole(u16),
    Fixed(Ty<'a>),
}

#[derive(Clone, Debug)]
struct Scheme<'a> {
    quantified: Vec<usize>,
    typ: Ty<'a>,
}

#[derive(Clone, Default)]
struct Env<'a> {
    locals: BTreeMap<u32, Scheme<'a>>,
    globals: BTreeMap<QualifiedName<'a>, Scheme<'a>>,
}

struct Infer<'a> {
    bump: &'a Bump,
    substitutions: Vec<Option<Ty<'a>>>,
}

pub fn run<'a>(
    bump: &'a Bump,
    constraints: &Constraints<'a>,
) -> Result<Annotations<'a>, Vec<Error>> {
    Infer::new(bump)
        .infer_module(constraints.module)
        .map_err(|error| vec![error])
}

impl<'a> Infer<'a> {
    fn new(bump: &'a Bump) -> Self {
        Self {
            bump,
            substitutions: Vec::new(),
        }
    }

    fn fresh(&mut self) -> Ty<'a> {
        let id = self.substitutions.len();
        self.substitutions.push(None);
        Ty::Var(id)
    }

    fn infer_module(&mut self, module: &'a Module<'a>) -> Result<Annotations<'a>, Error> {
        let mut env = Env::default();
        let mut value_items = BTreeMap::new();
        for item in module.items {
            match &item.value.kind {
                ItemKind::Fn(function) => {
                    self.predeclare(&mut env, function.name);
                    value_items.insert(function.name, *item);
                }
                ItemKind::Extern(alder_ast::ExternDecl::Fn { name, .. }) => {
                    self.predeclare(&mut env, *name);
                    value_items.insert(*name, *item);
                }
                ItemKind::Let(decl) => {
                    for binding in decl.bindings {
                        self.predeclare(&mut env, *binding);
                        value_items.insert(*binding, *item);
                    }
                }
                ItemKind::Component(component) => {
                    self.predeclare(&mut env, component.name);
                    value_items.insert(component.name, *item);
                }
                _ => {}
            }
        }

        for item in module.items {
            if !is_value_item(&item.value.kind) {
                self.infer_item(&mut env, &item.value.kind, item.region)?;
            }
        }

        for group in module.value_sccs {
            let mut inferred_items = BTreeSet::new();
            for member in group.members {
                let item = value_items
                    .get(member)
                    .expect("each value SCC member has a declaration");
                let identity: *const Located<alder_ast::Item<'a>> = *item;
                if inferred_items.insert(identity) {
                    self.infer_value_item(&mut env, &item.value.kind, item.region)?;
                }
            }
            for member in group.members {
                self.generalize_global(&mut env, *member);
            }
        }

        let mut annotations = BTreeMap::new();
        for (name, scheme) in env.globals {
            annotations.insert(name, self.annotation(&scheme.typ));
        }
        Ok(annotations)
    }

    fn predeclare(&mut self, env: &mut Env<'a>, name: QualifiedName<'a>) {
        let typ = self.fresh();
        env.globals.insert(
            name,
            Scheme {
                quantified: Vec::new(),
                typ,
            },
        );
    }

    fn infer_item(
        &mut self,
        env: &mut Env<'a>,
        item: &'a ItemKind<'a>,
        region: Region,
    ) -> Result<(), Error> {
        match item {
            ItemKind::Fn(function) => {
                self.infer_value_item(env, item, region)?;
                self.generalize_global(env, function.name);
            }
            ItemKind::Extern(alder_ast::ExternDecl::Fn { name, .. }) => {
                self.infer_value_item(env, item, region)?;
                self.generalize_global(env, *name);
            }
            ItemKind::Let(decl) => {
                self.infer_value_item(env, item, region)?;
                for binding in decl.bindings {
                    self.generalize_global(env, *binding);
                }
            }
            ItemKind::Component(component) => {
                self.infer_value_item(env, item, region)?;
                self.generalize_global(env, component.name);
            }
            ItemKind::Impl(impl_) => {
                for item in impl_.items {
                    if let alder_ast::ImplItem::Fn(function) = item {
                        self.infer_function(
                            env,
                            function.params,
                            function.ret,
                            function.body,
                            region,
                        )?;
                    }
                }
            }
            ItemKind::Trait(trait_) => {
                for item in trait_.items {
                    if let alder_ast::TraitItem::Fn(function) = item
                        && let Some(body) = function.body
                    {
                        self.infer_function(env, function.params, function.ret, body, region)?;
                    }
                }
            }
            ItemKind::Test(test) => {
                self.infer_block(&mut env.clone(), test.body, None)?;
            }
            ItemKind::Tests(items) => {
                let mut nested = env.clone();
                for item in *items {
                    self.infer_item(&mut nested, &item.value.kind, item.region)?;
                }
            }
            ItemKind::TypeAlias(_)
            | ItemKind::Enum(_)
            | ItemKind::ErrorGroup(_)
            | ItemKind::Table(_)
            | ItemKind::Schema(_)
            | ItemKind::Macro(_)
            | ItemKind::Comptime(_)
            | ItemKind::Extern(alder_ast::ExternDecl::Type { .. }) => {}
        }
        Ok(())
    }

    fn infer_value_item(
        &mut self,
        env: &mut Env<'a>,
        item: &'a ItemKind<'a>,
        region: Region,
    ) -> Result<(), Error> {
        match item {
            ItemKind::Fn(function) => {
                let typ =
                    self.infer_function(env, function.params, function.ret, function.body, region)?;
                self.unify_global(env, function.name, typ, region)
            }
            ItemKind::Extern(alder_ast::ExternDecl::Fn {
                name, params, ret, ..
            }) => {
                let mut vars = BTreeMap::new();
                let mut args = Vec::with_capacity(params.len());
                for param in *params {
                    args.push(match param.annotation {
                        Some(typ) => self.from_ast(typ, &mut vars),
                        None => self.fresh(),
                    });
                }
                let ret = self.from_ast(ret, &mut vars);
                self.unify_global(env, *name, Ty::Fn(args, Box::new(ret)), region)
            }
            ItemKind::Let(decl) => {
                let mut value = self.infer_expr(env, decl.value, None)?;
                if let Some(annotation) = decl.annotation {
                    let annotated = self.from_ast(annotation, &mut BTreeMap::new());
                    self.unify(value.clone(), annotated, annotation.region)?;
                    value = self.prune(value);
                }
                self.infer_pattern(env, decl.pattern, value, true)
            }
            ItemKind::Component(component) => {
                let typ =
                    self.infer_function(env, component.params, None, component.body, region)?;
                let Ty::Fn(args, inferred) = typ else {
                    unreachable!()
                };
                self.unify(*inferred, self.named("Html", Vec::new()), region)?;
                self.unify_global(
                    env,
                    component.name,
                    Ty::Fn(args, Box::new(self.named("Html", Vec::new()))),
                    region,
                )
            }
            _ => unreachable!("only value items are inferred by value SCCs"),
        }
    }

    fn infer_function(
        &mut self,
        env: &Env<'a>,
        params: &'a [alder_ast::Param<'a>],
        ret: Option<&'a Located<Type<'a>>>,
        body: &'a Located<Block<'a>>,
        region: Region,
    ) -> Result<Ty<'a>, Error> {
        let mut local = env.clone();
        let mut vars = BTreeMap::new();
        let mut args = Vec::with_capacity(params.len());
        for param in params {
            let typ = match param.annotation {
                Some(annotation) => self.from_ast(annotation, &mut vars),
                None => self.fresh(),
            };
            self.infer_pattern(&mut local, param.pattern, typ.clone(), false)?;
            args.push(typ);
        }
        let result = match ret {
            Some(ret) => self.from_ast(ret, &mut vars),
            None => self.fresh(),
        };
        let body_result = match self.prune(result.clone()) {
            Ty::App(head, args)
                if matches!(*head, Ty::Con(reference) if reference.name == "Task")
                    && args.len() == 1 =>
            {
                args.into_iter().next().expect("length checked")
            }
            _ => result.clone(),
        };
        let body_type = self.infer_block(&mut local, body, Some(body_result.clone()))?;
        if body.value.tail.is_some() || !block_contains_return(body) {
            self.unify(body_type, body_result, region)?;
        }
        Ok(Ty::Fn(args, Box::new(self.prune(result))))
    }

    fn infer_block(
        &mut self,
        env: &mut Env<'a>,
        block: &'a Located<Block<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        for statement in block.value.statements {
            self.infer_stmt(env, statement, return_type.clone())?;
        }
        match block.value.tail {
            Some(tail) => self.infer_expr(env, tail, return_type),
            None => Ok(Ty::Unit),
        }
    }

    fn infer_stmt(
        &mut self,
        env: &mut Env<'a>,
        statement: &'a Located<Stmt<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        match &statement.value {
            Stmt::Let(decl) => {
                let value = self.infer_expr(env, decl.value, return_type.clone())?;
                if let Some(annotation) = decl.annotation {
                    let annotated = self.from_ast(annotation, &mut BTreeMap::new());
                    self.unify(value.clone(), annotated, annotation.region)?;
                }
                self.infer_pattern(env, decl.pattern, value, false)?;
            }
            Stmt::Use { .. } => {}
            Stmt::Assign { place, value, .. } => {
                let expected = self.place_type(env, place, statement.region)?;
                let actual = self.infer_expr(env, value, return_type.clone())?;
                self.unify(actual, expected, statement.region)?;
            }
            Stmt::For {
                pattern,
                iter,
                body,
            } => {
                let item = self.fresh();
                let iter_type = self.infer_expr(env, iter, return_type.clone())?;
                self.unify(
                    iter_type,
                    self.named("Array", vec![item.clone()]),
                    iter.region,
                )?;
                let mut nested = env.clone();
                self.infer_pattern(&mut nested, pattern, item, false)?;
                self.infer_block(&mut nested, body, return_type)?;
            }
            Stmt::While { condition, body } => {
                let condition_type = self.infer_expr(env, condition, return_type.clone())?;
                self.unify(
                    condition_type,
                    self.named("Bool", Vec::new()),
                    condition.region,
                )?;
                self.infer_block(&mut env.clone(), body, return_type)?;
            }
            Stmt::Return(value) => {
                let expected = return_type.unwrap_or(Ty::Unit);
                let actual = match value {
                    Some(value) => self.infer_expr(env, value, Some(expected.clone()))?,
                    None => Ty::Unit,
                };
                self.unify(actual, expected, statement.region)?;
            }
            Stmt::Break(value) => {
                if let Some(value) = value {
                    self.infer_expr(env, value, return_type)?;
                }
            }
            Stmt::Continue => {}
            Stmt::Assert(expr) => {
                let actual = self.infer_expr(env, expr, return_type)?;
                self.unify(actual, self.named("Bool", Vec::new()), expr.region)?;
            }
            Stmt::Expr(expr) => {
                self.infer_expr(env, expr, return_type)?;
            }
        }
        Ok(())
    }

    fn infer_expr(
        &mut self,
        env: &Env<'a>,
        expression: &'a Located<Expr<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        let region = expression.region;
        match &expression.value {
            Expr::Number { .. } => Ok(self.named("Number", Vec::new())),
            Expr::BigInt(_) => Ok(self.named("BigInt", Vec::new())),
            Expr::Str(_) | Expr::Template(_) | Expr::TaggedTemplate { .. } => {
                if let Expr::Template(parts) | Expr::TaggedTemplate { parts, .. } =
                    &expression.value
                {
                    for part in *parts {
                        if let alder_ast::TemplatePart::Expr(expr) = part {
                            self.infer_expr(env, expr, return_type.clone())?;
                        }
                    }
                }
                Ok(self.named("String", Vec::new()))
            }
            Expr::Bool(_) => Ok(self.named("Bool", Vec::new())),
            Expr::Unit => Ok(Ty::Unit),
            Expr::Var { reference, .. } => self.infer_reference(env, *reference, region),
            Expr::Constructor(constructor) => {
                Ok(self.instantiate_annotation(constructor.annotation))
            }
            Expr::Tag { args, .. } => {
                for arg in *args {
                    self.infer_expr(env, arg, return_type.clone())?;
                }
                Ok(Ty::ErrorRow)
            }
            Expr::Array(items) => {
                let item_type = self.fresh();
                for item in *items {
                    let actual = self.infer_expr(env, item, return_type.clone())?;
                    self.unify(actual, item_type.clone(), item.region)?;
                }
                let item_type = self.prune(item_type);
                Ok(self.named("Array", vec![item_type]))
            }
            Expr::Tuple(items) => {
                let mut types = Vec::with_capacity(items.len());
                for item in *items {
                    types.push(self.infer_expr(env, item, return_type.clone())?);
                }
                Ok(Ty::Tuple(types))
            }
            Expr::Record(fields) | Expr::RecordConstructor { fields, .. } => {
                self.infer_record(env, fields, return_type)
            }
            Expr::Call {
                function,
                arguments,
                ..
            } => {
                let function_type = self.infer_expr(env, function, return_type.clone())?;
                let mut args = Vec::with_capacity(arguments.len());
                for argument in *arguments {
                    args.push(self.infer_expr(env, argument, return_type.clone())?);
                }
                let result = self.fresh();
                self.unify(
                    function_type,
                    Ty::Fn(args, Box::new(result.clone())),
                    region,
                )?;
                Ok(self.prune(result))
            }
            Expr::Access { record, field } => {
                let record_type = self.infer_expr(env, record, return_type)?;
                self.access_field(record_type, field.value, field.region)
            }
            Expr::TupleAccess { tuple, index } => {
                let tuple_type = self.infer_expr(env, tuple, return_type)?;
                let tuple_type = self.prune(tuple_type);
                match tuple_type {
                    Ty::Tuple(items) if (index.value as usize) < items.len() => {
                        Ok(items[index.value as usize].clone())
                    }
                    Ty::Var(id) => {
                        let mut items = Vec::with_capacity(index.value as usize + 1);
                        for _ in 0..=index.value {
                            items.push(self.fresh());
                        }
                        let result = items[index.value as usize].clone();
                        self.bind(id, Ty::Tuple(items), region)?;
                        Ok(result)
                    }
                    actual => Err(self.mismatch(region, actual, Ty::Tuple(Vec::new()))),
                }
            }
            Expr::Index { target, index } => {
                let item = self.fresh();
                let target_type = self.infer_expr(env, target, return_type.clone())?;
                self.unify(
                    target_type,
                    self.named("Array", vec![item.clone()]),
                    target.region,
                )?;
                let index_type = self.infer_expr(env, index, return_type)?;
                self.unify(index_type, self.named("Number", Vec::new()), index.region)?;
                Ok(self.prune(item))
            }
            Expr::Await(expr) => {
                let value = self.fresh();
                let actual = self.infer_expr(env, expr, return_type)?;
                self.unify(actual, self.named("Task", vec![value.clone()]), region)?;
                Ok(self.prune(value))
            }
            Expr::Try(expr) => {
                let value = self.fresh();
                let error = self.fresh();
                let actual = self.infer_expr(env, expr, return_type)?;
                self.unify(
                    actual,
                    self.named("Result", vec![value.clone(), error]),
                    region,
                )?;
                Ok(self.prune(value))
            }
            Expr::Pin(expr) | Expr::State(expr) => self.infer_expr(env, expr, return_type),
            Expr::Negate { expr, .. } => {
                let actual = self.infer_expr(env, expr, return_type)?;
                self.unify(actual, self.named("Number", Vec::new()), region)?;
                Ok(self.named("Number", Vec::new()))
            }
            Expr::Not(expr) => {
                let actual = self.infer_expr(env, expr, return_type)?;
                self.unify(actual, self.named("Bool", Vec::new()), region)?;
                Ok(self.named("Bool", Vec::new()))
            }
            Expr::Binop {
                op, left, right, ..
            } => self.infer_binop(env, op.value, left, right, return_type),
            Expr::Block(block) => self.infer_block(&mut env.clone(), block, return_type),
            Expr::Lambda { params, ret, body } => {
                let mut local = env.clone();
                let mut vars = BTreeMap::new();
                let mut args = Vec::with_capacity(params.len());
                for param in *params {
                    let typ = param
                        .annotation
                        .map(|annotation| self.from_ast(annotation, &mut vars))
                        .unwrap_or_else(|| self.fresh());
                    self.infer_pattern(&mut local, param.pattern, typ.clone(), false)?;
                    args.push(typ);
                }
                let result = ret
                    .map(|ret| self.from_ast(ret, &mut vars))
                    .unwrap_or_else(|| self.fresh());
                let body_type = self.infer_expr(&local, body, Some(result.clone()))?;
                self.unify(body_type, result.clone(), region)?;
                Ok(Ty::Fn(args, Box::new(self.prune(result))))
            }
            Expr::If {
                branches,
                final_else,
            } => {
                let result = self.fresh();
                for branch in *branches {
                    let condition = self.infer_expr(env, branch.condition, return_type.clone())?;
                    self.unify(
                        condition,
                        self.named("Bool", Vec::new()),
                        branch.condition.region,
                    )?;
                    let body =
                        self.infer_block(&mut env.clone(), branch.body, return_type.clone())?;
                    self.unify(body, result.clone(), branch.body.region)?;
                }
                if let Some(final_else) = final_else {
                    let body = self.infer_block(&mut env.clone(), final_else, return_type)?;
                    self.unify(body, result.clone(), final_else.region)?;
                } else {
                    self.unify(Ty::Unit, result.clone(), region)?;
                }
                Ok(self.prune(result))
            }
            Expr::Match { scrutinee, arms } => {
                let scrutinee_type = self.infer_expr(env, scrutinee, return_type.clone())?;
                let result = self.fresh();
                for arm in *arms {
                    let mut local = env.clone();
                    for pattern in arm.patterns {
                        self.infer_pattern(&mut local, pattern, scrutinee_type.clone(), false)?;
                    }
                    if let Some(guard) = arm.guard {
                        let guard_type = self.infer_expr(&local, guard, return_type.clone())?;
                        self.unify(guard_type, self.named("Bool", Vec::new()), guard.region)?;
                    }
                    let body = self.infer_expr(&local, arm.body, return_type.clone())?;
                    self.unify(body, result.clone(), arm.body.region)?;
                }
                Ok(self.prune(result))
            }
            Expr::Loop(block) => {
                self.infer_block(&mut env.clone(), block, return_type)?;
                Ok(Ty::Unit)
            }
            Expr::Provide { value, body, .. } => {
                self.infer_expr(env, value, return_type.clone())?;
                self.infer_block(&mut env.clone(), body, return_type)
            }
            Expr::Style(style) => {
                for entry in style.entries {
                    self.infer_style_value(env, entry.value, return_type.clone())?;
                }
                Ok(self.named("Style", Vec::new()))
            }
            Expr::Query(query) => {
                self.infer_query_pins(env, query, return_type)?;
                let result = self.fresh();
                Ok(self.named("Query", vec![result]))
            }
            Expr::Markup(markup) => {
                match markup {
                    alder_ast::Markup::Element(element) => {
                        self.infer_element(env, element, return_type)?
                    }
                    alder_ast::Markup::Fragment(children) => {
                        for child in *children {
                            self.infer_child(env, child, return_type.clone())?;
                        }
                    }
                }
                Ok(self.named("Html", Vec::new()))
            }
            Expr::MacroCall { .. } => Ok(Ty::Any),
        }
    }

    fn infer_reference(
        &mut self,
        env: &Env<'a>,
        reference: ValueRef<'a>,
        _region: Region,
    ) -> Result<Ty<'a>, Error> {
        match reference {
            ValueRef::Local(local) => Ok(self.instantiate(&env.locals[&local.id.0])),
            ValueRef::TopLevel(name) => Ok(self.instantiate(&env.globals[&name])),
            ValueRef::Foreign { annotation, .. } | ValueRef::TraitMethod { annotation, .. } => {
                Ok(self.instantiate_annotation(annotation))
            }
            ValueRef::Module(_)
            | ValueRef::Builtin(_)
            | ValueRef::Provider(_)
            | ValueRef::QueryName(_)
            | ValueRef::Opaque(_) => Ok(Ty::Any),
        }
    }

    fn infer_record(
        &mut self,
        env: &Env<'a>,
        fields: &'a [RecordField<'a>],
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        let mut result = BTreeMap::new();
        for field in fields {
            match field {
                RecordField::Field { name, value } => {
                    let typ = self.infer_expr(env, value, return_type.clone())?;
                    result.insert(name.value, (FieldPresence::Required, typ));
                }
                RecordField::Spread(expr) => {
                    let spread = self.infer_expr(env, expr, return_type.clone())?;
                    if let Ty::Record(fields, _) = self.prune(spread) {
                        result.extend(fields);
                    }
                }
            }
        }
        Ok(Ty::Record(result, false))
    }

    fn infer_pattern(
        &mut self,
        env: &mut Env<'a>,
        pattern: &'a Located<Pattern<'a>>,
        expected: Ty<'a>,
        top_level: bool,
    ) -> Result<(), Error> {
        match &pattern.value {
            Pattern::Anything => {}
            Pattern::Bind(binding) => match binding {
                BindingName::Local(local) => {
                    env.locals.insert(
                        local.id.0,
                        Scheme {
                            quantified: Vec::new(),
                            typ: expected,
                        },
                    );
                }
                BindingName::TopLevel(name) => {
                    if top_level {
                        self.unify_global(env, *name, expected, pattern.region)?;
                    }
                }
            },
            Pattern::Pin { value: expr, .. } => {
                let actual = self.infer_expr(env, expr, None)?;
                self.unify(actual, expected, pattern.region)?;
            }
            Pattern::Number { .. } => {
                self.unify(expected, self.named("Number", Vec::new()), pattern.region)?;
            }
            Pattern::BigInt(_) => {
                self.unify(expected, self.named("BigInt", Vec::new()), pattern.region)?;
            }
            Pattern::Str(_) => {
                self.unify(expected, self.named("String", Vec::new()), pattern.region)?;
            }
            Pattern::Bool(_) => {
                self.unify(expected, self.named("Bool", Vec::new()), pattern.region)?;
            }
            Pattern::Unit => self.unify(expected, Ty::Unit, pattern.region)?,
            Pattern::Constructor { constructor, args } => {
                let constructor_type = self.instantiate_annotation(constructor.annotation);
                if args.is_empty() {
                    self.unify(constructor_type, expected, pattern.region)?;
                } else {
                    let mut arg_types = Vec::with_capacity(args.len());
                    for _ in *args {
                        arg_types.push(self.fresh());
                    }
                    self.unify(
                        constructor_type,
                        Ty::Fn(arg_types.clone(), Box::new(expected)),
                        pattern.region,
                    )?;
                    for (arg, typ) in args.iter().zip(arg_types) {
                        self.infer_pattern(env, arg, typ, false)?;
                    }
                }
            }
            Pattern::ConstructorRecord {
                constructor,
                fields,
                ..
            } => {
                let constructor_type = self.instantiate_annotation(constructor.annotation);
                let declared = match constructor.payload {
                    alder_ast::VariantPayload::Record(fields) => fields,
                    _ => &[],
                };
                let mut arg_types = Vec::with_capacity(declared.len());
                for _ in declared {
                    arg_types.push(self.fresh());
                }
                self.unify(
                    constructor_type,
                    Ty::Fn(arg_types.clone(), Box::new(expected)),
                    pattern.region,
                )?;
                for field in *fields {
                    if let Some(index) = declared
                        .iter()
                        .position(|declared| declared.name == field.name.value)
                    {
                        self.infer_pattern(env, field.pattern, arg_types[index].clone(), false)?;
                    }
                }
            }
            Pattern::Record { fields, .. } => {
                let mut record = BTreeMap::new();
                for field in *fields {
                    let typ = self.fresh();
                    self.infer_pattern(env, field.pattern, typ.clone(), false)?;
                    record.insert(field.name.value, (FieldPresence::Required, typ));
                }
                self.unify(expected, Ty::Record(record, true), pattern.region)?;
            }
            Pattern::Tag { args, .. } => {
                for arg in *args {
                    let typ = self.fresh();
                    self.infer_pattern(env, arg, typ, false)?;
                }
                self.unify(expected, Ty::ErrorRow, pattern.region)?;
            }
            Pattern::Tuple(items) => {
                let mut types = Vec::with_capacity(items.len());
                for item in *items {
                    let typ = self.fresh();
                    self.infer_pattern(env, item, typ.clone(), false)?;
                    types.push(typ);
                }
                self.unify(expected, Ty::Tuple(types), pattern.region)?;
            }
            Pattern::Array { elements, rest } => {
                let item = self.fresh();
                for element in *elements {
                    self.infer_pattern(env, element, item.clone(), false)?;
                }
                if let Some(rest) = rest.and_then(|rest| rest.name)
                    && let BindingName::Local(local) = rest
                {
                    env.locals.insert(
                        local.id.0,
                        Scheme {
                            quantified: Vec::new(),
                            typ: self.named("Array", vec![item.clone()]),
                        },
                    );
                }
                self.unify(expected, self.named("Array", vec![item]), pattern.region)?;
            }
            Pattern::Alias { pattern, name } => {
                self.infer_pattern(env, pattern, expected.clone(), false)?;
                if let BindingName::Local(local) = name {
                    env.locals.insert(
                        local.id.0,
                        Scheme {
                            quantified: Vec::new(),
                            typ: expected,
                        },
                    );
                }
            }
        }
        Ok(())
    }

    fn infer_binop(
        &mut self,
        env: &Env<'a>,
        op: BinOp,
        left: &'a Located<Expr<'a>>,
        right: &'a Located<Expr<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<Ty<'a>, Error> {
        let left_type = self.infer_expr(env, left, return_type.clone())?;
        let right_type = self.infer_expr(env, right, return_type)?;
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                let number = self.named("Number", Vec::new());
                self.unify(left_type, number.clone(), left.region)?;
                self.unify(right_type, number.clone(), right.region)?;
                Ok(number)
            }
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
                self.unify(left_type, right_type, right.region)?;
                Ok(self.named("Bool", Vec::new()))
            }
            BinOp::And | BinOp::Or => {
                let bool_ = self.named("Bool", Vec::new());
                self.unify(left_type, bool_.clone(), left.region)?;
                self.unify(right_type, bool_.clone(), right.region)?;
                Ok(bool_)
            }
            BinOp::Coalesce => {
                self.unify(left_type.clone(), right_type, right.region)?;
                Ok(self.prune(left_type))
            }
            BinOp::Pipe => {
                let result = self.fresh();
                self.unify(
                    right_type,
                    Ty::Fn(vec![left_type], Box::new(result.clone())),
                    right.region,
                )?;
                Ok(self.prune(result))
            }
            BinOp::In => Ok(self.named("Bool", Vec::new())),
        }
    }

    fn place_type(
        &mut self,
        env: &Env<'a>,
        place: &'a alder_ast::Place<'a>,
        region: Region,
    ) -> Result<Ty<'a>, Error> {
        let mut typ = match place.root {
            BindingName::Local(local) => self.instantiate(&env.locals[&local.id.0]),
            BindingName::TopLevel(name) => self.instantiate(&env.globals[&name]),
        };
        for step in place.steps {
            typ = match step {
                alder_ast::PlaceStep::Field(field) => {
                    self.access_field(typ, field.value, field.region)?
                }
                alder_ast::PlaceStep::TupleIndex(index) => match self.prune(typ) {
                    Ty::Tuple(items) if (index.value as usize) < items.len() => {
                        items[index.value as usize].clone()
                    }
                    actual => return Err(self.mismatch(region, actual, Ty::Tuple(Vec::new()))),
                },
                alder_ast::PlaceStep::Index(index) => {
                    let item = self.fresh();
                    self.unify(typ, self.named("Array", vec![item.clone()]), region)?;
                    let index_type = self.infer_expr(env, index, None)?;
                    self.unify(index_type, self.named("Number", Vec::new()), index.region)?;
                    item
                }
            }
        }
        Ok(typ)
    }

    fn access_field(
        &mut self,
        record: Ty<'a>,
        field: &'a str,
        region: Region,
    ) -> Result<Ty<'a>, Error> {
        match self.prune(record) {
            Ty::Record(fields, _) => match fields.get(field) {
                Some((FieldPresence::Required, typ)) => Ok(typ.clone()),
                Some((FieldPresence::Optional, typ)) => Ok(self.named("Option", vec![typ.clone()])),
                None => Err(Error {
                    region,
                    kind: ErrorKind::MissingField {
                        field: field.to_owned(),
                    },
                }),
            },
            Ty::Var(id) => {
                let result = self.fresh();
                let fields = BTreeMap::from([(field, (FieldPresence::Required, result.clone()))]);
                self.bind(id, Ty::Record(fields, true), region)?;
                Ok(result)
            }
            Ty::Any => Ok(Ty::Any),
            actual => Err(self.mismatch(region, actual, Ty::Record(BTreeMap::new(), true))),
        }
    }

    fn infer_style_value(
        &mut self,
        env: &Env<'a>,
        value: alder_ast::StyleValue<'a>,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        match value {
            alder_ast::StyleValue::Dimension { .. } => {}
            alder_ast::StyleValue::Expr(expr) => {
                self.infer_expr(env, expr, return_type)?;
            }
            alder_ast::StyleValue::Nested(style) => {
                for entry in style.entries {
                    self.infer_style_value(env, entry.value, return_type.clone())?;
                }
            }
        }
        Ok(())
    }

    fn infer_query_pins(
        &mut self,
        env: &Env<'a>,
        query: &'a alder_ast::Query<'a>,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        let mut infer = |expr: &'a Located<Expr<'a>>| {
            self.infer_expr(env, expr, return_type.clone()).map(|_| ())
        };
        match query {
            alder_ast::Query::Select(select) => {
                if let alder_ast::Projection::Fields(fields) = select.projection {
                    for field in fields {
                        infer(field)?;
                    }
                }
                for join in select.joins {
                    infer(join.on)?;
                }
                if let Some(expr) = select.where_ {
                    infer(expr)?;
                }
                for expr in select.group_by {
                    infer(expr)?;
                }
                for order in select.order_by {
                    infer(order.expr)?;
                }
                if let Some(expr) = select.limit {
                    infer(expr)?;
                }
                if let Some(expr) = select.offset {
                    infer(expr)?;
                }
            }
            alder_ast::Query::Insert { values, .. } => infer(values)?,
            alder_ast::Query::Update { set, where_, .. } => {
                for field in *set {
                    match field {
                        RecordField::Field { value, .. } | RecordField::Spread(value) => {
                            infer(value)?
                        }
                    }
                }
                if let Some(expr) = where_ {
                    infer(expr)?;
                }
            }
            alder_ast::Query::Delete { where_, .. } => {
                if let Some(expr) = where_ {
                    infer(expr)?;
                }
            }
        }
        Ok(())
    }

    fn infer_element(
        &mut self,
        env: &Env<'a>,
        element: &'a alder_ast::Element<'a>,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        for attr in element.attrs {
            if let Some(alder_ast::AttrValue::Expr(expr)) = attr.value {
                self.infer_expr(env, expr, return_type.clone())?;
            }
        }
        for child in element.children {
            self.infer_child(env, child, return_type.clone())?;
        }
        Ok(())
    }

    fn infer_child(
        &mut self,
        env: &Env<'a>,
        child: &'a Located<Child<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        match &child.value {
            Child::Element(element) => self.infer_element(env, element, return_type)?,
            Child::Fragment(children) => {
                for child in *children {
                    self.infer_child(env, child, return_type.clone())?;
                }
            }
            Child::Text(_) => {}
            Child::Hole(expr) => {
                self.infer_expr(env, expr, return_type)?;
            }
            Child::If {
                branches,
                final_else,
            } => {
                for branch in *branches {
                    let condition = self.infer_expr(env, branch.condition, return_type.clone())?;
                    self.unify(
                        condition,
                        self.named("Bool", Vec::new()),
                        branch.condition.region,
                    )?;
                    self.infer_child_block(env, branch.body, return_type.clone())?;
                }
                if let Some(block) = final_else {
                    self.infer_child_block(env, block, return_type)?;
                }
            }
            Child::For {
                pattern,
                iter,
                key,
                body,
                empty,
            } => {
                let item = self.fresh();
                let iter_type = self.infer_expr(env, iter, return_type.clone())?;
                self.unify(
                    iter_type,
                    self.named("Array", vec![item.clone()]),
                    iter.region,
                )?;
                let mut local = env.clone();
                self.infer_pattern(&mut local, pattern, item, false)?;
                if let Some(key) = key {
                    self.infer_expr(&local, key, return_type.clone())?;
                }
                self.infer_child_block(&local, body, return_type.clone())?;
                if let Some(empty) = empty {
                    self.infer_child_block(env, empty, return_type)?;
                }
            }
            Child::Match { scrutinee, arms } => {
                let typ = self.infer_expr(env, scrutinee, return_type.clone())?;
                for arm in *arms {
                    let mut local = env.clone();
                    for pattern in arm.patterns {
                        self.infer_pattern(&mut local, pattern, typ.clone(), false)?;
                    }
                    if let Some(guard) = arm.guard {
                        let guard_type = self.infer_expr(&local, guard, return_type.clone())?;
                        self.unify(guard_type, self.named("Bool", Vec::new()), guard.region)?;
                    }
                    self.infer_child_block(&local, arm.body, return_type.clone())?;
                }
            }
        }
        Ok(())
    }

    fn infer_child_block(
        &mut self,
        env: &Env<'a>,
        block: &'a Located<ChildBlock<'a>>,
        return_type: Option<Ty<'a>>,
    ) -> Result<(), Error> {
        let mut local = env.clone();
        for item in block.value.items {
            match item {
                ChildItem::Stmt(stmt) => self.infer_stmt(&mut local, stmt, return_type.clone())?,
                ChildItem::Child(child) => self.infer_child(&local, child, return_type.clone())?,
            }
        }
        Ok(())
    }

    fn unify_global(
        &mut self,
        env: &Env<'a>,
        name: QualifiedName<'a>,
        typ: Ty<'a>,
        region: Region,
    ) -> Result<(), Error> {
        self.unify(env.globals[&name].typ.clone(), typ, region)
    }

    fn generalize_global(&mut self, env: &mut Env<'a>, name: QualifiedName<'a>) {
        let typ = self.prune(env.globals[&name].typ.clone());
        let mut vars = BTreeSet::new();
        self.free_vars(&typ, &mut vars);
        env.globals.insert(
            name,
            Scheme {
                quantified: vars.into_iter().collect(),
                typ,
            },
        );
    }

    fn instantiate(&mut self, scheme: &Scheme<'a>) -> Ty<'a> {
        let replacements: BTreeMap<_, _> = scheme
            .quantified
            .iter()
            .map(|id| (*id, self.fresh()))
            .collect();
        self.replace_vars(&scheme.typ, &replacements)
    }

    fn instantiate_annotation(&mut self, annotation: &'a Annotation<'a>) -> Ty<'a> {
        self.from_ast(annotation.typ, &mut BTreeMap::new())
    }

    #[allow(clippy::wrong_self_convention)]
    fn from_ast(
        &mut self,
        typ: &'a Located<Type<'a>>,
        vars: &mut BTreeMap<&'a str, Ty<'a>>,
    ) -> Ty<'a> {
        match &typ.value {
            Type::Var { name, args } => {
                let base = vars.entry(name).or_insert_with(|| self.fresh()).clone();
                let args = args.iter().map(|arg| self.from_ast(arg, vars)).collect();
                self.apply(base, args)
            }
            Type::Named { reference, args } => {
                let args = args.iter().map(|arg| self.from_ast(arg, vars)).collect();
                self.apply(Ty::Con(*reference), args)
            }
            Type::Partial { constructor, slots } => Ty::Partial(
                *constructor,
                slots
                    .iter()
                    .map(|slot| match slot {
                        TypeSlot::Hole(index) => TySlot::Hole(*index),
                        TypeSlot::Fixed(typ) => TySlot::Fixed(self.from_ast(typ, vars)),
                    })
                    .collect(),
            ),
            Type::Projection(projection) => Ty::Projection(
                projection.trait_ref.trait_,
                projection
                    .trait_ref
                    .args
                    .iter()
                    .map(|arg| self.from_ast(arg, vars))
                    .collect(),
                projection.assoc,
            ),
            Type::Fn { params, ret } => Ty::Fn(
                params
                    .iter()
                    .map(|param| self.from_ast(param, vars))
                    .collect(),
                Box::new(self.from_ast(ret, vars)),
            ),
            Type::Unit => Ty::Unit,
            Type::Tuple(items) => {
                Ty::Tuple(items.iter().map(|item| self.from_ast(item, vars)).collect())
            }
            Type::Record { fields, ext } => Ty::Record(
                fields
                    .iter()
                    .map(|field| (field.name, (field.presence, self.from_ast(field.typ, vars))))
                    .collect(),
                matches!(ext, RowExtension::Open(_)),
            ),
            Type::ErrorRow { .. } => Ty::ErrorRow,
            Type::Alias { target, .. } => match target {
                alder_ast::AliasType::Open(real) | alder_ast::AliasType::Filled(real) => {
                    self.from_ast(real, vars)
                }
            },
        }
    }

    fn unify(&mut self, left: Ty<'a>, right: Ty<'a>, region: Region) -> Result<(), Error> {
        let left = self.prune(left);
        let right = self.prune(right);
        if let Some(result) = self.unify_higher_kinded_pattern(&left, &right, region) {
            return result;
        }
        if let Some(result) = self.unify_higher_kinded_pattern(&right, &left, region) {
            return result;
        }
        match (left, right) {
            (Ty::Any, _) | (_, Ty::Any) | (Ty::ErrorRow, Ty::ErrorRow) => Ok(()),
            (Ty::Var(left), Ty::Var(right)) if left == right => Ok(()),
            (Ty::Var(id), typ) | (typ, Ty::Var(id)) => self.bind(id, typ, region),
            (Ty::Unit, Ty::Unit) => Ok(()),
            (Ty::Con(left), Ty::Con(right)) if left == right => Ok(()),
            (Ty::App(left_head, left_args), Ty::App(right_head, right_args))
                if left_args.len() == right_args.len() =>
            {
                self.unify(*left_head, *right_head, region)?;
                for (left, right) in left_args.into_iter().zip(right_args) {
                    self.unify(left, right, region)?;
                }
                Ok(())
            }
            (Ty::Partial(left, left_slots), Ty::Partial(right, right_slots))
                if left == right && left_slots.len() == right_slots.len() =>
            {
                let actual = Ty::Partial(left, left_slots.clone());
                let expected = Ty::Partial(right, right_slots.clone());
                for (left_slot, right_slot) in left_slots.into_iter().zip(right_slots) {
                    match (left_slot, right_slot) {
                        (TySlot::Hole(left), TySlot::Hole(right)) if left == right => {}
                        (TySlot::Fixed(left), TySlot::Fixed(right)) => {
                            self.unify(left, right, region)?;
                        }
                        _ => return Err(self.mismatch(region, actual, expected)),
                    }
                }
                Ok(())
            }
            (
                Ty::Projection(left_trait, left_args, left_assoc),
                Ty::Projection(right_trait, right_args, right_assoc),
            ) if left_trait == right_trait
                && left_assoc == right_assoc
                && left_args.len() == right_args.len() =>
            {
                for (left, right) in left_args.into_iter().zip(right_args) {
                    self.unify(left, right, region)?;
                }
                Ok(())
            }
            (Ty::Fn(left_args, left_ret), Ty::Fn(right_args, right_ret))
                if left_args.len() == right_args.len() =>
            {
                for (left, right) in left_args.into_iter().zip(right_args) {
                    self.unify(left, right, region)?;
                }
                self.unify(*left_ret, *right_ret, region)
            }
            (Ty::Tuple(left), Ty::Tuple(right)) if left.len() == right.len() => {
                for (left, right) in left.into_iter().zip(right) {
                    self.unify(left, right, region)?;
                }
                Ok(())
            }
            (Ty::Record(left, left_open), Ty::Record(right, right_open)) => {
                self.unify_records(left, left_open, right, right_open, region)
            }
            (left, right) => Err(self.mismatch(region, left, right)),
        }
    }

    fn unify_higher_kinded_pattern(
        &mut self,
        pattern: &Ty<'a>,
        rigid: &Ty<'a>,
        region: Region,
    ) -> Option<Result<(), Error>> {
        let Ty::App(head, pattern_args) = pattern else {
            return None;
        };
        let Ty::Var(head_var) = self.prune((**head).clone()) else {
            return None;
        };
        let (constructor, rigid_args) = match self.prune(rigid.clone()) {
            Ty::Con(constructor) => (constructor, Vec::new()),
            Ty::App(head, args) => match *head {
                Ty::Con(constructor) => (constructor, args),
                _ => return None,
            },
            _ => return None,
        };
        if pattern_args.is_empty() || rigid_args.len() < pattern_args.len() {
            return None;
        }

        let mut variables = Vec::with_capacity(pattern_args.len());
        let mut seen = BTreeSet::new();
        for argument in pattern_args {
            let Ty::Var(variable) = self.prune(argument.clone()) else {
                return Some(Err(Error {
                    region,
                    kind: ErrorKind::UnsupportedHigherKindedUnification,
                }));
            };
            if variable == head_var || !seen.insert(variable) {
                return Some(Err(Error {
                    region,
                    kind: ErrorKind::UnsupportedHigherKindedUnification,
                }));
            }
            variables.push(variable);
        }

        for (variable, rigid_arg) in variables.iter().zip(&rigid_args) {
            if let Err(error) = self.unify(Ty::Var(*variable), rigid_arg.clone(), region) {
                return Some(Err(error));
            }
        }
        let slots = rigid_args
            .into_iter()
            .enumerate()
            .map(|(index, typ)| {
                if index < variables.len() {
                    TySlot::Hole(index as u16)
                } else {
                    TySlot::Fixed(typ)
                }
            })
            .collect();
        Some(self.bind(head_var, Ty::Partial(constructor, slots), region))
    }

    fn unify_records(
        &mut self,
        left: BTreeMap<&'a str, (FieldPresence, Ty<'a>)>,
        left_open: bool,
        right: BTreeMap<&'a str, (FieldPresence, Ty<'a>)>,
        right_open: bool,
        region: Region,
    ) -> Result<(), Error> {
        for (name, (left_presence, left_type)) in &left {
            match right.get(name) {
                Some((right_presence, right_type)) => {
                    if left_presence != right_presence
                        && !matches!(
                            (left_presence, right_presence),
                            (FieldPresence::Required, FieldPresence::Optional)
                                | (FieldPresence::Optional, FieldPresence::Required)
                        )
                    {
                        return Err(self.mismatch(
                            region,
                            Ty::Record(left, left_open),
                            Ty::Record(right, right_open),
                        ));
                    }
                    self.unify(left_type.clone(), right_type.clone(), region)?;
                }
                None if !right_open && *left_presence == FieldPresence::Required => {
                    return Err(Error {
                        region,
                        kind: ErrorKind::MissingField {
                            field: (*name).to_owned(),
                        },
                    });
                }
                None => {}
            }
        }
        for (name, (presence, _)) in &right {
            if !left.contains_key(name) && !left_open && *presence == FieldPresence::Required {
                return Err(Error {
                    region,
                    kind: ErrorKind::MissingField {
                        field: (*name).to_owned(),
                    },
                });
            }
        }
        Ok(())
    }

    fn bind(&mut self, id: usize, typ: Ty<'a>, region: Region) -> Result<(), Error> {
        if self.occurs(id, &typ) {
            return Err(Error {
                region,
                kind: ErrorKind::InfiniteType,
            });
        }
        self.substitutions[id] = Some(typ);
        Ok(())
    }

    fn prune(&mut self, typ: Ty<'a>) -> Ty<'a> {
        match typ {
            Ty::Var(id) => match self.substitutions[id].clone() {
                Some(bound) => {
                    let pruned = self.prune(bound);
                    self.substitutions[id] = Some(pruned.clone());
                    pruned
                }
                None => Ty::Var(id),
            },
            Ty::App(head, args) => {
                let head = self.prune(*head);
                let args = args.into_iter().map(|arg| self.prune(arg)).collect();
                self.apply(head, args)
            }
            other => other,
        }
    }

    fn apply(&self, head: Ty<'a>, mut arguments: Vec<Ty<'a>>) -> Ty<'a> {
        if arguments.is_empty() {
            return head;
        }
        match head {
            Ty::App(head, mut existing) => {
                existing.append(&mut arguments);
                Ty::App(head, existing)
            }
            Ty::Partial(constructor, slots) => {
                let mut supplied = arguments.into_iter();
                let mut remaining_hole = 0;
                let mut filled = Vec::with_capacity(slots.len());
                let mut complete = true;
                for slot in slots {
                    match slot {
                        TySlot::Fixed(typ) => filled.push(TySlot::Fixed(typ)),
                        TySlot::Hole(_) => match supplied.next() {
                            Some(typ) => filled.push(TySlot::Fixed(typ)),
                            None => {
                                filled.push(TySlot::Hole(remaining_hole));
                                remaining_hole += 1;
                                complete = false;
                            }
                        },
                    }
                }
                let rest = supplied.collect::<Vec<_>>();
                if complete {
                    let base = Ty::App(
                        Box::new(Ty::Con(constructor)),
                        filled
                            .into_iter()
                            .map(|slot| match slot {
                                TySlot::Fixed(typ) => typ,
                                TySlot::Hole(_) => unreachable!("complete partial has no holes"),
                            })
                            .collect(),
                    );
                    if rest.is_empty() {
                        base
                    } else {
                        Ty::App(Box::new(base), rest)
                    }
                } else {
                    debug_assert!(rest.is_empty());
                    Ty::Partial(constructor, filled)
                }
            }
            other => Ty::App(Box::new(other), arguments),
        }
    }

    fn occurs(&mut self, needle: usize, typ: &Ty<'a>) -> bool {
        match self.prune(typ.clone()) {
            Ty::Var(id) => id == needle,
            Ty::Con(_) => false,
            Ty::App(head, args) => {
                self.occurs(needle, &head) || args.iter().any(|arg| self.occurs(needle, arg))
            }
            Ty::Tuple(args) => args.iter().any(|arg| self.occurs(needle, arg)),
            Ty::Partial(_, slots) => slots.iter().any(|slot| match slot {
                TySlot::Hole(_) => false,
                TySlot::Fixed(typ) => self.occurs(needle, typ),
            }),
            Ty::Projection(_, args, _) => args.iter().any(|arg| self.occurs(needle, arg)),
            Ty::Fn(args, ret) => {
                args.iter().any(|arg| self.occurs(needle, arg)) || self.occurs(needle, &ret)
            }
            Ty::Record(fields, _) => fields.values().any(|(_, typ)| self.occurs(needle, typ)),
            Ty::Unit | Ty::ErrorRow | Ty::Any => false,
        }
    }

    fn free_vars(&mut self, typ: &Ty<'a>, result: &mut BTreeSet<usize>) {
        match self.prune(typ.clone()) {
            Ty::Var(id) => {
                result.insert(id);
            }
            Ty::Con(_) => {}
            Ty::App(head, args) => {
                self.free_vars(&head, result);
                for arg in &args {
                    self.free_vars(arg, result);
                }
            }
            Ty::Tuple(args) => {
                for arg in &args {
                    self.free_vars(arg, result);
                }
            }
            Ty::Partial(_, slots) => {
                for slot in &slots {
                    if let TySlot::Fixed(typ) = slot {
                        self.free_vars(typ, result);
                    }
                }
            }
            Ty::Projection(_, args, _) => {
                for arg in &args {
                    self.free_vars(arg, result);
                }
            }
            Ty::Fn(args, ret) => {
                for arg in &args {
                    self.free_vars(arg, result);
                }
                self.free_vars(&ret, result);
            }
            Ty::Record(fields, _) => {
                for (_, typ) in fields.values() {
                    self.free_vars(typ, result);
                }
            }
            Ty::Unit | Ty::ErrorRow | Ty::Any => {}
        }
    }

    fn replace_vars(&mut self, typ: &Ty<'a>, replacements: &BTreeMap<usize, Ty<'a>>) -> Ty<'a> {
        match self.prune(typ.clone()) {
            Ty::Var(id) => replacements.get(&id).cloned().unwrap_or(Ty::Var(id)),
            Ty::Con(name) => Ty::Con(name),
            Ty::App(head, args) => Ty::App(
                Box::new(self.replace_vars(&head, replacements)),
                args.iter()
                    .map(|arg| self.replace_vars(arg, replacements))
                    .collect(),
            ),
            Ty::Partial(name, slots) => Ty::Partial(
                name,
                slots
                    .iter()
                    .map(|slot| match slot {
                        TySlot::Hole(index) => TySlot::Hole(*index),
                        TySlot::Fixed(typ) => TySlot::Fixed(self.replace_vars(typ, replacements)),
                    })
                    .collect(),
            ),
            Ty::Projection(trait_, args, assoc) => Ty::Projection(
                trait_,
                args.iter()
                    .map(|arg| self.replace_vars(arg, replacements))
                    .collect(),
                assoc,
            ),
            Ty::Fn(args, ret) => Ty::Fn(
                args.iter()
                    .map(|arg| self.replace_vars(arg, replacements))
                    .collect(),
                Box::new(self.replace_vars(&ret, replacements)),
            ),
            Ty::Tuple(items) => Ty::Tuple(
                items
                    .iter()
                    .map(|item| self.replace_vars(item, replacements))
                    .collect(),
            ),
            Ty::Record(fields, open) => Ty::Record(
                fields
                    .iter()
                    .map(|(name, (presence, typ))| {
                        (*name, (*presence, self.replace_vars(typ, replacements)))
                    })
                    .collect(),
                open,
            ),
            other => other,
        }
    }

    fn annotation(&mut self, typ: &Ty<'a>) -> &'a Annotation<'a> {
        let typ = self.prune(typ.clone());
        let mut arities = BTreeMap::new();
        self.collect_kind_arities(&typ, &mut arities);
        let mut names = BTreeMap::new();
        let typ = self.to_ast(&typ, &mut names);
        let mut params = names.into_iter().collect::<Vec<_>>();
        params.sort_by_key(|(_, name)| generated_type_name_rank(name));
        self.bump.alloc(Annotation {
            params: self
                .bump
                .alloc_slice_fill_iter(params.into_iter().map(|(id, name)| alder_ast::TypeParam {
                    name: Located::at(Region::zero(), name),
                    kind: self.kind_from_arity(arities.get(&id).copied().unwrap_or(0)),
                })),
            trait_predicates: &[],
            projection_equalities: &[],
            typ,
        })
    }

    fn collect_kind_arities(&mut self, typ: &Ty<'a>, arities: &mut BTreeMap<usize, usize>) {
        match self.prune(typ.clone()) {
            Ty::Var(id) => {
                arities.entry(id).or_insert(0);
            }
            Ty::Con(_) | Ty::Unit | Ty::ErrorRow | Ty::Any => {}
            Ty::App(head, args) => {
                match self.prune(*head) {
                    Ty::Var(id) => {
                        arities
                            .entry(id)
                            .and_modify(|arity| *arity = (*arity).max(args.len()))
                            .or_insert(args.len());
                    }
                    other => self.collect_kind_arities(&other, arities),
                }
                for arg in &args {
                    self.collect_kind_arities(arg, arities);
                }
            }
            Ty::Partial(_, slots) => {
                for slot in &slots {
                    if let TySlot::Fixed(typ) = slot {
                        self.collect_kind_arities(typ, arities);
                    }
                }
            }
            Ty::Projection(_, args, _) | Ty::Tuple(args) => {
                for arg in &args {
                    self.collect_kind_arities(arg, arities);
                }
            }
            Ty::Fn(args, ret) => {
                for arg in &args {
                    self.collect_kind_arities(arg, arities);
                }
                self.collect_kind_arities(&ret, arities);
            }
            Ty::Record(fields, _) => {
                for (_, typ) in fields.values() {
                    self.collect_kind_arities(typ, arities);
                }
            }
        }
    }

    fn kind_from_arity(&self, arity: usize) -> alder_ast::Kind<'a> {
        let mut kind = alder_ast::Kind::Type;
        for _ in 0..arity {
            kind = alder_ast::Kind::Arrow {
                param: self.bump.alloc(alder_ast::Kind::Type),
                result: self.bump.alloc(kind),
            };
        }
        kind
    }

    #[allow(clippy::wrong_self_convention)]
    fn to_ast(
        &mut self,
        typ: &Ty<'a>,
        names: &mut BTreeMap<usize, &'a str>,
    ) -> &'a Located<Type<'a>> {
        let typ = match self.prune(typ.clone()) {
            Ty::Var(id) => {
                let name = self.type_var_name(id, names);
                Type::Var { name, args: &[] }
            }
            Ty::Con(reference) => Type::Named {
                reference,
                args: &[],
            },
            Ty::App(head, args) => match self.prune(*head) {
                Ty::Con(reference) => Type::Named {
                    reference,
                    args: self
                        .bump
                        .alloc_slice_fill_iter(args.iter().map(|arg| self.to_ast(arg, names))),
                },
                Ty::Var(id) => {
                    let name = self.type_var_name(id, names);
                    Type::Var {
                        name,
                        args: self
                            .bump
                            .alloc_slice_fill_iter(args.iter().map(|arg| self.to_ast(arg, names))),
                    }
                }
                other => panic!("unsupported public type application head: {other:?}"),
            },
            Ty::Partial(constructor, slots) => Type::Partial {
                constructor,
                slots: self
                    .bump
                    .alloc_slice_fill_iter(slots.iter().map(|slot| match slot {
                        TySlot::Hole(index) => TypeSlot::Hole(*index),
                        TySlot::Fixed(typ) => TypeSlot::Fixed(self.to_ast(typ, names)),
                    })),
            },
            Ty::Projection(trait_, args, assoc) => Type::Projection(alder_ast::ProjectionType {
                trait_ref: alder_ast::TraitRef {
                    trait_,
                    args: self
                        .bump
                        .alloc_slice_fill_iter(args.iter().map(|arg| self.to_ast(arg, names))),
                },
                assoc,
            }),
            Ty::Fn(params, ret) => Type::Fn {
                params: self
                    .bump
                    .alloc_slice_fill_iter(params.iter().map(|param| self.to_ast(param, names))),
                ret: self.to_ast(&ret, names),
            },
            Ty::Unit | Ty::Any => Type::Unit,
            Ty::Tuple(items) => Type::Tuple(
                self.bump
                    .alloc_slice_fill_iter(items.iter().map(|item| self.to_ast(item, names))),
            ),
            Ty::Record(fields, open) => Type::Record {
                fields: self
                    .bump
                    .alloc_slice_fill_iter(fields.iter().enumerate().map(
                        |(index, (name, (presence, typ)))| alder_ast::RecordTypeField {
                            index: index as u16,
                            name,
                            presence: *presence,
                            typ: self.to_ast(typ, names),
                        },
                    )),
                ext: if open {
                    RowExtension::Open("r")
                } else {
                    RowExtension::Closed
                },
            },
            Ty::ErrorRow => Type::ErrorRow {
                tags: &[],
                ext: RowExtension::Open("e"),
            },
        };
        self.bump.alloc(Located::at_zero(typ))
    }

    fn type_var_name(&self, id: usize, names: &mut BTreeMap<usize, &'a str>) -> &'a str {
        let next = names.len();
        names.entry(id).or_insert_with(|| {
            let generated = if next < 26 {
                ((b'a' + next as u8) as char).to_string()
            } else {
                format!("t{next}")
            };
            self.bump.alloc_str(&generated)
        })
    }

    fn named(&self, name: &'a str, args: Vec<Ty<'a>>) -> Ty<'a> {
        self.apply(
            Ty::Con(QualifiedName {
                module: ModuleId {
                    package: PackageId::Builtin,
                    path: &[],
                },
                name,
            }),
            args,
        )
    }

    fn mismatch(&mut self, region: Region, actual: Ty<'a>, expected: Ty<'a>) -> Error {
        Error {
            region,
            kind: ErrorKind::Mismatch {
                actual: self.render(actual),
                expected: self.render(expected),
            },
        }
    }

    fn render(&mut self, typ: Ty<'a>) -> String {
        match self.prune(typ) {
            Ty::Var(_) => "a".to_owned(),
            Ty::Con(name) => name.name.to_owned(),
            Ty::App(head, args) => format!(
                "{}[{}]",
                self.render(*head),
                args.into_iter()
                    .map(|arg| self.render(arg))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Fn(args, ret) => format!(
                "fn({}) -> {}",
                args.into_iter()
                    .map(|arg| self.render(arg))
                    .collect::<Vec<_>>()
                    .join(", "),
                self.render(*ret)
            ),
            Ty::Unit => "()".to_owned(),
            Ty::Tuple(items) => format!(
                "({})",
                items
                    .into_iter()
                    .map(|item| self.render(item))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Record(fields, _) => format!(
                "{{ {} }}",
                fields
                    .into_iter()
                    .map(|(name, (presence, typ))| format!(
                        "{}{}: {}",
                        name,
                        if presence == FieldPresence::Optional {
                            "?"
                        } else {
                            ""
                        },
                        self.render(typ)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Partial(reference, slots) => format!(
                "{}[{}]",
                reference.name,
                slots
                    .into_iter()
                    .map(|slot| match slot {
                        TySlot::Hole(_) => "_".to_owned(),
                        TySlot::Fixed(typ) => self.render(typ),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Projection(trait_, args, assoc) => format!(
                "{}[{}]::{}",
                trait_.0.name,
                args.into_iter()
                    .map(|arg| self.render(arg))
                    .collect::<Vec<_>>()
                    .join(", "),
                assoc.name
            ),
            Ty::ErrorRow => "[:_ | e]".to_owned(),
            Ty::Any => "_".to_owned(),
        }
    }
}

fn block_contains_return(block: &Located<Block<'_>>) -> bool {
    block
        .value
        .statements
        .iter()
        .any(|statement| match &statement.value {
            Stmt::Return(_) => true,
            Stmt::For { body, .. } | Stmt::While { body, .. } => block_contains_return(body),
            Stmt::Let(_)
            | Stmt::Use { .. }
            | Stmt::Assign { .. }
            | Stmt::Break(_)
            | Stmt::Continue
            | Stmt::Assert(_)
            | Stmt::Expr(_) => false,
        })
}

fn is_value_item(item: &ItemKind<'_>) -> bool {
    matches!(
        item,
        ItemKind::Fn(_)
            | ItemKind::Let(_)
            | ItemKind::Component(_)
            | ItemKind::Extern(alder_ast::ExternDecl::Fn { .. })
    )
}

fn generated_type_name_rank(name: &str) -> usize {
    match name.as_bytes() {
        [letter @ b'a'..=b'z'] => usize::from(*letter - b'a'),
        _ => name
            .strip_prefix('t')
            .and_then(|index| index.parse().ok())
            .unwrap_or(usize::MAX),
    }
}
