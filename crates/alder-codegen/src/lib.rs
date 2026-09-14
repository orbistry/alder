//! Deterministic Oxc AST generation from Alder's canonical AST.
//!
//! Generated programs stay in Rolldown's owned `EcmaAst` container. JavaScript
//! text is produced only for requested output artifacts and diagnostics.

mod js_ast;
mod oxc_backend;
pub mod support;

use alder_ast::{BindingName, Module, ModuleId, PackageId, QualifiedName};
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
    /// Physical Alder source location, supplied by the driver for extern resolution.
    pub source_path: Option<std::path::PathBuf>,
    /// Owned source and extern declaration regions for late resolution errors.
    pub source_text: Option<String>,
    pub extern_regions: Vec<(String, Region)>,
    pub ast: EcmaAst,
    pub dependencies: Vec<String>,
    /// Local scoped stores, for client-reachability-controlled hydration.
    pub store_keys: Vec<String>,
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
            source_path: self.source_path.clone(),
            source_text: self.source_text.clone(),
            extern_regions: self.extern_regions.clone(),
            ast: self.ast.clone_with_another_arena(),
            dependencies: self.dependencies.clone(),
            store_keys: self.store_keys.clone(),
        }
    }
}

impl std::fmt::Debug for EmittedModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmittedModule")
            .field("module_id", &self.module_id)
            .field("source_path", &self.source_path)
            .field("dependencies", &self.dependencies)
            .finish_non_exhaustive()
    }
}

impl PartialEq for EmittedModule {
    fn eq(&self, other: &Self) -> bool {
        self.module_id == other.module_id
            && self.source_path == other.source_path
            && self.source_text == other.source_text
            && self.extern_regions == other.extern_regions
            && self.dependencies == other.dependencies
            && self.store_keys == other.store_keys
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
        source_path: None,
        source_text: None,
        extern_regions: vec![],
        ast: generated.ast,
        dependencies: generated.dependencies,
        store_keys: module
            .store_bindings
            .iter()
            .filter(|name| name.module == module.id)
            .map(|name| format!("{}#{}", module_specifier(name.module), name.name))
            .collect(),
    })
}

pub fn module_specifier(module: ModuleId<'_>) -> String {
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

pub fn qualified_key(name: QualifiedName<'_>) -> String {
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
    #[test]
    fn nested_option_ordering_emits_each_payload_once() {
        for depth in [2, 4, 8] {
            let ty = format!("{}Number{}", "Option[".repeat(depth), "]".repeat(depth));
            let source = format!(
                "pub fn order(left: {ty}, right: {ty}) Ordering {{ compare(left, right) }}"
            );
            let code = emit_solved(&source);
            assert_eq!(code.matches("$compareContainer(").count(), depth);
            assert_eq!(code.matches("$equalContainer(").count(), depth);
        }
    }

    #[test]
    fn nested_single_method_evidence_emits_each_payload_once() {
        for depth in [2, 4, 8] {
            let ty = format!("{}Number{}", "Array[".repeat(depth), "]".repeat(depth));
            let equality = emit_solved(&format!(
                "pub fn same(left: {ty}, right: {ty}) Bool {{ left == right }}"
            ));
            assert_eq!(equality.matches("$equalStructural(").count(), depth);
            let showing = emit_solved(&format!(
                "pub fn render(value: {ty}) String {{ show(value) }}"
            ));
            assert_eq!(showing.matches("$showContainer(").count(), depth);
        }
    }

    #[test]
    fn coalesce_unwraps_only_the_present_branch() {
        assert_solved_emit_snapshot! {r#"
            pub fn default_unit(value: Option[()], events: Array[Number]) () {
                value ?? {
                    let ignored = array.push(events, 1)
                    ()
                }
            }
            pub fn nested(value: Option[Option[Number]]) Option[Number] {
                value ?? Some(42)
            }
        "#};
    }

    #[test]
    fn later_pin_mutation_cannot_invalidate_captured_payloads() {
        assert_solved_emit_snapshot! {r#"
            pub fn read() Number {
                let source = { item: Some(42), flag: 0 }
                match source {
                    { item: Some(value), flag: ^{
                        source.item = None
                        0
                    } } => value
                    _ => 0
                }
            }
        "#};
    }

    #[test]
    fn alternative_patterns_share_guard_and_body_binding_identity() {
        assert_solved_emit_snapshot! {r#"
            enum Choice { Left(Number), Right(Number) }
            pub fn read(input: Choice) Number {
                match input { Left(value) | Right(value) if value > 0 => value, _ => 0 }
            }
        "#};
    }

    #[test]
    fn nested_pin_effects_follow_enclosing_pattern_checks() {
        assert_solved_emit_snapshot! {r#"
            fn expected(events: Array[Number]) Number {
                array.push(events, 1)
                42
            }
            pub fn same(input: Option[(Number, Number)], events: Array[Number]) Bool {
                match input { Some((0, ^expected(events))) => true, _ => false }
            }
        "#};
    }

    #[test]
    fn pin_uses_outer_binding_before_pattern_shadowing() {
        assert_solved_emit_snapshot! {r#"
            pub fn same(expected: Number, input: (Number, Number)) Bool {
                match input { (expected, ^expected) => expected == 1, _ => false }
            }
        "#};
    }

    #[test]
    fn refutable_let_checks_before_extracting_payload() {
        // The checker rejects this source. Exercise the emitter directly to
        // retain its defensive runtime check for callers of the raw AST API.
        assert_emit_snapshot! {r#"
            pub fn read(value: Option[Number]) Number {
                let Some(number) = value
                number
            }
        "#};
    }

    #[test]
    fn recursive_option_fields_use_kernel_wrapping() {
        assert_solved_emit_snapshot! {r#"
            pub fn main() {
                let value: { nested: Option[Option[Number]] } = { nested: 42 }
                value
            }
        "#};
    }

    #[test]
    fn omitted_record_option_fields_emit_none() {
        assert_solved_emit_snapshot! {r#"
            pub fn empty() {
                let record: { value: Option[Number] } = {}
                record
            }
            pub fn nested() {
                let record: { value?: Option[Number] } = { value: Some(None) }
                record
            }
        "#};
    }

    #[test]
    fn enum_option_fields_default_and_read_without_presence_wrapping() {
        assert_solved_emit_snapshot! {r#"
            enum Config { Config { value?: Option[Number] } }
            pub fn empty() { Config::Config {} }
            pub fn nested() { Config::Config { value: Some(None) } }
            pub fn read(config: Config) Option[Option[Number]] {
                match config { Config::Config { value } => value }
            }
        "#};
    }

    #[test]
    fn generic_option_lifting_keeps_a_universal_input_opaque() {
        assert_solved_emit_snapshot!(
            r#"
            fn consume(value?: a) {}
            pub fn relay(value: a) a {
                consume(value)
                value
            }
        "#
        );
    }

    #[test]
    fn recursive_option_arguments_use_kernel_wrapping() {
        assert_solved_emit_snapshot! {r#"
            fn nested(value?: Option[Number]) Option[Option[Number]] { value }
            pub fn main() Option[Option[Number]] { 42 |> nested() }
        "#};
    }

    #[test]
    fn omitted_option_arguments_emit_explicit_none_values() {
        assert_solved_emit_snapshot! {r#"
            fn choose(value: Number, extra?: Number) Number { value }
            pub fn main() Number {
                let direct = choose(20)
                let piped = 22 |> choose()
                direct + piped
            }
        "#};
    }

    #[test]
    fn option_constructors_and_nested_patterns_use_kernel_representation() {
        assert_solved_emit_snapshot! {r#"
            pub fn nested() Option[Option[Number]] { Some(None) }
            pub fn read(value: Option[Option[Number]]) Number {
                match value { Some(Some(number)) => number, _ => 0 }
            }
        "#};
    }

    #[test]
    fn structural_error_equality_uses_runtime_tag_names() {
        assert_solved_emit_snapshot! {r#"
            pub fn same(left: Result[Number, [:payload(Number, String)]], right: Result[Number, [:payload(Number, String)]]) Bool {
                left == right
            }
        "#};
    }

    #[test]
    fn explicit_async_without_await_is_lazy() {
        assert_solved_emit_snapshot!("pub async fn answer() Number { 42 }");
    }

    #[test]
    fn explicit_async_nested_tasks_are_not_flattened() {
        assert_solved_emit_snapshot!("pub async fn nested() Task[Number] { async { 42 } }");
    }

    #[test]
    fn explicit_async_block_captures_reassigned_binding() {
        assert_solved_emit_snapshot!(
            r#"
            pub fn make() Task[Number] {
                let value = 1
                let task = async { value }
                value = 42
                task
            }
        "#
        );
    }

    #[test]
    fn explicit_async_lambda_returns_task() {
        assert_solved_emit_snapshot!("pub let worker = (x: Number) Task[Number] -> async { x }");
    }

    use super::*;
    use alder_ast::PackageId;
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
                imports: alder_can::resolve_imports(&bump, &parsed, PackageId::Application),
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
                imports: alder_can::resolve_imports(&bump, &parsed, PackageId::Application),
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
    fn nested_json_dictionary_size_grows_linearly() {
        let sizes = [2, 4, 8].map(|depth| {
            let ty = format!("{}Number{}", "Array[".repeat(depth), "]".repeat(depth));
            let source =
                format!("import json\npub fn encode(value: {ty}) String {{ json.encode(value) }}");
            let code = emit_solved(&source);
            assert_eq!(code.matches("$jsonEncodeContainer(").count(), depth);
            assert_eq!(code.matches("$jsonDecodeContainer(").count(), depth);
            code.len()
        });
        assert!(
            sizes[2] < sizes[1] * 3 && sizes[1] < sizes[0] * 3,
            "doubling codec depth must not cause exponential output growth: {sizes:?}"
        );
    }

    #[test]
    fn nested_hash_dictionary_emits_each_payload_once() {
        for depth in [2, 4, 8] {
            let ty = format!("{}Number{}", "Array[".repeat(depth), "]".repeat(depth));
            let source = format!("pub fn fingerprint(value: {ty}) BigInt {{ hash(value) }}");
            let code = emit_solved(&source);
            assert_eq!(
                code.matches("$hashContainer(").count(),
                depth,
                "each nested Hash dictionary must occur once (depth {depth}, {} bytes)",
                code.len(),
            );
            assert_eq!(code.matches("$equalContainer(").count(), depth);
        }
    }

    #[test]
    fn primitive_hash_superclass_uses_primitive_equality() {
        assert_solved_emit_snapshot! {r#"
            fn equal_with_hash(left: a, right: a) Bool where a: Hash {
                left == right
            }
            pub fn check() Bool { equal_with_hash(0, -0) }
        "#};
    }

    #[test]
    fn container_hash_shares_lazy_payloads_with_equality() {
        assert_solved_emit_snapshot! {r#"
            pub fn fingerprint(value: Array[Option[Number]]) BigInt {
                hash(value)
            }
        "#};
    }

    #[test]
    fn nested_structural_json_dictionary_emits_each_payload_once() {
        for depth in [2, 4, 8] {
            let ty = (0..depth).fold("Number".to_owned(), |payload, _| {
                format!("Result[Number, [:nested({payload})]]")
            });
            let source =
                format!("import json\npub fn encode(value: {ty}) String {{ json.encode(value) }}");
            let code = emit_solved(&source);
            assert_eq!(code.matches("$jsonEncodeDerived(").count(), depth);
            assert_eq!(code.matches("$jsonDecodeDerived(").count(), depth);
        }
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
    fn while_condition_break_preserves_outer_target() {
        assert_emit_snapshot! {r#"
            pub fn answer() Number {
                loop {
                    while { break 42 } {}
                    break 0
                }
            }
        "#};
    }

    #[test]
    fn mutable_loop_emission() {
        assert_emit_snapshot! {r#"
            pub fn sum() {
                let total = 0
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
    fn constrained_extern_consumes_hidden_dictionaries() {
        assert_solved_emit_snapshot! {r#"
            #[extern("library", "identity")]
            pub fn identity(value: a) a where a: Show + Eq

            #[extern("library", "pending", "abort")]
            pub fn pending(value: a) Task[a] where a: Show
        "#};
    }

    #[test]
    fn option_try_uses_null_check_and_representation_aware_unboxing() {
        assert_solved_emit_snapshot! {r#"
            pub fn flatten(value: Option[Option[Number]]) Option[Number] {
                let inner = value?
                Some(inner?)
            }
        "#};
    }

    #[test]
    fn option_try_in_pipe_await_uses_the_async_return_boundary() {
        assert_solved_emit_snapshot! {r#"
            async fn start(value: Option[Number]) Option[Number] { value }
            pub async fn run(value: Option[Number]) Option[Number] {
                Some(value |> start().await?)
            }
        "#};
    }

    #[test]
    fn tags_and_try_lower_directly_and_preserve_the_err_object() {
        assert_solved_emit_snapshot! {r#"
            fn source() Result[Number, [:invalid(String)]] {
                Err(:invalid("number"))
            }

            pub fn load() Result[Number] {
                let value = source()?
                Ok(value + 1)
            }
        "#};
    }

    #[test]
    fn result_and_tag_patterns_lower_directly() {
        assert_solved_emit_snapshot! {r#"
            error Failure {
                :invalid(String),
                :missing
            }

            pub fn render(value: Result[Number, Failure]) String {
                match value {
                    Ok(number) => "ok",
                    Err(:invalid(message)) => message,
                    Err(:missing) => "missing",
                }
            }
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
    fn async_functions_and_pipe_postfixes_lower_to_direct_generator_asts() {
        assert_solved_emit_snapshot! {r#"
            import task
            #[extern("globalThis", "Promise.resolve")]
            fn resolved(value: a) Task[a]

            async fn load(value: Number) Result[Number] {
                task.sleep(1).await
                Ok(value)
            }

            pub async fn main() Result[Number] {
                let value = 42 |> resolved().await |> load().await?
                Ok(value)
            }
        "#};
    }

    #[test]
    fn abort_aware_extern_receives_the_runtime_signal() {
        assert_solved_emit_snapshot! {r#"
            #[extern("globalThis", "fetch", "abort")]
            fn fetch(url: String) Task[String]

            pub async fn request(url: String) String {
                fetch(url).await
            }
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
        assert_solved_emit_snapshot! {r#"
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
    fn structural_error_json_emits_payload_codecs() {
        assert_solved_emit_snapshot! {r#"
            import json
            pub fn encode(value: Result[Number, [:missing | :bad(Number)]]) String {
                json.encode(value)
            }
        "#};
    }

    #[test]
    fn structural_error_show_emits_payload_dictionaries() {
        assert_solved_emit_snapshot! {r#"
            pub fn render(value: Result[Number, [:missing | :bad(Number)]]) String {
                show(value)
            }
        "#};
    }

    #[test]
    fn error_groups_emit_no_nominal_dictionaries() {
        assert_solved_emit_snapshot! {r#"
            pub error Failure { :later, :first(Number) }
        "#};
    }

    #[test]
    fn structural_hash_shares_payloads_with_equality() {
        assert_solved_emit_snapshot! {r#"
            pub fn fingerprint(value: [:bad(Number) | :missing]) BigInt { hash(value) }
        "#};
    }

    #[test]
    fn nested_structural_hash_emits_each_payload_once() {
        for depth in [2, 4, 8] {
            let ty = (0..depth).fold("Number".to_owned(), |payload, _| {
                format!("Result[Number, [:nested({payload})]]")
            });
            let source = format!("pub fn fingerprint(value: {ty}) BigInt {{ hash(value) }}");
            let code = emit_solved(&source);
            assert_eq!(code.matches("$hashErrorRow(").count(), depth);
            assert_eq!(code.matches("$equalStructural(").count(), depth);
        }
    }

    #[test]
    fn nested_recursive_derived_evidence_uses_its_emitted_binding() {
        let source = indoc::indoc! {r#"
            #[derive(Show, Hash)]
            pub enum Node { Link(Array[Node]) }
            pub fn inspect(value: Node) (Bool, String, BigInt) {
                (value == value, show(value), hash(value))
            }
        "#};
        let emitted = emit_solved(source);
        assert!(!emitted.contains("$self"), "{emitted}");
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
                let value = Token::Token
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
                let values = [Token::Token]
                values[next_index()] += Token::Token
                values[0]
            }
        "#};
    }
}
