//! Deterministic Oxc AST generation from Alder's canonical AST.
//!
//! Generated programs stay in Rolldown's owned `EcmaAst` container. JavaScript
//! text is produced only for requested output artifacts and diagnostics.

mod js_ast;
mod oxc_backend;
pub mod support;

use alder_ast::{
    BindingName, Block, Expr, Module, ModuleId, PackageId, QualifiedName, RecordField, Stmt,
};
use alder_region::{Located, Region};
use rolldown_ecmascript::{EcmaAst, EcmaCompiler, PrintOptions};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmitMode {
    Build,
    Test,
}

#[derive(Clone, Copy, Debug)]
pub struct EmitOptions {
    pub mode: EmitMode,
}

impl Default for EmitOptions {
    fn default() -> Self {
        Self {
            mode: EmitMode::Build,
        }
    }
}

pub struct EmittedModule {
    pub module_id: String,
    pub ast: EcmaAst,
    pub dependencies: Vec<String>,
}

impl EmittedModule {
    /// Serialize this AST for a requested output artifact or diagnostic view.
    pub fn code(&self) -> String {
        EcmaCompiler::print_with(&self.ast, PrintOptions::default()).code
    }
}

impl Clone for EmittedModule {
    fn clone(&self) -> Self {
        Self {
            module_id: self.module_id.clone(),
            ast: self.ast.clone_with_another_arena(),
            dependencies: self.dependencies.clone(),
        }
    }
}

impl std::fmt::Debug for EmittedModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmittedModule")
            .field("module_id", &self.module_id)
            .field("dependencies", &self.dependencies)
            .finish_non_exhaustive()
    }
}

impl PartialEq for EmittedModule {
    fn eq(&self, other: &Self) -> bool {
        self.module_id == other.module_id
            && self.dependencies == other.dependencies
            && self.code() == other.code()
    }
}
impl Eq for EmittedModule {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub region: Region,
    pub message: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Import {
    Value {
        module: String,
        exported: String,
        local: String,
    },
    Extern {
        module: String,
        exported: String,
        local: String,
    },
}

pub fn emit_module(module: &Module<'_>, options: EmitOptions) -> Result<EmittedModule, Error> {
    emit_module_with_solution(module, None, options)
}

pub fn emit_solved_module(
    module: &Module<'_>,
    solved: &alder_solve::SolveOutput<'_>,
    options: EmitOptions,
) -> Result<EmittedModule, Error> {
    emit_module_with_solution(module, Some(solved), options)
}

fn emit_module_with_solution(
    module: &Module<'_>,
    solved: Option<&alder_solve::SolveOutput<'_>>,
    options: EmitOptions,
) -> Result<EmittedModule, Error> {
    let generated = oxc_backend::emit_module_ast(module, solved, options)?;
    Ok(EmittedModule {
        module_id: generated.module_id,
        ast: generated.ast,
        dependencies: generated.dependencies,
    })
}

fn contains_await_block(block: &Located<Block<'_>>) -> bool {
    block.value.tail.is_some_and(contains_await_expr)
        || block
            .value
            .statements
            .iter()
            .any(|statement| match &statement.value {
                Stmt::Let(decl) => contains_await_expr(decl.value),
                Stmt::Assign { value, .. } | Stmt::Assert(value) | Stmt::Expr(value) => {
                    contains_await_expr(value)
                }
                Stmt::Return(Some(value)) | Stmt::Break(Some(value)) => contains_await_expr(value),
                Stmt::For { iter, body, .. } => {
                    contains_await_expr(iter) || contains_await_block(body)
                }
                Stmt::While { condition, body } => {
                    contains_await_expr(condition) || contains_await_block(body)
                }
                Stmt::Use { .. } | Stmt::Return(None) | Stmt::Break(None) | Stmt::Continue => false,
            })
}

fn contains_await_expr(expression: &Located<Expr<'_>>) -> bool {
    match &expression.value {
        Expr::Await(_) => true,
        Expr::Array(items) | Expr::Tuple(items) => items.iter().any(|item| contains_await_expr(item)),
        Expr::Call {
            function,
            arguments,
            ..
        } => contains_await_expr(function) || arguments.iter().any(|arg| contains_await_expr(arg)),
        Expr::Access { record, .. } => contains_await_expr(record),
        Expr::Index { target, index } => contains_await_expr(target) || contains_await_expr(index),
        Expr::Try(expr) | Expr::Pin(expr) | Expr::Not(expr) | Expr::State(expr) => contains_await_expr(expr),
        Expr::Negate { expr, .. } => contains_await_expr(expr),
        Expr::Binop { left, right, .. } => contains_await_expr(left) || contains_await_expr(right),
        Expr::Block(block) | Expr::Loop(block) => contains_await_block(block),
        Expr::If { branches, final_else } => branches.iter().any(|branch| contains_await_expr(branch.condition) || contains_await_block(branch.body)) || final_else.is_some_and(contains_await_block),
        Expr::Match { scrutinee, arms } => contains_await_expr(scrutinee) || arms.iter().any(|arm| arm.guard.is_some_and(contains_await_expr) || contains_await_expr(arm.body)),
        Expr::Provide { value, body, .. } => contains_await_expr(value) || contains_await_block(body),
        Expr::Record(fields) | Expr::RecordConstructor { fields, .. } => fields.iter().any(|field| match field { RecordField::Field { value, .. } | RecordField::Spread(value) => contains_await_expr(value) }),
        Expr::TaggedTemplate { tag, parts } => contains_await_expr(tag) || parts.iter().any(|part| matches!(part, alder_ast::TemplatePart::Expr(expr) if contains_await_expr(expr))),
        Expr::Template(parts) => parts.iter().any(|part| matches!(part, alder_ast::TemplatePart::Expr(expr) if contains_await_expr(expr))),
        Expr::Lambda { .. } | Expr::Number { .. } | Expr::BigInt(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Unit | Expr::Var { .. } | Expr::Constructor(_) | Expr::Tag { .. } | Expr::TupleAccess { .. } | Expr::Style(_) | Expr::Query(_) | Expr::Markup(_) | Expr::MacroCall { .. } => false,
    }
}

fn module_specifier(module: ModuleId<'_>) -> String {
    let mut result = match module.package {
        PackageId::Application => "alder://app".to_owned(),
        PackageId::ApplicationMember(member) => {
            format!("alder://app/{}", escaped(member))
        }
        PackageId::Builtin => "alder://std".to_owned(),
        PackageId::Named(package) => format!(
            "alder://pkg/{}/{}",
            escaped(package.author),
            escaped(package.project)
        ),
    };
    if module.path.is_empty() {
        result.push_str("/mod.mjs");
    } else {
        for part in module.path {
            result.push('/');
            result.push_str(&escaped(part));
        }
        result.push_str(".mjs");
    }
    result
}

fn type_named(typ: &Located<alder_ast::Type<'_>>, expected: &str) -> bool {
    match &typ.value {
        alder_ast::Type::Named { reference, .. } => reference.name == expected,
        alder_ast::Type::Alias { target, .. } => match target {
            alder_ast::AliasType::Open(target) | alder_ast::AliasType::Filled(target) => {
                type_named(target, expected)
            }
        },
        _ => false,
    }
}

fn qualified_key(name: QualifiedName<'_>) -> String {
    format!("{}::{}", module_specifier(name.module), name.name)
}
fn top_name(name: QualifiedName<'_>) -> String {
    format!("$v_{}", escaped(name.name))
}
fn local_name(name: alder_ast::LocalName<'_>) -> String {
    format!("$l{}_{}", name.id.0, escaped(name.text))
}
fn binding_name(name: BindingName<'_>) -> String {
    match name {
        BindingName::Local(name) => local_name(name),
        BindingName::TopLevel(name) => top_name(name),
    }
}
fn constructor_name(name: alder_ast::ConstructorName<'_>) -> String {
    constructor_name_from_parts(name.enum_.name, name.variant)
}
fn constructor_name_from_parts(enum_name: &str, variant: &str) -> String {
    format!("$c_{}_{}", escaped(enum_name), escaped(variant))
}
fn constructor_export(name: alder_ast::ConstructorName<'_>) -> String {
    constructor_name(name)
}

fn escaped(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'_' {
            result.push(byte as char);
        } else {
            result.push_str(&format!("_{byte:02X}"));
        }
    }
    result
}

#[cfg(test)]
macro_rules! assert_emit_snapshot {
    ($source:literal) => {{
        let source = indoc::indoc!($source);
        let generated = emit(source);
        insta::with_settings!({
            description => source,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(generated);
        });
    }};
}

#[cfg(test)]
macro_rules! assert_solved_emit_snapshot {
    ($source:literal) => {{
        let source = indoc::indoc!($source);
        let generated = emit_solved(source);
        insta::with_settings!({
            description => source,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(generated);
        });
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use alder_ast::{PackageId, ResolvedImport};
    use bumpalo::Bump;

    fn emit(source: &str) -> String {
        let bump = Bump::new();
        let source = bump.alloc_str(source);
        let parsed = alder_parse::parse_module(&bump, source).expect("source parses");
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
        .expect("source canonicalizes");
        emit_module(canonical.module, EmitOptions::default())
            .expect("module emits")
            .code()
    }

    fn emit_solved(source: &str) -> String {
        let bump = Bump::new();
        let source = bump.alloc_str(source);
        let parsed = alder_parse::parse_module(&bump, source).expect("source parses");
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
        .expect("source canonicalizes");
        let constraints = alder_constrain::constrain(&bump, canonical.module);
        let traits = alder_solve::TraitDatabase::build(&bump, canonical.module, &[]);
        let solved = alder_solve::solve(&bump, &constraints, &traits).expect("module solves");
        emit_solved_module(canonical.module, &solved, EmitOptions::default())
            .expect("module emits")
            .code()
    }

    #[test]
    fn function_and_block_lifting() {
        assert_emit_snapshot! {r#"
            pub fn answer() {
                let x = 40
                x + 2
            }
        "#};
    }
    #[test]
    fn enum_representation() {
        assert_emit_snapshot!(
            "pub enum Shape { Point, Circle(Number), Rect { width: Number, height: Number } }"
        );
    }
    #[test]
    fn match_emission() {
        assert_emit_snapshot! {r#"
            enum Maybe[a] { Nothing, Just(a) }
            pub fn unwrap(value) { match value { Maybe::Just(x) => x, Maybe::Nothing => 0 } }
        "#};
    }
    #[test]
    fn pin_pattern_evaluates_once() {
        assert_emit_snapshot! {r#"
            fn expected() { 1 }
            pub fn same(value) { match value { ^expected() => true, _ => false } }
        "#};
    }
    #[test]
    fn if_and_short_circuit_lifting() {
        assert_emit_snapshot!(
            "pub fn choose(flag, fallback) { if flag && fallback() { 1 } else { 2 } }"
        );
    }
    #[test]
    fn records_arrays_and_indexing() {
        assert_emit_snapshot! {r#"
            pub fn first(name) {
                let value = { name: name, scores: [10, 20] }
                value.scores[0]
            }
        "#};
    }
    #[test]
    fn mutable_loop_emission() {
        assert_emit_snapshot! {r#"
            pub fn sum() {
                let mut total = 0
                for value in [1, 2] { total += value }
                total
            }
        "#};
    }
    #[test]
    fn result_extern_is_guarded() {
        assert_emit_snapshot! {r#"
            #[extern("library", "parse")]
            pub fn parse(value: String) Result[Number, String]
        "#};
    }

    #[test]
    fn pipe_forwards_into_first_call_argument() {
        assert_solved_emit_snapshot! {r#"
            fn subtract(left: Number, right: Number) Number { left - right }
            pub fn answer() Number { 44 |> subtract(2) }
        "#};
    }

    #[test]
    fn pipe_placeholder_controls_call_argument() {
        assert_solved_emit_snapshot! {r#"
            fn subtract(left: Number, right: Number) Number { left - right }
            pub fn answer() Number { 2 |> subtract(44, _) }
        "#};
    }

    #[test]
    fn pipe_evaluates_left_before_callee_and_existing_arguments() {
        assert_solved_emit_snapshot! {r#"
            fn left() Number { 40 }
            fn right() Number { 2 }
            fn factory() fn(Number, Number) Number {
                (a: Number, b: Number) Number -> a + b
            }
            pub fn answer() Number { left() |> factory()(right()) }
        "#};
    }

    #[test]
    fn trait_dictionary_passing() {
        assert_solved_emit_snapshot! {r#"
            trait Show[a] { fn show(value: a) String }
            impl Show[Number] { fn show(value: Number) String { "number" } }
            fn describe(value: a) String where a: Show { show(value) }
            pub fn main() String { describe(1) }
        "#};
    }

    #[test]
    fn solved_primitive_equality_is_strict() {
        assert_solved_emit_snapshot!("pub fn same() Bool { 1 == 2 }");
    }

    #[test]
    fn solved_record_equality_uses_field_dictionaries() {
        assert_solved_emit_snapshot! {r#"
            pub fn same(
                left: { name: String, score: Number },
                right: { name: String, score: Number },
            ) Bool {
                left == right
            }
        "#};
    }

    #[test]
    fn generic_ordering_uses_the_compare_result_tag() {
        assert_solved_emit_snapshot! {r#"
            pub fn less_than(left: a, right: a) Bool where a: Ord {
                left < right
            }
        "#};
    }

    #[test]
    fn prerequisite_dictionary_factory() {
        assert_solved_emit_snapshot! {r#"
            trait Show[a] { fn show(value: a) String }
            impl Show[Number] { fn show(value: Number) String { "number" } }
            impl Show[Array[a]] where a: Show {
                fn show(value: Array[a]) String { "array" }
            }
            pub fn main() String { show([1]) }
        "#};
    }

    #[test]
    fn default_method_dictionary_entry() {
        assert_solved_emit_snapshot! {r#"
            trait Show[a] {
                fn show(value: a) String
                fn render(value: a) String { show(value) }
            }
            impl Show[Number] { fn show(value: Number) String { "number" } }
            pub fn main() String { render(1) }
        "#};
    }

    #[test]
    fn mutually_recursive_defaults_use_the_selected_dictionary() {
        assert_solved_emit_snapshot! {r#"
            trait Alternate[a] {
                fn first(value: a) String { second(value) }
                fn second(value: a) String { first(value) }
            }
            impl Alternate[Number] {}
            pub fn render(value: Number) String { first(value) }
        "#};
    }

    #[test]
    fn method_local_dictionaries_follow_predicate_order() {
        assert_solved_emit_snapshot! {r#"
            trait Render[a] {
                fn render(value: a, first: b, second: c) String
                    where b: Show, c: Eq + Show
            }
            impl Render[Number] {
                fn render(value: Number, first: b, second: c) String
                    where b: Show, c: Eq + Show
                {
                    show(first)
                }
            }
            pub fn main() String { render(0, "first", true) }
        "#};
    }

    #[test]
    fn projection_equalities_do_not_add_dictionary_arguments() {
        assert_solved_emit_snapshot! {r#"
            trait Source[a] {
                type Item
                fn next(value: a) Option[Item]
            }
            fn take(value: a) Option[Number]
                where a: Source, a.Item == Number
            {
                next(value)
            }
        "#};
    }

    #[test]
    fn built_in_derive_dictionaries() {
        assert_emit_snapshot! {r#"
            #[derive(Show, Ord, Hash, Json)]
            pub enum Status { Ready, Failed(String) }
        "#};
    }

    #[test]
    fn derived_json_marks_optional_record_fields() {
        assert_emit_snapshot! {r#"
            #[derive(Json)]
            pub enum Config {
                Config { name: String, note?: String },
            }
        "#};
    }

    #[test]
    fn derived_show_uses_record_payload_source_order() {
        assert_emit_snapshot! {r#"
            #[derive(Show)]
            pub enum Status {
                Meta { code: Number, note: String },
            }
        "#};
    }

    #[test]
    fn derived_show_uses_resolved_payload_dictionary() {
        assert_solved_emit_snapshot! {r#"
            enum Secret { Secret }
            impl Show[Secret] {
                fn show(value: Secret) String { "redacted" }
            }
            #[derive(Show)]
            enum Wrapped { Wrapped(Secret) }
            pub fn main() String { show(Wrapped::Wrapped(Secret::Secret)) }
        "#};
    }

    #[test]
    fn derived_generic_payload_uses_prerequisite_dictionary() {
        assert_solved_emit_snapshot! {r#"
            #[derive(Show)]
            enum Box[a] { Box(a) }
            pub fn render(value: Box[String]) String { show(value) }
        "#};
    }

    #[test]
    fn error_group_ord_preserves_declaration_order() {
        assert_solved_emit_snapshot! {r#"
            #[derive(Ord)]
            pub error Failure { :later, :first(Number) }
        "#};
    }

    #[test]
    fn recursive_derived_dictionary_references_its_emitted_binding() {
        assert_solved_emit_snapshot! {r#"
            #[derive(Show)]
            pub enum Chain { End, Link(Number, Chain) }

            pub fn render(value: Chain) String { show(value) }
        "#};
    }

    #[test]
    fn compound_assignment_uses_the_selected_num_dictionary() {
        assert_solved_emit_snapshot! {r#"
            enum Token { Token }
            impl Ord[Token] {
                fn compare(left: Token, right: Token) Ordering { Ordering::Equal }
            }
            impl Num[Token] {
                fn add(left: Token, right: Token) Token { right }
                fn sub(left: Token, right: Token) Token { right }
                fn mul(left: Token, right: Token) Token { right }
                fn div(left: Token, right: Token) Token { right }
                fn rem(left: Token, right: Token) Token { right }
                fn negate(value: Token) Token { value }
            }
            pub fn update() Token {
                let mut value = Token::Token
                value += Token::Token
                value
            }
        "#};
    }

    #[test]
    fn indexed_dictionary_assignment_evaluates_its_place_once() {
        assert_solved_emit_snapshot! {r#"
            enum Token { Token }
            impl Ord[Token] {
                fn compare(left: Token, right: Token) Ordering { Ordering::Equal }
            }
            impl Num[Token] {
                fn add(left: Token, right: Token) Token { right }
                fn sub(left: Token, right: Token) Token { right }
                fn mul(left: Token, right: Token) Token { right }
                fn div(left: Token, right: Token) Token { right }
                fn rem(left: Token, right: Token) Token { right }
                fn negate(value: Token) Token { value }
            }
            fn next_index() Number { 0 }
            pub fn update() Token {
                let mut values = [Token::Token]
                values[next_index()] += Token::Token
                values[0]
            }
        "#};
    }
}
