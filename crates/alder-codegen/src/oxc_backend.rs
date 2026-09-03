//! Direct canonical-AST to Oxc-AST lowering.

use std::{cell::RefCell, collections::BTreeSet};

use alder_ast::{Expr, ItemKind, Module, ModuleId, Pattern, RecordField, ValueRef, Visibility};
use alder_region::Located;
use alder_solve::{DirectTarget, Evidence, Intrinsic, SolveOutput, UseAction};
use oxc_allocator::Vec as ArenaVec;
use oxc_ast::ast::{Expression, ObjectPropertyKind, Program, Statement, VariableDeclarationKind};
use oxc_span::SourceType;
use oxc_syntax::operator::AssignmentOperator;
use oxc_syntax::operator::{BinaryOperator, UnaryOperator};
use rolldown_ecmascript::EcmaAst;

use super::{
    EmitOptions, Error, Import, binding_name, constructor_export, constructor_name,
    constructor_name_from_parts, escaped, module_specifier, qualified_key, top_name,
};
use crate::js_ast::JsAst;

pub(crate) struct AstModule {
    pub(crate) module_id: String,
    pub(crate) ast: EcmaAst,
    pub(crate) dependencies: Vec<String>,
}

struct Value<'js> {
    prefix: ArenaVec<'js, Statement<'js>>,
    expr: Expression<'js>,
}

struct Metadata {
    module_id: String,
    dependencies: Vec<String>,
}

fn impl_origin_index(origin: alder_ast::ImplOrigin) -> u32 {
    match origin {
        alder_ast::ImplOrigin::Source { item_ordinal }
        | alder_ast::ImplOrigin::Derived {
            type_ordinal: item_ordinal,
            ..
        }
        | alder_ast::ImplOrigin::AutomaticEq {
            type_ordinal: item_ordinal,
        } => item_ordinal,
        alder_ast::ImplOrigin::Builtin { index } => u32::from(index),
    }
}

fn trait_operator_method(operator: alder_ast::BinOp) -> Option<&'static str> {
    match operator {
        alder_ast::BinOp::Add => Some("add"),
        alder_ast::BinOp::Sub => Some("sub"),
        alder_ast::BinOp::Mul => Some("mul"),
        alder_ast::BinOp::Div => Some("div"),
        alder_ast::BinOp::Rem => Some("rem"),
        alder_ast::BinOp::Lt => Some("lt"),
        alder_ast::BinOp::LtEq => Some("lte"),
        alder_ast::BinOp::Gt => Some("gt"),
        alder_ast::BinOp::GtEq => Some("gte"),
        _ => None,
    }
}

struct Emitter<'src, 'js> {
    js: JsAst<'js>,
    home: ModuleId<'src>,
    next_temp: u32,
    imports: BTreeSet<Import>,
    kernel: BTreeSet<&'static str>,
    loop_results: Vec<Option<String>>,
    solved: Option<&'src SolveOutput<'src>>,
}

#[derive(Clone)]
enum PatternStep {
    Field(String),
    Index(usize),
}

pub(crate) fn emit_module_ast(
    module: &Module<'_>,
    solved: Option<&SolveOutput<'_>>,
    options: EmitOptions,
) -> Result<AstModule, Error> {
    let output = RefCell::new(None);
    let mut ast = EcmaAst {
        source_type: SourceType::mjs(),
        ..EcmaAst::default()
    };
    ast.program.with_mut(|fields| {
        let emitter = Emitter {
            js: JsAst::new(fields.allocator),
            home: module.id,
            next_temp: 0,
            imports: BTreeSet::new(),
            kernel: BTreeSet::new(),
            loop_results: Vec::new(),
            solved,
        };
        match emitter.module(module, options) {
            Ok((program, metadata)) => {
                *fields.program = program;
                *output.borrow_mut() = Some(Ok(metadata));
            }
            Err(error) => *output.borrow_mut() = Some(Err(error)),
        }
    });
    let metadata = output
        .into_inner()
        .expect("Oxc AST lowering must set its result")?;
    Ok(AstModule {
        module_id: metadata.module_id,
        ast,
        dependencies: metadata.dependencies,
    })
}

impl<'src, 'js> Emitter<'src, 'js> {
    fn module(
        mut self,
        module: &Module<'src>,
        options: EmitOptions,
    ) -> Result<(Program<'js>, Metadata), Error> {
        let mut body = self.js.vec();
        let mut declarations = self.js.vec();
        let mut exports = Vec::new();

        for item in module.items {
            let public = matches!(item.value.visibility, Visibility::Public(_));
            match &item.value.kind {
                ItemKind::Enum(enum_) => {
                    for variant in enum_.variants {
                        declarations.push(self.constructor(*variant));
                        if public {
                            exports.push((
                                constructor_name(variant.name),
                                constructor_export(variant.name),
                            ));
                        }
                    }
                }
                ItemKind::Fn(function) => {
                    declarations.push(self.named_function(
                        function.name,
                        function.params,
                        function.body,
                    )?);
                    if public {
                        exports.push((top_name(function.name), function.name.name.to_owned()));
                    }
                }
                ItemKind::Component(component) => {
                    declarations.push(self.named_function(
                        component.name,
                        component.params,
                        component.body,
                    )?);
                    if public {
                        exports.push((top_name(component.name), component.name.name.to_owned()));
                    }
                }
                ItemKind::Let(decl) => {
                    declarations.extend(self.top_let(decl)?);
                    if public {
                        exports.extend(
                            decl.bindings
                                .iter()
                                .map(|name| (top_name(*name), name.name.to_owned())),
                        );
                    }
                }
                ItemKind::Extern(alder_ast::ExternDecl::Fn {
                    module,
                    symbol,
                    name,
                    params,
                    ret,
                    ..
                }) => {
                    declarations.push(self.extern_function(module, symbol, *name, params, ret));
                    if public {
                        exports.push((top_name(*name), name.name.to_owned()));
                    }
                }
                ItemKind::Trait(trait_) => {
                    for trait_item in trait_.items {
                        if let alder_ast::TraitItem::Fn(method) = trait_item
                            && let Some(default) = method.body
                        {
                            let symbol =
                                format!("$default${}${}", trait_.id.0.name, method.id.name);
                            let mut leading = vec!["$self".to_owned()];
                            leading.extend(
                                (0..method.scheme.trait_predicates.len())
                                    .map(|index| format!("$dict{index}")),
                            );
                            declarations.push(self.lowered_function(
                                &symbol,
                                leading,
                                method.params,
                                default,
                            )?);
                            exports.push((symbol.clone(), symbol));
                        }
                    }
                }
                ItemKind::Impl(implementation) => {
                    let symbol = format!(
                        "$dict${}${}",
                        implementation.trait_ref.trait_.0.name,
                        impl_origin_index(implementation.id.origin)
                    );
                    declarations.extend(self.implementation(module, implementation, &symbol)?);
                    exports.push((symbol.clone(), symbol));
                }
                ItemKind::Test(test) if options.mode == super::EmitMode::Test => {
                    declarations.push(self.test(module.id, test)?);
                }
                ItemKind::Tests(items) if options.mode == super::EmitMode::Test => {
                    for nested in *items {
                        if let ItemKind::Test(test) = &nested.value.kind {
                            declarations.push(self.test(module.id, test)?);
                        }
                    }
                }
                ItemKind::TypeAlias(_)
                | ItemKind::ErrorGroup(_)
                | ItemKind::Table(_)
                | ItemKind::Schema(_)
                | ItemKind::Macro(_)
                | ItemKind::Comptime(_)
                | ItemKind::Extern(alder_ast::ExternDecl::Type { .. })
                | ItemKind::Test(_)
                | ItemKind::Tests(_) => {}
            }
        }

        if !self.kernel.is_empty() {
            let names = self
                .kernel
                .iter()
                .map(|name| ((*name).to_owned(), (*name).to_owned()))
                .collect::<Vec<_>>();
            body.push(self.js.import("alder:kernel", &names));
        }
        for import in &self.imports {
            let (module, exported, local) = match import {
                Import::Value {
                    module,
                    exported,
                    local,
                }
                | Import::Extern {
                    module,
                    exported,
                    local,
                } => (module, exported, local),
            };
            body.push(self.js.import(module, &[(exported.clone(), local.clone())]));
        }
        body.extend(declarations);
        if !exports.is_empty() {
            exports.sort();
            body.push(self.js.export(&exports));
        }

        let dependencies = self
            .imports
            .iter()
            .filter_map(|import| match import {
                Import::Value { module, .. } => Some(module.clone()),
                Import::Extern { .. } => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok((
            self.js.program(body),
            Metadata {
                module_id: module_specifier(module.id),
                dependencies,
            },
        ))
    }

    fn constructor(&self, variant: alder_ast::Variant<'src>) -> Statement<'js> {
        let name = constructor_name(variant.name);
        match variant.payload {
            alder_ast::VariantPayload::Unit => {
                let mut properties = self.js.vec();
                properties.push(self.js.property("$", self.js.string(variant.name.variant)));
                let frozen = self.js.call(
                    self.js.member(self.js.identifier("Object"), "freeze"),
                    [self.js.object(properties)],
                );
                self.js
                    .variable(VariableDeclarationKind::Const, &name, Some(frozen))
            }
            alder_ast::VariantPayload::Tuple(types) => {
                let args = (0..types.len())
                    .map(|index| format!("$a{index}"))
                    .collect::<Vec<_>>();
                let mut properties = self.js.vec();
                properties.push(self.js.property("$", self.js.string(variant.name.variant)));
                for (index, argument) in args.iter().enumerate() {
                    properties.push(
                        self.js
                            .property(&format!("_{index}"), self.js.identifier(argument)),
                    );
                }
                let mut body = self.js.vec();
                body.push(self.js.return_statement(self.js.object(properties)));
                self.js.function(&name, &args, body, false)
            }
            alder_ast::VariantPayload::Record(fields) => {
                let args = fields
                    .iter()
                    .map(|field| escaped(field.name))
                    .collect::<Vec<_>>();
                let mut properties = self.js.vec();
                properties.push(self.js.property("$", self.js.string(variant.name.variant)));
                for (field, argument) in fields.iter().zip(&args) {
                    properties.push(self.js.property(field.name, self.js.identifier(argument)));
                }
                let mut body = self.js.vec();
                body.push(self.js.return_statement(self.js.object(properties)));
                self.js.function(&name, &args, body, false)
            }
        }
    }

    fn extern_function(
        &mut self,
        module: &str,
        symbol: &str,
        name: alder_ast::QualifiedName<'src>,
        params: &[alder_ast::Param<'src>],
        ret: &Located<alder_ast::Type<'src>>,
    ) -> Statement<'js> {
        let imported = self.extern_import(module, symbol);
        let args = (0..params.len())
            .map(|index| format!("$a{index}"))
            .collect::<Vec<_>>();
        let call = self.js.call(
            self.js.identifier(&imported),
            args.iter().map(|argument| self.js.identifier(argument)),
        );
        let value = if super::type_named(ret, "Result") {
            self.kernel.insert("$tryCatch");
            let mut callback = self.js.vec();
            callback.push(self.js.return_statement(call));
            let callback = self.js.arrow(&[], callback, false);
            self.js.call(self.js.identifier("$tryCatch"), [callback])
        } else {
            call
        };
        let mut body = self.js.vec();
        body.push(self.js.return_statement(value));
        self.js.function(&top_name(name), &args, body, false)
    }

    fn test(
        &mut self,
        module: ModuleId<'src>,
        test: &alder_ast::TestDecl<'src>,
    ) -> Result<Statement<'js>, Error> {
        self.kernel.insert("$registerTest");
        let body = self.block_return(test.body)?;
        let callback = self.js.arrow(&[], body, true);
        let call = self.js.call(
            self.js.identifier("$registerTest"),
            [
                self.js.string(&module_specifier(module)),
                self.js.string(test.name.value),
                callback,
            ],
        );
        Ok(self.js.expression_statement(call))
    }

    fn named_function(
        &mut self,
        name: alder_ast::QualifiedName<'src>,
        params: &[alder_ast::Param<'src>],
        body: &Located<alder_ast::Block<'src>>,
    ) -> Result<Statement<'js>, Error> {
        let dictionary_count = self
            .solved
            .and_then(|solved| solved.bindings.get(&name))
            .map_or(0, |binding| binding.dictionary_params.len());
        let leading = (0..dictionary_count)
            .map(|index| format!("$dict{index}"))
            .collect::<Vec<_>>();
        self.lowered_function(&top_name(name), leading, params, body)
    }

    fn lowered_function(
        &mut self,
        name: &str,
        leading: Vec<String>,
        params: &[alder_ast::Param<'src>],
        body: &Located<alder_ast::Block<'src>>,
    ) -> Result<Statement<'js>, Error> {
        let source_args = (0..params.len())
            .map(|index| format!("$a{index}"))
            .collect::<Vec<_>>();
        let args = leading
            .into_iter()
            .chain(source_args.iter().cloned())
            .collect::<Vec<_>>();
        let mut statements = self.js.vec();
        for (param, arg) in params.iter().zip(&source_args) {
            self.bind_pattern(param.pattern, arg, &[], &mut statements);
        }
        statements.extend(self.block_return(body)?);
        Ok(self
            .js
            .function(name, &args, statements, super::contains_await_block(body)))
    }

    fn implementation(
        &mut self,
        module: &Module<'src>,
        implementation: &alder_ast::ImplDecl<'src>,
        dictionary_symbol: &str,
    ) -> Result<ArenaVec<'js, Statement<'js>>, Error> {
        if implementation.synthetic.is_some() {
            return Ok(self.derived_dictionary(implementation, dictionary_symbol));
        }
        let mut declarations = self.js.vec();
        let prerequisite_args = (0..implementation.trait_predicates.len())
            .map(|index| format!("$dict{index}"))
            .collect::<Vec<_>>();
        let trait_declaration = module.items.iter().find_map(|item| match &item.value.kind {
            ItemKind::Trait(trait_) if trait_.id == implementation.trait_ref.trait_ => {
                Some(*trait_)
            }
            _ => None,
        });
        let mut methods = Vec::new();
        if let Some(trait_) = trait_declaration {
            for trait_item in trait_.items {
                let alder_ast::TraitItem::Fn(trait_method) = trait_item else {
                    continue;
                };
                let provided = implementation.items.iter().find_map(|item| match item {
                    alder_ast::ImplItem::Fn(method) if method.method == trait_method.id => {
                        Some(*method)
                    }
                    _ => None,
                });
                if let Some(method) = provided {
                    let helper = format!(
                        "$impl${}${}",
                        impl_origin_index(implementation.id.origin),
                        method.method.name
                    );
                    let mut leading = vec!["$self".to_owned()];
                    leading.extend(prerequisite_args.iter().cloned());
                    leading.extend(
                        (0..method.scheme.trait_predicates.len())
                            .map(|index| format!("$dict{}", prerequisite_args.len() + index)),
                    );
                    declarations.push(self.lowered_function(
                        &helper,
                        leading,
                        method.params,
                        method.body,
                    )?);
                    methods.push((
                        method.method,
                        method.params.len(),
                        method.scheme.trait_predicates.len(),
                        helper,
                        true,
                    ));
                } else if trait_method.body.is_some() {
                    methods.push((
                        trait_method.id,
                        trait_method.params.len(),
                        trait_method.scheme.trait_predicates.len(),
                        format!("$default${}${}", trait_.id.0.name, trait_method.id.name),
                        false,
                    ));
                }
            }
        } else {
            for item in implementation.items {
                let alder_ast::ImplItem::Fn(method) = item else {
                    continue;
                };
                let helper = format!(
                    "$impl${}${}",
                    impl_origin_index(implementation.id.origin),
                    method.method.name
                );
                let mut leading = vec!["$self".to_owned()];
                leading.extend(prerequisite_args.iter().cloned());
                leading.extend(
                    (0..method.scheme.trait_predicates.len())
                        .map(|index| format!("$dict{}", prerequisite_args.len() + index)),
                );
                declarations.push(self.lowered_function(
                    &helper,
                    leading,
                    method.params,
                    method.body,
                )?);
                methods.push((
                    method.method,
                    method.params.len(),
                    method.scheme.trait_predicates.len(),
                    helper,
                    true,
                ));
            }
        }

        let self_name = if prerequisite_args.is_empty() {
            dictionary_symbol
        } else {
            "$self"
        };
        let mut dictionary_body = self.js.vec();
        dictionary_body.push(self.js.variable(
            VariableDeclarationKind::Const,
            self_name,
            Some(self.js.object(self.js.vec())),
        ));
        for (method, parameter_count, method_dictionary_count, helper, provided) in methods {
            let method_dictionary_args = (0..method_dictionary_count)
                .map(|index| format!("$methodDict{index}"))
                .collect::<Vec<_>>();
            let source_args = (0..parameter_count)
                .map(|index| format!("$a{index}"))
                .collect::<Vec<_>>();
            let arrow_args = method_dictionary_args
                .iter()
                .chain(&source_args)
                .cloned()
                .collect::<Vec<_>>();
            let mut helper_args = self.js.vec();
            helper_args.push(self.js.identifier(self_name));
            if provided {
                helper_args.extend(
                    prerequisite_args
                        .iter()
                        .map(|argument| self.js.identifier(argument)),
                );
            }
            helper_args.extend(
                method_dictionary_args
                    .iter()
                    .map(|argument| self.js.identifier(argument)),
            );
            helper_args.extend(
                source_args
                    .iter()
                    .map(|argument| self.js.identifier(argument)),
            );
            let call = self.js.call(self.js.identifier(&helper), helper_args);
            let mut arrow_body = self.js.vec();
            arrow_body.push(self.js.return_statement(call));
            let arrow = self.js.arrow(&arrow_args, arrow_body, false);
            let target = self.js.member(self.js.identifier(self_name), method.name);
            let assignment = self
                .js
                .assignment(target, AssignmentOperator::Assign, arrow);
            dictionary_body.push(self.js.expression_statement(assignment));
        }
        let frozen = self.js.call(
            self.js.member(self.js.identifier("Object"), "freeze"),
            [self.js.identifier(self_name)],
        );
        if prerequisite_args.is_empty() {
            dictionary_body.push(self.js.expression_statement(frozen));
            declarations.extend(dictionary_body);
        } else {
            dictionary_body.push(self.js.return_statement(frozen));
            declarations.push(self.js.function(
                dictionary_symbol,
                &prerequisite_args,
                dictionary_body,
                false,
            ));
        }
        Ok(declarations)
    }

    fn derived_dictionary(
        &mut self,
        implementation: &alder_ast::ImplDecl<'src>,
        dictionary_symbol: &str,
    ) -> ArenaVec<'js, Statement<'js>> {
        let kind = implementation
            .synthetic
            .expect("derived dictionaries have a derive kind");
        let prerequisite_args = (0..implementation.trait_predicates.len())
            .map(|index| format!("$dict{index}"))
            .collect::<Vec<_>>();
        let self_name = if prerequisite_args.is_empty() {
            dictionary_symbol
        } else {
            "$self"
        };
        let mut body = self.js.vec();
        body.push(self.js.variable(
            VariableDeclarationKind::Const,
            self_name,
            Some(self.js.object(self.js.vec())),
        ));
        match kind {
            alder_ast::DeriveKind::Eq => {
                self.derived_kernel_method(&mut body, self_name, "eq", "$equal", 2, None);
            }
            alder_ast::DeriveKind::Show => {
                self.derived_kernel_method(&mut body, self_name, "show", "$show", 1, None);
            }
            alder_ast::DeriveKind::Ord => {
                let equality = format!("$dict$Eq${}", impl_origin_index(implementation.id.origin));
                let super_target = self.js.member(self.js.identifier(self_name), "$super0");
                let equality = if prerequisite_args.is_empty() {
                    self.js.identifier(&equality)
                } else {
                    self.js.call(
                        self.js.identifier(&equality),
                        prerequisite_args.iter().map(|argument| {
                            self.js.member(self.js.identifier(argument), "$super0")
                        }),
                    )
                };
                let super_assignment =
                    self.js
                        .assignment(super_target, AssignmentOperator::Assign, equality);
                body.push(self.js.expression_statement(super_assignment));
                self.derived_kernel_method(
                    &mut body,
                    self_name,
                    "lt",
                    "$compare",
                    2,
                    Some(BinaryOperator::LessThan),
                );
                self.derived_kernel_method(
                    &mut body,
                    self_name,
                    "lte",
                    "$compare",
                    2,
                    Some(BinaryOperator::LessEqualThan),
                );
                self.derived_kernel_method(
                    &mut body,
                    self_name,
                    "gt",
                    "$compare",
                    2,
                    Some(BinaryOperator::GreaterThan),
                );
                self.derived_kernel_method(
                    &mut body,
                    self_name,
                    "gte",
                    "$compare",
                    2,
                    Some(BinaryOperator::GreaterEqualThan),
                );
            }
            alder_ast::DeriveKind::Hash => {
                self.derived_kernel_method(&mut body, self_name, "hash", "$hash", 1, None);
            }
            alder_ast::DeriveKind::Json => {
                self.derived_kernel_method(&mut body, self_name, "encode", "$jsonEncode", 1, None);
                self.derived_kernel_method(&mut body, self_name, "decode", "$jsonDecode", 1, None);
            }
        }
        let frozen = self.js.call(
            self.js.member(self.js.identifier("Object"), "freeze"),
            [self.js.identifier(self_name)],
        );
        let mut declarations = self.js.vec();
        if prerequisite_args.is_empty() {
            body.push(self.js.expression_statement(frozen));
            declarations.extend(body);
        } else {
            body.push(self.js.return_statement(frozen));
            declarations.push(
                self.js
                    .function(dictionary_symbol, &prerequisite_args, body, false),
            );
        }
        declarations
    }

    fn derived_kernel_method(
        &mut self,
        body: &mut ArenaVec<'js, Statement<'js>>,
        dictionary: &str,
        method: &str,
        kernel: &'static str,
        arity: usize,
        comparison: Option<BinaryOperator>,
    ) {
        self.kernel.insert(kernel);
        let args = (0..arity)
            .map(|index| format!("$a{index}"))
            .collect::<Vec<_>>();
        let call = self.js.call(
            self.js.identifier(kernel),
            args.iter().map(|argument| self.js.identifier(argument)),
        );
        let result = match comparison {
            Some(operator) => self.js.binary(call, operator, self.js.number(0.0)),
            None => call,
        };
        let mut method_body = self.js.vec();
        method_body.push(self.js.return_statement(result));
        let target = self.js.member(self.js.identifier(dictionary), method);
        let assignment = self.js.assignment(
            target,
            AssignmentOperator::Assign,
            self.js.arrow(&args, method_body, false),
        );
        body.push(self.js.expression_statement(assignment));
    }

    fn top_let(
        &mut self,
        decl: &alder_ast::TopLevelLet<'src>,
    ) -> Result<ArenaVec<'js, Statement<'js>>, Error> {
        let value = self.expr(decl.value)?;
        let mut statements = value.prefix;
        let temp = self.temp();
        statements.push(
            self.js
                .variable(VariableDeclarationKind::Const, &temp, Some(value.expr)),
        );
        self.bind_pattern(decl.pattern, &temp, &[], &mut statements);
        Ok(statements)
    }

    fn expr(&mut self, node: &Located<Expr<'src>>) -> Result<Value<'js>, Error> {
        let value = match &node.value {
            Expr::Number { text, .. } => self.pure(self.js.number_source(text)),
            Expr::BigInt(text) => self.pure(self.js.bigint_source(text)),
            Expr::Str(text) => self.pure(self.js.string(text)),
            Expr::Bool(value) => self.pure(self.js.boolean(*value)),
            Expr::Unit => self.pure(self.js.undefined()),
            Expr::Template(parts) => {
                let mut prefix = self.js.vec();
                let mut values = self.js.vec();
                for part in *parts {
                    match part {
                        alder_ast::TemplatePart::Text(text) => values.push(self.js.string(text)),
                        alder_ast::TemplatePart::Expr(expression) => {
                            let value = self.expr(expression)?;
                            prefix.extend(value.prefix);
                            values.push(self.js.call(self.js.identifier("String"), [value.expr]));
                        }
                    }
                }
                let join = self.js.member(self.js.array(values), "join");
                Value {
                    prefix,
                    expr: self.js.call(join, [self.js.string("")]),
                }
            }
            Expr::TaggedTemplate { tag, parts } => {
                let tag = self.expr(tag)?;
                let mut prefix = tag.prefix;
                let tag = self.materialize(tag.expr, &mut prefix);
                let mut strings = self.js.vec();
                let mut arguments = self.js.vec();
                for part in *parts {
                    match part {
                        alder_ast::TemplatePart::Text(text) => strings.push(self.js.string(text)),
                        alder_ast::TemplatePart::Expr(expression) => {
                            let value = self.expr(expression)?;
                            prefix.extend(value.prefix);
                            arguments.push(value.expr);
                        }
                    }
                }
                arguments.insert(0, self.js.array(strings));
                Value {
                    prefix,
                    expr: self.js.call(tag, arguments),
                }
            }
            Expr::Var { reference, .. } => {
                let expression = self.reference(*reference);
                self.pure(expression)
            }
            Expr::Constructor(constructor) => {
                let expression = self.constructor_reference(*constructor);
                self.pure(expression)
            }
            Expr::Tag { name, args, .. } => {
                let (prefix, args) = self.values(args)?;
                let mut properties = self.js.vec();
                properties.push(
                    self.js
                        .property("$", self.js.string(&format!(":{}", name.value))),
                );
                for (index, argument) in args.into_iter().enumerate() {
                    properties.push(self.js.property(&format!("_{index}"), argument));
                }
                Value {
                    prefix,
                    expr: self.js.object(properties),
                }
            }
            Expr::Array(items) | Expr::Tuple(items) => {
                let (prefix, values) = self.values(items)?;
                Value {
                    prefix,
                    expr: self.js.array(values),
                }
            }
            Expr::Record(fields) => self.record(fields, None)?,
            Expr::RecordConstructor {
                constructor,
                fields,
            } => self.record(fields, Some(*constructor))?,
            Expr::Call {
                use_id,
                function,
                arguments,
            } => {
                let action = self
                    .solved
                    .and_then(|solved| solved.uses.get(use_id))
                    .cloned();
                let direct = match &action {
                    Some(UseAction::DirectCall {
                        dictionaries,
                        target: Some(DirectTarget::TraitMethod(method)),
                        ..
                    }) if !dictionaries.is_empty() => {
                        let dictionary = self.evidence(&dictionaries[0]);
                        Some((
                            self.js.vec(),
                            self.js.member(dictionary, method.name),
                            dictionaries[1..].to_vec(),
                        ))
                    }
                    Some(UseAction::DirectCall { dictionaries, .. }) => {
                        let function = self.expr(function)?;
                        Some((function.prefix, function.expr, dictionaries.clone()))
                    }
                    _ => None,
                };
                let (mut prefix, function, dictionaries) = match direct {
                    Some(direct) => direct,
                    None => {
                        let function = self.expr(function)?;
                        (function.prefix, function.expr, Vec::new())
                    }
                };
                let function = self.materialize(function, &mut prefix);
                let (other_prefix, arguments) = self.values(arguments)?;
                prefix.extend(other_prefix);
                let mut call_arguments = self.js.vec();
                call_arguments.extend(
                    dictionaries
                        .iter()
                        .map(|dictionary| self.evidence(dictionary)),
                );
                call_arguments.extend(arguments);
                Value {
                    prefix,
                    expr: self.js.call(function, call_arguments),
                }
            }
            Expr::Access { record, field } => {
                let record = self.expr(record)?;
                Value {
                    prefix: record.prefix,
                    expr: self.js.member(record.expr, field.value),
                }
            }
            Expr::TupleAccess { tuple, index } => {
                let tuple = self.expr(tuple)?;
                Value {
                    prefix: tuple.prefix,
                    expr: self
                        .js
                        .index(tuple.expr, self.js.number(f64::from(index.value))),
                }
            }
            Expr::Index { target, index } => {
                let target = self.expr(target)?;
                let mut prefix = target.prefix;
                let target = self.materialize(target.expr, &mut prefix);
                let index = self.expr(index)?;
                prefix.extend(index.prefix);
                Value {
                    prefix,
                    expr: self.js.index(target, index.expr),
                }
            }
            Expr::Await(expression) => {
                let value = self.expr(expression)?;
                Value {
                    prefix: value.prefix,
                    expr: self.js.builder.expression_await(oxc_span::SPAN, value.expr),
                }
            }
            Expr::Try(expression) => {
                let value = self.expr(expression)?;
                let mut prefix = value.prefix;
                let temp = self.temp();
                prefix.push(self.js.variable(
                    VariableDeclarationKind::Const,
                    &temp,
                    Some(value.expr),
                ));
                let is_error = self.js.binary(
                    self.js.member(self.js.identifier(&temp), "$"),
                    BinaryOperator::StrictEquality,
                    self.js.string("Err"),
                );
                let mut consequent = self.js.vec();
                consequent.push(self.js.return_statement(self.js.identifier(&temp)));
                prefix.push(self.js.if_statement(is_error, consequent, None));
                Value {
                    prefix,
                    expr: self.js.member(self.js.identifier(&temp), "_0"),
                }
            }
            Expr::Pin(expression) | Expr::State(expression) => self.expr(expression)?,
            Expr::Negate {
                use_id,
                expr: expression,
            } => self.negate(*use_id, expression)?,
            Expr::Not(expression) => self.unary(UnaryOperator::LogicalNot, expression)?,
            Expr::Binop {
                use_id,
                op,
                left,
                right,
            } => self.binop(*use_id, op.value, left, right)?,
            Expr::Block(block) => self.block_value(block)?,
            Expr::Lambda { params, body, .. } => self.lambda(params, body)?,
            Expr::If {
                branches,
                final_else,
            } => self.if_value(branches, *final_else)?,
            Expr::Match { scrutinee, arms } => self.match_value(scrutinee, arms, node.region)?,
            Expr::Loop(block) => self.loop_value(block)?,
            Expr::Provide {
                provider,
                value,
                body,
            } => self.provide(*provider, value, body)?,
            Expr::Style(style) => {
                self.kernel.insert("$style");
                let style = self.style(style)?;
                Value {
                    prefix: style.prefix,
                    expr: self.js.call(self.js.identifier("$style"), [style.expr]),
                }
            }
            Expr::Query(_) => {
                self.kernel.insert("$query");
                self.pure(self.js.call(self.js.identifier("$query"), []))
            }
            Expr::Markup(markup) => self.markup(markup)?,
            Expr::MacroCall { .. } => {
                return Err(Error {
                    region: node.region,
                    message: "macros are not available until M5",
                });
            }
        };
        Ok(value)
    }

    fn values(
        &mut self,
        nodes: &[&Located<Expr<'src>>],
    ) -> Result<
        (
            ArenaVec<'js, Statement<'js>>,
            ArenaVec<'js, Expression<'js>>,
        ),
        Error,
    > {
        let mut prefix = self.js.vec();
        let mut values = self.js.builder.vec_with_capacity(nodes.len());
        for node in nodes {
            let value = self.expr(node)?;
            prefix.extend(value.prefix);
            values.push(value.expr);
        }
        Ok((prefix, values))
    }

    fn record(
        &mut self,
        fields: &[RecordField<'src>],
        constructor: Option<alder_ast::ConstructorRef<'src>>,
    ) -> Result<Value<'js>, Error> {
        let mut prefix = self.js.vec();
        let mut properties: ArenaVec<'js, ObjectPropertyKind<'js>> = self.js.vec();
        if let Some(constructor) = constructor {
            properties.push(
                self.js
                    .property("$", self.js.string(constructor.name.variant)),
            );
        }
        for field in fields {
            match field {
                RecordField::Field { name, value } => {
                    let value = self.expr(value)?;
                    prefix.extend(value.prefix);
                    properties.push(self.js.property(name.value, value.expr));
                }
                RecordField::Spread(value) => {
                    let value = self.expr(value)?;
                    prefix.extend(value.prefix);
                    properties.push(self.js.spread_property(value.expr));
                }
            }
        }
        Ok(Value {
            prefix,
            expr: self.js.object(properties),
        })
    }

    fn unary(
        &mut self,
        operator: UnaryOperator,
        expression: &Located<Expr<'src>>,
    ) -> Result<Value<'js>, Error> {
        let value = self.expr(expression)?;
        Ok(Value {
            prefix: value.prefix,
            expr: self.js.unary(operator, value.expr),
        })
    }

    fn negate(
        &mut self,
        use_id: alder_ast::UseId,
        expression: &Located<Expr<'src>>,
    ) -> Result<Value<'js>, Error> {
        let value = self.expr(expression)?;
        let evidence = self
            .solved
            .and_then(|solved| solved.uses.get(&use_id))
            .and_then(|action| match action {
                UseAction::Operator { dictionary } => Some(dictionary.clone()),
                _ => None,
            });
        let expr = match evidence {
            Some(Evidence::Intrinsic(_)) | None => {
                self.js.unary(UnaryOperator::UnaryNegation, value.expr)
            }
            Some(evidence) => {
                let dictionary = self.evidence(&evidence);
                self.js
                    .call(self.js.member(dictionary, "negate"), [value.expr])
            }
        };
        Ok(Value {
            prefix: value.prefix,
            expr,
        })
    }

    fn binop(
        &mut self,
        use_id: alder_ast::UseId,
        op: alder_ast::BinOp,
        left: &Located<Expr<'src>>,
        right: &Located<Expr<'src>>,
    ) -> Result<Value<'js>, Error> {
        let left = self.expr(left)?;
        let mut prefix = left.prefix;
        let left = self.materialize(left.expr, &mut prefix);
        if matches!(
            op,
            alder_ast::BinOp::And | alder_ast::BinOp::Or | alder_ast::BinOp::Coalesce
        ) {
            let result = self.temp();
            prefix.push(
                self.js
                    .variable(VariableDeclarationKind::Let, &result, Some(left)),
            );
            let condition = match op {
                alder_ast::BinOp::And => self.js.identifier(&result),
                alder_ast::BinOp::Or => self
                    .js
                    .unary(UnaryOperator::LogicalNot, self.js.identifier(&result)),
                alder_ast::BinOp::Coalesce => self.js.binary(
                    self.js.identifier(&result),
                    BinaryOperator::Equality,
                    self.js.builder.expression_null_literal(oxc_span::SPAN),
                ),
                _ => unreachable!(),
            };
            let right = self.expr(right)?;
            let mut consequent = right.prefix;
            let assignment = self.js.assign_identifier(&result, right.expr);
            consequent.push(self.js.expression_statement(assignment));
            prefix.push(self.js.if_statement(condition, consequent, None));
            return Ok(Value {
                prefix,
                expr: self.js.identifier(&result),
            });
        }
        let right = self.expr(right)?;
        prefix.extend(right.prefix);
        let right = right.expr;
        let evidence = self
            .solved
            .and_then(|solved| solved.uses.get(&use_id))
            .and_then(|action| match action {
                UseAction::Operator { dictionary } => Some(dictionary.clone()),
                _ => None,
            });
        let intrinsic = matches!(evidence, Some(Evidence::Intrinsic(_)));
        let expr = match op {
            alder_ast::BinOp::Pipe => self.js.call(right, [left]),
            alder_ast::BinOp::Eq | alder_ast::BinOp::NotEq if evidence.is_some() && !intrinsic => {
                let dictionary = self.evidence(evidence.as_ref().expect("guarded"));
                let equal = self
                    .js
                    .call(self.js.member(dictionary, "eq"), [left, right]);
                if op == alder_ast::BinOp::NotEq {
                    self.js.unary(UnaryOperator::LogicalNot, equal)
                } else {
                    equal
                }
            }
            alder_ast::BinOp::Eq | alder_ast::BinOp::NotEq if intrinsic => {
                let equal = self.js.binary(left, BinaryOperator::StrictEquality, right);
                if op == alder_ast::BinOp::NotEq {
                    self.js.unary(UnaryOperator::LogicalNot, equal)
                } else {
                    equal
                }
            }
            alder_ast::BinOp::Eq | alder_ast::BinOp::NotEq => {
                self.kernel.insert("$equal");
                let equal = self.js.call(self.js.identifier("$equal"), [left, right]);
                if op == alder_ast::BinOp::NotEq {
                    self.js.unary(UnaryOperator::LogicalNot, equal)
                } else {
                    equal
                }
            }
            op if evidence.is_some() && !intrinsic && trait_operator_method(op).is_some() => {
                let dictionary = self.evidence(evidence.as_ref().expect("guarded"));
                self.js.call(
                    self.js.member(
                        dictionary,
                        trait_operator_method(op).expect("guarded by match"),
                    ),
                    [left, right],
                )
            }
            alder_ast::BinOp::Add => self.js.binary(left, BinaryOperator::Addition, right),
            alder_ast::BinOp::Sub => self.js.binary(left, BinaryOperator::Subtraction, right),
            alder_ast::BinOp::Mul => self.js.binary(left, BinaryOperator::Multiplication, right),
            alder_ast::BinOp::Div => self.js.binary(left, BinaryOperator::Division, right),
            alder_ast::BinOp::Rem => self.js.binary(left, BinaryOperator::Remainder, right),
            alder_ast::BinOp::Lt => self.js.binary(left, BinaryOperator::LessThan, right),
            alder_ast::BinOp::LtEq => self.js.binary(left, BinaryOperator::LessEqualThan, right),
            alder_ast::BinOp::Gt => self.js.binary(left, BinaryOperator::GreaterThan, right),
            alder_ast::BinOp::GtEq => self
                .js
                .binary(left, BinaryOperator::GreaterEqualThan, right),
            alder_ast::BinOp::In => self.js.binary(left, BinaryOperator::In, right),
            alder_ast::BinOp::And | alder_ast::BinOp::Or | alder_ast::BinOp::Coalesce => {
                unreachable!()
            }
        };
        Ok(Value { prefix, expr })
    }

    fn block_value(
        &mut self,
        block: &Located<alder_ast::Block<'src>>,
    ) -> Result<Value<'js>, Error> {
        let result = self.temp();
        let mut prefix = self.js.vec();
        prefix.push(
            self.js
                .variable(VariableDeclarationKind::Let, &result, None),
        );
        prefix.extend(self.block_assign(block, &result)?);
        Ok(Value {
            prefix,
            expr: self.js.identifier(&result),
        })
    }

    fn lambda(
        &mut self,
        params: &[alder_ast::Param<'src>],
        body: &Located<Expr<'src>>,
    ) -> Result<Value<'js>, Error> {
        let args = (0..params.len())
            .map(|index| format!("$a{index}"))
            .collect::<Vec<_>>();
        let mut statements = self.js.vec();
        for (param, arg) in params.iter().zip(&args) {
            self.bind_pattern(param.pattern, arg, &[], &mut statements);
        }
        let value = self.expr(body)?;
        statements.extend(value.prefix);
        statements.push(self.js.return_statement(value.expr));
        Ok(self.pure(
            self.js
                .arrow(&args, statements, super::contains_await_expr(body)),
        ))
    }

    fn if_value(
        &mut self,
        branches: &[alder_ast::IfBranch<'src>],
        final_else: Option<&Located<alder_ast::Block<'src>>>,
    ) -> Result<Value<'js>, Error> {
        let result = self.temp();
        let done = self.temp();
        let mut prefix = self.js.vec();
        prefix.push(
            self.js
                .variable(VariableDeclarationKind::Let, &result, None),
        );
        prefix.push(self.js.variable(
            VariableDeclarationKind::Let,
            &done,
            Some(self.js.boolean(false)),
        ));
        for branch in branches {
            let condition = self.expr(branch.condition)?;
            let mut evaluate_branch = condition.prefix;
            let mut consequent = self.block_assign(branch.body, &result)?;
            consequent.push(
                self.js
                    .expression_statement(self.js.assign_identifier(&done, self.js.boolean(true))),
            );
            evaluate_branch.push(self.js.if_statement(condition.expr, consequent, None));
            let not_done = self
                .js
                .unary(UnaryOperator::LogicalNot, self.js.identifier(&done));
            prefix.push(self.js.if_statement(not_done, evaluate_branch, None));
        }
        let not_done = self
            .js
            .unary(UnaryOperator::LogicalNot, self.js.identifier(&done));
        let final_body =
            match final_else {
                Some(block) => self.block_assign(block, &result)?,
                None => {
                    let mut body = self.js.vec();
                    body.push(self.js.expression_statement(
                        self.js.assign_identifier(&result, self.js.undefined()),
                    ));
                    body
                }
            };
        prefix.push(self.js.if_statement(not_done, final_body, None));
        Ok(Value {
            prefix,
            expr: self.js.identifier(&result),
        })
    }

    fn provide(
        &mut self,
        provider: alder_ast::QualifiedName<'src>,
        value: &Located<Expr<'src>>,
        body: &Located<alder_ast::Block<'src>>,
    ) -> Result<Value<'js>, Error> {
        self.kernel.extend(["$providerPush", "$providerPop"]);
        let value = self.expr(value)?;
        let result = self.temp();
        let key = qualified_key(provider);
        let mut prefix = value.prefix;
        prefix.push(
            self.js
                .variable(VariableDeclarationKind::Let, &result, None),
        );
        prefix.push(self.js.expression_statement(self.js.call(
            self.js.identifier("$providerPush"),
            [self.js.string(&key), value.expr],
        )));
        let body = self.block_assign(body, &result)?;
        let mut finalizer = self.js.vec();
        finalizer.push(
            self.js.expression_statement(
                self.js
                    .call(self.js.identifier("$providerPop"), [self.js.string(&key)]),
            ),
        );
        prefix.push(self.js.try_finally(body, finalizer));
        Ok(Value {
            prefix,
            expr: self.js.identifier(&result),
        })
    }

    fn match_value(
        &mut self,
        scrutinee: &Located<Expr<'src>>,
        arms: &[alder_ast::MatchArm<'src>],
        region: alder_region::Region,
    ) -> Result<Value<'js>, Error> {
        self.kernel.insert("$matchFailure");
        let scrutinee = self.expr(scrutinee)?;
        let value = self.temp();
        let result = self.temp();
        let label = format!("$match{}", self.next_temp);
        self.next_temp += 1;
        let mut prefix = scrutinee.prefix;
        prefix.push(
            self.js
                .variable(VariableDeclarationKind::Const, &value, Some(scrutinee.expr)),
        );
        prefix.push(
            self.js
                .variable(VariableDeclarationKind::Let, &result, None),
        );
        let mut match_body = self.js.vec();
        for arm in arms {
            if arm.patterns.is_empty() {
                match_body
                    .extend(self.match_branch(None, arm.guard, arm.body, &value, &result, &label)?);
            } else {
                for pattern in arm.patterns {
                    match_body.extend(self.match_branch(
                        Some(pattern),
                        arm.guard,
                        arm.body,
                        &value,
                        &result,
                        &label,
                    )?);
                }
            }
        }
        let failure = self.js.call(
            self.js.identifier("$matchFailure"),
            [
                self.js.string(&module_specifier(self.home)),
                self.js
                    .string(&format!("{}:{}", region.start.line, region.start.column)),
                self.js.identifier(&value),
            ],
        );
        match_body.push(self.js.expression_statement(failure));
        prefix.push(self.js.labeled(&label, match_body));
        Ok(Value {
            prefix,
            expr: self.js.identifier(&result),
        })
    }

    fn match_branch(
        &mut self,
        pattern: Option<&Located<Pattern<'src>>>,
        guard: Option<&Located<Expr<'src>>>,
        arm_body: &Located<Expr<'src>>,
        value: &str,
        result: &str,
        label: &str,
    ) -> Result<ArenaVec<'js, Statement<'js>>, Error> {
        let test = match pattern {
            Some(pattern) => self.pattern_test(pattern, value, &[]),
            None => Ok(self.pure(self.js.boolean(true))),
        };
        let test = test?;
        let mut branch = test.prefix;
        let mut body = self.js.vec();
        if let Some(pattern) = pattern {
            self.bind_pattern(pattern, value, &[], &mut body);
        }
        if let Some(guard) = guard {
            let guard = self.expr(guard)?;
            body.extend(guard.prefix);
            let mut matched = self.js.vec();
            let arm_body = self.expr(arm_body)?;
            matched.extend(arm_body.prefix);
            matched.push(
                self.js
                    .expression_statement(self.js.assign_identifier(result, arm_body.expr)),
            );
            matched.push(self.js.break_statement(Some(label)));
            body.push(self.js.if_statement(guard.expr, matched, None));
        } else {
            let arm_body = self.expr(arm_body)?;
            body.extend(arm_body.prefix);
            body.push(
                self.js
                    .expression_statement(self.js.assign_identifier(result, arm_body.expr)),
            );
            body.push(self.js.break_statement(Some(label)));
        }
        branch.push(self.js.if_statement(test.expr, body, None));
        Ok(branch)
    }

    fn loop_value(&mut self, block: &Located<alder_ast::Block<'src>>) -> Result<Value<'js>, Error> {
        let result = self.temp();
        self.loop_results.push(Some(result.clone()));
        let body = self.block_statements(block)?;
        self.loop_results.pop();
        let mut prefix = self.js.vec();
        prefix.push(
            self.js
                .variable(VariableDeclarationKind::Let, &result, None),
        );
        prefix.push(self.js.while_statement(self.js.boolean(true), body));
        Ok(Value {
            prefix,
            expr: self.js.identifier(&result),
        })
    }

    fn style(&mut self, style: &alder_ast::Style<'src>) -> Result<Value<'js>, Error> {
        let mut prefix = self.js.vec();
        let mut properties = self.js.vec();
        for entry in style.entries {
            let key = match entry.key.value {
                alder_ast::StyleKey::Ident(key) | alder_ast::StyleKey::Str(key) => key,
            };
            let value = match entry.value {
                alder_ast::StyleValue::Dimension { text, unit, .. } => {
                    self.js.string(&format!("{text}{unit}"))
                }
                alder_ast::StyleValue::Expr(expression) => {
                    let value = self.expr(expression)?;
                    prefix.extend(value.prefix);
                    value.expr
                }
                alder_ast::StyleValue::Nested(style) => {
                    let value = self.style(style)?;
                    prefix.extend(value.prefix);
                    value.expr
                }
            };
            properties.push(self.js.property(key, value));
        }
        Ok(Value {
            prefix,
            expr: self.js.object(properties),
        })
    }

    fn markup(&mut self, markup: &alder_ast::Markup<'src>) -> Result<Value<'js>, Error> {
        self.kernel.insert("$html");
        match markup {
            alder_ast::Markup::Element(element) => self.element(element),
            alder_ast::Markup::Fragment(children) => {
                let mut prefix = self.js.vec();
                let mut values = self.js.vec();
                for child in *children {
                    let value = self.child(child)?;
                    prefix.extend(value.prefix);
                    values.push(value.expr);
                }
                Ok(Value {
                    prefix,
                    expr: self.js.call(
                        self.js.identifier("$html"),
                        [
                            self.js.builder.expression_null_literal(oxc_span::SPAN),
                            self.js.builder.expression_null_literal(oxc_span::SPAN),
                            self.js.array(values),
                        ],
                    ),
                })
            }
        }
    }

    fn element(&mut self, element: &alder_ast::Element<'src>) -> Result<Value<'js>, Error> {
        let name = match element.name.value {
            alder_ast::ElementName::Tag(name) => self.js.string(name),
            alder_ast::ElementName::Component(name) => self.reference(ValueRef::TopLevel(name)),
        };
        let mut prefix = self.js.vec();
        let mut attributes = self.js.vec();
        for attribute in element.attrs {
            let value = match attribute.value {
                None => self.pure(self.js.boolean(true)),
                Some(alder_ast::AttrValue::Str(value)) => self.pure(self.js.string(value.value)),
                Some(alder_ast::AttrValue::Expr(expression)) => self.expr(expression)?,
            };
            prefix.extend(value.prefix);
            attributes.push(self.js.property(attribute.name.value, value.expr));
        }
        let mut children = self.js.vec();
        for child in element.children {
            let value = self.child(child)?;
            prefix.extend(value.prefix);
            children.push(value.expr);
        }
        Ok(Value {
            prefix,
            expr: self.js.call(
                self.js.identifier("$html"),
                [name, self.js.object(attributes), self.js.array(children)],
            ),
        })
    }

    fn child(&mut self, child: &Located<alder_ast::Child<'src>>) -> Result<Value<'js>, Error> {
        match &child.value {
            alder_ast::Child::Element(element) => self.element(element),
            alder_ast::Child::Fragment(children) => {
                let mut prefix = self.js.vec();
                let mut values = self.js.vec();
                for child in *children {
                    let value = self.child(child)?;
                    prefix.extend(value.prefix);
                    values.push(value.expr);
                }
                Ok(Value {
                    prefix,
                    expr: self.js.array(values),
                })
            }
            alder_ast::Child::Text(text) => Ok(self.pure(self.js.string(text))),
            alder_ast::Child::Hole(expression) => self.expr(expression),
            alder_ast::Child::If { .. }
            | alder_ast::Child::For { .. }
            | alder_ast::Child::Match { .. } => Ok(self.pure(self.js.undefined())),
        }
    }

    fn block_return(
        &mut self,
        block: &Located<alder_ast::Block<'src>>,
    ) -> Result<ArenaVec<'js, Statement<'js>>, Error> {
        let mut statements = self.block_statements(block)?;
        let value = match block.value.tail {
            Some(tail) => self.expr(tail)?,
            None => self.pure(self.js.undefined()),
        };
        statements.extend(value.prefix);
        statements.push(self.js.return_statement(value.expr));
        Ok(statements)
    }

    fn block_assign(
        &mut self,
        block: &Located<alder_ast::Block<'src>>,
        target: &str,
    ) -> Result<ArenaVec<'js, Statement<'js>>, Error> {
        let mut statements = self.block_statements(block)?;
        let value = match block.value.tail {
            Some(tail) => self.expr(tail)?,
            None => self.pure(self.js.undefined()),
        };
        statements.extend(value.prefix);
        statements.push(
            self.js
                .expression_statement(self.js.assign_identifier(target, value.expr)),
        );
        Ok(statements)
    }

    fn block_statements(
        &mut self,
        block: &Located<alder_ast::Block<'src>>,
    ) -> Result<ArenaVec<'js, Statement<'js>>, Error> {
        let mut statements = self.js.vec();
        for statement in block.value.statements {
            statements.extend(self.statement(statement)?);
        }
        Ok(statements)
    }

    fn statement(
        &mut self,
        statement: &Located<alder_ast::Stmt<'src>>,
    ) -> Result<ArenaVec<'js, Statement<'js>>, Error> {
        let mut statements = self.js.vec();
        match &statement.value {
            alder_ast::Stmt::Let(decl) => {
                let value = self.expr(decl.value)?;
                statements.extend(value.prefix);
                let temp = self.temp();
                statements.push(self.js.variable(
                    VariableDeclarationKind::Const,
                    &temp,
                    Some(value.expr),
                ));
                self.bind_pattern(decl.pattern, &temp, &[], &mut statements);
            }
            alder_ast::Stmt::Use { provider } => {
                self.kernel.insert("$providerGet");
                let call = self.js.call(
                    self.js.identifier("$providerGet"),
                    [self.js.string(&qualified_key(*provider))],
                );
                statements.push(self.js.expression_statement(call));
            }
            alder_ast::Stmt::Assign {
                use_id,
                place,
                op,
                value,
            } => {
                let value = self.expr(value)?;
                statements.extend(value.prefix);
                let evidence = use_id.and_then(|use_id| {
                    self.solved
                        .and_then(|solved| solved.uses.get(&use_id))
                        .and_then(|action| match action {
                            UseAction::CompoundAssign { dictionary } => Some(dictionary.clone()),
                            _ => None,
                        })
                });
                let assignment = if op.value == alder_ast::AssignOp::Set
                    || matches!(evidence, Some(Evidence::Intrinsic(_)) | None)
                {
                    let target = self.place(place)?;
                    let operator = match op.value {
                        alder_ast::AssignOp::Set => AssignmentOperator::Assign,
                        alder_ast::AssignOp::Add => AssignmentOperator::Addition,
                        alder_ast::AssignOp::Sub => AssignmentOperator::Subtraction,
                        alder_ast::AssignOp::Mul => AssignmentOperator::Multiplication,
                        alder_ast::AssignOp::Div => AssignmentOperator::Division,
                    };
                    self.js.assignment(target, operator, value.expr)
                } else {
                    let (place_prefix, read, write) = self.place_pair(place)?;
                    statements.extend(place_prefix);
                    let dictionary =
                        self.evidence(&evidence.expect("non-intrinsic evidence exists"));
                    let method = match op.value {
                        alder_ast::AssignOp::Add => "add",
                        alder_ast::AssignOp::Sub => "sub",
                        alder_ast::AssignOp::Mul => "mul",
                        alder_ast::AssignOp::Div => "div",
                        alder_ast::AssignOp::Set => unreachable!("handled above"),
                    };
                    let result = self
                        .js
                        .call(self.js.member(dictionary, method), [read, value.expr]);
                    self.js
                        .assignment(write, AssignmentOperator::Assign, result)
                };
                statements.push(self.js.expression_statement(assignment));
            }
            alder_ast::Stmt::For {
                pattern,
                iter,
                body,
            } => {
                let iter = self.expr(iter)?;
                statements.extend(iter.prefix);
                let item = self.temp();
                let mut loop_body = self.js.vec();
                self.bind_pattern(pattern, &item, &[], &mut loop_body);
                loop_body.extend(self.block_statements(body)?);
                statements.push(self.js.for_of(&item, iter.expr, loop_body));
            }
            alder_ast::Stmt::While { condition, body } => {
                let condition = self.expr(condition)?;
                let loop_body = self.block_statements(body)?;
                if condition.prefix.is_empty() {
                    statements.push(self.js.while_statement(condition.expr, loop_body));
                } else {
                    let mut repeated = condition.prefix;
                    let not_condition = self.js.unary(UnaryOperator::LogicalNot, condition.expr);
                    let mut break_body = self.js.vec();
                    break_body.push(self.js.break_statement(None));
                    repeated.push(self.js.if_statement(not_condition, break_body, None));
                    repeated.extend(loop_body);
                    statements.push(self.js.while_statement(self.js.boolean(true), repeated));
                }
            }
            alder_ast::Stmt::Return(value) => {
                let value = match value {
                    Some(value) => self.expr(value)?,
                    None => self.pure(self.js.undefined()),
                };
                statements.extend(value.prefix);
                statements.push(self.js.return_statement(value.expr));
            }
            alder_ast::Stmt::Break(value) => {
                if let Some(result) = self.loop_results.last().cloned().flatten() {
                    let value = match value {
                        Some(value) => self.expr(value)?,
                        None => self.pure(self.js.undefined()),
                    };
                    statements.extend(value.prefix);
                    statements.push(
                        self.js
                            .expression_statement(self.js.assign_identifier(&result, value.expr)),
                    );
                }
                statements.push(self.js.break_statement(None));
            }
            alder_ast::Stmt::Continue => statements.push(self.js.continue_statement()),
            alder_ast::Stmt::Assert(expression) => {
                self.kernel.insert("$assert");
                let value = self.expr(expression)?;
                statements.extend(value.prefix);
                let call = self.js.call(self.js.identifier("$assert"), [value.expr]);
                statements.push(self.js.expression_statement(call));
            }
            alder_ast::Stmt::Expr(expression) => {
                let value = self.expr(expression)?;
                statements.extend(value.prefix);
                statements.push(self.js.expression_statement(value.expr));
            }
        }
        Ok(statements)
    }

    fn place(&mut self, place: &alder_ast::Place<'src>) -> Result<Expression<'js>, Error> {
        let mut target = self.js.identifier(&binding_name(place.root));
        for step in place.steps {
            target = match step {
                alder_ast::PlaceStep::Field(field) => self.js.member(target, field.value),
                alder_ast::PlaceStep::TupleIndex(index) => self
                    .js
                    .index(target, self.js.number(f64::from(index.value))),
                alder_ast::PlaceStep::Index(index) => {
                    let value = self.expr(index)?;
                    if !value.prefix.is_empty() {
                        return Err(Error {
                            region: index.region,
                            message: "lifted assignment index is not implemented",
                        });
                    }
                    self.js.index(target, value.expr)
                }
            };
        }
        Ok(target)
    }

    fn place_pair(
        &mut self,
        place: &alder_ast::Place<'src>,
    ) -> Result<
        (
            ArenaVec<'js, Statement<'js>>,
            Expression<'js>,
            Expression<'js>,
        ),
        Error,
    > {
        let mut prefix = self.js.vec();
        let mut read = self.js.identifier(&binding_name(place.root));
        let mut write = self.js.identifier(&binding_name(place.root));
        for step in place.steps {
            match step {
                alder_ast::PlaceStep::Field(field) => {
                    read = self.js.member(read, field.value);
                    write = self.js.member(write, field.value);
                }
                alder_ast::PlaceStep::TupleIndex(index) => {
                    read = self.js.index(read, self.js.number(f64::from(index.value)));
                    write = self.js.index(write, self.js.number(f64::from(index.value)));
                }
                alder_ast::PlaceStep::Index(index) => {
                    let value = self.expr(index)?;
                    prefix.extend(value.prefix);
                    let index_name = self.temp();
                    prefix.push(self.js.variable(
                        VariableDeclarationKind::Const,
                        &index_name,
                        Some(value.expr),
                    ));
                    read = self.js.index(read, self.js.identifier(&index_name));
                    write = self.js.index(write, self.js.identifier(&index_name));
                }
            }
        }
        Ok((prefix, read, write))
    }

    fn bind_pattern(
        &self,
        pattern: &Located<Pattern<'src>>,
        root: &str,
        steps: &[PatternStep],
        statements: &mut ArenaVec<'js, Statement<'js>>,
    ) {
        match &pattern.value {
            Pattern::Bind(name) => statements.push(self.js.variable(
                VariableDeclarationKind::Let,
                &binding_name(*name),
                Some(self.pattern_place(root, steps)),
            )),
            Pattern::Alias { pattern, name } => {
                statements.push(self.js.variable(
                    VariableDeclarationKind::Let,
                    &binding_name(*name),
                    Some(self.pattern_place(root, steps)),
                ));
                self.bind_pattern(pattern, root, steps, statements);
            }
            Pattern::Tuple(items)
            | Pattern::Constructor { args: items, .. }
            | Pattern::Tag { args: items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    let mut nested = steps.to_vec();
                    match pattern.value {
                        Pattern::Tuple(_) => nested.push(PatternStep::Index(index)),
                        _ => nested.push(PatternStep::Field(format!("_{index}"))),
                    }
                    self.bind_pattern(item, root, &nested, statements);
                }
            }
            Pattern::Array { elements, rest } => {
                for (index, item) in elements.iter().enumerate() {
                    let mut nested = steps.to_vec();
                    nested.push(PatternStep::Index(index));
                    self.bind_pattern(item, root, &nested, statements);
                }
                if let Some(name) = rest.and_then(|rest| rest.name) {
                    let slice = self.js.member(self.pattern_place(root, steps), "slice");
                    let value = self.js.call(slice, [self.js.number(elements.len() as f64)]);
                    statements.push(self.js.variable(
                        VariableDeclarationKind::Let,
                        &binding_name(name),
                        Some(value),
                    ));
                }
            }
            Pattern::ConstructorRecord { fields, .. } | Pattern::Record { fields, .. } => {
                for field in *fields {
                    let mut nested = steps.to_vec();
                    nested.push(PatternStep::Field(field.name.value.to_owned()));
                    self.bind_pattern(field.pattern, root, &nested, statements);
                }
            }
            Pattern::Anything
            | Pattern::Pin { .. }
            | Pattern::Number { .. }
            | Pattern::BigInt(_)
            | Pattern::Str(_)
            | Pattern::Bool(_)
            | Pattern::Unit => {}
        }
    }

    fn pattern_place(&self, root: &str, steps: &[PatternStep]) -> Expression<'js> {
        let mut value = self.js.identifier(root);
        for step in steps {
            value = match step {
                PatternStep::Field(field) => self.js.member(value, field),
                PatternStep::Index(index) => self.js.index(value, self.js.number(*index as f64)),
            };
        }
        value
    }

    fn pattern_test(
        &mut self,
        pattern: &Located<Pattern<'src>>,
        root: &str,
        steps: &[PatternStep],
    ) -> Result<Value<'js>, Error> {
        let value = match &pattern.value {
            Pattern::Anything | Pattern::Bind(_) => self.pure(self.js.boolean(true)),
            Pattern::Alias { pattern, .. } => self.pattern_test(pattern, root, steps)?,
            Pattern::Pin {
                use_id,
                value: expression,
            } => {
                let mut pin = self.expr(expression)?;
                let value = self.materialize(pin.expr, &mut pin.prefix);
                let evidence = self
                    .solved
                    .and_then(|solved| solved.uses.get(use_id))
                    .and_then(|action| match action {
                        UseAction::Pin { dictionary } => Some(dictionary.clone()),
                        _ => None,
                    });
                let left = self.pattern_place(root, steps);
                let comparison = match evidence {
                    Some(Evidence::Intrinsic(_)) => {
                        self.js.binary(left, BinaryOperator::StrictEquality, value)
                    }
                    Some(evidence) => {
                        let dictionary = self.evidence(&evidence);
                        self.js
                            .call(self.js.member(dictionary, "eq"), [left, value])
                    }
                    None => {
                        self.kernel.insert("$equal");
                        self.js.call(self.js.identifier("$equal"), [left, value])
                    }
                };
                Value {
                    prefix: pin.prefix,
                    expr: comparison,
                }
            }
            Pattern::Number { text, .. } => self.pure(self.js.binary(
                self.pattern_place(root, steps),
                BinaryOperator::StrictEquality,
                self.js.number_source(text),
            )),
            Pattern::BigInt(text) => self.pure(self.js.binary(
                self.pattern_place(root, steps),
                BinaryOperator::StrictEquality,
                self.js.bigint_source(text),
            )),
            Pattern::Str(text) => self.pure(self.js.binary(
                self.pattern_place(root, steps),
                BinaryOperator::StrictEquality,
                self.js.string(text),
            )),
            Pattern::Bool(expected) => self.pure(self.js.binary(
                self.pattern_place(root, steps),
                BinaryOperator::StrictEquality,
                self.js.boolean(*expected),
            )),
            Pattern::Unit => self.pure(self.js.binary(
                self.pattern_place(root, steps),
                BinaryOperator::StrictEquality,
                self.js.undefined(),
            )),
            Pattern::Constructor { constructor, args } => {
                let mut prefix = self.js.vec();
                let mut tests = Vec::new();
                tests.push(self.not_null(self.pattern_place(root, steps)));
                tests.push(self.js.binary(
                    self.js.member(self.pattern_place(root, steps), "$"),
                    BinaryOperator::StrictEquality,
                    self.js.string(constructor.name.variant),
                ));
                for (index, pattern) in args.iter().enumerate() {
                    let mut nested = steps.to_vec();
                    nested.push(PatternStep::Field(format!("_{index}")));
                    let test = self.pattern_test(pattern, root, &nested)?;
                    prefix.extend(test.prefix);
                    tests.push(test.expr);
                }
                Value {
                    prefix,
                    expr: self.and_all(tests),
                }
            }
            Pattern::ConstructorRecord {
                constructor,
                fields,
                ..
            } => {
                let mut prefix = self.js.vec();
                let mut tests = vec![self.not_null(self.pattern_place(root, steps))];
                tests.push(self.js.binary(
                    self.js.member(self.pattern_place(root, steps), "$"),
                    BinaryOperator::StrictEquality,
                    self.js.string(constructor.name.variant),
                ));
                for field in *fields {
                    let mut nested = steps.to_vec();
                    nested.push(PatternStep::Field(field.name.value.to_owned()));
                    let test = self.pattern_test(field.pattern, root, &nested)?;
                    prefix.extend(test.prefix);
                    tests.push(test.expr);
                }
                Value {
                    prefix,
                    expr: self.and_all(tests),
                }
            }
            Pattern::Tag { name, args, .. } => {
                let mut prefix = self.js.vec();
                let mut tests = vec![self.not_null(self.pattern_place(root, steps))];
                tests.push(self.js.binary(
                    self.js.member(self.pattern_place(root, steps), "$"),
                    BinaryOperator::StrictEquality,
                    self.js.string(&format!(":{}", name.value)),
                ));
                for (index, pattern) in args.iter().enumerate() {
                    let mut nested = steps.to_vec();
                    nested.push(PatternStep::Field(format!("_{index}")));
                    let test = self.pattern_test(pattern, root, &nested)?;
                    prefix.extend(test.prefix);
                    tests.push(test.expr);
                }
                Value {
                    prefix,
                    expr: self.and_all(tests),
                }
            }
            Pattern::Tuple(items) => {
                let mut prefix = self.js.vec();
                let mut tests = vec![self.is_array(self.pattern_place(root, steps))];
                tests.push(self.js.binary(
                    self.js.member(self.pattern_place(root, steps), "length"),
                    BinaryOperator::StrictEquality,
                    self.js.number(items.len() as f64),
                ));
                for (index, pattern) in items.iter().enumerate() {
                    let mut nested = steps.to_vec();
                    nested.push(PatternStep::Index(index));
                    let test = self.pattern_test(pattern, root, &nested)?;
                    prefix.extend(test.prefix);
                    tests.push(test.expr);
                }
                Value {
                    prefix,
                    expr: self.and_all(tests),
                }
            }
            Pattern::Record { fields, .. } => {
                let mut prefix = self.js.vec();
                let mut tests = vec![self.not_null(self.pattern_place(root, steps))];
                tests.push(
                    self.js.binary(
                        self.js
                            .unary(UnaryOperator::Typeof, self.pattern_place(root, steps)),
                        BinaryOperator::StrictEquality,
                        self.js.string("object"),
                    ),
                );
                for field in *fields {
                    let mut nested = steps.to_vec();
                    nested.push(PatternStep::Field(field.name.value.to_owned()));
                    let test = self.pattern_test(field.pattern, root, &nested)?;
                    prefix.extend(test.prefix);
                    tests.push(test.expr);
                }
                Value {
                    prefix,
                    expr: self.and_all(tests),
                }
            }
            Pattern::Array { elements, rest } => {
                let mut prefix = self.js.vec();
                let mut tests = vec![self.is_array(self.pattern_place(root, steps))];
                tests.push(self.js.binary(
                    self.js.member(self.pattern_place(root, steps), "length"),
                    if rest.is_some() {
                        BinaryOperator::GreaterEqualThan
                    } else {
                        BinaryOperator::StrictEquality
                    },
                    self.js.number(elements.len() as f64),
                ));
                for (index, pattern) in elements.iter().enumerate() {
                    let mut nested = steps.to_vec();
                    nested.push(PatternStep::Index(index));
                    let test = self.pattern_test(pattern, root, &nested)?;
                    prefix.extend(test.prefix);
                    tests.push(test.expr);
                }
                Value {
                    prefix,
                    expr: self.and_all(tests),
                }
            }
        };
        Ok(value)
    }

    fn not_null(&self, value: Expression<'js>) -> Expression<'js> {
        self.js.binary(
            value,
            BinaryOperator::Inequality,
            self.js.builder.expression_null_literal(oxc_span::SPAN),
        )
    }

    fn is_array(&self, value: Expression<'js>) -> Expression<'js> {
        self.js.call(
            self.js.member(self.js.identifier("Array"), "isArray"),
            [value],
        )
    }

    fn and_all(&self, tests: Vec<Expression<'js>>) -> Expression<'js> {
        tests
            .into_iter()
            .reduce(|left, right| {
                self.js
                    .logical(left, oxc_syntax::operator::LogicalOperator::And, right)
            })
            .unwrap_or_else(|| self.js.boolean(true))
    }

    fn reference(&mut self, reference: ValueRef<'src>) -> Expression<'js> {
        match reference {
            ValueRef::Local(local) => self.js.identifier(&super::local_name(local)),
            ValueRef::TopLevel(name) if name.module == self.home => {
                self.js.identifier(&top_name(name))
            }
            ValueRef::TopLevel(name) => {
                let local = self.value_import(name, top_name(name));
                self.js.identifier(&local)
            }
            ValueRef::Foreign { reference, .. } | ValueRef::Builtin(reference) => {
                let local = self.value_import(reference, reference.name.to_owned());
                self.js.identifier(&local)
            }
            ValueRef::TraitMethod { method, .. } => {
                let reference = alder_ast::QualifiedName {
                    module: method.trait_.0.module,
                    name: method.name,
                };
                if reference.module == self.home {
                    self.js.identifier(&top_name(reference))
                } else {
                    let local = self.value_import(reference, reference.name.to_owned());
                    self.js.identifier(&local)
                }
            }
            ValueRef::Provider(provider) => {
                self.kernel.insert("$providerGet");
                self.js.call(
                    self.js.identifier("$providerGet"),
                    [self.js.string(&qualified_key(provider))],
                )
            }
            ValueRef::QueryName(name) => self.js.string(name),
            ValueRef::Opaque(_) | ValueRef::Module(_) => self.js.undefined(),
        }
    }

    fn evidence(&mut self, evidence: &Evidence<'src>) -> Expression<'js> {
        match evidence {
            Evidence::Param(index) => self.js.identifier(&format!("$dict{index}")),
            Evidence::ParamSuper { param, slot } => self.js.member(
                self.js.identifier(&format!("$dict{param}")),
                &format!("$super{slot}"),
            ),
            Evidence::SelfDictionary => self.js.identifier("$self"),
            Evidence::Super(index) => self
                .js
                .member(self.js.identifier("$self"), &format!("$super{index}")),
            Evidence::Impl {
                module,
                symbol,
                kind,
                arguments,
                ..
            } => {
                let dictionary = if *module == self.home {
                    self.js.identifier(symbol)
                } else {
                    let reference = alder_ast::QualifiedName {
                        module: *module,
                        name: symbol,
                    };
                    let local = self.value_import(reference, (*symbol).to_owned());
                    self.js.identifier(&local)
                };
                if *kind == alder_ast::DictionaryKind::Factory {
                    let arguments = arguments
                        .iter()
                        .map(|argument| self.evidence(argument))
                        .collect::<Vec<_>>();
                    self.js.call(dictionary, arguments)
                } else {
                    dictionary
                }
            }
            Evidence::Intrinsic(intrinsic) => self.intrinsic_dictionary(*intrinsic),
            Evidence::StructuralEq(_) => {
                self.kernel.insert("$equal");
                let args = vec!["$a".to_owned(), "$b".to_owned()];
                let call = self.js.call(
                    self.js.identifier("$equal"),
                    args.iter().map(|arg| self.js.identifier(arg)),
                );
                let mut body = self.js.vec();
                body.push(self.js.return_statement(call));
                let mut properties = self.js.vec();
                properties.push(self.js.property("eq", self.js.arrow(&args, body, false)));
                self.js.object(properties)
            }
        }
    }

    fn intrinsic_dictionary(&self, intrinsic: Intrinsic) -> Expression<'js> {
        let mut properties = self.js.vec();
        match intrinsic {
            Intrinsic::EqNumber
            | Intrinsic::EqString
            | Intrinsic::EqBool
            | Intrinsic::EqBigInt
            | Intrinsic::EqUnit => properties
                .push(self.intrinsic_binary_property("eq", BinaryOperator::StrictEquality)),
            Intrinsic::OrdNumber | Intrinsic::OrdString | Intrinsic::OrdBigInt => {
                properties.push(self.intrinsic_binary_property("lt", BinaryOperator::LessThan));
                properties
                    .push(self.intrinsic_binary_property("lte", BinaryOperator::LessEqualThan));
                properties.push(self.intrinsic_binary_property("gt", BinaryOperator::GreaterThan));
                properties
                    .push(self.intrinsic_binary_property("gte", BinaryOperator::GreaterEqualThan));
                let equality = match intrinsic {
                    Intrinsic::OrdNumber => Intrinsic::EqNumber,
                    Intrinsic::OrdString => Intrinsic::EqString,
                    Intrinsic::OrdBigInt => Intrinsic::EqBigInt,
                    _ => unreachable!(),
                };
                properties.push(
                    self.js
                        .property("$super0", self.intrinsic_dictionary(equality)),
                );
            }
            Intrinsic::NumNumber | Intrinsic::NumBigInt => {
                properties.push(self.intrinsic_binary_property("add", BinaryOperator::Addition));
                properties.push(self.intrinsic_binary_property("sub", BinaryOperator::Subtraction));
                properties
                    .push(self.intrinsic_binary_property("mul", BinaryOperator::Multiplication));
                properties.push(self.intrinsic_binary_property("div", BinaryOperator::Division));
                properties.push(self.intrinsic_binary_property("rem", BinaryOperator::Remainder));
                let args = vec!["$a".to_owned()];
                let expression = self
                    .js
                    .unary(UnaryOperator::UnaryNegation, self.js.identifier(&args[0]));
                let mut body = self.js.vec();
                body.push(self.js.return_statement(expression));
                properties.push(
                    self.js
                        .property("negate", self.js.arrow(&args, body, false)),
                );
            }
        }
        self.js.object(properties)
    }

    fn intrinsic_binary_property(
        &self,
        name: &str,
        operator: BinaryOperator,
    ) -> ObjectPropertyKind<'js> {
        let args = vec!["$a".to_owned(), "$b".to_owned()];
        let expression = self.js.binary(
            self.js.identifier(&args[0]),
            operator,
            self.js.identifier(&args[1]),
        );
        let mut body = self.js.vec();
        body.push(self.js.return_statement(expression));
        self.js.property(name, self.js.arrow(&args, body, false))
    }

    fn constructor_reference(
        &mut self,
        constructor: alder_ast::ConstructorRef<'src>,
    ) -> Expression<'js> {
        let exported =
            constructor_name_from_parts(constructor.name.enum_.name, constructor.name.variant);
        if constructor.name.enum_.module == self.home {
            self.js.identifier(&exported)
        } else {
            let local =
                self.value_import(constructor.name.enum_, constructor_export(constructor.name));
            self.js.identifier(&local)
        }
    }

    fn value_import(&mut self, name: alder_ast::QualifiedName<'src>, exported: String) -> String {
        let module = module_specifier(name.module);
        let local = format!("$i_{}_{}", escaped(&module), escaped(&exported));
        self.imports.insert(Import::Value {
            module,
            exported,
            local: local.clone(),
        });
        local
    }

    fn extern_import(&mut self, module: &str, symbol: &str) -> String {
        let local = format!("$x_{}_{}", escaped(module), escaped(symbol));
        self.imports.insert(Import::Extern {
            module: module.to_owned(),
            exported: symbol.to_owned(),
            local: local.clone(),
        });
        local
    }

    fn materialize(
        &mut self,
        expression: Expression<'js>,
        prefix: &mut ArenaVec<'js, Statement<'js>>,
    ) -> Expression<'js> {
        let temp = self.temp();
        prefix.push(
            self.js
                .variable(VariableDeclarationKind::Const, &temp, Some(expression)),
        );
        self.js.identifier(&temp)
    }

    fn temp(&mut self) -> String {
        let temp = format!("$t{}", self.next_temp);
        self.next_temp += 1;
        temp
    }

    fn pure(&self, expr: Expression<'js>) -> Value<'js> {
        Value {
            prefix: self.js.vec(),
            expr,
        }
    }
}

#[cfg(test)]
mod tests {
    use alder_ast::{ModuleId, PackageId, ResolvedImport};
    use bumpalo::Bump;
    use rolldown_ecmascript::{EcmaCompiler, PrintOptions};

    use super::*;

    fn emit(source_text: &str) -> AstModule {
        let bump = Bump::new();
        let source = bump.alloc_str(source_text);
        let parsed = alder_parse::parse_module(&bump, source).unwrap();
        let canonical = alder_can::canonicalize(
            &bump,
            alder_can::Context {
                home: ModuleId {
                    package: PackageId::Application,
                    path: &["main"],
                },
                imports: &[] as &[ResolvedImport<'_>],
                interfaces: &[],
            },
            &parsed,
        )
        .unwrap();
        emit_module_ast(canonical.module, None, EmitOptions::default()).unwrap()
    }

    #[test]
    fn lowers_a_canonical_module_to_an_owned_rolldown_ast() {
        let generated = emit("pub fn main() { 40 + 2 }");

        assert_eq!(generated.module_id, "alder://app/main.mjs");
        assert_eq!(
            EcmaCompiler::print_with(&generated.ast, PrintOptions::default()).code,
            "function $v_main() {\n\tconst $t0 = 40;\n\treturn $t0 + 2;\n}\nexport { $v_main as main };\n"
        );
    }

    #[test]
    fn lowers_the_existing_codegen_surface_without_source_fragments() {
        for source in [
            "pub fn answer() { let x = 40\n x + 2 }",
            "pub enum Shape { Point, Circle(Number), Rect { width: Number, height: Number } }",
            "enum Maybe[a] { Nothing, Just(a) }\npub fn unwrap(value) { match value { Maybe::Just(x) => x, Maybe::Nothing => 0 } }",
            "pub fn choose(flag, fallback) { if flag && fallback() { 1 } else { 2 } }",
            "pub fn first(name) { let value = { name: name, scores: [10, 20] }\n value.scores[0] }",
            "pub fn sum() { let mut total = 0\n for value in [1, 2] { total += value }\n total }",
            "#[extern(\"library\", \"parse\")]\npub fn parse(value: String) -> Result[Number, String]",
        ] {
            let generated = emit(source);
            let code = EcmaCompiler::print_with(&generated.ast, PrintOptions::default()).code;
            assert!(!code.is_empty(), "no output for {source}");
        }
    }
}
