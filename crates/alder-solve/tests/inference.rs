//! End-to-end Alder inference tests: parse → canonicalize → constrain → solve.

use alder_ast::{Annotation, FieldPresence, Kind, ModuleId, PackageId, RowExtension, Type};
use alder_can::{Annotations, Context};
use alder_constrain::{Error, ErrorKind};
use alder_region::Located;
use bumpalo::Bump;
use indoc::indoc;

fn solve_input<'a>(
    bump: &'a Bump,
    input: &str,
) -> Result<alder_solve::SolveOutput<'a>, Vec<alder_solve::SolveError<'a>>> {
    let src = bump.alloc_str(input);
    let parsed = alder_parse::parse_module(bump, src).expect("source parses");
    let canonical = alder_can::canonicalize(
        bump,
        Context {
            home: ModuleId {
                package: PackageId::Application,
                path: &["Main"],
            },
            imports: &[],
            interfaces: &[],
        },
        &parsed,
    )
    .expect("source canonicalizes");
    let constraints = alder_constrain::constrain(bump, canonical.module);
    let database = alder_solve::TraitDatabase::build(bump, canonical.module, &[]);
    alder_solve::solve(bump, &constraints, &database)
}

fn infer<'a>(bump: &'a Bump, input: &str) -> Result<Annotations<'a>, Vec<Error>> {
    let src = bump.alloc_str(input);
    let module = alder_parse::parse_module(bump, src).expect("source parses");
    let can_result = alder_can::canonicalize(
        bump,
        Context {
            home: ModuleId {
                package: PackageId::Application,
                path: &["Main"],
            },
            imports: &[],
            interfaces: &[],
        },
        &module,
    )
    .expect("source canonicalizes");
    let constraints = alder_constrain::constrain(bump, can_result.module);
    alder_solve::run(bump, &constraints)
}

fn render_annotations(annotations: &Annotations<'_>) -> String {
    annotations
        .iter()
        .map(|(name, annotation)| format!("{}: {}", name.name, render_annotation(annotation)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_annotation(annotation: &Annotation<'_>) -> String {
    let typ = render_type(annotation.typ);
    if annotation.params.is_empty() {
        typ
    } else {
        format!(
            "forall {}. {typ}",
            annotation
                .params
                .iter()
                .map(|param| param.name.value)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn render_type(typ: &Located<Type<'_>>) -> String {
    match &typ.value {
        Type::Var { name, args: [] } => (*name).to_owned(),
        Type::Var { name, args } => format!(
            "{}[{}]",
            name,
            args.iter()
                .map(|arg| render_type(arg))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Named {
            reference,
            args: [],
        } => reference.name.to_owned(),
        Type::Named { reference, args } => format!(
            "{}[{}]",
            reference.name,
            args.iter()
                .map(|arg| render_type(arg))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Partial { constructor, slots } => format!(
            "{}[{}]",
            constructor.name,
            slots
                .iter()
                .map(|slot| match slot {
                    alder_ast::TypeSlot::Hole(_) => "_".to_owned(),
                    alder_ast::TypeSlot::Fixed(typ) => render_type(typ),
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Projection(projection) => format!(
            "{}[{}]::{}",
            projection.trait_ref.trait_.0.name,
            projection
                .trait_ref
                .args
                .iter()
                .map(|arg| render_type(arg))
                .collect::<Vec<_>>()
                .join(", "),
            projection.assoc.name
        ),
        Type::Fn { params, ret } => format!(
            "fn({}) -> {}",
            params
                .iter()
                .map(|param| render_type(param))
                .collect::<Vec<_>>()
                .join(", "),
            render_type(ret)
        ),
        Type::Unit => "()".to_owned(),
        Type::Tuple(items) => format!(
            "({})",
            items
                .iter()
                .map(|item| render_type(item))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Record { fields, ext } => {
            let fields = fields
                .iter()
                .map(|field| {
                    format!(
                        "{}{}: {}",
                        field.name,
                        if field.presence == FieldPresence::Optional {
                            "?"
                        } else {
                            ""
                        },
                        render_type(field.typ)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            match ext {
                RowExtension::Closed => format!("{{ {fields} }}"),
                RowExtension::Open(row) => format!("{{ {fields} | {row} }}"),
            }
        }
        Type::ErrorRow { .. } => "[:_ | e]".to_owned(),
        Type::Alias { reference, .. } => reference.name.to_owned(),
    }
}

#[test]
fn direct_trait_method_selects_the_unique_impl() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            impl Show[Number] { fn show(value: Number) -> String { "number" } }
            fn render() -> String { show(1) }
        "#},
    )
    .expect("trait obligation resolves");
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Reference {
            dictionaries,
            method: Some(method),
        } if method.name == "show"
            && matches!(dictionaries.as_slice(), [alder_solve::Evidence::Impl { .. }])
    )));
}

#[test]
fn declared_bound_supplies_trait_method_evidence() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            fn describe(value: a) -> String where a: Show { show(value) }
        "#},
    )
    .expect("declared bound resolves");
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Reference { dictionaries, .. }
            if matches!(dictionaries.as_slice(), [alder_solve::Evidence::Param(0)])
    )));
}

#[test]
fn implementation_body_uses_its_current_dictionary() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            impl Show[Number] {
                fn show(value: Number) -> String { show(value) }
            }
        "#},
    )
    .expect("recursive method dispatch uses the current dictionary");
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Reference {
            dictionaries,
            method: Some(_),
        } if matches!(dictionaries.as_slice(), [alder_solve::Evidence::SelfDictionary])
    )));
}

#[test]
fn implementation_prerequisite_is_available_to_method_bodies() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            impl Show[Array[a]] where a: Show {
                fn show(values: Array[a]) -> String { show(values[0]) }
            }
        "#},
    )
    .expect("the factory prerequisite is in the method evidence scope");
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Reference {
            dictionaries,
            method: Some(_),
        } if matches!(dictionaries.as_slice(), [alder_solve::Evidence::Param(0)])
    )));
}

#[test]
fn default_body_can_dispatch_through_its_current_dictionary() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] {
                fn show(value: a) -> String
                fn render(value: a) -> String { show(value) }
            }
        "#},
    )
    .expect("default methods receive the current dictionary");
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Reference {
            dictionaries,
            method: Some(method),
        } if method.name == "show"
            && matches!(dictionaries.as_slice(), [alder_solve::Evidence::SelfDictionary])
    )));
}

#[test]
fn default_body_can_use_a_superclass_dictionary() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            trait Equal[a] { fn equal(left: a, right: a) -> Bool }
            trait Ordered[a] where a: Equal {
                fn compare(left: a, right: a) -> Number
                fn same(left: a, right: a) -> Bool { equal(left, right) }
            }
        "#},
    )
    .expect("default methods receive direct superclass slots");
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Reference {
            dictionaries,
            method: Some(method),
        } if method.name == "equal"
            && matches!(dictionaries.as_slice(), [alder_solve::Evidence::Super(0)])
    )));
}

#[test]
fn declared_bounds_are_preserved_in_the_binding_abi() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            fn describe(value: a) -> String where a: Show { show(value) }
        "#},
    )
    .expect("declared bound resolves");
    let (name, binding) = solved
        .bindings
        .iter()
        .find(|(name, _)| name.name == "describe")
        .expect("describe has elaboration metadata");
    assert_eq!(binding.abi, alder_solve::BindingAbi::DirectFunction);
    assert_eq!(binding.dictionary_params.len(), 1);
    assert_eq!(binding.dictionary_params[0].trait_.0.name, "Show");
    assert_eq!(solved.schemes[name].trait_predicates.len(), 1);
}

#[test]
fn constrained_binding_references_instantiate_their_predicates() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            impl Show[Number] { fn show(value: Number) -> String { "number" } }
            fn describe(value: a) -> String where a: Show { show(value) }
            fn render() -> String { describe(1) }
        "#},
    )
    .expect("the constrained callee selects its dictionary");
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::DirectCall {
            dictionaries,
            target: Some(alder_solve::DirectTarget::Binding(name)),
            ..
        } if matches!(dictionaries.as_slice(), [alder_solve::Evidence::Impl { .. }])
            && name.name == "describe"
    )));
}

#[test]
fn missing_trait_instance_is_structured() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            impl Show[Number] { fn show(value: Number) -> String { "number" } }
            fn render() -> String { show("nope") }
        "#},
    )
    .expect_err("missing instance must fail");
    assert!(matches!(
        &errors[0],
        alder_solve::SolveError::Trait(alder_solve::SolveTraitError::MissingInstance {
            trait_, subject, ..
        }) if trait_.0.name == "Show" && *subject == "String"
    ));
}

#[test]
fn associated_equality_normalizes_a_generic_method_result() {
    let bump = Bump::new();
    let output = solve_input(
        &bump,
        indoc! {r#"
            trait Iterator[i] {
                type Item
                fn next(value: i) -> Item
            }
            fn increment(value: i) -> Number
                where i: Iterator, i.Item == Number
            {
                next(value) + 1
            }
        "#},
    )
    .expect("the declared projection equality should normalize Item to Number");
    let increment = output
        .schemes
        .iter()
        .find(|(name, _)| name.name == "increment")
        .expect("increment has an inferred scheme")
        .1;
    assert_eq!(increment.projection_equalities.len(), 1);
    assert_eq!(
        increment.projection_equalities[0].projection.assoc.name,
        "Item"
    );
}

#[test]
fn an_impl_binding_normalizes_a_concrete_method_result() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            enum Counter { Counter }
            trait Iterator[i] {
                type Item
                fn next(value: i) -> Item
            }
            impl Iterator[Counter] {
                type Item = Number
                fn next(value: Counter) -> Number { 1 }
            }
            fn increment(value: Counter) -> Number { next(value) + 1 }
        "#},
    )
    .expect("the selected impl should normalize Item to Number");
}

#[test]
fn impl_method_must_match_the_substituted_associated_type() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            enum Counter { Counter }
            trait Iterator[i] {
                type Item
                fn next(value: i) -> Item
            }
            impl Iterator[Counter] {
                type Item = Number
                fn next(value: Counter) -> String { "wrong" }
            }
        "#},
    )
    .expect_err("the method result must equal the impl's Item binding");
    assert!(matches!(
        &errors[0],
        alder_solve::SolveError::Core(Error {
            kind: ErrorKind::Mismatch { actual, expected },
            ..
        }) if (actual == "String" && expected == "Number")
            || (actual == "Number" && expected == "String")
    ));
}

#[test]
fn trait_method_projection_equalities_are_instantiated_at_use_sites() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            trait NumericIterator[i] {
                type Item
                fn next(value: i) -> Item where i.Item == Number
            }
            fn increment(value: i) -> Number where i: NumericIterator {
                next(value) + 1
            }
        "#},
    )
    .expect("method scheme equalities should remain active after instantiation");
}

#[test]
fn overlapping_trait_instances_are_rejected_before_search() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            impl Show[Number] { fn show(value: Number) -> String { "one" } }
            impl Show[Number] { fn show(value: Number) -> String { "two" } }
            fn render() -> String { show(1) }
        "#},
    )
    .expect_err("overlapping candidates must fail coherence");
    assert!(matches!(
        &errors[0],
        alder_solve::SolveError::Coherence(alder_solve::CoherenceError::OverlappingImpl {
            trait_, ..
        }) if trait_.0.name == "Show"
    ));
}

#[test]
fn foreign_trait_for_foreign_subject_is_an_orphan() {
    let bump = Bump::new();
    let foreign_module = ModuleId {
        package: PackageId::Named(alder_ast::PackageName {
            author: "vendor",
            project: "traits",
        }),
        path: &["Foreign"],
    };
    let trait_id = alder_ast::TraitId(alder_ast::QualifiedName {
        module: foreign_module,
        name: "ForeignEq",
    });
    let interface = alder_ast::Interface {
        home: foreign_module,
        values: &[],
        types: &[],
        enums: &[],
        traits: bump.alloc_slice_copy(&[alder_ast::InterfaceTrait {
            exported_as: "ForeignEq",
            id: trait_id,
            params: bump.alloc_slice_copy(&[alder_ast::TypeParam {
                name: Located::at_zero("a"),
                kind: Kind::Type,
            }]),
            superclasses: &[],
            associated_types: &[],
            methods: &[],
        }]),
        instances: &[],
        modules: &[],
        private_names: &[],
    };
    let resolved_import = alder_ast::ResolvedImport {
        module: foreign_module,
        region: alder_region::Region::zero(),
        visibility: alder_ast::Visibility::Private,
        kind: alder_ast::ResolvedImportKind::All,
    };
    let interfaces = bump.alloc_slice_copy(&[interface]);
    let source = bump.alloc_str("impl ForeignEq[Number] {}");
    let parsed = alder_parse::parse_module(&bump, source).expect("source parses");
    let canonical = alder_can::canonicalize(
        &bump,
        Context {
            home: ModuleId {
                package: PackageId::Application,
                path: &["Main"],
            },
            imports: bump.alloc_slice_copy(&[resolved_import]),
            interfaces,
        },
        &parsed,
    )
    .expect("source canonicalizes");
    let constraints = alder_constrain::constrain(&bump, canonical.module);
    let database = alder_solve::TraitDatabase::build(&bump, canonical.module, interfaces);
    let errors = alder_solve::solve(&bump, &constraints, &database)
        .expect_err("a local module cannot own either side of this implementation");
    assert!(matches!(
        &errors[0],
        alder_solve::SolveError::Coherence(alder_solve::CoherenceError::OrphanImpl {
            trait_, subject, ..
        }) if trait_.0.name == "ForeignEq" && *subject == "Number"
    ));
}

#[test]
fn generic_and_concrete_heads_overlap() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            impl Show[a] { fn show(value: a) -> String { "any" } }
            impl Show[Number] { fn show(value: Number) -> String { "number" } }
        "#},
    )
    .expect_err("generic and concrete heads overlap");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Coherence(alder_solve::CoherenceError::OverlappingImpl {
            trait_, ..
        }) if trait_.0.name == "Show"
    )));
}

#[test]
fn non_decreasing_instance_prerequisite_is_rejected() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            impl Show[a] where a: Show { fn show(value: a) -> String { "loop" } }
        "#},
    )
    .expect_err("the prerequisite must be structurally smaller than the head");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Coherence(alder_solve::CoherenceError::InvalidTermination {
            prerequisite, ..
        }) if prerequisite.0.name == "Show"
    )));
}

#[test]
fn structurally_decreasing_container_instance_is_accepted() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) -> String }
            impl Show[Array[a]] where a: Show {
                fn show(value: Array[a]) -> String { "array" }
            }
        "#},
    )
    .expect("the element prerequisite is smaller than the container head");
}

#[test]
fn superclass_cycles_are_rejected() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            trait A[a] where a: B { fn a(value: a) -> a }
            trait B[a] where a: A { fn b(value: a) -> a }
        "#},
    )
    .expect_err("superclass graphs must be acyclic");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Coherence(alder_solve::CoherenceError::SuperclassCycle {
            traits,
        }) if traits.len() == 2
            && traits.iter().any(|trait_| trait_.0.name == "A")
            && traits.iter().any(|trait_| trait_.0.name == "B")
    )));
}

#[test]
fn numeric_operators_select_number_and_bigint_intrinsics() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            fn number() -> Number { 1 + 2 }
            fn bigint() -> BigInt { 1n + 2n }
        "#},
    )
    .expect("numeric instances resolve");
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Operator {
            dictionary: alder_solve::Evidence::Intrinsic(alder_solve::Intrinsic::NumNumber)
        }
    )));
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Operator {
            dictionary: alder_solve::Evidence::Intrinsic(alder_solve::Intrinsic::NumBigInt)
        }
    )));
}

#[test]
fn functions_have_no_structural_eq_instance() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            fn identity(value: a) -> a { value }
            fn bad() -> Bool { identity == identity }
        "#},
    )
    .expect_err("function equality must fail");
    assert!(matches!(
        &errors[0],
        alder_solve::SolveError::Trait(
            alder_solve::SolveTraitError::MissingInstance { trait_, .. }
                | alder_solve::SolveTraitError::UnsatisfiedBound { trait_, .. }
        ) if trait_.0.name == "Eq"
    ));
}

macro_rules! assert_inference_snapshot {
    ($source:expr) => {{
        let source = indoc!($source);
        let bump = Bump::new();
        let annotations = infer(&bump, source).expect("inference succeeds");
        insta::with_settings!({ description => source, omit_expression => true }, {
            insta::assert_snapshot!(render_annotations(&annotations));
        });
    }};
}

macro_rules! assert_inference_error_snapshot {
    ($source:expr) => {{
        let source = indoc!($source);
        let bump = Bump::new();
        let errors = infer(&bump, source).expect_err("inference fails");
        insta::with_settings!({ description => source, omit_expression => true }, {
            insta::assert_debug_snapshot!(errors);
        });
    }};
}

#[test]
fn polymorphic_identity() {
    assert_inference_snapshot!("fn identity(value) { value }");
}

#[test]
fn dependency_scc_generalizes_before_earlier_source_use() {
    let bump = Bump::new();
    let annotations = infer(
        &bump,
        indoc! {r#"
            fn pair() { (identity(1), identity("x")) }
            fn identity(value) { value }
        "#},
    )
    .unwrap();

    assert_eq!(
        render_annotations(&annotations),
        "identity: forall a. fn(a) -> a\npair: fn() -> (Number, String)"
    );
}

#[test]
fn mutually_recursive_scc_is_unified_before_generalization() {
    let bump = Bump::new();
    let annotations = infer(
        &bump,
        indoc! {r#"
            fn first(value) { second(value) }
            fn second(value) { first(value) }
        "#},
    )
    .unwrap();

    assert_eq!(
        render_annotations(&annotations),
        "first: forall a, b. fn(a) -> b\nsecond: forall a, b. fn(a) -> b"
    );
}

#[test]
fn mutually_recursive_calls_receive_preseeded_dictionary_arguments() {
    let bump = Bump::new();
    let output = solve_input(
        &bump,
        indoc! {r#"
            trait Display[a] { fn display(value: a) -> String }
            impl Display[Number] {
                fn display(value: Number) -> String { "number" }
            }
            fn first(value: a) -> String where a: Display { second(value) }
            fn second(value: a) -> String where a: Display {
                if true { display(value) } else { first(value) }
            }
            fn main() -> String { first(1) }
        "#},
    )
    .expect("recursive peers should see each other's declared predicates");
    assert!(output.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::DirectCall {
            dictionaries,
            target: Some(alder_solve::DirectTarget::Binding(name)),
            ..
        } if name.name == "second" && dictionaries.len() == 1
    )));
}

#[test]
fn higher_kinded_application_is_preserved_and_specialized() {
    let bump = Bump::new();
    let annotations = infer(
        &bump,
        indoc! {r#"
            fn adapt(value: f[a]) -> f[a] { value }
            fn specialize(value: Result[Number, String]) { adapt(value) }
        "#},
    )
    .unwrap();

    assert_eq!(
        render_annotations(&annotations),
        concat!(
            "adapt: forall a, b. fn(a[b]) -> a[b]\n",
            "specialize: fn(Result[Number, String]) -> Result[Number, String]"
        )
    );
    let adapt = annotations
        .iter()
        .find_map(|(name, annotation)| (name.name == "adapt").then_some(*annotation))
        .unwrap();
    assert!(matches!(adapt.params[0].kind, Kind::Arrow { .. }));
    assert!(matches!(adapt.params[1].kind, Kind::Type));
}

#[test]
fn repeated_higher_kinded_pattern_argument_is_rejected() {
    let bump = Bump::new();
    let errors = infer(
        &bump,
        indoc! {r#"
            fn adapt(value: f[a, a]) -> f[a, a] { value }
            fn specialize(value: Result[Number, Number]) { adapt(value) }
        "#},
    )
    .unwrap_err();

    assert!(matches!(
        errors.as_slice(),
        [Error {
            kind: ErrorKind::UnsupportedHigherKindedUnification,
            ..
        }]
    ));
}

#[test]
fn arbitrary_tuple_and_array() {
    assert_inference_snapshot!("let values = [(1, true, \"three\")]");
}

#[test]
fn block_and_sequential_let() {
    assert_inference_snapshot!(
        r#"
        fn answer() {
            let value = 40
            value + 2
        }
    "#
    );
}

#[test]
fn placeholder_lambda() {
    assert_inference_snapshot!("fn add(x, y) { x + y }\nlet increment = add(1, _)");
}

#[test]
fn optional_record_field_annotation() {
    assert_inference_snapshot!("fn name(user: { name?: String }) { user.name }");
}

#[test]
fn mismatch_reports_new_type_syntax() {
    assert_inference_error_snapshot!("fn bad() -> Number { \"nope\" }");
}

#[test]
fn mutable_loop_and_assignment() {
    assert_inference_snapshot!(
        r#"
        fn sum(values: Array[Number]) -> Number {
            let mut total = 0
            for value in values {
                total += value
            }
            total
        }
    "#
    );
}

#[test]
fn explicit_return_unifies_with_declared_result() {
    assert_inference_snapshot!(
        r#"
        fn choose(flag: Bool) -> Number {
            if flag { return 1 }
            return 2
        }
    "#
    );
}

#[test]
fn nested_optional_record_rows() {
    assert_inference_snapshot!(
        r#"
        fn display(user: { id: Number, name?: String, profile: { bio?: String, active: Bool, score?: Number } }) {
            (user.name, user.profile.bio, user.profile.active)
        }
    "#
    );
}

#[test]
fn try_unwraps_result_value() {
    assert_inference_snapshot!(
        r#"
        fn unwrap(value: Result[Number, String]) -> Result[Number, String] {
            Result.ok(value? + 1)
        }
    "#
    );
}

#[test]
fn await_unwraps_task_inside_task_function() {
    assert_inference_snapshot!(
        r#"
        fn wait() -> Task[()] {
            Task.sleep(1).await
        }
    "#
    );
}

#[test]
fn constructor_call_arity_is_checked() {
    assert_inference_error_snapshot!(
        "enum Maybe[a] { Just(a) }\nfn invalid() { Maybe::Just(1, 2) }"
    );
}
