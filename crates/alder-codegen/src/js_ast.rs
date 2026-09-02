//! Small construction facade over the Oxc AST consumed by Rolldown.
//!
//! Alder lowering creates these nodes directly. JavaScript text only exists
//! after `finish` invokes Oxc's precedence-aware code generator.

use oxc_allocator::{Allocator, Vec as ArenaVec};
use oxc_ast::{AstBuilder, NONE, ast::*};
#[cfg(test)]
use oxc_codegen::Codegen;
use oxc_span::{SPAN, SourceType};
use oxc_syntax::{
    number::{BigintBase, NumberBase},
    operator::{AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator},
};

pub(crate) struct JsAst<'a> {
    pub(crate) builder: AstBuilder<'a>,
}

impl<'a> JsAst<'a> {
    pub(crate) fn new(allocator: &'a Allocator) -> Self {
        Self {
            builder: AstBuilder::new(allocator),
        }
    }

    pub(crate) fn vec<T>(&self) -> ArenaVec<'a, T> {
        self.builder.vec()
    }

    pub(crate) fn identifier(&self, name: &str) -> Expression<'a> {
        self.builder
            .expression_identifier(SPAN, self.builder.allocator.alloc_str(name))
    }

    pub(crate) fn string(&self, value: &str) -> Expression<'a> {
        self.builder
            .expression_string_literal(SPAN, self.builder.allocator.alloc_str(value), None)
    }

    pub(crate) fn number(&self, value: f64) -> Expression<'a> {
        self.builder
            .expression_numeric_literal(SPAN, value, None, NumberBase::Decimal)
    }

    pub(crate) fn number_source(&self, source: &str) -> Expression<'a> {
        let normalized = source.replace('_', "");
        let (base, value) = if let Some(digits) = normalized.strip_prefix("0x") {
            (
                NumberBase::Hex,
                u64::from_str_radix(digits, 16).unwrap_or(0) as f64,
            )
        } else if let Some(digits) = normalized.strip_prefix("0o") {
            (
                NumberBase::Octal,
                u64::from_str_radix(digits, 8).unwrap_or(0) as f64,
            )
        } else if let Some(digits) = normalized.strip_prefix("0b") {
            (
                NumberBase::Binary,
                u64::from_str_radix(digits, 2).unwrap_or(0) as f64,
            )
        } else {
            (
                if normalized.contains(['.', 'e', 'E']) {
                    NumberBase::Float
                } else {
                    NumberBase::Decimal
                },
                normalized.parse().unwrap_or(0.0),
            )
        };
        let raw = self.builder.allocator.alloc_str(source).into();
        self.builder
            .expression_numeric_literal(SPAN, value, Some(raw), base)
    }

    pub(crate) fn bigint_source(&self, source: &str) -> Expression<'a> {
        let source = source.strip_suffix('n').unwrap_or(source);
        let normalized = source.replace('_', "");
        let (base, value) = if let Some(digits) = normalized.strip_prefix("0x") {
            (BigintBase::Hex, digits)
        } else if let Some(digits) = normalized.strip_prefix("0o") {
            (BigintBase::Octal, digits)
        } else if let Some(digits) = normalized.strip_prefix("0b") {
            (BigintBase::Binary, digits)
        } else {
            (BigintBase::Decimal, normalized.as_str())
        };
        self.builder.expression_big_int_literal(
            SPAN,
            self.builder.allocator.alloc_str(value),
            None,
            base,
        )
    }

    pub(crate) fn boolean(&self, value: bool) -> Expression<'a> {
        self.builder.expression_boolean_literal(SPAN, value)
    }

    pub(crate) fn undefined(&self) -> Expression<'a> {
        self.identifier("undefined")
    }

    pub(crate) fn array<I>(&self, elements: I) -> Expression<'a>
    where
        I: IntoIterator<Item = Expression<'a>>,
    {
        let elements = self
            .builder
            .vec_from_iter(elements.into_iter().map(ArrayExpressionElement::from));
        self.builder.expression_array(SPAN, elements)
    }

    pub(crate) fn object(
        &self,
        properties: ArenaVec<'a, ObjectPropertyKind<'a>>,
    ) -> Expression<'a> {
        self.builder.expression_object(SPAN, properties)
    }

    pub(crate) fn property(&self, name: &str, value: Expression<'a>) -> ObjectPropertyKind<'a> {
        let key = PropertyKey::StringLiteral(self.builder.alloc_string_literal(
            SPAN,
            self.builder.allocator.alloc_str(name),
            None,
        ));
        self.builder.object_property_kind_object_property(
            SPAN,
            PropertyKind::Init,
            key,
            value,
            false,
            false,
            false,
        )
    }

    pub(crate) fn spread_property(&self, value: Expression<'a>) -> ObjectPropertyKind<'a> {
        self.builder
            .object_property_kind_spread_property(SPAN, value)
    }

    pub(crate) fn member(&self, object: Expression<'a>, name: &str) -> Expression<'a> {
        Expression::from(self.builder.member_expression_computed(
            SPAN,
            object,
            self.string(name),
            false,
        ))
    }

    pub(crate) fn index(&self, object: Expression<'a>, index: Expression<'a>) -> Expression<'a> {
        Expression::from(
            self.builder
                .member_expression_computed(SPAN, object, index, false),
        )
    }

    pub(crate) fn call<I>(&self, callee: Expression<'a>, arguments: I) -> Expression<'a>
    where
        I: IntoIterator<Item = Expression<'a>>,
    {
        let arguments = self
            .builder
            .vec_from_iter(arguments.into_iter().map(Argument::from));
        self.builder
            .expression_call(SPAN, callee, NONE, arguments, false)
    }

    pub(crate) fn binding(&self, name: &str) -> BindingPattern<'a> {
        self.builder.binding_pattern(
            self.builder.binding_pattern_kind_binding_identifier(
                SPAN,
                self.builder.allocator.alloc_str(name),
            ),
            NONE,
            false,
        )
    }

    pub(crate) fn variable(
        &self,
        kind: VariableDeclarationKind,
        name: &str,
        init: Option<Expression<'a>>,
    ) -> Statement<'a> {
        let declarator =
            self.builder
                .variable_declarator(SPAN, kind, self.binding(name), init, false);
        Statement::from(self.builder.declaration_variable(
            SPAN,
            kind,
            self.builder.vec1(declarator),
            false,
        ))
    }

    pub(crate) fn expression_statement(&self, expression: Expression<'a>) -> Statement<'a> {
        self.builder.statement_expression(SPAN, expression)
    }

    pub(crate) fn return_statement(&self, expression: Expression<'a>) -> Statement<'a> {
        self.builder.statement_return(SPAN, Some(expression))
    }

    pub(crate) fn assign_identifier(&self, name: &str, value: Expression<'a>) -> Expression<'a> {
        let target = AssignmentTarget::AssignmentTargetIdentifier(
            self.builder
                .alloc_identifier_reference(SPAN, self.builder.allocator.alloc_str(name)),
        );
        self.builder
            .expression_assignment(SPAN, AssignmentOperator::Assign, target, value)
    }

    pub(crate) fn assignment(
        &self,
        target: Expression<'a>,
        operator: AssignmentOperator,
        value: Expression<'a>,
    ) -> Expression<'a> {
        let target = match target {
            Expression::Identifier(identifier) => {
                AssignmentTarget::AssignmentTargetIdentifier(identifier)
            }
            Expression::ComputedMemberExpression(member) => {
                AssignmentTarget::ComputedMemberExpression(member)
            }
            Expression::StaticMemberExpression(member) => {
                AssignmentTarget::StaticMemberExpression(member)
            }
            Expression::PrivateFieldExpression(member) => {
                AssignmentTarget::PrivateFieldExpression(member)
            }
            _ => unreachable!("canonical assignment places only lower to JS assignment targets"),
        };
        self.builder
            .expression_assignment(SPAN, operator, target, value)
    }

    pub(crate) fn unary(
        &self,
        operator: UnaryOperator,
        argument: Expression<'a>,
    ) -> Expression<'a> {
        self.builder.expression_unary(SPAN, operator, argument)
    }

    pub(crate) fn binary(
        &self,
        left: Expression<'a>,
        operator: BinaryOperator,
        right: Expression<'a>,
    ) -> Expression<'a> {
        self.builder.expression_binary(SPAN, left, operator, right)
    }

    pub(crate) fn logical(
        &self,
        left: Expression<'a>,
        operator: LogicalOperator,
        right: Expression<'a>,
    ) -> Expression<'a> {
        self.builder.expression_logical(SPAN, left, operator, right)
    }

    pub(crate) fn await_expression(&self, argument: Expression<'a>) -> Expression<'a> {
        self.builder.expression_await(SPAN, argument)
    }

    pub(crate) fn conditional(
        &self,
        test: Expression<'a>,
        consequent: Expression<'a>,
        alternate: Expression<'a>,
    ) -> Expression<'a> {
        self.builder
            .expression_conditional(SPAN, test, consequent, alternate)
    }

    pub(crate) fn block(&self, body: ArenaVec<'a, Statement<'a>>) -> Statement<'a> {
        self.builder.statement_block(SPAN, body)
    }

    pub(crate) fn if_statement(
        &self,
        test: Expression<'a>,
        consequent: ArenaVec<'a, Statement<'a>>,
        alternate: Option<Statement<'a>>,
    ) -> Statement<'a> {
        self.builder
            .statement_if(SPAN, test, self.block(consequent), alternate)
    }

    pub(crate) fn while_statement(
        &self,
        test: Expression<'a>,
        body: ArenaVec<'a, Statement<'a>>,
    ) -> Statement<'a> {
        self.builder.statement_while(SPAN, test, self.block(body))
    }

    pub(crate) fn for_of(
        &self,
        name: &str,
        iterable: Expression<'a>,
        body: ArenaVec<'a, Statement<'a>>,
    ) -> Statement<'a> {
        let kind = VariableDeclarationKind::Const;
        let declarator =
            self.builder
                .variable_declarator(SPAN, kind, self.binding(name), None, false);
        let left = self.builder.for_statement_left_variable_declaration(
            SPAN,
            kind,
            self.builder.vec1(declarator),
            false,
        );
        self.builder
            .statement_for_of(SPAN, false, left, iterable, self.block(body))
    }

    pub(crate) fn break_statement(&self, label: Option<&str>) -> Statement<'a> {
        let label = label.map(|label| {
            self.builder
                .label_identifier(SPAN, self.builder.allocator.alloc_str(label))
        });
        self.builder.statement_break(SPAN, label)
    }

    pub(crate) fn continue_statement(&self) -> Statement<'a> {
        self.builder.statement_continue(SPAN, None)
    }

    pub(crate) fn labeled(&self, label: &str, body: ArenaVec<'a, Statement<'a>>) -> Statement<'a> {
        self.builder.statement_labeled(
            SPAN,
            self.builder
                .label_identifier(SPAN, self.builder.allocator.alloc_str(label)),
            self.block(body),
        )
    }

    pub(crate) fn try_finally(
        &self,
        body: ArenaVec<'a, Statement<'a>>,
        finalizer: ArenaVec<'a, Statement<'a>>,
    ) -> Statement<'a> {
        self.builder.statement_try(
            SPAN,
            self.builder.block_statement(SPAN, body),
            NONE,
            Some(self.builder.block_statement(SPAN, finalizer)),
        )
    }

    pub(crate) fn function(
        &self,
        name: &str,
        parameters: &[String],
        body: ArenaVec<'a, Statement<'a>>,
        r#async: bool,
    ) -> Statement<'a> {
        let parameters = self.builder.vec_from_iter(parameters.iter().map(|name| {
            self.builder.formal_parameter(
                SPAN,
                self.builder.vec(),
                self.binding(name),
                None,
                false,
                false,
            )
        }));
        let parameters = self.builder.formal_parameters(
            SPAN,
            FormalParameterKind::FormalParameter,
            parameters,
            NONE,
        );
        let body = self.builder.function_body(SPAN, self.builder.vec(), body);
        Statement::from(
            self.builder.declaration_function(
                SPAN,
                FunctionType::FunctionDeclaration,
                Some(
                    self.builder
                        .binding_identifier(SPAN, self.builder.allocator.alloc_str(name)),
                ),
                false,
                r#async,
                false,
                NONE,
                NONE,
                parameters,
                NONE,
                Some(body),
            ),
        )
    }

    pub(crate) fn arrow(
        &self,
        parameters: &[String],
        body: ArenaVec<'a, Statement<'a>>,
        r#async: bool,
    ) -> Expression<'a> {
        let parameters = self.builder.vec_from_iter(parameters.iter().map(|name| {
            self.builder.formal_parameter(
                SPAN,
                self.builder.vec(),
                self.binding(name),
                None,
                false,
                false,
            )
        }));
        let parameters = self.builder.formal_parameters(
            SPAN,
            FormalParameterKind::ArrowFormalParameters,
            parameters,
            NONE,
        );
        let body = self.builder.function_body(SPAN, self.builder.vec(), body);
        self.builder
            .expression_arrow_function(SPAN, false, r#async, NONE, parameters, NONE, body)
    }

    pub(crate) fn import(&self, source: &str, names: &[(String, String)]) -> Statement<'a> {
        let specifiers = self
            .builder
            .vec_from_iter(names.iter().map(|(imported, local)| {
                self.builder.import_declaration_specifier_import_specifier(
                    SPAN,
                    self.builder.module_export_name_identifier_name(
                        SPAN,
                        self.builder.allocator.alloc_str(imported),
                    ),
                    self.builder
                        .binding_identifier(SPAN, self.builder.allocator.alloc_str(local)),
                    ImportOrExportKind::Value,
                )
            }));
        Statement::from(
            self.builder.module_declaration_import_declaration(
                SPAN,
                Some(specifiers),
                self.builder
                    .string_literal(SPAN, self.builder.allocator.alloc_str(source), None),
                None,
                NONE,
                ImportOrExportKind::Value,
            ),
        )
    }

    pub(crate) fn side_effect_import(&self, source: &str) -> Statement<'a> {
        Statement::from(
            self.builder.module_declaration_import_declaration(
                SPAN,
                None,
                self.builder
                    .string_literal(SPAN, self.builder.allocator.alloc_str(source), None),
                None,
                NONE,
                ImportOrExportKind::Value,
            ),
        )
    }

    pub(crate) fn export(&self, names: &[(String, String)]) -> Statement<'a> {
        let specifiers = self
            .builder
            .vec_from_iter(names.iter().map(|(local, exported)| {
                self.builder.export_specifier(
                    SPAN,
                    self.builder.module_export_name_identifier_reference(
                        SPAN,
                        self.builder.allocator.alloc_str(local),
                    ),
                    self.builder.module_export_name_identifier_name(
                        SPAN,
                        self.builder.allocator.alloc_str(exported),
                    ),
                    ImportOrExportKind::Value,
                )
            }));
        Statement::from(self.builder.module_declaration_export_named_declaration(
            SPAN,
            None,
            specifiers,
            None,
            ImportOrExportKind::Value,
            NONE,
        ))
    }

    pub(crate) fn export_default(&self, expression: Expression<'a>) -> Statement<'a> {
        Statement::from(
            self.builder
                .module_declaration_export_default_declaration(SPAN, expression.into()),
        )
    }

    pub(crate) fn program(&self, body: ArenaVec<'a, Statement<'a>>) -> Program<'a> {
        self.builder.program(
            SPAN,
            SourceType::mjs(),
            "",
            self.builder.vec(),
            None,
            self.builder.vec(),
            body,
        )
    }

    #[cfg(test)]
    pub(crate) fn finish(&self, body: ArenaVec<'a, Statement<'a>>) -> String {
        Codegen::new().build(&self.program(body)).code
    }
}

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::VariableDeclarationKind;
    use oxc_span::SourceType;
    use rolldown_ecmascript::{EcmaAst, EcmaCompiler, PrintOptions};

    use super::JsAst;

    #[test]
    fn builds_and_prints_oxc_nodes_without_source_fragments() {
        let allocator = Allocator::default();
        let js = JsAst::new(&allocator);
        let answer = js.call(js.identifier("compute"), [js.number(40.0), js.number(2.0)]);
        let declaration = js.variable(VariableDeclarationKind::Const, "answer", Some(answer));
        let output = js.finish(js.builder.vec1(declaration));

        assert_eq!(output, "const answer = compute(40, 2);\n");
    }

    #[test]
    fn owns_the_generated_program_in_rolldowns_ast_container() {
        let mut ast = EcmaAst {
            source_type: SourceType::mjs(),
            ..EcmaAst::default()
        };
        ast.program.with_mut(|fields| {
            let js = JsAst::new(fields.allocator);
            let answer = js.variable(
                VariableDeclarationKind::Const,
                "answer",
                Some(js.number(42.0)),
            );
            *fields.program = js.program(js.builder.vec1(answer));
        });

        let output = EcmaCompiler::print_with(&ast, PrintOptions::default()).code;
        assert_eq!(output, "const answer = 42;\n");
    }
}
