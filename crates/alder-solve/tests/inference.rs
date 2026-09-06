//! End-to-end Alder inference tests: parse → canonicalize → constrain → solve.

use alder_ast::{Annotation, Kind, ModuleId, PackageId, RowExtension, Type};
use alder_can::{Annotations, Context};
use alder_constrain::{DiagnosticType, Error, ErrorKind};
use alder_region::Located;
use bumpalo::Bump;
use indoc::indoc;

#[test]
fn coalesce_unwraps_exactly_one_option_layer() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn with_default(value: Option[a], fallback: a) a { value ?? fallback }
        fn inferred(value, fallback) { value ?? fallback }
        pub fn number() Number { inferred(Some(42), 0) }
        pub fn text() String { inferred(None, "fallback") }
        pub fn nested(value: Option[Option[Number]]) Option[Number] {
            with_default(value, Some(42))
        }
        pub fn unit(value: Option[()]) () { value ?? () }
    "#},
    )
    .expect("coalesce returns the payload type, not the Option wrapper");
}

#[test]
fn coalesce_requires_an_option_and_a_matching_payload_default() {
    for source in [
        "pub fn invalid() Number { 42 ?? 0 }",
        "pub fn invalid(value: Option[Number]) String { value ?? \"wrong\" }",
        "pub fn invalid(value: Option[Number]) Option[Number] { value ?? Some(0) }",
    ] {
        let bump = Bump::new();
        assert!(solve_input(&bump, source).is_err(), "{source}");
    }
}

#[test]
fn structural_error_hash_rejects_payloads_without_hash() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn fingerprint(value: [:callback(fn(Number) Number)]) BigInt { hash(value) }
    "#}
        )
        .is_err()
    );
}

#[test]
fn structural_error_rows_do_not_provide_ord() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        error Failure { :later, :first(Number) }
        fn order(left: Failure, right: Failure) Bool { left < right }
    "#}
        )
        .is_err()
    );
}

#[test]
fn structural_error_hash_requires_payload_hash_without_group_derives() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        error First { :bad(Number), :missing }
        error Second { :missing, :bad(Number) }
        fn first(value: First) BigInt { hash(value) }
        fn second(value: Second) BigInt { hash(value) }
        fn literal(value: [:bad(Number)]) BigInt { hash(value) }
    "#},
    )
    .expect("error hashing must depend on structural payload capabilities, not nominal derives");
}

#[test]
fn direct_error_group_annotations_share_structural_identity() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        error First { :bad(Number), :missing }
        error Second { :missing, :bad(Number) }
        fn relay(value: First) Second { value }
        fn render(value: First) String { show(value) }
        fn encode(value: Second) String { Json.encode(value) }
        type Alias = First
        fn array(values: Array[Alias]) Array[Second] { values }
        fn record(value: { failure: First }) ({ failure: Second }) { value }
    "#},
    )
    .expect("named error groups are structural aliases outside Result annotations too");
}

#[test]
fn direct_error_group_annotations_reject_incompatible_payloads() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        error First { :bad(Number) }
        error Second { :bad(String) }
        fn relay(value: First) Second { value }
    "#}
        )
        .is_err()
    );
}

#[test]
fn structural_error_json_requires_only_payload_codecs() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        error First { :bad(Number), :missing }
        error Second { :missing, :bad(Number) }
        fn first(value: Result[Number, First]) String { Json.encode(value) }
        fn second(text: String) Result[Result[Number, Second], [:invalid_json(String)]] {
            Json.decode(text)
        }
    "#},
    )
    .expect("equivalent closed error rows share structural Json capabilities");
}

#[test]
fn structural_error_json_rejects_payloads_without_codecs() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn encode(value: Result[Number, [:callback(fn(Number) Number)]]) String {
            Json.encode(value)
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn structural_error_show_does_not_depend_on_group_names_or_derives() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        error First { :bad(Number), :missing }
        error Second { :missing, :bad(Number) }
        fn first(value: Result[Number, First]) String { show(value) }
        fn second(value: Result[Number, Second]) String { show(value) }
        fn literal(value: Result[Number, [:missing | :bad(Number)]]) String { show(value) }
    "#},
    )
    .expect("closed error rows provide Show from their payload capabilities");
}

#[test]
fn structural_error_show_requires_payload_show() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        error Failed { :callback(fn(Number) Number) }
        fn render(value: Result[Number, Failed]) String { show(value) }
    "#}
        )
        .is_err()
    );
}

#[test]
fn qualified_builtin_aliases_check_their_structural_payload() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn defaults() Fiber::MapOptions { {} }
        fn bounded() Fiber::MapOptions { { concurrency: 8 } }
        fn read(options: Fiber::MapOptions) Option[Number] { options.concurrency }
    "#},
    )
    .expect("builtin alias must expand in caller annotations");
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn invalid() Fiber::MapOptions { { concurrency: "eight" } }
    "#}
        )
        .is_err()
    );
}

#[test]
fn sparse_tuple_constraints_do_not_generalize_captured_arrays() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        let shared = []
        fn expose(value) {
            value.0 = shared
            value
        }
        fn numbers() {
            let pair = expose(([], ()))
            Array.push(pair.0, 42)
        }
        fn strings() {
            let pair = expose(([], ()))
            let invalid: Array[String] = pair.0
            invalid
        }
    "#}
        )
        .is_err(),
        "the sparse element relation must retain shared mutable payload types"
    );
}

#[test]
fn tuple_projection_maximum_index_is_stored_sparsely() {
    let bump = Bump::new();
    let solved = solve_input(&bump, "pub fn last(value) { value.4294967295 }").unwrap();
    let annotation = solved.annotations.values().next().unwrap();
    assert_eq!(annotation.tuple_shapes.len(), 1);
    let shape = &annotation.tuple_shapes[0];
    assert_eq!(shape.length, u64::from(u32::MAX) + 1);
    assert_eq!(shape.elements.len(), 1);
    assert_eq!(shape.elements[0].0, u32::MAX);
    assert_eq!(
        annotation.params.len(),
        2,
        "only operand and projected element are quantified"
    );
}

#[test]
fn open_spread_defaults_are_preserved_in_generic_call_constraints() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            type Config = { value: Option[Number] }
            fn copy(record) Config { { ..record } }
            fn check() {
                let absent: Config = copy({})
                let present: Config = copy({ value: Some(42) })
            }
        "#},
    )
    .expect("a fresh spread supplies None without requiring it from its input");
}

#[test]
fn open_spread_defaults_use_later_callback_context() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            type Config = { value: Option[Number] }
            fn apply(value: a, callback: fn(a) b) b { callback(value) }
            fn identity(record: Config) Config { record }
            fn copy(record) Config { apply({ ..record }, identity) }
            fn check() Config { copy({}) }
        "#},
    )
    .expect("open spreads retain fresh-construction context from later arguments");
}

#[test]
fn open_spread_defaults_do_not_hide_incompatible_supplied_fields() {
    for source in [
        indoc! {r#"
            type Config = { value: Option[Number] }
            fn copy(record) Config { { ..record } }
            fn invalid() { copy({ value: Some("wrong") }) }
        "#},
        indoc! {r#"
            type Config = { value: Option[Number] }
            fn copy(record) Config { { ..record } }
            fn invalid() {
                let record = { value: 42 }
                copy(record)
            }
        "#},
        indoc! {r#"
            type Config = { required: Number, value: Option[Number] }
            fn copy(record) Config { { ..record } }
            fn invalid() { copy({}) }
        "#},
    ] {
        let bump = Bump::new();
        solve_input(&bump, source)
            .expect_err("defaults cannot replace a supplied field or invent a required field");
    }
}

#[test]
fn record_defaults_use_context_from_later_call_arguments() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn apply(value: a, callback: fn(a) b) b { callback(value) }
            fn read(record: { value: Option[Number] }) Option[Number] { record.value }
            fn check() Option[Number] { apply({}, read) }
            fn spread() Option[Number] { apply({ ..{} }, read) }
            fn branch(flag: Bool) Option[Number] { apply(if flag { {} } else { {} }, read) }
        "#},
    )
    .expect("a later callback supplies the fresh record's field context");
}

#[test]
fn late_record_context_cannot_invent_required_fields_or_convert_aliases() {
    for source in [
        indoc! {r#"
            fn apply(value: a, callback: fn(a) b) b { callback(value) }
            fn read(record: { value: Number }) Number { record.value }
            fn invalid() Number { apply({}, read) }
        "#},
        indoc! {r#"
            fn apply(value: a, callback: fn(a) b) b { callback(value) }
            fn read(record: { value: Option[Number] }) Option[Number] { record.value }
            fn invalid() Option[Number] {
                let record = {}
                apply(record, read)
            }
        "#},
        indoc! {r#"
            fn empty() { {} }
            fn read(record: { value: Option[Number] }) Option[Number] { record.value }
            fn invalid() Option[Number] { read(empty()) }
        "#},
    ] {
        let bump = Bump::new();
        let errors = solve_input(&bump, source)
            .expect_err("only a fresh contextual literal may acquire Option defaults");
        assert!(
            errors.iter().any(|error| matches!(
                error,
                alder_solve::SolveError::Core(Error {
                    kind: ErrorKind::RecordFieldsMismatch { .. },
                    ..
                })
            )),
            "{errors:?}"
        );
    }
}

#[test]
fn omitted_generic_record_field_constrains_its_type_to_option() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn choose(record: { value: a }, fallback: a) a { fallback }
            fn check() Option[Number] { choose({}, Some(42)) }
            fn spread() Option[Number] { choose({ ..{} }, Some(42)) }
            fn reverse(fallback: a, record: { value: a }) a { record.value }
            fn reverse_check() Option[Number] { reverse(Some(42), {}) }
        "#},
    )
    .expect("omission supplies None even when the field payload is inferred later");
}

#[test]
fn omitted_generic_record_fields_cannot_satisfy_universal_or_concrete_payloads() {
    for source in [
        indoc! {r#"
            type Record[a] = { value: a }
            pub fn invalid() Record[a] { {} }
        "#},
        indoc! {r#"
            fn choose(record: { value: a }, fallback: a) a { record.value }
            fn invalid() Number { choose({}, 42) }
        "#},
    ] {
        let bump = Bump::new();
        let errors = solve_input(&bump, source)
            .expect_err("None cannot implement an arbitrary or non-Option field type");
        assert!(
            errors.iter().any(|error| matches!(
                error,
                alder_solve::SolveError::Core(Error {
                    kind: ErrorKind::GenericSpecialization { .. } | ErrorKind::Mismatch { .. },
                    ..
                })
            )),
            "{errors:?}"
        );
    }
}

#[test]
fn explicit_option_record_fields_accept_omission_like_shorthand() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn read(record: { value: Option[Number] }) Option[Number] { record.value }
            fn shorthand(record: { value?: Number }) Option[Number] { read(record) }
            fn check() {
                let absent: { value: Option[Number] } = {}
                let explicit: { value?: Number } = { value: None }
                let first: Option[Number] = read({})
                let second: Option[Number] = shorthand(absent)
                let third: Option[Number] = read(explicit)
            }
        "#},
    )
    .expect("optional record spelling must not create a separate field-presence type");
}

#[test]
fn sparse_tuple_shapes_reject_transitive_element_cycles() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        fn cycle(first, second) {
            first.0 = second
            second.0 = first
        }
    "#},
    )
    .expect_err("finite tuple types cannot contain each other recursively");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::InfiniteType { .. },
                ..
            })
        )),
        "{errors:?}"
    );
}

#[test]
fn contextual_block_preserves_its_enclosing_return_boundary() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn answer() Number {
            let unused: String = { return 42 }
            0
        }
    "#},
    )
    .expect("an exiting block does not need a value of the initializer type");
}

#[test]
fn contextual_block_rejects_reachable_unit_fallthrough() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn answer(flag: Bool) Number {
            let value: String = { if flag { return 42 } }
            0
        }
    "#}
        )
        .is_err(),
        "a reachable unit block cannot initialize a String"
    );
}

#[test]
fn singleton_error_constructor_keeps_context_through_block() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        error Failure { :missing, :failed(Number) }
        fn make() {
            let failure: Result[Number, Failure] = {
                let code = 7
                Err(:failed(code))
            }
            failure
        }
    "#},
    )
    .expect("a block preserves contextual construction at its result expression");
}

#[test]
fn singleton_error_constructor_keeps_context_through_branches() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        error Failure { :missing, :failed(Number), :cancelled }
        fn make(flag: Bool) {
            let failure: Result[Number, Failure] = if flag {
                Err(:failed(7))
            } else {
                Err(:missing)
            }
            failure
        }
    "#},
    )
    .expect("branches preserve contextual construction without requiring every tag");
}

#[test]
fn singleton_error_constructor_fills_named_multi_tag_row() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        error Failure { :missing, :failed(Number) }
        fn make() {
            let failure: Result[Number, Failure] = Err(:failed(7))
            failure
        }
    "#},
    )
    .expect("constructing one permitted tag satisfies the declared error row");
}

#[test]
fn singleton_error_constructors_fill_contextual_arrays_and_arguments() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        error Failure { :missing, :failed(Number) }
        fn accept(value: Result[Number, Failure]) { () }
        fn make() {
            let failures: Array[Result[Number, Failure]] = [
                Err(:missing), Result.err(:failed(7)),
            ]
            accept(Err(:failed(8)))
            failures
        }
    "#},
    )
    .expect("fresh error constructors use the expected row in nested contexts");
}

#[test]
fn singleton_error_constructor_rejects_unlisted_tag() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        error Failure { :missing, :failed(Number) }
        fn make() {
            let failure: Result[Number, Failure] = Err(:other)
            failure
        }
    "#}
        )
        .is_err(),
        "contextual construction must not admit an unlisted error tag"
    );
}

#[test]
fn singleton_error_constructor_rejects_wrong_payload() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        error Failure { :missing, :failed(Number) }
        fn make() {
            let failure: Result[Number, Failure] = Err(:failed("wrong"))
            failure
        }
    "#}
        )
        .is_err(),
        "contextual construction preserves the tag payload contract"
    );
}

#[test]
fn result_instance_heads_ignore_error_group_names() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            error First { :failed(Number), :missing }
            error Second { :missing, :failed(Number) }
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[Result[Number, First]] {}
            impl Marker[Result[Number, Second]] {}
        "#},
    )
    .expect_err("equivalent structural error rows make the Result heads overlap");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Coherence(alder_solve::CoherenceError::OverlappingImpl { .. })
        )),
        "{errors:?}"
    );
}

#[test]
fn partial_result_instance_heads_expand_error_group_aliases() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        error Failure { :failed(Number), :missing }
        type Alias = Failure
        trait Marker[f] { fn pass(value: f[a]) f[a] { value } }
        impl Marker[Result[_, Alias]] {}
        impl Marker[Result[_, [:missing | :failed(Number)]]] {}
    "#},
    )
    .expect_err("a named error row and its structural spelling overlap");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Coherence(alder_solve::CoherenceError::OverlappingImpl { .. })
        )),
        "{errors:?}"
    );
}

#[test]
fn result_instance_heads_preserve_distinct_error_payloads() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        error First { :failed(Number) }
        error Second { :failed(String) }
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[Result[Number, First]] {}
        impl Marker[Result[Number, Second]] {}
    "#},
    )
    .expect("different error payload contracts keep these heads disjoint");
}

#[test]
fn result_instance_selection_ignores_error_group_names() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        error First { :failed(Number), :missing }
        error Second { :missing, :failed(Number) }
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[Result[Number, First]] {}
        fn consume(value: Result[Number, Second]) Result[Number, Second] { pass(value) }
    "#},
    )
    .expect("structurally equivalent Result error rows select the same implementation");
}

#[test]
fn named_error_groups_reject_custom_trait_implementations() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        error Failure { :failed }
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[Failure] {}
    "#},
    )
    .expect_err("structural error-group aliases cannot own nominal custom implementations");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, alder_solve::SolveError::Coherence(_))),
        "{errors:?}"
    );
}

#[test]
fn named_error_group_aliases_reject_custom_trait_implementations() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        error Failure { :failed }
        type Alias = Failure
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[Alias] {}
    "#},
    )
    .expect_err("a transparent alias cannot hide the error-group implementation target");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Coherence(
                alder_solve::CoherenceError::NamedErrorGroupImpl { .. }
            )
        )),
        "{errors:?}"
    );
}

#[test]
fn nominal_wrappers_of_error_groups_can_have_custom_implementations() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        error Failure { :failed }
        enum Wrapper { Wrapper(Result[Number, Failure]) }
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[Wrapper] {}
    "#},
    )
    .expect("the wrapper is a distinct nominal type, not an error-group alias");
}

#[test]
fn higher_kinded_coherence_honors_later_explicit_section() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        enum Pair[a, b] { Pair(a, b) }
        trait Marker[a, f] { fn pass(value: a, other: fn(Number) f[Number]) a { value } }
        impl Marker[f[a], f] {}
        impl Marker[Pair[Number, String], Pair[Number, _]] {}
    "#},
    )
    .expect_err("an explicit section in a later argument establishes overlapping heads");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Coherence(alder_solve::CoherenceError::OverlappingImpl { .. })
        )),
        "{errors:?}"
    );
}

#[test]
fn higher_kinded_coherence_honors_earlier_explicit_section() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        enum Pair[a, b] { Pair(a, b) }
        trait Marker[f, a] { fn pass(value: a, other: fn(Number) f[Number]) a { value } }
        impl Marker[f, f[a]] {}
        impl Marker[Pair[Number, _], Pair[Number, String]] {}
    "#},
    )
    .expect_err("reordering trait arguments preserves the explicit section overlap");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Coherence(alder_solve::CoherenceError::OverlappingImpl { .. })
        )),
        "{errors:?}"
    );
}

#[test]
fn higher_kinded_coherence_rejects_incompatible_explicit_fixed_slot() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        enum Pair[a, b] { Pair(a, b) }
        trait Marker[a, f] { fn pass(value: a, other: fn(Number) f[Number]) a { value } }
        impl Marker[f[a], f] {}
        impl Marker[Pair[Bool, String], Pair[Number, _]] {}
    "#},
    )
    .expect("a fixed Number slot cannot also satisfy Bool");
}

#[test]
fn higher_kinded_instance_head_recovers_leftmost_result_section() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[Array[f[a]]] where f: Functor {}
        fn use_results(value: Array[Result[Number, [:failed]]]) { pass(value) }
    "#},
    )
    .expect("the constructor pattern recovers Result[_, [:failed]]");
}

#[test]
fn higher_kinded_instance_coherence_detects_leftmost_section_overlap() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[Array[f[a]]] {}
        impl Marker[Array[Result[Number, [:failed]]]] {}
    "#},
    )
    .expect_err("partial recovery makes these implementation heads overlap");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Coherence(alder_solve::CoherenceError::OverlappingImpl { .. })
        )),
        "{errors:?}"
    );
}

#[test]
fn higher_kinded_instance_coherence_preserves_recovered_fixed_slots() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[(f[a], f[b])] {}
        impl Marker[(Result[Number, [:left]], Result[String, [:right]])] {}
    "#},
    )
    .expect("a shared recovered constructor cannot have two incompatible fixed error rows");
}

#[test]
fn higher_kinded_instance_coherence_applies_recovered_section_again() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[(f[a], f[b])] {}
        impl Marker[(Result[Number, [:failed]], Result[String, [:failed]])] {}
    "#},
    )
    .expect_err("the recovered section must apply consistently to the second payload");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Coherence(alder_solve::CoherenceError::OverlappingImpl { .. })
        )),
        "{errors:?}"
    );
}

#[test]
fn higher_kinded_instance_head_matches_nested_constructor_application() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[Array[f[a]]] {}
        fn use_options(value: Array[Option[Number]]) { pass(value) }
        fn use_arrays(value: Array[Array[String]]) { pass(value) }
    "#},
    )
    .expect("applied constructor variables in instance heads must match actual applications");
}

#[test]
fn higher_kinded_instance_head_resolves_constructor_prerequisites() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[Array[f[a]]] where f: Functor {}
        fn use_options(value: Array[Option[Number]]) { pass(value) }
        fn use_arrays(value: Array[Array[String]]) { pass(value) }
    "#},
    )
    .expect("constructor bindings must be available to prerequisite dictionary selection");
}

#[test]
fn higher_kinded_instance_head_rejects_inconsistent_constructor_bindings() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[fn(f[a]) f[a]] {}
        fn invalid(value: fn(Option[Number]) Array[Number]) { pass(value) }
    "#},
    )
    .expect_err("the same constructor variable cannot mean both Option and Array");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, alder_solve::SolveError::Trait(_))),
        "{errors:?}"
    );
}

#[test]
fn function_implementation_dispatch_matches_parameters_and_result() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Invoke[a] { fn invoke(value: a) Number }
        impl Invoke[fn(Number) Number] {
            fn invoke(value: fn(Number) Number) Number { value(41) }
        }
        fn increment(value: Number) Number { value + 1 }
        fn use_function() Number { invoke(increment) }
    "#},
    )
    .expect("function type implementations must participate in dictionary selection");
}

#[test]
fn function_implementation_dispatch_rejects_mismatched_signatures() {
    for source in [
        indoc! {r#"
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[fn(Number) Number] {}
            fn invalid(value: fn(String) Number) { pass(value) }
        "#},
        indoc! {r#"
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[fn(Number) Number] {}
            fn invalid(value: fn(Number) String) { pass(value) }
        "#},
        indoc! {r#"
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[fn(Number) Number] {}
            fn invalid(value: fn(Number, Number) Number) { pass(value) }
        "#},
        indoc! {r#"
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[fn(a) a] {}
            fn invalid(value: fn(Number) String) { pass(value) }
        "#},
    ] {
        let bump = Bump::new();
        let errors = solve_input(&bump, source).expect_err("function signature does not match");
        assert!(
            errors
                .iter()
                .any(|error| matches!(error, alder_solve::SolveError::Trait(_))),
            "{errors:?}"
        );
    }
}

#[test]
fn function_implementation_dispatch_preserves_shared_type_parameters() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[fn(a) a] {}
        fn number(value: fn(Number) Number) { pass(value) }
        fn text(value: fn(String) String) { pass(value) }
    "#},
    )
    .expect("one generic function head selects independently at different payloads");
}

#[test]
fn function_implementation_dispatch_preserves_task_layers() {
    for source in [
        indoc! {r#"
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[fn(Number) Task[Number]] {}
            fn invalid(value: fn(Number) Number) { pass(value) }
        "#},
        indoc! {r#"
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[fn(Number) Task[Number]] {}
            fn invalid(value: fn(Number) Task[Task[Number]]) { pass(value) }
        "#},
    ] {
        let bump = Bump::new();
        let errors = solve_input(&bump, source)
            .expect_err("dictionary matching must not add or flatten Task layers");
        assert!(
            errors
                .iter()
                .any(|error| matches!(error, alder_solve::SolveError::Trait(_))),
            "{errors:?}"
        );
    }
}

#[test]
fn record_coherence_treats_shorthand_and_explicit_option_as_identical() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[{ value?: Number }] {}
            impl Marker[{ value: Option[Number] }] {}
        "#},
    )
    .expect_err("equivalent Option spellings cannot define separate instances");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, alder_solve::SolveError::Coherence(_)))
    );
}

#[test]
fn record_coherence_distinguishes_option_from_its_payload() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            trait Read[a] { fn read(record: a) Number }
            impl Read[{ value: Number }] {
                fn read(record: { value: Number }) Number { record.value }
            }
            impl Read[{ value?: Number }] {
                fn read(record: { value: Option[Number] }) Number {
                    match record.value { Some(value) => value, None => 0 }
                }
            }
            fn plain(record: { value: Number }) Number { read(record) }
            fn optional(record: { value: Option[Number] }) Number { read(record) }
        "#},
    )
    .expect("Number and Option[Number] are distinct ordinary field types");
}

#[test]
fn record_coherence_rejects_reordered_duplicate_fields() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[{ x: Number, y: String }] {}
        impl Marker[{ y: String, x: Number }] {}
    "#},
    )
    .expect_err("field declaration order cannot hide identical record instances");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, alder_solve::SolveError::Coherence(_)))
    );
}

#[test]
fn record_coherence_rejects_open_closed_overlap() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[{ r | x: Number }] {}
        impl Marker[{ x: Number, y: String }] {}
    "#},
    )
    .expect_err("an open record implementation also matches its closed extension");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, alder_solve::SolveError::Coherence(_)))
    );
}

#[test]
fn record_coherence_preserves_shared_tail_relationships() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a, b] { fn pass(value: a, other: b) a { value } }
        impl Marker[{ r | x: Number }, { r | y: String }] {}
        impl Marker[{ x: Number, extra: Bool }, { y: String, other: Bool }] {}
    "#},
    )
    .expect("one shared tail cannot simultaneously contain two different field sets");
}

#[test]
fn record_coherence_rejects_compatible_shared_tails() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a, b] { fn pass(value: a, other: b) a { value } }
        impl Marker[{ r | x: Number }, { r | y: String }] {}
        impl Marker[{ x: Number, extra: Bool }, { y: String, extra: Bool }] {}
    "#},
    )
    .expect_err("the same residual fields satisfy both occurrences of the shared tail");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, alder_solve::SolveError::Coherence(_)))
    );
}

#[test]
fn record_coherence_rejects_independent_open_overlap() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[{ r | x: Number }] {}
        impl Marker[{ s | y: String }] {}
    "#},
    )
    .expect_err("a record containing both required fields satisfies both implementations");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, alder_solve::SolveError::Coherence(_)))
    );
}

#[test]
fn record_coherence_accepts_disjoint_required_field_types() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[{ r | x: Number }] {}
        impl Marker[{ s | x: String }] {}
    "#},
    )
    .expect("incompatible required payloads keep record implementations disjoint");
}

#[test]
fn record_coherence_reuses_already_expanded_shared_tails() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a, b] { fn pass(value: a, other: b) a { value } }
        impl Marker[{ r | x: Number }, { r | y: String }] {}
        impl Marker[{ s | x: Number, extra: Bool }, { s | y: String, extra: Bool }] {}
    "#},
    )
    .expect_err("the second argument must reuse the first argument's residual row equality");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Coherence(alder_solve::CoherenceError::OverlappingImpl { .. })
        )),
        "{errors:?}"
    );
}

#[test]
fn record_implementation_dispatch_matches_open_required_fields() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] { fn pass(value: a) a { value } }
        impl Marker[{ r | x: Number }] {}
        fn use_record() { pass({ x: 42, label: "hello" }) }
    "#},
    )
    .expect("a valid open-record implementation must actually be callable");
}

#[test]
fn record_implementation_dispatch_rejects_incompatible_shapes() {
    for source in [
        indoc! {r#"
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[{ r | x: Number }] {}
            fn invalid() { pass({ x: "wrong" }) }
        "#},
        indoc! {r#"
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[{ r | x: Number }] {}
            fn invalid() { pass({ y: 42 }) }
        "#},
        indoc! {r#"
            trait Marker[a] { fn pass(value: a) a { value } }
            impl Marker[{ x: Number }] {}
            fn invalid() { pass({ x: 42, extra: true }) }
        "#},
    ] {
        let bump = Bump::new();
        let errors = solve_input(&bump, source).expect_err("no record instance matches this shape");
        assert!(
            errors
                .iter()
                .any(|error| matches!(error, alder_solve::SolveError::Trait(_))),
            "{errors:?}"
        );
    }
}

#[test]
fn record_implementation_dispatch_checks_shared_residual_rows() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a, b] { fn pass(value: a, other: b) a { value } }
        impl Marker[{ r | x: Number }, { r | y: Number }] {}
        fn valid() { pass({ x: 42, extra: true }, { y: 1, extra: true }) }
    "#},
    )
    .expect("matching residual fields satisfy the implementation");
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a, b] { fn pass(value: a, other: b) a { value } }
        impl Marker[{ r | x: Number }, { r | y: Number }] {}
        fn invalid() { pass({ x: 42, extra: true }, { y: 1, other: true }) }
    "#},
    )
    .expect_err("residual fields must agree across the two arguments");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, alder_solve::SolveError::Trait(_))),
        "{errors:?}"
    );
}

#[test]
fn record_implementation_cannot_read_optional_field_as_required() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Read[a] { fn read(value: a) Number }
        impl Read[{ value: Number }] {
            fn read(record: { value: Number }) Number { record.value }
        }
        fn invalid(record: { value?: Number }) Number { read(record) }
    "#},
    )
    .expect_err("required-field dictionary code cannot receive a possibly missing field");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, alder_solve::SolveError::Trait(_))),
        "{errors:?}"
    );
}

#[test]
fn record_implementation_matches_explicit_optional_field_types() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Read[a] { fn read(value: a) Option[Number] }
        impl Read[{ value?: Number }] {
            fn read(record: { value?: Number }) Option[Number] { record.value }
        }
        fn valid(record: { value?: Number }) Option[Number] { read(record) }
        fn explicit(record: { value: Option[Number] }) Option[Number] { read(record) }
    "#},
    )
    .expect("an explicitly optional record selects its matching implementation");
}

#[test]
fn option_try_propagates_in_option_returning_functions() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn read(value: Option[Number]) Option[Number] { Some(value? + 1) }
        async fn read_async(value: Option[Number]) Option[Number] { Some(value?) }
        fn inferred(value) { Some(value?) }
    "#},
    )
    .expect("Option propagation works with explicit and inferred boundaries");
}

#[test]
fn option_try_waits_for_mutually_recursive_return_constraints() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn first(value, again: Bool) {
            let item = value?
            second(Some(item), again)
        }
        fn second(value, again: Bool) {
            if again { first(value, false) } else { Some(value?) }
        }
        fn number() Option[Number] { first(Some(42), true) }
        fn text() Option[String] { first(Some("hello"), true) }
    "#},
    )
    .expect("recursive peers must constrain propagation before selecting its carrier");
}

#[test]
fn option_try_recursive_return_constraints_do_not_depend_on_declaration_order() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn second(value, again: Bool) {
            if again { first(value, false) } else { Some(value?) }
        }
        fn first(value, again: Bool) {
            let item = value?
            second(Some(item), again)
        }
        fn number() Option[Number] { first(Some(42), true) }
        fn text() Option[String] { first(Some("hello"), true) }
    "#},
    )
    .expect("reordering recursive declarations preserves Option inference");
}

#[test]
fn result_try_waits_for_mutually_recursive_return_constraints() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn first(value, again: Bool) {
            let item = value?
            second(Ok(item), again)
        }
        fn second(value, again: Bool) {
            if again { first(value, false) } else { Ok(value?) }
        }
        fn number() Result[Number] { first(Ok(42), true) }
        fn text() Result[String] { first(Ok("hello"), true) }
    "#},
    )
    .expect("deferring carrier selection preserves recursive Result inference");
}

#[test]
fn option_try_recursive_explicit_return_waits_for_peer() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn first(value, again: Bool) {
            let item = value?
            return second(Some(item), again)
        }
        fn second(value, again: Bool) {
            if again { first(value, false) } else { Some(value?) }
        }
        fn number() Option[Number] { first(Some(42), true) }
    "#},
    )
    .expect("an explicit recursive return must preserve deferred carrier selection");
}

#[test]
fn option_try_infers_from_explicit_return_paths() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn read(value, stop: Bool) {
            let item = value?
            if stop { return None }
            return Some(item)
        }
        fn lambda(value: Option[Number]) Option[Number] {
            let run = item -> {
                let found = item?
                return Some(found)
            }
            run(value)
        }
        async fn delayed(value) {
            let item = value?
            return Some(item)
        }
        fn number() Option[Number] { read(Some(42), false) }
        fn text() Task[Option[String]] { delayed(Some("hello")) }
    "#},
    )
    .expect("explicit exits establish Option in function, lambda, and async boundaries");
}

#[test]
fn option_try_does_not_hide_reachable_fallthrough() {
    for source in [
        "fn read(value: Option[Number]) Option[Number] { let item = value? }",
        "async fn read(value: Option[Number]) Option[Number] { let item = value? }",
        indoc! {r#"
            fn read(value: Option[Number], stop: Bool) Option[Number] {
                let item = value?
                if stop { return Some(item) }
            }
        "#},
    ] {
        let bump = Bump::new();
        assert!(
            solve_input(&bump, source).is_err(),
            "Some reaches an exit without an Option result: {source}"
        );
    }
}

#[test]
fn option_try_does_not_convert_to_result_or_plain_values() {
    for source in [
        "fn invalid(value: Option[Number]) Result[Number] { Ok(value?) }",
        "fn invalid(value: Result[Number]) Option[Number] { Some(value?) }",
        "fn invalid(value: Option[Number]) Number { value? }",
    ] {
        let bump = Bump::new();
        assert!(solve_input(&bump, source).is_err(), "must reject: {source}");
    }
}

#[test]
fn result_error_argument_rejects_ordinary_types_even_without_construction() {
    for source in [
        "fn identity(value: Result[Number, String]) { value }",
        "fn identity(value: Result[Number, Bool]) { value }",
        "fn identity(value: Result[Number, Array[String]]) { value }",
        "fn identity(value: Result[Number, (Number, String)]) { value }",
        "fn identity(value: Result[Number, { message: String }]) { value }",
    ] {
        let bump = Bump::new();
        assert!(
            solve_input(&bump, source).is_err(),
            "Result must require an error row even when no constructor is used: {source}"
        );
    }
}

#[test]
fn result_error_argument_rejects_fixed_partial_constructor_arguments() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
            trait Marker[f] { fn pass(value: f[a]) f[a] { value } }
            impl Marker[Result[_, String]] {}
        "#},
        )
        .is_err(),
        "a partial Result must validate its fixed error argument"
    );
}

#[test]
fn result_row_coherence_rejects_reordered_duplicate_instances() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[f] { fn pass(value: f[a]) f[a] { value } }
        impl Marker[Result[_, [:left(Number) | :right]]] {}
        impl Marker[Result[_, [:right | :left(Number)]]] {}
    "#},
    )
    .expect_err("tag order must not hide duplicate instances");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, alder_solve::SolveError::Coherence(_)))
    );
}

#[test]
fn result_row_coherence_rejects_open_and_closed_overlap() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[f] { fn pass(value: f[a]) f[a] { value } }
        impl Marker[Result[_, [:left | e]]] {}
        impl Marker[Result[_, [:left | :right]]] {}
    "#},
    )
    .expect_err("the open instance also matches the closed row");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, alder_solve::SolveError::Coherence(_)))
    );
}

#[test]
fn result_row_coherence_rejects_independent_open_overlap() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[f] { fn pass(value: f[a]) f[a] { value } }
        impl Marker[Result[_, [:left | e]]] {}
        impl Marker[Result[_, [:right | e]]] {}
    "#},
    )
    .expect_err("both rows can include left and right");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, alder_solve::SolveError::Coherence(_)))
    );
}

#[test]
fn result_row_coherence_accepts_disjoint_payloads() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[f] { fn pass(value: f[a]) f[a] { value } }
        impl Marker[Result[_, [:failed(Number) | e]]] {}
        impl Marker[Result[_, [:failed(String) | e]]] {}
    "#},
    )
    .expect("incompatible required payloads make the instances disjoint");
}

#[test]
fn result_row_coherence_accepts_disjoint_closed_tags() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[f] { fn pass(value: f[a]) f[a] { value } }
        impl Marker[Result[_, [:left]]] {}
        impl Marker[Result[_, [:right]]] {}
    "#},
    )
    .expect("closed rows with different tags do not overlap");
}

#[test]
fn error_row_coherence_preserves_shared_tail_constraints() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a, b] {}
        impl Marker[Result[Number, [:left | e]], Result[Number, [:right | e]]] {}
        impl Marker[Result[Number, [:left | :extra]], Result[Number, [:right | :other]]] {}
    "#},
    )
    .expect("one shared tail cannot be both extra and other");
}

#[test]
fn error_row_coherence_detects_compatible_shared_tail_constraints() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a, b] {}
        impl Marker[Result[Number, [:left | e]], Result[Number, [:right | e]]] {}
        impl Marker[Result[Number, [:left | :extra]], Result[Number, [:right | :extra]]] {}
    "#},
    )
    .expect_err("both arguments admit the same extra tail");
    assert!(
        errors
            .iter()
            .any(|error| matches!(error, alder_solve::SolveError::Coherence(_)))
    );
}

#[test]
fn derived_equality_accepts_generic_result_with_structural_error_row() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        #[derive(Eq)]
        enum Wrapper[a] { Wrapped(Result[a, [:failed]]) }
        fn check(left: Wrapper[Number], right: Wrapper[Number]) Bool { left == right }
    "#},
    )
    .expect("derived Eq handles a generic Result field with a structural error row");
}

#[test]
fn result_partial_constructor_accepts_fixed_error_rows() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            trait Marker[f] { fn pass(value: f[a]) f[a] { value } }
            impl Marker[Result[_, [:failed]]] {}
            fn check(value: Result[Number, [:failed]]) Result[Number, [:failed]] {
                pass(value)
            }
        "#},
    )
    .expect("fixed structural error rows support higher-kinded dispatch");
}

#[test]
fn result_partial_constructor_matches_rows_independent_of_tag_order() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[f] { fn pass(value: f[a]) f[a] { value } }
        impl Marker[Result[_, [:left(Number) | :right]]] {}
        fn check(value: Result[Number, [:right | :left(Number)]]) {
            pass(value)
        }
    "#},
    )
    .expect("structurally equal fixed rows must match");
}

#[test]
fn result_partial_constructor_matches_an_open_error_row() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Marker[f] { fn pass(value: f[a]) f[a] { value } }
        impl Marker[Result[_, [:failed | e]]] {}
        fn check(value: Result[Number, [:failed | :other]]) {
            pass(value)
        }
    "#},
    )
    .expect("an open implementation row retains residual tags");
}

#[test]
fn result_partial_constructor_rejects_incompatible_fixed_rows() {
    for error_type in [
        "[:failed(String)]",
        "[:other(Number)]",
        "[:failed(Number) | :other]",
    ] {
        let source = format!(
            indoc! {r#"
            trait Marker[f] {{ fn pass(value: f[a]) f[a] {{ value }} }}
            impl Marker[Result[_, [:failed(Number)]]] {{}}
            fn check(value: Result[Number, {error_type}]) {{ pass(value) }}
        "#},
            error_type = error_type
        );
        let bump = Bump::new();
        assert!(
            solve_input(&bump, &source).is_err(),
            "must reject {error_type}"
        );
    }
}

#[test]
fn recursive_structural_error_groups_do_not_overflow() {
    for source in [
        "error Recursive { :nested(Result[Number, Recursive]) }",
        "error Recursive { :nested(Array[Result[Number, Recursive]]) }",
        indoc! {r#"
            error First { :first(Result[Number, Second]) }
            error Second { :second(Result[Number, First]) }
        "#},
    ] {
        let bump = Bump::new();
        let errors = solve_input(&bump, source).expect_err("recursive row must fail");
        assert!(
            matches!(
                errors.as_slice(),
                [alder_solve::SolveError::Core(Error {
                    kind: ErrorKind::RecursiveErrorGroup { .. },
                    ..
                })]
            ),
            "expected a cycle diagnostic for {source}: {errors:?}"
        );
    }
}

#[test]
fn error_group_expansion_allows_shared_groups_and_recursive_enum_payloads() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            enum Tree { Leaf, Branch(Array[Tree]) }
            error Inner { :tree(Tree) }
            error Outer {
                :left(Result[Number, Inner]),
                :right(Result[String, Inner]),
            }
            fn identity(value: Result[Number, Outer]) { value }
        "#},
    )
    .expect("shared acyclic groups and nominal recursion remain valid");
}

#[test]
fn result_error_argument_rejects_unused_aliases() {
    let bump = Bump::new();
    assert!(
        solve_input(&bump, "type Invalid = Result[Number, String]").is_err(),
        "an unused alias must not export an invalid Result error argument"
    );
}

#[test]
fn result_error_argument_rejects_unused_enum_payloads() {
    for source in [
        "enum Invalid { Value(Result[Number, String]) }",
        "enum Invalid { Value { result: Result[Number, String] } }",
    ] {
        let bump = Bump::new();
        assert!(
            solve_input(&bump, source).is_err(),
            "unused enum payloads must be validated: {source}"
        );
    }
}

#[test]
fn result_error_argument_rejects_bodyless_trait_signatures() {
    for source in [
        "trait Read[a] { fn read(value: a) Result[Number, String] }",
        "trait Read[a] { fn read(value: a, result: Result[Number, String]) Number }",
    ] {
        let bump = Bump::new();
        assert!(solve_input(&bump, source).is_err(), "must reject: {source}");
    }
}

#[test]
fn result_error_argument_rejects_unused_error_group_payloads() {
    let bump = Bump::new();
    assert!(
        solve_input(&bump, "error Invalid { :failed(Result[Number, String]) }").is_err(),
        "error-group payloads must be checked without a use site"
    );
}

#[test]
fn result_error_argument_rejects_unused_associated_binding() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
                trait Container[a] { type Item }
                impl Container[Number] { type Item = Result[Number, String] }
            "#},
        )
        .is_err(),
        "associated type bindings must not publish an invalid Result"
    );
}

#[test]
fn result_error_argument_rejects_alias_substitution() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
                type Wrapped[a, e] = Result[a, e]
                fn identity(value: Wrapped[Number, String]) { value }
            "#},
        )
        .is_err(),
        "alias substitution must preserve the Result error kind"
    );
}

#[test]
fn result_error_argument_accepts_rows_groups_and_generic_tails() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            error Failure { :failed(String) }
            fn named(value: Result[Number, Failure]) { value }
            fn structural(value: Result[Number, [:failed(String)]]) { value }
            fn generic(value: Result[Number, e]) Result[Number, e] { value }
            fn success() Result[Number, Failure] { Ok(42) }
        "#},
    )
    .expect("supported error arguments must retain their contracts");
}

#[test]
fn alternative_pattern_bindings_require_compatible_payload_types() {
    for source in [
        indoc! {r#"
            enum Mixed { NumberValue(Number), TextValue(String) }
            fn invalid(input: Mixed) Number {
                match input { NumberValue(value) | TextValue(value) => value }
            }
        "#},
        indoc! {r#"
            enum Mixed { Numbers(Array[Number]), Texts(Array[String]) }
            fn invalid(input: Mixed) Array[Number] {
                match input { Numbers([..values]) | Texts([..values]) => values }
            }
        "#},
        indoc! {r#"
            enum Mixed { NumberValue(Number), TextValue(String) }
            fn invalid(input: Mixed) Number {
                match input { NumberValue(_ as value) | TextValue(_ as value) => value }
            }
        "#},
    ] {
        let bump = Bump::new();
        assert!(
            solve_input(&bump, source).is_err(),
            "must reject incompatible alternatives: {source}"
        );
    }
}

#[test]
fn annotated_lambda_record_returns_receive_field_context() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            type Payload = { value: Option[Option[Number]] }
            fn check() {
                let build = () Payload -> { { value: 42 } }
                let result: Payload = build()
            }
        "#},
    )
    .expect("lambda record return annotations must contextualize fresh fields");
}

#[test]
fn bare_pipe_destinations_use_optional_call_rules() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn nested(value?: Option[Number]) Option[Option[Number]] { value }
            fn choose(value: Number, extra?: Number) Number { value }
            fn check() {
                let lifted: Option[Option[Number]] = 42 |> nested
                let omitted: Number = 42 |> choose
            }
        "#},
    )
    .expect("bare pipes and explicit call destinations must share argument rules");
}

#[test]
fn option_wrapping_does_not_rewrite_existing_record_payloads() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
            type Payload = { value: Option[Number] }
            fn take(value?: Payload) Option[Payload] { value }
            fn invalid() {
                let original = { value: 42 }
                take(original)
            }
        "#},
        )
        .is_err(),
        "lifting the outer value cannot convert its mutable fields"
    );
}

#[test]
fn option_wrapping_preserves_context_for_fresh_record_payloads() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            type Payload = { value: Option[Number] }
            fn take(value?: Payload) Option[Payload] { value }
            fn take_many(value?: Array[Payload]) Option[Array[Payload]] { value }
            fn check() {
                let direct: Option[Payload] = take({ value: 42 })
                let array: Option[Array[Payload]] = take_many([{ value: 42 }])
                let field: { nested: Option[Payload] } = { nested: { value: 42 } }
            }
        "#},
    )
    .expect("outer lifting must retain fresh payload field contexts");
}

#[test]
fn pipe_inputs_preserve_context_for_fresh_record_payloads() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            type Payload = { value: Option[Number] }
            fn take(value?: Payload) Option[Payload] { value }
            fn take_many(value?: Array[Payload]) Option[Array[Payload]] { value }
            fn check() {
                let bare: Option[Payload] = { value: 42 } |> take
                let called: Option[Payload] = { value: 42 } |> take()
                let array: Option[Array[Payload]] = [{ value: 42 }] |> take_many
            }
        "#},
    )
    .expect("piped fresh initializers need the same field context as direct arguments");
}

#[test]
fn awaited_pipe_inputs_preserve_fresh_record_context() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        type Payload = { value: Option[Number] }
        async fn take(value?: Payload) Result[Option[Payload], [:failed]] { Ok(value) }
        async fn check() Result[Option[Payload], [:failed]] {
            Ok({ value: 42 } |> take().await?)
        }
    "#},
    )
    .expect("await and propagation wrappers must preserve the pipe argument context");
}

#[test]
fn pipe_field_context_does_not_convert_existing_mutable_aliases() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        type Payload = { value: Option[Number] }
        fn take(value?: Payload) Option[Payload] { value }
        fn invalid() {
            let original = { value: 42 }
            original |> take
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn pipe_input_returns_remain_reachable() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn take(value: Number) Number { value }
        fn invalid() Number { { return "wrong" } |> take }
    "#}
        )
        .is_err()
    );
}

#[test]
fn pipe_destinations_after_a_break_remain_unreachable() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn take(value: Number, extra: Number) Number { value }
        fn valid() Number {
            loop {
                { break 42 } |> ({
                    break "unreachable"
                    take
                })({ break "also unreachable" })
            }
        }
    "#},
    )
    .expect("unreachable pipe destinations cannot contribute loop result values");
}

#[test]
fn explicit_some_preserves_fresh_record_payload_context() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        type Payload = { value: Option[Number] }
        fn take(value?: Payload) Option[Payload] { value }
        fn check() {
            let annotated: Option[Payload] = Some({ value: 42 })
            let argument: Option[Payload] = take(Some({ value: 42 }))
        }
    "#},
    )
    .expect("an explicit Some must preserve its fresh payload initializer context");
}

#[test]
fn explicit_some_context_does_not_convert_mutable_payload_aliases() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        type Payload = { value: Option[Number] }
        fn check() {
            let original = { value: 42 }
            let invalid: Option[Payload] = Some(original)
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn optional_record_arguments_preserve_branch_context() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        type Payload = { value: Option[Number] }
        fn take(value?: Payload) Option[Payload] { value }
        fn check(flag: Bool) {
            take(if flag { { value: 42 } } else { { value: 7 } })
        }
    "#},
    )
    .expect("branch-local fresh fields need the argument payload context");
}

#[test]
fn contextual_match_and_block_inputs_preserve_record_fields() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        type Payload = { value: Option[Number] }
        fn take(value?: Payload) Option[Payload] { value }
        fn direct(flag: Bool) Payload {
            if flag { { value: 42 } } else { { value: 7 } }
        }
        fn check(flag: Bool) {
            take({ let value = 42
                { value }
            })
            take(match flag {
                true => ({ value: 42 }),
                false => ({ value: 7 }),
            })
            take(if flag { Some({ value: 42 }) } else { Some({ value: 7 }) })
        }
    "#},
    )
    .expect("context must reach block/match fields without removing explicit Some layers");
}

#[test]
fn branch_context_does_not_wrap_function_returns() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn invalid(flag: Bool) Option[Number] {
            if flag { 42 } else { 7 }
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn branch_context_does_not_convert_mutable_payload_aliases() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        type Payload = { value: Option[Number] }
        fn take(value?: Payload) Option[Payload] { value }
        fn invalid(flag: Bool) {
            let original = { value: 42 }
            take(if flag { original } else { { value: 7 } })
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn spread_record_initializers_preserve_written_field_context() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        type Payload = { label: String, value: Option[Number] }
        fn take(value?: Payload) Option[Payload] { value }
        fn check() {
            let base = { label: "base" }
            let annotated: Payload = { ..base, value: 42 }
            take({ ..base, value: 42 })
            take({ value: 42, ..base })
        }
    "#},
    )
    .expect("spread operands must not disable context for written fields");
}

#[test]
fn contextual_spreads_preserve_nested_fresh_payloads_and_option_overwrites() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        type Payload = { value: Option[Number] }
        fn check(later: { value?: Number }) {
            let base = { label: "base" }
            let nested: { label: String, nested: Option[Payload] } = {
                ..base, nested: { value: 42 },
            }
            let explicit: { label: String, nested: Option[Payload] } = {
                ..base, nested: Some({ value: 42 }),
            }
            let array: { label: String, items: Array[Payload] } = {
                ..base, items: [{ value: 42 }],
            }
            let fallback: Payload = { value: 42, ..later }
        }
    "#},
    )
    .expect("nested initializers retain context and the final spread supplies an Option field");
}

#[test]
fn contextual_spreads_discard_overwritten_field_expectations() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn check() {
            let replacement = { value: Some(42) }
            let result: { value: Option[Number] } = { value: true, ..replacement }
        }
    "#},
    )
    .expect("an overwritten field does not determine the final record's payload type");
}

#[test]
fn spread_record_context_does_not_convert_inherited_payloads() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn invalid() {
            let original = { nested: { value: 42 } }
            let converted: { nested: { value: Option[Number] } } = { ..original }
            original.nested.value = 7
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn record_field_lifting_does_not_convert_existing_mutable_records() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
            fn invalid() {
                let original = { value: 42 }
                let converted: { value: Option[Number] } = original
                original.value = 7
                converted
            }
        "#},
        )
        .is_err(),
        "wrapping an initializer must not become invariant record subtyping"
    );
}

#[test]
fn record_return_initializers_receive_field_context() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            type NestedRecord = { value: Option[Option[Number]] }
            fn tail() NestedRecord { { value: 42 } }
            fn early() NestedRecord { return { value: 42 } }
        "#},
    )
    .expect("record return annotations must reach their fresh field initializers");
}

#[test]
fn record_initializers_recursively_lift_option_values() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn check() {
                let bare: { value: Option[Option[Number]] } = { value: 42 }
                let once: { value: Option[Option[Number]] } = { value: Some(42) }
                let direct: { value: Option[Option[Number]] } = { value: Some(None) }
                let absent: { value: Option[Option[Number]] } = { value: None }
            }
        "#},
    )
    .expect("contextual field initializers must use recursive Option lifting");
}

#[test]
fn option_argument_lifting_reports_incompatible_inference_preferences() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            fn relate(first?: a, second: a) {}
            fn conflict(first, second) {
                relate(first, second)
                relate(second, first)
            }
        "#},
    )
    .expect_err("competing direct matches must not choose source order");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Core(Error {
            kind: ErrorKind::AmbiguousOptionLifting,
            ..
        })
    )));
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn relate(first?: a, second: a) {}
            fn resolved(first: Number, second: Number) {
                relate(first, second)
                relate(second, first)
            }
        "#},
    )
    .expect("explicit types fix both wrapping depths");
}

#[test]
fn option_arguments_recursively_lift_concrete_values() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn nested(value?: Option[Number]) Option[Option[Number]] { value }
            fn check() {
                let bare: Option[Option[Number]] = nested(42)
                let once: Option[Option[Number]] = nested(Some(42))
                let twice: Option[Option[Number]] = nested(Some(Some(42)))
                let outer_absent: Option[Option[Number]] = nested(None)
                let inner_absent: Option[Option[Number]] = nested(Some(None))
                let piped: Option[Option[Number]] = 42 |> nested()
            }
        "#},
    )
    .expect("calls must recursively lift values while preserving direct Option matches");
}

#[test]
fn option_lifting_preserves_inferred_error_row_kinds() {
    for source in [
        indoc! {r#"
            fn consume(value?: a) {}
            fn relay(failure) {
                let result = Err(failure)
                consume(failure)
                result
            }
            fn check(failure: [:bad]) Result[Number, [:bad]] { relay(failure) }
        "#},
        indoc! {r#"
            fn consume(value?: a) {}
            fn relay(failure) {
                consume(failure)
                Err(failure)
            }
            fn check(failure: [:bad]) Result[Number, [:bad]] { relay(failure) }
        "#},
    ] {
        let bump = Bump::new();
        solve_input(&bump, source)
            .expect("an inferred error row is opaque to outer Option-depth selection");
    }
}

#[test]
fn option_lifting_preserves_explicit_universal_contracts() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn consume(value?: a) {}
            fn relay(value: a) a {
                consume(value)
                value
            }
            fn check() {
                let number: Number = relay(42)
                let text: String = relay("hello")
                let nested: Option[Number] = relay(Some(42))
            }
        "#},
    )
    .expect("contextual Some insertion must preserve an explicitly universal input");
}

#[test]
fn option_lifting_preserves_trait_method_universals() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn consume(value?: a) {}
            trait Relay[a] {
                fn relay(marker: a, value: b) b
                fn fallback(marker: a, value: b) b {
                    consume(value)
                    value
                }
            }
            impl Relay[Number] {
                fn relay(marker: Number, value) {
                    consume(value)
                    value
                }
            }
            fn check() {
                let number: Number = relay(0, 42)
                let text: String = relay(0, "hello")
                let defaulted: String = fallback(0, "default")
            }
        "#},
    )
    .expect("trait promises must constrain lifting even without implementation annotations");
}

#[test]
fn option_lifting_preserves_generic_lambda_and_field_contracts() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn consume(value?: a) {}
            fn wrap_record(value: a) ({ value: Option[a] }) { { value } }
            fn relay(value: a) a {
                let forward = (argument: a) a -> {
                    consume(argument)
                    argument
                }
                let record = wrap_record(value)
                forward(value)
            }
            fn check() {
                let number: Number = relay(42)
                let text: String = relay("hello")
            }
        "#},
    )
    .expect("lambda and fresh-field lifting must respect enclosing universals");
}

#[test]
fn option_lifting_does_not_erase_incompatible_universal_payloads() {
    for source in [
        indoc! {r#"
            fn consume(value?: Number) {}
            fn invalid(value: a) a {
                consume(value)
                value
            }
        "#},
        indoc! {r#"
            fn consume(first?: a, second?: a) {}
            fn invalid(first: a, second: b) a {
                consume(first, second)
                first
            }
        "#},
    ] {
        let bump = Bump::new();
        assert!(solve_input(&bump, source).is_err(), "{source}");
    }
}

#[test]
fn option_argument_lifting_is_independent_of_shared_input_call_order() {
    for source in [
        indoc! {r#"
            fn single(value?: Number) {}
            fn double(value?: Option[Number]) {}
            fn consume(value) {
                single(value)
                double(value)
            }
            fn check() { consume(Some(42)) }
        "#},
        indoc! {r#"
            fn single(value?: Number) {}
            fn double(value?: Option[Number]) {}
            fn consume(value) {
                double(value)
                single(value)
            }
            fn check() { consume(Some(42)) }
        "#},
    ] {
        let bump = Bump::new();
        solve_input(&bump, source).expect("shared actual types need joint wrapping constraints");
    }
}

#[test]
fn option_argument_lifting_is_independent_of_shared_payload_argument_order() {
    for source in [
        indoc! {r#"
            fn choose(first?: a, second?: a) Option[a] { first }
            fn check() Option[Option[Number]] { choose(42, Some(Some(7))) }
        "#},
        indoc! {r#"
            fn choose(first?: a, second?: a) Option[a] { second }
            fn check() Option[Option[Number]] { choose(Some(Some(7)), 42) }
        "#},
    ] {
        let bump = Bump::new();
        solve_input(&bump, source)
            .expect("shared expected payloads need joint wrapping constraints");
    }
}

#[test]
fn option_arguments_before_required_parameters_remain_required() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn read(first?: Number, second: String, third?: Number) String { second }
            fn check() String { read(None, "present") }
        "#},
    )
    .expect("Option before a non-Option slot is a required argument, not an invalid declaration");
    for source in [
        indoc! {r#"
            fn read(first?: Number, second: String) {}
            fn check() { read() }
        "#},
        indoc! {r#"
            fn read(first?: Number, second: String) {}
            fn check() { read(None) }
        "#},
        indoc! {r#"
            fn read(first?: Number, second: String) {}
            fn check() { read("skip") }
        "#},
        indoc! {r#"
            fn read(first: Number, second?: Number) {}
            fn check() { read() }
        "#},
        indoc! {r#"
            fn read(first?: Number) {}
            fn check() { read(None, None) }
        "#},
    ] {
        let bump = Bump::new();
        assert!(solve_input(&bump, source).is_err(), "must reject: {source}");
    }
}

#[test]
fn trailing_option_arguments_can_be_omitted() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn choose(value: Number, first?: Number, second: Option[String]) Number {
                value
            }
            fn nested(value?: Option[Number]) Option[Option[Number]] { value }
            fn check() {
                let first: Number = choose(42)
                let second: Number = choose(42, Some(7))
                let third: Number = 42 |> choose()
                let function: fn(Number, Option[Number], Option[String]) Number = choose
                let fourth: Number = function(42)
                let absent: Option[Option[Number]] = nested()
            }
        "#},
    )
    .expect("trailing ordinary Option parameters must permit omission");
}

#[test]
fn optional_parameter_shorthand_checks_as_ordinary_option() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn read(value?: Number) Option[Number] { value }
            fn nested(value?: Option[Number]) Option[Option[Number]] { value }
            fn check() {
                let function: fn(Option[Number]) Option[Number] = read
                let lambda: fn(Option[Number]) Option[Number] = (value?: Number) -> value
                let first: Option[Number] = function(Some(42))
                let second: Option[Number] = lambda(None)
                let third: Option[Option[Number]] = nested(Some(None))
            }
        "#},
    )
    .expect("optional shorthand must expose ordinary, possibly nested Option types");
}

#[test]
fn option_constructors_preserve_payload_types() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn read(value: Option[Number]) Number {
            match value { Some(number) => number, None => 0 }
        }
        fn build() Option[Option[Number]] { Some(None) }
    "#},
    )
    .expect("Option constructors and exhaustive patterns must typecheck");
    for source in [
        "fn invalid() Option[Number] { Some(\"wrong\") }",
        "fn invalid(value: Option[Number]) String { match value { Some(text) => text, None => \"none\" } }",
    ] {
        let bump = Bump::new();
        assert!(solve_input(&bump, source).is_err(), "must reject: {source}");
    }
}

#[test]
fn empty_record_overlay_inputs_cannot_hide_conflicting_result_fields() {
    let source = indoc! {r#"
        fn invalid(left, right) {
            let padded = { ..{}, ..left, ..{}, ..right, ..{} }
            let plain = { ..left, ..right }
            let number: Number = padded.value
            let text: String = plain.value
            (number, text)
        }
    "#};
    let bump = Bump::new();
    assert!(
        solve_input(&bump, source).is_err(),
        "empty spreads must not create independent output contracts"
    );
}

#[test]
fn empty_record_overlay_inputs_do_not_erase_unknown_row_overwrites() {
    let source = indoc! {r#"
        pub fn padded(left, extra, right) {
            { ..{}, ..left, ..extra, ..right, ..{} }
        }
        fn number() Number {
            padded({ value: false }, { value: 42 }, {}).value
        }
        fn string() String {
            padded({}, {}, { value: "last" }).value
        }
    "#};
    let bump = Bump::new();
    solve_input(&bump, source).expect("unknown rows remain ordered overlay operands");
}

#[test]
fn repeated_record_overlay_inputs_cannot_hide_conflicting_result_fields() {
    let source = indoc! {r#"
        fn invalid(left, right) {
            let repeated = { ..left, ..right, ..left }
            let reduced = { ..right, ..left }
            let number: Number = repeated.value
            let text: String = reduced.value
            (number, text)
        }
    "#};
    let bump = Bump::new();
    assert!(
        solve_input(&bump, source).is_err(),
        "rightmost repeated input must determine the same field contract"
    );
}

#[test]
fn repeated_record_overlay_inputs_preserve_rightmost_priority() {
    let source = indoc! {r#"
        pub fn both(left, right) {
            ({ ..left, ..right, ..left }, { ..right, ..left, ..right })
        }
        fn run() (Number, String) {
            let records = both({ value: 42 }, { value: "last" })
            (records.0.value, records.1.value)
        }
    "#};
    let bump = Bump::new();
    solve_input(&bump, source).expect("deduplication must retain the rightmost input order");
}

#[test]
fn adjacent_closed_overlay_operands_cannot_hide_conflicting_result_fields() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            fn split(record) { { ..record, x: 0, y: true } }
            fn grouped(record) { { ..record, ..{ x: 0, y: true } } }
            fn invalid(record) {
                let first: Number = split(record).value
                let second: String = grouped(record).value
                (first, second)
            }
        "#},
    )
    .expect_err("equivalent closed-field grouping cannot hide contradictory field requirements");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::Mismatch { .. },
                ..
            })
        )),
        "{errors:?}"
    );
}

#[test]
fn overwritten_closed_overlay_fields_cannot_hide_conflicting_result_fields() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            fn padded(left, right) { { ..left, ..{ x: "discarded" }, ..right, x: 0 } }
            fn plain(left, right) { { ..left, ..right, x: 0 } }
            fn invalid(left, right) {
                let first: Number = padded(left, right).value
                let second: String = plain(left, right).value
                (first, second)
            }
        "#},
    )
    .expect_err("overwritten fields cannot distinguish equivalent overlay contracts");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::Mismatch { .. },
                ..
            })
        )),
        "{errors:?}"
    );
}

#[test]
fn guaranteed_open_overlay_fields_cannot_hide_conflicting_result_fields() {
    let bump = Bump::new();
    let source = indoc! {r#"
        fn padded(left, right: { r | x: Number }) { { ..left, x: "discarded", ..right } }
        fn plain(left, right: { r | x: Number }) { { ..left, ..right } }
        fn invalid(left, right) {
            let first: Number = padded(left, right).value
            let second: String = plain(left, right).value
            (first, second)
        }
    "#};
    let errors = solve_input(&bump, source)
        .expect_err("guaranteed overwrites preserve equivalent contracts");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::Mismatch { .. },
                ..
            })
        )),
        "{source}\n{errors:?}"
    );
}

#[test]
fn guaranteed_open_overlay_overwrites_preserve_other_fields_and_none() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn merge(left, right: { r | x: Option[Number] }) {
            { ..left, x: "discarded", kept: true, ..right }
        }
        fn check() {
            let first = merge({ value: 42 }, { x: None, kept: "right" })
            let x: Option[Number] = first.x
            let kept: String = first.kept
            let value: Number = first.value
            let second = merge({ value: "left" }, { x: Some(7) })
            let other: String = second.value
            let remaining: Bool = second.kept
        }
    "#},
    )
    .expect("known open fields overwrite without erasing other input requirements");
}

#[test]
fn adjacent_closed_overlay_normalization_preserves_rightmost_writes_and_open_barriers() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn split(record) { { ..record, ..{ x: 0 }, ..{ x: "final", y: true } } }
            fn grouped(record) { { ..record, ..{ x: "final", y: true } } }
            fn before(left, right) { { ..left, value: 42, ..right } }
            fn after(left, right) { { ..left, ..right, value: 42 } }
            fn check() {
                let first: String = split({ marker: true }).x
                let second: String = grouped({ marker: true }).x
                let third: String = before({}, { value: "right" }).value
                let fourth: Number = after({}, { value: "right" }).value
            }
        "#},
    )
    .expect("normalization must respect overwrite order and intervening unknown rows");
}

#[test]
fn associative_record_overlays_cannot_promise_incompatible_payloads() {
    let source = indoc! {r#"
        fn invalid(left, middle, right) {
            let first = { ..{ ..left, ..middle }, ..right }
            let second = { ..left, ..{ ..middle, ..right } }
            let number: Number = first.value
            let text: String = second.value
            (number, text)
        }
    "#};
    let bump = Bump::new();
    let errors = solve_input(&bump, source).expect_err("equivalent overlays cannot disagree");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::Mismatch { .. },
                ..
            })
        )),
        "{source}\n{errors:?}"
    );
}

#[test]
fn associative_record_overlays_preserve_independent_right_biased_instantiations() {
    let source = indoc! {r#"
        pub fn both(left, middle, right) {
            let first = { ..{ ..left, ..middle }, ..right }
            let second = { ..left, ..{ ..middle, ..right } }
            (first, second)
        }
        fn numbers() (Number, Number) {
            let records = both({ value: "old" }, { value: true }, { value: 42 })
            (records.0.value, records.1.value)
        }
        fn strings() (String, String) {
            let records = both({ value: 42 }, { value: true }, { value: "last" })
            (records.0.value, records.1.value)
        }
    "#};
    let bump = Bump::new();
    solve_input(&bump, source)
        .expect("association preserves rightmost overrides and generalization");
}

#[test]
fn associative_record_overlays_generalize_exports_without_local_calls() {
    let source = indoc! {r#"
        pub fn both(left, middle, right) {
            ({ ..{ ..left, ..middle }, ..right }, { ..left, ..{ ..middle, ..right } })
        }
    "#};
    let bump = Bump::new();
    solve_input(&bump, source)
        .expect("overlay residual tails must be quantified before publication");
}

#[test]
fn associative_record_overlays_do_not_equate_distinct_final_overrides() {
    let source = indoc! {r#"
        fn both(left, middle, right) (Number, String) {
            let first = { ..{ ..left, ..middle }, ..right, value: 42 }
            let second = { ..left, ..{ ..middle, ..right }, value: "last" }
            (first.value, second.value)
        }
        fn run() (Number, String) {
            both({ x: 1 }, { y: true }, { value: false })
        }
    "#};
    let bump = Bump::new();
    solve_input(&bump, source).expect("different final writes must retain distinct result types");
}

#[test]
fn associative_record_overlays_do_not_generalize_captured_shared_arrays() {
    let source = indoc! {r#"
        let shared = []
        fn views(middle, right) {
            let left = { items: shared }
            ({ ..{ ..left, ..middle }, ..right }, { ..left, ..{ ..middle, ..right } })
        }
        fn write() {
            let records = views({}, {})
            Array.push(records.0.items, 42)
        }
        fn invalid() Array[String] {
            let records = views({}, {})
            records.1.items
        }
    "#};
    let bump = Bump::new();
    let errors = solve_input(&bump, source).expect_err("shared payloads cannot be re-instantiated");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::Mismatch { .. },
                ..
            })
        )),
        "{source}\n{errors:?}"
    );
}

#[test]
fn associative_record_overlays_allow_fresh_array_factories() {
    let source = indoc! {r#"
        fn views(middle, right) {
            let left = { items: [] }
            ({ ..{ ..left, ..middle }, ..right }, { ..left, ..{ ..middle, ..right } })
        }
        fn numbers() Array[Number] {
            let records = views({}, {})
            Array.push(records.0.items, 42)
            records.1.items
        }
        fn strings() Array[String] {
            let records = views({}, {})
            Array.push(records.1.items, "fresh")
            records.0.items
        }
    "#};
    let bump = Bump::new();
    solve_input(&bump, source).expect("each factory call owns its freshly allocated array");
}

#[test]
fn call_arguments_constrain_later_literal_arguments_from_left_to_right() {
    let source = indoc! {r#"
        fn put(items: Array[a], value: Array[a]) {}
        pub fn main() {
            let numbers = [1]
            put(numbers, ["wrong"])
        }
    "#};
    let bump = Bump::new();
    let errors = solve_input(&bump, source).unwrap_err();
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::Mismatch { actual, expected },
                ..
            }) if *actual == DiagnosticType::Named("String".into()) && *expected == DiagnosticType::Named("Number".into())
        )),
        "{source}\n{errors:?}"
    );
}

#[test]
fn piped_argument_constrains_later_literal_arguments() {
    let source = indoc! {r#"
        fn put(items: Array[a], value: Array[a]) {}
        pub fn main() {
            let numbers = [1]
            numbers |> put(["wrong"])
        }
    "#};
    let bump = Bump::new();
    let errors = solve_input(&bump, source).unwrap_err();
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::Mismatch { actual, expected },
                ..
            }) if *actual == DiagnosticType::Named("String".into()) && *expected == DiagnosticType::Named("Number".into())
        )),
        "{source}\n{errors:?}"
    );
}

#[test]
fn ordinary_bindings_and_parameters_allow_type_checked_assignment() {
    let source = indoc! {r#"
        let total = 0
        fn update(value: Number, items: Array[Number], record: { count: Number }) Number {
            value += 1
            items[0] = value
            record.count = value
            let local = 0
            local = value
            total = local
            total
        }
        fn update_lambda() {
            value -> {
                value = 42
                value
            }
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn replacement_constraints_are_independent_of_declaration_order() {
    for source in [
        indoc! {r#"
            fn invalid() String { forward("wrong") }
            fn forward(value) { operation(value) }
            fn writer() { operation = (value: Number) -> value + 1 }
            let operation = value -> value
        "#},
        indoc! {r#"
            fn writer() { operation = (value: Number) -> value + 1 }
            let operation = value -> value
            fn invalid() String { forward("wrong") }
            fn forward(value) { operation(value) }
        "#},
    ] {
        let bump = Bump::new();
        let errors = solve_input(&bump, source).unwrap_err();
        assert!(
            errors.iter().any(|error| matches!(
                error,
                alder_solve::SolveError::Core(Error {
                    kind: ErrorKind::Mismatch { .. },
                    ..
                })
            )),
            "{source}\n{errors:?}"
        );
    }
}

#[test]
fn unassigned_function_binding_retains_independent_instantiations() {
    let source = indoc! {r#"
        let identity = value -> value
        fn number() Number { identity(42) }
        fn string() String { identity("text") }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn nested_writer_keeps_captured_function_monomorphic() {
    let source = indoc! {r#"
        let operation = value -> value
        fn writer() { () -> { operation = (value: Number) -> value + 1 } }
        fn forward(value) { operation(value) }
        fn invalid() String {
            writer()()
            forward("text")
        }
    "#};
    let bump = Bump::new();
    let errors = solve_input(&bump, source).unwrap_err();
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::Mismatch { .. },
                ..
            })
        )),
        "{errors:?}"
    );
}

#[test]
fn local_assignment_does_not_restrict_shadowed_function_polymorphism() {
    let source = indoc! {r#"
        let identity = value -> value
        fn replace() {
            let identity = 0
            identity = 42
        }
        fn number() Number { identity(42) }
        fn string() String { identity("text") }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn reassigned_function_cannot_be_instantiated_at_incompatible_types() {
    let source = indoc! {r#"
        let operation = value -> value
        fn replace() { operation = (value: Number) -> 42 }
        fn invalid() String {
            replace()
            operation("text")
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn captured_reassigned_function_cannot_escape_through_generalization() {
    let source = indoc! {r#"
        let operation = value -> value
        fn forward(value) { operation(value) }
        fn replace() { operation = (value: Number) -> 42 }
        fn invalid() String {
            replace()
            forward("text")
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn reassigned_function_preserves_valid_monomorphic_calls() {
    let source = indoc! {r#"
        let operation = value -> value
        fn forward(value) { operation(value) }
        fn replace() { operation = (value: Number) -> value + 1 }
        fn valid() Number {
            let before = forward(10)
            replace()
            before + forward(20)
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn trait_overlay_default_rejects_hidden_overwrite() {
    let source = indoc! {r#"
        trait ReadOverlay[a] {
            fn read_overlay(subject: a, left: { r | value: Number }, right: { s | other: Bool }) Number {
                let merged = { ..left, ..right }
                merged.value
            }
        }
    "#};
    let bump = Bump::new();
    let errors = solve_input(&bump, source).unwrap_err();
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::GenericSpecialization { .. },
                ..
            })
        )),
        "{errors:?}"
    );
}

#[test]
fn trait_overlay_default_accepts_guaranteed_rightmost_field() {
    let source = indoc! {r#"
        trait ReadOverlay[a] {
            fn read_overlay(subject: a, left: { r | other: Bool }, right: { s | value: Number }) Number {
                let merged = { ..left, ..right }
                merged.value
            }
        }
        impl ReadOverlay[Number] {}
        fn run() Number { read_overlay(0, { other: true, value: "old" }, { value: 42 }) }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn trait_overlay_rejects_hidden_overwrite_in_method_rows() {
    let source = indoc! {r#"
        trait ReadOverlay[a] {
            fn read_overlay(subject: a, left: { r | value: Number }, right: { s | other: Bool }) Number
        }
        impl ReadOverlay[Number] {
            fn read_overlay(subject, left, right) {
                let merged = { ..left, ..right }
                merged.value
            }
        }
    "#};
    let bump = Bump::new();
    let errors = solve_input(&bump, source).unwrap_err();
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::GenericSpecialization { .. },
                ..
            })
        )),
        "{errors:?}"
    );
}

#[test]
fn trait_overlay_accepts_guaranteed_rightmost_field() {
    let source = indoc! {r#"
        trait ReadOverlay[a] {
            fn read_overlay(subject: a, left: { r | other: Bool }, right: { s | value: Number }) Number
        }
        impl ReadOverlay[Number] {
            fn read_overlay(subject, left, right) {
                let merged = { ..left, ..right }
                merged.value
            }
        }
        fn run() Number { read_overlay(0, { other: true, value: "old" }, { value: 42 }) }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn piped_record_overlay_exposes_optional_fields_before_access() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn merge_pair((left, right)) { merge(left, right) }
        fn read(left: { value?: Number }) Option[Number] {
            ((left, {}) |> merge_pair).value
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn nested_async_lambda_in_assignment_index_does_not_suspend_the_function() {
    let source = indoc! {r#"
        fn valid() Number {
            let values = [10]
            values[{
                let deferred = () -> async { Task.sleep(1).await }
                0
            }] = 42
            values[0]
        }
        fn caller() Number { valid() }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn assignment_index_propagation_cannot_escape_its_error_contract() {
    let source = indoc! {r#"
        fn index() Result[Number, [:index_failed]] { Err(:index_failed) }
        fn invalid() Result[(), [:other]] {
            let values = [10]
            values[index()?] = 42
            Ok(())
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn assignment_index_return_must_satisfy_the_function_contract() {
    let source = indoc! {r#"
        fn invalid() Number {
            let values = [10]
            values[{ return "wrong" }] = 42
            0
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn tagged_template_checks_the_tag_function() {
    let source = indoc! {r#"
        fn invalid() String {
            let tag = 42
            tag`hello`
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn tagged_template_preserves_the_tag_return_type() {
    let source = indoc! {r#"
        fn count(parts: Array[String], value: Number) Number { value }
        fn valid() Number { count`value: ${42}` }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn tagged_template_checks_interpolation_types() {
    let source = indoc! {r#"
        fn tag(parts: Array[String], value: Number) String { "text" }
        fn invalid() String { tag`value: ${"wrong"}` }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn json_module_decode_requires_a_json_instance() {
    let source = indoc! {r#"
        fn invalid() Result[fn(Number) Number, [:invalid_json(String)]] {
            Json.decode("42")
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn json_module_generic_calls_require_the_declared_bound() {
    let source = indoc! {r#"
        fn invalid(value: a) String { Json.encode(value) }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
    let source = indoc! {r#"
        fn valid(value: a) String where a: Json { Json.encode(value) }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn higher_order_error_rows_preserve_forwarding_constraints() {
    let source = indoc! {r#"
        fn forward(value: Result[Number, [:known | e]]) {
            let number = value?
            Ok(number)
        }
        fn factory() { forward }
        fn apply(operation, value) { operation(value) }
        fn source() Result[Number, [:known | :extra]] { Err(:extra) }
        fn run() Number {
            match apply(factory(), source()) {
                Ok(number) => number,
                Err(:known) => 0,
                Err(:extra) => 1,
            }
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn higher_order_error_rows_cannot_drop_a_forwarded_tag() {
    let source = indoc! {r#"
        fn forward(value: Result[Number, [:known | e]]) {
            let number = value?
            Ok(number)
        }
        fn factory() { forward }
        fn apply(operation, value) { operation(value) }
        fn source() Result[Number, [:known | :extra]] { Err(:extra) }
        fn run() Result[Number, [:known]] { apply(factory(), source()) }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn inferred_error_union_includes_an_early_success_and_final_error() {
    let source = indoc! {r#"
        fn choose(flag: Bool) {
            if flag { return Ok(42) }
            Err(:failed)
        }
        fn run() Number {
            match choose(false) {
                Ok(value) => value,
                Err(:failed) => 0,
            }
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn inferred_error_union_includes_early_and_final_returns() {
    let source = indoc! {r#"
        fn choose(flag: Bool) {
            if flag { return Err(:first) }
            Err(:second)
        }
        fn run() Number {
            match choose(false) {
                Ok(value) => value,
                Err(:first) => 1,
                Err(:second) => 2,
            }
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn recursive_error_union_supports_exhaustive_matching() {
    let source = indoc! {r#"
        fn first(n: Number) {
            if n == 0 { return Err(:first) }
            let value = second(n - 1)?
            Ok(value)
        }
        fn second(n: Number) {
            if n == 0 { return Err(:second) }
            let value = first(n - 1)?
            Ok(value)
        }
        fn run() Number {
            match first(4) {
                Ok(value) => value,
                Err(:first) => 1,
                Err(:second) => 2,
            }
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn recursive_error_union_preserves_an_external_open_source() {
    let source = indoc! {r#"
        fn first(n: Number, value: Result[Number, [:known | e]]) {
            if n == 0 { return value }
            let number = second(n - 1, value)?
            Ok(number)
        }
        fn second(n: Number, value: Result[Number, [:known | e]]) {
            if n == 0 { return value }
            let number = first(n - 1, value)?
            Ok(number)
        }
        fn run(value: Result[Number, [:known | e]]) Number {
            match first(4, value) {
                Ok(number) => number,
                Err(:known) => 0,
            }
        }
    "#};
    let errors = infer(&Bump::new(), source).unwrap_err();
    assert!(matches!(
        &errors[0].kind,
        ErrorKind::NonExhaustiveErrorMatch { open: true, .. }
    ));
}

#[test]
fn recursive_error_union_closes_after_external_source_instantiation() {
    let source = indoc! {r#"
        fn first(n: Number, value: Result[Number, [:known | e]]) {
            if n == 0 { return value }
            let number = second(n - 1, value)?
            Ok(number)
        }
        fn second(n: Number, value: Result[Number, [:known | e]]) {
            if n == 0 { return value }
            let number = first(n - 1, value)?
            Ok(number)
        }
        fn source() Result[Number, [:known | :extra]] { Err(:extra) }
        fn run() Number {
            match first(4, source()) {
                Ok(number) => number,
                Err(:known) => 0,
                Err(:extra) => 1,
            }
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn instantiated_error_row_failure_points_to_the_local_reference() {
    let source = indoc! {r#"
        fn combine(left: Result[Number, [:known | e]], right: Result[Number, [:known | f]]) {
            let x = left?
            let y = right?
            Ok(x + y)
        }
        fn left() Result[Number, [:known | :left]] { Err(:left) }
        fn right() Result[Number, [:known | :right]] { Err(:right) }
        fn run() Result[Number, [:known | :left]] {
            let result: Result[Number, [:known | :left]] = combine(left(), right())
            result
        }
    "#};
    let errors = infer(&Bump::new(), source).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].region.start.line, 9);
    assert_eq!(errors[0].region.start.column, 52);
    assert_eq!(errors[0].region.end.column, 59);
}

#[test]
fn error_row_inclusion_metadata_tracks_instantiated_function_rows() {
    let source = indoc! {r#"
        fn forward(value: Result[Number, [:known | e]]) {
            let number = value?
            Ok(number)
        }
        fn relay(value: Result[Number, [:known | f]]) { forward(value) }
    "#};
    let bump = Bump::new();
    let annotations = infer(&bump, source).expect("forwarding infers");
    for name in ["forward", "relay"] {
        let annotation = annotations
            .iter()
            .find(|(key, _)| key.name == name)
            .unwrap()
            .1;
        let Type::Fn { params, ret } = annotation.typ.value else {
            panic!("expected function");
        };
        let Type::Named {
            args: source_args, ..
        } = params[0].value
        else {
            panic!("expected Result parameter");
        };
        let Type::Named {
            args: target_args, ..
        } = ret.value
        else {
            panic!("expected Result return");
        };
        let source_row = render_type(source_args[1]);
        let target_row = render_type(target_args[1]);
        assert_ne!(
            source_row, target_row,
            "this test exercises distinct related rows"
        );
        assert!(
            annotation.error_row_inclusions.iter().any(|inclusion| {
                render_type(inclusion.source) == source_row
                    && render_type(inclusion.target) == target_row
            }),
            "{name} must preserve its input-to-output inclusion: {annotation:?}"
        );
    }
}

#[test]
fn error_row_inclusion_cannot_lose_an_inferred_forwarded_tail() {
    let source = indoc! {r#"
        fn forward(value: Result[Number, [:known | e]]) {
            let number = value?
            Ok(number)
        }
        fn run() Result[Number, [:known]] { forward(Err(:hidden)) }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn error_row_inclusion_combines_independent_source_tails() {
    let source = indoc! {r#"
        fn combine(left: Result[Number, [:known | e]], right: Result[Number, [:known | f]]) {
            let x = left?
            let y = right?
            Ok(x + y)
        }
        fn left() Result[Number, [:known | :left]] { Err(:left) }
        fn right() Result[Number, [:known | :right]] { Err(:right) }
        fn run() Result[Number, [:known | :left | :right]] {
            combine(left(), right())
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn error_row_inclusion_union_rejects_a_missing_source_tag() {
    let source = indoc! {r#"
        fn combine(left: Result[Number, [:known | e]], right: Result[Number, [:known | f]]) {
            let x = left?
            let y = right?
            Ok(x + y)
        }
        fn left() Result[Number, [:known | :left]] { Err(:left) }
        fn right() Result[Number, [:known | :right]] { Err(:right) }
        fn run() Result[Number, [:known | :left]] { combine(left(), right()) }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn error_row_inclusion_infers_a_closed_concrete_union_without_an_annotation() {
    let source = indoc! {r#"
        fn combine(left: Result[Number, [:known | e]], right: Result[Number, [:known | f]]) {
            let x = left?
            let y = right?
            Ok(x + y)
        }
        fn left() Result[Number, [:known | :left]] { Err(:left) }
        fn right() Result[Number, [:known | :right]] { Err(:right) }
        fn run() Number {
            match combine(left(), right()) {
                Ok(value) => value,
                Err(:known) => 0,
                Err(:left) => 1,
                Err(:right) => 2,
            }
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn error_row_inclusion_respects_multiple_closed_upper_bounds() {
    let source = indoc! {r#"
        fn forward(value: Result[Number, [:known | e]]) {
            let number = value?
            Ok(number)
        }
        fn fail() Result[Number, [:known]] { Err(:known) }
        fn run() {
            let shared = forward(fail())
            let first = () Result[Number, [:known | :left]] -> shared
            let second = () Result[Number, [:known | :right]] -> shared
            (first(), second())
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn error_row_inclusion_inferred_union_preserves_an_unannotated_return_source() {
    let source = indoc! {r#"
        fn fail() Result[Number, [:known]] { Err(:known) }
        fn forward(value) {
            let ignored = fail()?
            value
        }
        fn other() Result[Number, [:other]] { Err(:other) }
        fn run() Result[Number, [:known | :other]] { forward(other()) }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn error_row_inclusion_does_not_close_an_explicit_open_contract() {
    let source = indoc! {r#"
        fn run(value: Result[Number, [:known | e]]) Number {
            match value {
                Ok(number) => number,
                Err(:known) => 0,
            }
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn error_row_inclusion_exact_lambda_result_supports_exhaustive_matching() {
    let source = indoc! {r#"
        fn fail() Result[Number, [:known]] { Err(:known) }
        fn run() Number {
            let forward = value -> {
                let number = value?
                Ok(number)
            }
            match forward(fail()) {
                Ok(number) => number,
                Err(:known) => 0,
            }
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn error_row_inclusion_cannot_replace_an_arbitrary_tail() {
    let source = indoc! {r#"
        fn forget(value: Result[Number, [:known | e]]) Result[Number, [:known | f]] {
            value
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn error_row_inclusion_preserves_a_shared_tail_when_widening() {
    let source = indoc! {r#"
        fn widen(value: Result[Number, [:known | e]]) Result[Number, [:known | :extra | e]] {
            value
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn error_row_inclusion_cannot_erase_a_tail_during_try() {
    let source = indoc! {r#"
        fn forget(value: Result[Number, [:known | e]]) Result[Number, [:known | f]] {
            let number = value?
            Ok(number)
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn error_row_inclusion_cannot_replace_a_tail_through_an_inferred_helper() {
    let source = indoc! {r#"
        fn forward(value: Result[Number, [:known | e]]) {
            let number = value?
            Ok(number)
        }
        fn forget(value: Result[Number, [:known | e]]) Result[Number, [:known | f]] {
            forward(value)
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn generic_error_row_identity_does_not_specialize_its_tail() {
    let source = indoc! {r#"
        fn preserve(value: Result[a, [:missing(String) | e]]) Result[a, [:missing(String) | e]] {
            value
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn error_row_aliases_preserve_open_tails_and_concrete_tags() {
    let source = indoc! {r#"
        type Outcome[a, e] = Result[a, [:missing(String) | e]]
        fn preserve(value: Outcome[a, e]) Outcome[a, e] { value }
        fn fail() Outcome[Number, [:timeout]] { Err(:timeout) }
        fn propagate() Outcome[Number, [:timeout]] {
            let value = preserve(fail())?
            Ok(value)
        }
        fn run() Number {
            match propagate() {
                Ok(value) => value,
                Err(:missing(message)) => String.length(message),
                Err(:timeout) => 42,
            }
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn error_row_aliases_reject_unlisted_tags_and_wrong_payloads() {
    for source in [
        indoc! {r#"
            type Outcome = Result[Number, [:missing(String)]]
            fn invalid() Outcome { Err(:timeout) }
        "#},
        indoc! {r#"
            type Outcome[e] = Result[Number, [:missing(String) | e]]
            fn invalid() Outcome[[:timeout]] { Err(:missing(42)) }
        "#},
    ] {
        assert!(infer(&Bump::new(), source).is_err(), "{source}");
    }
}

#[test]
fn record_aliases_substitute_open_and_concrete_row_arguments() {
    let source = indoc! {r#"
        type WithX[r] = { r | x: Number }
        fn preserve(record: WithX[r]) WithX[r] { record }
        fn run() String {
            let record: WithX[{ label: String }] = { x: 42, label: "kept" }
            preserve(record).label
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn alias_expansion_does_not_capture_caller_variable_names() {
    let source = indoc! {r#"
        type Pair[a, b] = (a, b)
        fn swap_names(left: b, right: a) Pair[b, a] { (left, right) }
        fn run() String { swap_names(42, "kept").1 }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn aliases_cannot_hide_generic_specialization_or_wrong_payloads() {
    for source in [
        indoc! {r#"
            type Identity[a] = a
            fn invalid(value: a) Identity[a] { 42 }
        "#},
        indoc! {r#"
            type Boxed[a] = { value: a }
            fn read(record: Boxed[String]) String { record.value }
            fn run() String { read({ value: 42 }) }
        "#},
    ] {
        assert!(infer(&Bump::new(), source).is_err(), "{source}");
    }
}

#[test]
fn record_aliases_expand_at_value_boundaries() {
    let source = indoc! {r#"
        type OptionalName = { name?: String }
        fn choose(flag: Bool, record: OptionalName) {
            if flag { { name: Some("present") } } else { record }
        }
        fn run() Option[String] { choose(false, {}).name }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn generic_aliases_substitute_independently_through_forward_references() {
    let source = indoc! {r#"
        type Outer[a] = Inner[a]
        type Inner[b] = { value: b }
        fn number(record: Outer[Number]) Number { record.value }
        fn text(record: Outer[String]) String { record.value }
        fn run() Number {
            let label: String = text({ value: "hello" })
            number({ value: 42 })
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn alias_expansion_preserves_a_generic_contract() {
    let source = indoc! {r#"
        type Identity[a] = a
        fn identity(value: a) Identity[a] { value }
        fn run() Number {
            let label: String = identity("hello")
            identity(42)
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn loop_results_preserve_optional_fields_in_both_break_orders() {
    for source in [
        indoc! {r#"
            fn choose(flag: Bool, record: { value?: Number }) {
                loop {
                    if flag { break { value: Some(42) } }
                    break record
                }
            }
            fn run() Option[Number] { choose(false, {}).value }
        "#},
        indoc! {r#"
            fn choose(flag: Bool, record: { value?: Number }) {
                loop {
                    if flag { break record }
                    break { value: Some(42) }
                }
            }
            fn run() Option[Number] { choose(true, {}).value }
        "#},
    ] {
        let bump = Bump::new();
        let result = infer(&bump, source);
        assert!(result.is_ok(), "{source}\n{result:?}");
    }
}

#[test]
fn loop_results_cannot_hide_optional_fields_from_required_readers() {
    let source = indoc! {r#"
        fn choose(flag: Bool, record: { value?: Number }) {
            loop {
                if flag { break { value: 42 } }
                break record
            }
        }
        fn run() Number { choose(false, {}).value }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn optional_record_patterns_bind_option_valued_reads() {
    let source = indoc! {r#"
        fn read(record: { value?: Number }) Option[Number] {
            let { value } = record
            value
        }
        fn run() Option[Number] { read({}) }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn optional_constructor_record_patterns_bind_option_valued_reads() {
    let source = indoc! {r#"
        enum Config { Config { value?: Number } }
        fn read(config: Config) Option[Number] {
            match config { Config::Config { value } => value }
        }
        fn run() Option[Number] { read(Config::Config {}) }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn option_ordering_requires_payload_ordering() {
    let source = indoc! {r#"
        fn invalid(left: Option[fn(Number) Number], right: Option[fn(Number) Number]) Ordering {
            compare(left, right)
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn existing_record_aliases_cannot_gain_option_defaults_or_payload_lifts() {
    for source in [
        indoc! {r#"
            fn read(record: { value?: Number }) Option[Number] { record.value }
            fn invalid() {
                let record = {}
                read(record)
            }
        "#},
        indoc! {r#"
            fn read(record: { value?: Number }) Option[Number] { record.value }
            fn invalid() {
                let record = { value: 42 }
                read(record)
            }
        "#},
    ] {
        assert!(solve_input(&Bump::new(), source).is_err(), "{source}");
    }
}

#[test]
fn optional_record_patterns_cannot_extract_a_required_payload() {
    let source = indoc! {r#"
        fn read(record: { value?: Number }) Number {
            let { value } = record
            value
        }
        fn run() Number { read({}) }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn optional_record_assignment_stores_an_option() {
    let source = indoc! {r#"
        fn run() Option[Number] {
            let record: { value?: Number } = {}
            record.value = Some(42)
            record.value
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn optional_record_assignment_does_not_lift_a_bare_payload() {
    let source = indoc! {r#"
        fn run() {
            let record: { value?: Number } = {}
            record.value = 42
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn optional_record_assignment_cannot_read_an_absent_compound_target() {
    let source = indoc! {r#"
        fn run() {
            let record: { value?: Number } = {}
            record.value += 1
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn optional_record_assignment_cannot_traverse_an_absent_parent() {
    let source = indoc! {r#"
        fn run() {
            let record: { child?: { value: Number } } = {}
            record.child.value = 42
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn branch_results_preserve_option_fields() {
    for source in [
        indoc! {r#"
            fn choose(flag: Bool, user: { name?: String }) {
                if flag { { name: Some("present") } } else { user }
            }
            fn run() Option[String] { choose(false, {}).name }
        "#},
        indoc! {r#"
            fn choose(flag: Bool, user: { name?: String }) {
                if flag { user } else { { name: Some("present") } }
            }
            fn run() Option[String] { choose(false, {}).name }
        "#},
        indoc! {r#"
            fn choose(flag: Bool, user: { name?: String }) {
                match flag {
                    true => ({ name: Some("present") }),
                    false => user,
                }
            }
            fn run() Option[String] { choose(false, {}).name }
        "#},
    ] {
        assert!(infer(&Bump::new(), source).is_ok(), "{source}");
    }
}

#[test]
fn branch_results_cannot_hide_optional_fields_from_required_readers() {
    let source = indoc! {r#"
        fn choose(flag: Bool, user: { name?: String }) {
            if flag { { name: "present" } } else { user }
        }
        fn run() Number { String.length(choose(false, {}).name) }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn pipes_cannot_turn_optional_fields_into_required_fields() {
    let source = indoc! {r#"
        fn required(user: { name: String }) Number { String.length(user.name) }
        fn optional(user: { name?: String }) Number { user |> required }
        fn run() Number { optional({}) }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn lambda_returns_cannot_turn_optional_fields_into_required_fields() {
    let source = indoc! {r#"
        fn run(user: { name?: String }) Number {
            let get = () { name: String } -> user
            String.length(get().name)
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn fresh_optional_record_containers_accept_present_fields() {
    let source = indoc! {r#"
        fn run() Option[String] {
            let users: Array[{ name?: String }] = [{ name: "present" }]
            users[0].name
        }
    "#};
    assert!(infer(&Bump::new(), source).is_ok());
}

#[test]
fn shared_record_containers_cannot_weaken_field_presence() {
    let source = indoc! {r#"
        fn run() Number {
            let users = [{ name: "present" }]
            let alias: Array[{ name?: String }] = users
            Array.push(alias, {})
            String.length(users[1].name)
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn optional_record_annotations_survive_local_and_global_construction() {
    let source = indoc! {r#"
        let shared: { name?: String } = {}
        fn read() Option[String] {
            let local: { name?: String } = {}
            let global: Option[String] = shared.name
            local.name
        }
        fn present() Option[String] {
            let local: { name?: String } = { name: "present" }
            local.name
        }
    "#};
    assert!(infer(&Bump::new(), source).is_ok());
}

#[test]
fn record_rows_reject_direct_and_mutual_payload_cycles() {
    for source in [
        indoc! {r#"
            fn cycle(record) { record.next = record }
        "#},
        indoc! {r#"
            fn cycle(left, right) {
                left.next = right
                right.next = left
            }
        "#},
    ] {
        let bump = Bump::new();
        let errors = solve_input(&bump, source)
            .expect_err("finite record types cannot contain themselves through fields");
        assert!(
            errors.iter().any(|error| matches!(
                error,
                alder_solve::SolveError::Core(Error {
                    kind: ErrorKind::InfiniteType { .. },
                    ..
                })
            )),
            "{errors:?}"
        );
    }
}

#[test]
fn record_rows_instantiate_independently_and_retain_extra_fields() {
    let source = indoc! {r#"
        fn retain(row) {
            let value: Number = row.x
            row
        }
        fn run() Number {
            let first = retain({ x: 1, extra: 42 })
            let second = retain({ x: 2, label: "hello" })
            let label: String = second.label
            first.extra
        }
    "#};
    assert!(infer(&Bump::new(), source).is_ok());
}

#[test]
fn shared_record_tails_reject_incompatible_extra_fields() {
    let source = indoc! {r#"
        fn both(left: { r | x: Number }, right: { r | y: Number }) Number {
            left.x + right.y
        }
        fn run() Number {
            both({ x: 1, extra: 42 }, { y: 2, extra: "wrong" })
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn record_rows_accumulate_fields_independently_of_access_order() {
    for source in [
        indoc! {r#"
            fn sum(record) Number { record.x + record.y }
            fn run() Number { sum({ x: 20, y: 22 }) }
        "#},
        indoc! {r#"
            fn sum(record) Number { record.y + record.x }
            fn run() Number { sum({ x: 20, y: 22 }) }
        "#},
    ] {
        assert!(infer(&Bump::new(), source).is_ok(), "{source}");
    }
}

#[test]
fn independent_open_spreads_cannot_promise_only_the_left_universal_tail() {
    let source = indoc! {r#"
        fn invalid(left: { r | value: Number }, right: { s | value: Number }) ({ r | value: Number }) {
            { ..left, ..right }
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn shared_open_spreads_preserve_their_common_universal_tail() {
    let source = indoc! {r#"
        fn valid(left: { r | value: Number }, right: { r | value: Number }) ({ r | value: Number }) {
            { ..left, ..right }
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn independent_open_spreads_allow_a_guaranteed_rightmost_field() {
    let source = indoc! {r#"
        fn valid(left: { r | other: Bool }, right: { s | value: Number }) Number {
            let merged = { ..left, ..right }
            merged.value
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn independent_open_spreads_cannot_strengthen_a_declared_universal_contract() {
    let source = indoc! {r#"
        fn invalid(left: { r | value: Number }, right: { s | other: Bool }) Number {
            let merged = { ..left, ..right }
            merged.value
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn trait_overlay_contracts_reject_unknown_right_hand_overwrites() {
    for source in [
        indoc! {r#"
            fn merge(left, right) { { ..left, ..right } }
            trait Read[a] {
                fn read(marker: a, left: { r | value: Number }, right: { s | other: Bool }) Number {
                    merge(left, right).value
                }
            }
            impl Read[Number] {}
        "#},
        indoc! {r#"
            fn merge(left, right) { { ..left, ..right } }
            trait Read[a] {
                fn read(marker: a, left: { r | value: Number }, right: { s | other: Bool }) Number
            }
            impl Read[Number] {
                fn read(marker: Number, left: { r | value: Number }, right: { s | other: Bool }) Number {
                    merge(left, right).value
                }
            }
        "#},
    ] {
        let bump = Bump::new();
        let errors = solve_input(&bump, source)
            .expect_err("the universal right row may overwrite value with a String");
        assert!(
            errors.iter().any(|error| matches!(
                error,
                alder_solve::SolveError::Core(Error {
                    kind: ErrorKind::GenericSpecialization { .. },
                    ..
                })
            )),
            "{source}\n{errors:?}"
        );
    }
}

#[test]
fn trait_overlay_contracts_accept_a_guaranteed_final_overwrite() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        trait Read[a] {
            fn read(marker: a, left: { r | value: Number }, right: { s | other: Bool }) Number {
                let result = { ..merge(left, right), value: 42 }
                result.value
            }
        }
        impl Read[Number] {}
        impl Read[String] {
            fn read(marker: String, left: { r | value: Number }, right: { s | other: Bool }) Number {
                let result = { ..merge(left, right), value: 7 }
                result.value
            }
        }
        fn run() (Number, Number) {
            (
                read(0, { value: 0, extra: true }, { value: "overwritten", other: false }),
                read("marker", { value: 0, extra: "text" }, { value: false, other: true }),
            )
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn independent_open_spreads_preserve_disjoint_fields() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn run() Number {
            let merged = merge({ x: 20 }, { y: 22 })
            merged.x + merged.y
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn independent_open_spreads_compose_through_intermediate_results() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn merge_three(first, second, third) { merge(merge(first, second), third) }
        fn run() Number {
            let merged = merge_three({ x: 20 }, { y: 22 }, { extra: true })
            merged.x + merged.y
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn independent_open_spreads_survive_higher_order_forwarding() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn apply(operation, left, right) { operation(left, right) }
        fn run() Number {
            let merged = apply(merge, { x: 20 }, { y: 22 })
            merged.x + merged.y
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn independent_open_spreads_reject_bad_composed_result_contracts() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn merge_three(first, second, third) { merge(merge(first, second), third) }
        fn run() Number {
            merge_three({ value: 42 }, { extra: true }, { value: "wrong" }).value
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn independent_open_spreads_reject_hidden_contracts_through_composition() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn invalid(left: { r | value: Number }, right: { s | other: Bool }) Number {
            let intermediate = merge(left, right)
            let merged = merge(intermediate, { marker: true })
            merged.value
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn independent_open_spreads_allow_final_overrides_through_composition() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn valid(left: { r | value: Number }, right: { s | other: Bool }) Number {
            let intermediate = merge(left, right)
            let merged = merge(intermediate, { value: 42 })
            merged.value
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn independent_open_spreads_instantiate_separately_through_an_alias() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        let operation = merge
        fn run() Number {
            let first = operation({ value: "discarded" }, { value: 42 })
            let second = operation({ value: false }, { value: "text" })
            first.value + String.length(second.value)
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn partial_overlay_preserves_a_guaranteed_generic_payload() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn read(left: { r | marker: Bool }, right: { s | value: a }) a {
            merge(left, right).value
        }
        fn run() String { read({ marker: true, value: 42 }, { value: "right" }) }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn partial_overlay_rejects_wrong_known_payload_without_closed_inputs() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn invalid(left) Number { merge(left, { value: "wrong" }).value }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn partial_overlay_preserves_optional_presence_with_an_unrelated_open_tail() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn read(right: { r | value?: Number }) Option[Number] {
            merge({ marker: true }, right).value
        }
        fn run() Option[Number] { read({ extra: "unrelated" }) }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn partial_overlay_option_field_overwrites_an_unknown_row_field() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn replace(left: { r | marker: Bool }, right: { value?: Number }) Option[Number] {
            merge(left, right).value
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_ok());
}

#[test]
fn partial_overlay_option_field_replaces_an_earlier_value_with_an_open_tail() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn read(right: { r | value?: Number }) Option[Number] {
            merge({ value: 42, marker: true }, right).value
        }
        fn run() Option[Number] { read({ marker: "overwritten" }) }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn overlay_cannot_hide_a_known_overwrite_in_a_preserved_universal_tail() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn invalid(record: { r | marker: Bool }) ({ r | marker: Bool }) {
            merge(record, { value: "replacement" })
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn overlay_can_describe_a_known_overwrite_explicitly_in_its_result() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn replace(record: { r | marker: Bool }) ({ r | marker: Bool, value: String }) {
            merge(record, { value: "replacement" })
        }
        fn run() Number {
            let result = replace({ marker: true, value: 42, extra: 7 })
            String.length(result.value) + result.extra
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn explicit_option_fallbacks_preserve_three_declared_generic_errors() {
    let source = indoc! {r#"
        fn forward(left: { r | result: Result[Number, e] }, middle: { s | result?: Result[Number, f] }, right: { t | result?: Result[Number, g] }) {
            let value = match right.result {
                Some(result) => result?,
                None => match middle.result {
                    Some(result) => result?,
                    None => left.result?,
                },
            }
            Ok(value)
        }
        fn first() Result[Number, [:first]] { Err(:first) }
        fn second() Result[Number, [:second]] { Err(:second) }
        fn third() Result[Number, [:third]] { Err(:third) }
        fn run() Number {
            match forward({ result: first() }, { result: second() }, { result: third() }) {
                Ok(value) => value,
                Err(:first) => 1,
                Err(:second) => 2,
                Err(:third) => 3,
            }
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn explicit_option_fallbacks_reject_a_missing_third_error() {
    let source = indoc! {r#"
        fn invalid(left: { r | result: Result[Number, [:first]] }, middle: { s | result?: Result[Number, [:second]] }, right: { t | result?: Result[Number, [:third]] }) Result[Number, [:first | :second]] {
            let value = match right.result {
                Some(result) => result?,
                None => match middle.result {
                    Some(result) => result?,
                    None => left.result?,
                },
            }
            Ok(value)
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn explicit_option_fallback_instantiations_remain_independent() {
    let source = indoc! {r#"
        fn forward(left: { result: Result[Number, e] }, right: { result?: Result[Number, f] }) {
            let value = match right.result { Some(result) => result?, None => left.result? }
            Ok(value)
        }
        fn first() Result[Number, [:first]] { Err(:first) }
        fn second() Result[Number, [:second]] { Err(:second) }
        fn run() Number {
            let a = forward({ result: first() }, { result: first() })
            let b = forward({ result: second() }, { result: second() })
            let x = match a { Ok(value) => value, Err(:first) => 1 }
            let y = match b { Ok(value) => value, Err(:second) => 2 }
            x + y
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn explicit_option_fallback_preserves_mutable_success_payload_types() {
    let source = indoc! {r#"
        fn invalid(left: { result: Result[Array[Number], [:first]] }, right: { result?: Result[Array[String], [:second]] }) {
            match right.result { Some(result) => result, None => left.result }
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn explicit_option_fallback_cannot_change_nested_mutable_field_types() {
    let source = indoc! {r#"
        fn invalid(left: { result: Result[Array[{ value: Number }], [:first]] }, right: { result?: Result[Array[{ value?: Number }], [:second]] }) {
            match right.result { Some(result) => result, None => left.result }
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn explicit_option_fallback_generic_error_rows_preserve_both_sources() {
    let source = indoc! {r#"
        fn forward(left: { result: Result[Number, e] }, right: { result?: Result[Number, f] }) {
            let value = match right.result { Some(result) => result?, None => left.result? }
            Ok(value)
        }
        fn first() Result[Number, [:first]] { Err(:first) }
        fn second() Result[Number, [:second]] { Err(:second) }
        fn run() Number {
            match forward({ result: first() }, { result: second() }) {
                Ok(value) => value,
                Err(:first) => 1,
                Err(:second) => 2,
            }
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn explicit_option_fallback_generic_error_rows_cannot_promise_only_one_source() {
    let source = indoc! {r#"
        fn invalid(left: { result: Result[Number, e] }, right: { result?: Result[Number, f] }) Result[Number, e] {
            let value = match right.result { Some(result) => result?, None => left.result? }
            Ok(value)
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn explicit_option_fallback_error_rows_include_both_sources() {
    let source = indoc! {r#"
        fn forward(left: { result: Result[Number, [:first]] }, right: { result?: Result[Number, [:second]] }) {
            let value = match right.result { Some(result) => result?, None => left.result? }
            Ok(value)
        }
        fn first() Result[Number, [:first]] { Err(:first) }
        fn run() Number {
            match forward({ result: first() }, {}) {
                Ok(value) => value,
                Err(:first) => 1,
                Err(:second) => 2,
            }
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn explicit_option_fallback_error_rows_cannot_drop_the_none_branch() {
    let source = indoc! {r#"
        fn forward(left: { result: Result[Number, [:first]] }, right: { result?: Result[Number, [:second]] }) {
            let value = match right.result { Some(result) => result?, None => left.result? }
            Ok(value)
        }
        fn first() Result[Number, [:first]] { Err(:first) }
        fn run() Result[Number, [:second]] { forward({ result: first() }, {}) }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn overlay_error_rows_preserve_the_selected_payload() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn forward(left, right) {
            let value = merge(left, right).result?
            Ok(value)
        }
        fn first() Result[Number, [:first]] { Err(:first) }
        fn second() Result[Number, [:second]] { Err(:second) }
        fn run() Result[Number, [:second]] {
            forward({ result: first() }, { result: second() })
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn overlay_error_rows_cannot_drop_the_selected_error() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn forward(left, right) {
            let value = merge(left, right).result?
            Ok(value)
        }
        fn first() Result[Number, [:first]] { Err(:first) }
        fn second() Result[Number, [:second]] { Err(:second) }
        fn run() Result[Number, [:first]] {
            forward({ result: first() }, { result: second() })
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn overlay_factory_preserves_captured_input_relationships() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn factory(left) { right -> merge(left, right) }
        fn run() Number {
            let operation = factory({ x: 20 })
            let result = operation({ y: 22 })
            result.x + result.y
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn overlay_factory_rejects_a_wrong_captured_overwrite_result() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn factory(left) { right -> merge(left, right) }
        fn run() Number {
            let operation = factory({ value: 42 })
            operation({ value: "wrong" }).value
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn overlay_factory_cannot_regeneralize_captured_mutable_payloads() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        let shared = []
        fn factory() { patch -> merge({ values: shared }, patch) }
        fn run() {
            Array.push(shared, 42)
            let operation = factory()
            let strings: Array[String] = operation({ marker: true }).values
            String.length(strings[0])
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn identity_overlays_cannot_hide_contradictory_field_reads() {
    for source in [
        indoc! {r#"
            fn copy(value) { { ..value } }
            fn invalid(value) {
                let first: Number = copy(value).field
                let second: String = value.field
                (first, second)
            }
        "#},
        indoc! {r#"
            fn copy(value) { { ..value, ..value } }
            fn invalid(value) {
                let first: Number = copy(value).field
                let second: String = value.field
                (first, second)
            }
        "#},
        indoc! {r#"
            fn copy(value) { { ..{}, ..value, ..{} } }
            fn invalid(value) {
                let first: Number = copy(value).field
                let second: String = value.field
                (first, second)
            }
        "#},
    ] {
        let bump = Bump::new();
        let errors = solve_input(&bump, source)
            .expect_err("identity overlays cannot change inherited field types");
        assert!(
            errors.iter().any(|error| matches!(
                error,
                alder_solve::SolveError::Core(Error {
                    kind: ErrorKind::Mismatch { .. },
                    ..
                })
            )),
            "{source}\n{errors:?}"
        );
    }
}

#[test]
fn mutually_recursive_overlays_reject_unbroken_payload_growth() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn even(count: Number, value, other) {
            let merged = merge(value, other)
            if count == 0 { merged }
            else { odd(count - 1, { nested: merged }, other) }
        }
        fn odd(count: Number, value, other) {
            let merged = merge(value, other)
            if count == 0 { merged }
            else { even(count - 1, { nested: merged }, other) }
        }
        fn run() { even(2, { nested: { nested: 0 } }, {}) }
    "#};
    let bump = Bump::new();
    let errors = solve_input(&bump, source)
        .expect_err("mutual recursion cannot hide incompatible recursive payload growth");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::InfiniteType { .. } | ErrorKind::Mismatch { .. },
                ..
            })
        )),
        "{errors:?}"
    );
}

#[test]
fn mutually_recursive_overlays_preserve_cycle_breaking_overwrites() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn even(count: Number, value, other) {
            let merged = merge(value, other)
            if count == 0 { merged }
            else { odd(count - 1, { nested: merged }, other) }
        }
        fn odd(count: Number, value, other) {
            let merged = merge(value, other)
            if count == 0 { merged }
            else { even(count - 1, { nested: merged }, other) }
        }
        fn number() Number { even(2, { nested: { nested: 0 } }, { nested: 42 }).nested }
        fn text() String { odd(3, { nested: { nested: "seed" } }, { nested: "text" }).nested }
    "#};
    solve_input(&Bump::new(), source)
        .expect("independent calls retain overwrites that break mutual payload recursion");
}

#[test]
fn recursive_overlay_instantiation_rejects_an_unbroken_payload_cycle() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn repeat(count: Number, value, other) {
            let merged = merge(value, other)
            if count == 0 { merged }
            else { repeat(count - 1, { nested: merged }, other) }
        }
        fn run() { repeat(2, { nested: { nested: 0 } }, {}) }
    "#};
    let bump = Bump::new();
    let errors = solve_input(&bump, source)
        .expect_err("a call cannot instantiate a recursive overlay with an infinite payload");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::InfiniteType { .. } | ErrorKind::Mismatch { .. },
                ..
            })
        )),
        "{errors:?}"
    );
}

#[test]
fn recursive_overlay_instantiation_keeps_a_cycle_breaking_overwrite() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn repeat(count: Number, value, other) {
            let merged = merge(value, other)
            if count == 0 { merged }
            else { repeat(count - 1, { nested: merged }, other) }
        }
        fn run() Number { repeat(2, { nested: { nested: 0 } }, { nested: 42 }).nested }
    "#};
    solve_input(&Bump::new(), source)
        .expect("a guaranteed overwrite breaks the recursive payload equation");
}

#[test]
fn recursive_overlay_rejects_an_infinite_nested_payload() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn invalid(value, other: { marker: Bool }) {
            let merged = merge(value, other)
            invalid({ nested: merged }, other)
        }
    "#};
    let bump = Bump::new();
    let errors = solve_input(&bump, source).expect_err("nested payload must fail the occurs check");
    assert!(
        matches!(
            errors.as_slice(),
            [alder_solve::SolveError::Core(Error {
                kind: ErrorKind::InfiniteType { .. },
                ..
            })]
        ),
        "{errors:?}"
    );
}

#[test]
fn recursive_overlay_allows_a_required_override_to_break_payload_recursion() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn repeat(value, other: { nested: Number }) {
            let merged = merge(value, other)
            repeat({ nested: merged }, other)
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn recursive_overlay_cannot_prove_a_universal_field_from_its_result_obligation() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn repeat(count: Number, left, right) {
            let merged = merge(left, right)
            if count == 0 { merged }
            else { repeat(count - 1, merged, right) }
        }
        fn invalid(left: { r | value: Number }, right: { s | marker: Bool }) Number {
            repeat(2, left, right).value
        }
    "#};
    let bump = Bump::new();
    let errors = solve_input(&bump, source)
        .expect_err("recursive result requirements cannot prove an unknown overwrite is Number");
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::GenericSpecialization { .. },
                ..
            })
        )),
        "{errors:?}"
    );
}

#[test]
fn recursive_overlay_preserves_a_final_field_with_a_shared_universal_tail() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn repeat(count: Number, left, right) {
            let merged = { ..merge(left, right), value: 42 }
            if count == 0 { merged }
            else { repeat(count - 1, merged, right) }
        }
        fn valid(left: { r | value: Number, marker: Bool }, right: { r | value: String, marker: Bool }) Number {
            repeat(2, left, right).value
        }
    "#};
    solve_input(&Bump::new(), source)
        .expect("a final explicit overwrite proves the field regardless of recursive inputs");
}

#[test]
fn recursive_overlay_forwarding_preserves_independent_inputs() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn repeat(count: Number, left, right) {
            if count == 0 { merge(left, right) }
            else { repeat(count - 1, left, right) }
        }
        fn run() Number {
            let result = repeat(3, { x: 20 }, { y: 22 })
            result.x + result.y
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn reversed_overlay_inputs_can_have_different_result_payloads() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn both(left, right) {
            let forward: Number = merge(left, right).value
            let reverse: String = merge(right, left).value
            (forward, reverse)
        }
        fn run() (Number, String) { both({ value: "left" }, { value: 42 }) }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn repeated_overlay_inputs_cannot_have_conflicting_result_payloads() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn invalid(left, right) {
            let first: Number = merge(left, right).value
            let second: String = merge(left, right).value
            (first, second)
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn partial_overlay_preserves_optional_presence_of_a_known_field() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn read(left: { value?: Number }, right: { marker: Bool }) Option[Number] {
            merge(left, right).value
        }
        fn run() Option[Number] { read({}, { marker: true }) }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn independent_open_spreads_preserve_right_biased_overwrites() {
    let source = indoc! {r#"
        fn merge(left, right) { { ..left, ..right } }
        fn run() String { merge({ value: 42 }, { value: "right" }).value }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn open_spread_cannot_hide_an_incompatible_overwrite() {
    let source = indoc! {r#"
        fn fallback(record) Number {
            let result = { value: 42, ..record }
            result.value
        }
        fn run() Number { fallback({ value: "wrong" }) }
    "#};
    assert!(solve_input(&Bump::new(), source).is_err());
}

#[test]
fn open_spread_accepts_absent_and_compatible_overwrites() {
    let source = indoc! {r#"
        fn fallback(record) Number {
            let result = { value: 42, ..record }
            result.value
        }
        fn run() Number {
            fallback({}) + fallback({ extra: true }) + fallback({ value: 7 })
        }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn option_field_spread_overwrites_fields_hidden_in_an_open_row() {
    let source = indoc! {r#"
        fn fallback(record, patch: { value?: String }) Option[String] {
            let result = { ..record, ..patch }
            result.value
        }
        fn run() Option[String] { fallback({ value: 42 }, {}) }
    "#};
    assert!(solve_input(&Bump::new(), source).is_ok());
}

#[test]
fn final_required_override_does_not_constrain_hidden_row_payloads() {
    let source = indoc! {r#"
        fn replace(record: { r | marker: Bool }) ({ r | marker: Bool, value: Bool }) {
            { ..{ value: 42 }, ..record, value: true }
        }
        fn run() Bool { replace({ marker: true, value: "discarded" }).value }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn option_field_spread_replaces_an_earlier_required_value() {
    let source = indoc! {r#"
        fn fallback(record: { value?: Number }) Option[Number] {
            let result = { value: 42, ..record }
            result.value
        }
        fn run() Option[Number] { fallback({}) }
    "#};
    let bump = Bump::new();
    let result = solve_input(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn optional_spreads_keep_optional_presence_without_a_required_fallback() {
    let source = indoc! {r#"
        fn merge(left: { value?: Number }, right: { value?: Number }) Option[Number] {
            let result = { ..left, ..right }
            result.value
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_ok());
}

#[test]
fn final_required_field_replaces_all_optional_spread_alternatives() {
    let source = indoc! {r#"
        fn replace(record: { value?: String }) Bool {
            let result = { ..{ value: 42 }, ..record, value: true }
            result.value
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_ok());
}

#[test]
fn required_spread_can_replace_an_incompatible_earlier_payload() {
    let source = indoc! {r#"
        fn replace(record: { value: String }) String {
            let result = { value: 42, ..record }
            result.value
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_ok());
}

#[test]
fn option_field_spread_discards_an_incompatible_earlier_value() {
    let source = indoc! {r#"
        fn fallback(record: { value?: String }) Option[String] {
            let result = { value: 42, ..record }
            result.value
        }
    "#};
    assert!(solve_input(&Bump::new(), source).is_ok());
}

#[test]
fn record_rows_preserve_input_output_relationships_through_spread() {
    let source = indoc! {r#"
        fn rename(user: { r | name: String }, name: String) ({ r | name: String }) {
            { ..user, name }
        }
        fn run() Number {
            let user = rename({ name: "old", score: 42 }, "new")
            user.score
        }
    "#};
    assert!(infer(&Bump::new(), source).is_ok());
}

#[test]
fn record_rows_do_not_allow_fabricating_a_promised_tail() {
    let source = indoc! {r#"
        fn lose(user: { r | name: String }) ({ r | name: String }) {
            { name: "lost" }
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn optional_record_fields_cannot_satisfy_required_field_access() {
    let source = indoc! {r#"
        fn required(user: { name: String }) Number { String.length(user.name) }
        fn optional(user: { name?: String }) Number { required(user) }
        fn run() Number { optional({}) }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn skipped_boolean_operands_do_not_constrain_loop_exits() {
    let source = indoc! {r#"
        fn answer() Number {
            loop {
                let skipped = false && { break "unreachable" }
                let also_skipped = true || { break "unreachable" }
                break 42
            }
        }
    "#};
    assert!(infer(&Bump::new(), source).is_ok());
}

#[test]
fn false_match_guards_do_not_constrain_loop_exits() {
    let source = indoc! {r#"
        fn answer(flag: Bool) Number {
            loop {
                match flag {
                    true if false => { break "unreachable" },
                    _ => { break 42 },
                }
            }
        }
    "#};
    assert!(infer(&Bump::new(), source).is_ok());
}

#[test]
fn skipped_exits_do_not_make_an_infinite_loop_produce_unit() {
    let source = indoc! {r#"
        fn boolean_exit() Number {
            loop { false && { break } }
        }
        fn guarded_exit(flag: Bool) Number {
            loop {
                match flag {
                    true if false => { break },
                    _ => { continue },
                }
            }
        }
        fn required_operand() Number {
            let result = true && { return 42 }
        }
    "#};
    assert!(infer(&Bump::new(), source).is_ok());
}

#[test]
fn potentially_reached_conditional_breaks_must_agree() {
    for source in [
        indoc! {r#"
            fn answer(flag: Bool) Number {
                loop {
                    let result = flag && { break "wrong" }
                    break 42
                }
            }
        "#},
        indoc! {r#"
            fn answer(flag: Bool) Number {
                loop {
                    match flag {
                        true if flag => { break "wrong" },
                        _ => { break 42 },
                    }
                }
            }
        "#},
    ] {
        assert!(infer(&Bump::new(), source).is_err(), "{source}");
    }
}

#[test]
fn remaining_operands_after_an_exit_do_not_constrain_loop_results() {
    for source in [
        indoc! {r#"
            fn answer() Number {
                loop { Err(:pair({ break 42 }, { break "unreachable" })) }
            }
        "#},
        indoc! {r#"
            fn answer() Number {
                loop { `${{ break 42 }}${{ break "unreachable" }}` }
            }
        "#},
        indoc! {r#"
            fn tag(parts: Array[String], a: Number, b: Number) String { "unused" }
            fn answer() Number {
                loop { tag`${{ break 42 }}${{ break "unreachable" }}` }
            }
        "#},
        indoc! {r#"
            fn answer() Number {
                loop { ({ break 42 })[{ break "unreachable" }] }
            }
        "#},
        indoc! {r#"
            fn answer() Number {
                loop { let ignored = { a: { break 42 }, b: { break "unreachable" } } }
            }
        "#},
        indoc! {r#"
            fn answer() Number {
                loop {
                    let ignored: { a: Number, b: Number } = {
                        a: { break 42 }, b: { break "unreachable" }
                    }
                }
            }
        "#},
    ] {
        let bump = Bump::new();
        let result = infer(&bump, source);
        assert!(result.is_ok(), "{source}: {result:?}");
    }
}

#[test]
fn assignment_operands_after_an_exit_do_not_constrain_loop_results() {
    let source = indoc! {r#"
        fn answer() Number {
            let values = [[0]]
            loop {
                values[{ break 42 }][{ break "unreachable index" }] = { break "unreachable value" }
            }
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn aggregate_operands_after_an_exit_do_not_constrain_loop_results() {
    for source in [
        indoc! {r#"
            fn answer() Number {
                loop { [{ break 42 }, { break "unreachable" }] }
            }
        "#},
        indoc! {r#"
            fn answer() Number {
                loop { ({ break 42 }, { break "unreachable" }) }
            }
        "#},
        indoc! {r#"
            fn answer() Number {
                loop {
                    let values: Array[Number] = [{ break 42 }, { break "unreachable" }]
                }
            }
        "#},
    ] {
        let bump = Bump::new();
        let result = infer(&bump, source);
        assert!(result.is_ok(), "{source}: {result:?}");
    }
}

#[test]
fn potentially_reached_aggregate_exits_still_must_agree() {
    let source = indoc! {r#"
        fn answer(flag: Bool) Number {
            loop { [if flag { break 42 } else { 0 }, { break "wrong" }] }
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn loop_break_values_determine_the_result_type() {
    let source = indoc! {r#"
        fn answer() Number { loop { break 42 } }
        fn nested() Number {
            loop {
                let inner: String = loop { break "inner" }
                while false { break }
                break 42
            }
        }
        async fn suspended() Number {
            loop {
                Task.sleep(1).await
                break 42
            }
        }
    "#};
    assert!(infer(&Bump::new(), source).is_ok());
}

#[test]
fn incompatible_breaks_and_statement_loop_values_are_rejected() {
    for source in [
        indoc! {r#"
            fn incompatible(flag: Bool) {
                loop {
                    if flag { break 42 }
                    break "wrong"
                }
            }
        "#},
        indoc! {r#"
            fn incompatible(flag: Bool) {
                loop {
                    if flag { break }
                    break 42
                }
            }
        "#},
        "fn incompatible() { while true { break 42 } }",
        "fn incompatible() { for value in [42] { break value } }",
    ] {
        assert!(infer(&Bump::new(), source).is_err(), "{source}");
    }
}

#[test]
fn unreachable_breaks_do_not_constrain_a_live_loop_result() {
    let source = indoc! {r#"
        fn after_break() Number {
            loop {
                break 42
                break "unreachable"
            }
        }
        fn false_branch() Number {
            loop {
                if false { break "unreachable" }
                break 42
            }
        }
    "#};
    assert!(infer(&Bump::new(), source).is_ok());
}

#[test]
fn possibly_reached_sibling_pins_still_constrain_loop_results() {
    let source = indoc! {r#"
        fn invalid(flag: Bool) Number {
            loop {
                match (0, 0) {
                    (^{ if flag { break 42 } else { 0 } }, ^{ break "wrong" }) => (),
                    _ => (),
                }
            }
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn exiting_nested_pins_skip_later_pattern_operands() {
    for source in [
        indoc! {r#"
        fn answer() Number {
            loop {
                match (0, 0) {
                    (^{ break 42 }, ^{ break "unreachable" }) => (),
                    _ => (),
                }
            }
        }
    "#},
        indoc! {r#"
        fn answer() Number {
            loop {
                match [0, 0] {
                    [^{ break 42 }, ^{ break "unreachable" }] => (),
                    _ => (),
                }
            }
        }
    "#},
        indoc! {r#"
        fn answer() Number {
            loop {
                match { a: 0, b: 0 } {
                    { a: ^{ break 42 }, b: ^{ break "unreachable" } } => (),
                    _ => (),
                }
            }
        }
    "#},
        indoc! {r#"
        enum Pair { Both(Number, Number) }
        fn answer() Number {
            loop {
                match Pair::Both(0, 0) {
                    Pair::Both(^{ break 42 }, ^{ break "unreachable" }) => (),
                    _ => (),
                }
            }
        }
    "#},
        indoc! {r#"
        enum Pair { Both { a: Number, b: Number } }
        fn answer() Number {
            loop {
                    match (Pair::Both { a: 0, b: 0 }) {
                    Pair::Both { a: ^{ break 42 }, b: ^{ break "unreachable" } } => (),
                    _ => (),
                }
            }
        }
    "#},
        indoc! {r#"
        fn answer(input: Result[(), [:pair(Number, Number)]]) Number {
            loop {
                match input {
                    Err(:pair(^{ break 42 }, ^{ break "unreachable" })) => (),
                    _ => (),
                }
            }
        }
    "#},
    ] {
        let bump = Bump::new();
        let result = infer(&bump, source);
        assert!(result.is_ok(), "{source}: {result:?}");
    }
}

#[test]
fn exiting_pins_skip_guards_bodies_and_later_arms() {
    let source = indoc! {r#"
        fn answer() Number {
            loop {
                match 0 {
                    ^{ break 42 } if { break "guard" } => { break "body" },
                    ^{ break "later pin" } => { break "later body" },
                    _ => { break "fallback" },
                }
            }
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn successful_guards_skip_later_alternative_pin_exits() {
    let source = indoc! {r#"
        fn answer() Number {
            loop {
                match (0, 0) {
                    (_, _) | (^{ break "unreachable" }, _) if true => { break 42 },
                    _ => { break "unreachable fallback" },
                }
            }
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn failed_guards_reach_later_alternative_pin_exits() {
    let source = indoc! {r#"
        fn invalid() Number {
            loop {
                match (0, 0) {
                    (_, _) | (^{ break "wrong" }, _) if false => (),
                    _ => { break 42 },
                }
            }
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn pin_breaks_cannot_disguise_a_loop_as_divergent() {
    let source = indoc! {r#"
        fn invalid() Number {
            loop { match 0 { ^{ break "wrong" } => (), _ => () } }
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn pin_expression_returns_must_satisfy_the_enclosing_function() {
    for source in [
        indoc! {r#"
        fn invalid() Number {
            match 0 { ^{ return } => 42, _ => 42 }
        }
    "#},
        indoc! {r#"
        fn invalid() Number {
            match (0, 1) { (^{ return }, _) => 42, _ => 42 }
        }
    "#},
        indoc! {r#"
        fn invalid() Number {
            match [0] { [^{ return }] => 42, _ => 42 }
        }
    "#},
        indoc! {r#"
        fn invalid() Number {
            match { value: 0 } { { value: ^{ return } } => 42, _ => 42 }
        }
    "#},
        indoc! {r#"
        fn invalid() Number {
            match Some(0) { Some(^{ return }) => 42, _ => 42 }
        }
    "#},
    ] {
        assert!(infer(&Bump::new(), source).is_err(), "{source}");
    }
}

#[test]
fn method_bodies_reject_zero_iteration_return_paths() {
    for source in [
        indoc! {r#"
            trait Read[a] {
                fn read(value: a, flag: Bool) Number {
                    while flag { return 42 }
                }
            }
        "#},
        indoc! {r#"
            trait Read[a] { fn read(value: a, flag: Bool) Number }
            impl Read[Number] {
                fn read(value: Number, flag: Bool) Number {
                    while flag { return 42 }
                }
            }
        "#},
        indoc! {r#"
            trait Read[a] {
                async fn read(value: a, flag: Bool) Number {
                    Task.sleep(0).await
                    while flag { return 42 }
                }
            }
        "#},
        indoc! {r#"
            trait Read[a] { async fn read(value: a, flag: Bool) Number }
            impl Read[Number] {
                async fn read(value: Number, flag: Bool) Number {
                    Task.sleep(0).await
                    while flag { return 42 }
                }
            }
        "#},
    ] {
        let bump = Bump::new();
        let errors = infer(&bump, source).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| matches!(error.kind, ErrorKind::MissingReturn { .. })),
            "{source}: {errors:?}"
        );
    }
}

#[test]
fn zero_iteration_loops_do_not_satisfy_a_return_contract() {
    for source in [
        indoc! {r#"
            fn missing(flag: Bool) Number {
                while flag { return 42 }
            }
        "#},
        indoc! {r#"
            fn missing(values: Array[Number]) Number {
                for value in values { return value }
            }
        "#},
        indoc! {r#"
            async fn missing(flag: Bool) Number {
                Task.sleep(1).await
                while flag { return 42 }
            }
        "#},
        indoc! {r#"
            fn missing(flag: Bool) Number {
                loop { if flag { break } }
            }
        "#},
        indoc! {r#"
            fn missing() Number {
                let inner = () -> { return 42 }
            }
        "#},
    ] {
        assert!(infer(&Bump::new(), source).is_err(), "{source}");
    }
}

#[test]
fn explicit_returns_in_all_branches_have_no_unit_fallthrough() {
    let source = indoc! {r#"
        fn choose(flag: Bool) Number {
            if flag { return 42 } else { return 43 }
        }
        fn choose_again(flag: Bool) Number {
            match flag {
                true => { return 42 },
                false => { return 43 },
            }
        }
    "#};
    assert!(infer(&Bump::new(), source).is_ok());
}

#[test]
fn diverging_loops_do_not_require_a_fake_return_value() {
    let source = indoc! {r#"
        fn forever() Number { loop {} }
        fn forever_while() String { while true {} }
        fn nested() Number { loop { loop { break } } }
        fn unreachable_exit() Number { loop { if false { break } } }
    "#};
    assert!(infer(&Bump::new(), source).is_ok());
}

#[test]
fn returning_lambda_block_has_no_unit_fallthrough() {
    let source = indoc! {r#"
        fn call() Number {
            let value = () -> { return 42 }
            value()
        }
    "#};
    assert!(infer(&Bump::new(), source).is_ok());
}

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
    let mut typ = render_type(annotation.typ);
    if !annotation.tuple_shapes.is_empty() {
        let constraints = annotation
            .tuple_shapes
            .iter()
            .map(|shape| {
                let elements = shape
                    .elements
                    .iter()
                    .map(|(index, typ)| format!("{index}: {}", render_type(typ)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "tuple {} length {} {{{elements}}}",
                    render_type(shape.tuple),
                    shape.length
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        typ.push_str(&format!(" where {constraints}"));
    }
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
            "fn({}) {}",
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
                .map(|field| format!("{}: {}", field.name, render_type(field.typ)))
                .collect::<Vec<_>>()
                .join(", ");
            match ext {
                RowExtension::Closed => format!("{{ {fields} }}"),
                RowExtension::Open(row) => format!("{{ {fields} | {row} }}"),
            }
        }
        Type::ErrorRow { tags, ext } => {
            let mut parts = tags
                .iter()
                .map(|tag| {
                    if tag.args.is_empty() {
                        format!(":{}", tag.name)
                    } else {
                        format!(
                            ":{}({})",
                            tag.name,
                            tag.args
                                .iter()
                                .map(|arg| render_type(arg))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                })
                .collect::<Vec<_>>();
            if let RowExtension::Open(row) = ext {
                parts.push((*row).to_owned());
            }
            format!("[{}]", parts.join(" | "))
        }
        Type::Alias { reference, .. } => reference.name.to_owned(),
    }
}

#[test]
fn synchronized_ref_accepts_task_callbacks_and_preserves_payload_types() {
    let bump = Bump::new();
    let result = solve_input(
        &bump,
        indoc! {r#"
        async fn fresh(value: a) SynchronizedRef[a] { SynchronizedRef.make(value).await }
        async fn run() String {
            let number = fresh(1).await
            let text = fresh("text").await
            SynchronizedRef.update(number, value -> async { value + 1 }).await
            SynchronizedRef.modify(number, value -> async { ("previous", value + 1) }).await
            SynchronizedRef.get(text).await
        }
    "#},
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn synchronized_ref_rejects_synchronous_callback_and_incompatible_alias() {
    for source in [
        indoc! {r#"
        async fn bad() {
            let cell = SynchronizedRef.make(0).await
            SynchronizedRef.update(cell, value -> value + 1).await
        }
    "#},
        indoc! {r#"
        async fn bad() String {
            let cell = SynchronizedRef.make([]).await
            let alias = cell
            SynchronizedRef.set(cell, [42]).await
            let strings: Array[String] = SynchronizedRef.get(alias).await
            strings[0]
        }
    "#},
    ] {
        let bump = Bump::new();
        let errors = solve_input(&bump, source).unwrap_err();
        assert!(
            errors.iter().any(|error| matches!(
                error,
                alder_solve::SolveError::Core(Error {
                    kind: ErrorKind::Mismatch { .. },
                    ..
                })
            )),
            "{source}\n{errors:?}"
        );
    }
}

#[test]
fn semaphore_preserves_independent_protected_result_types() {
    let bump = Bump::new();
    let result = solve_input(
        &bump,
        indoc! {r#"
        async fn protect(gate: Semaphore, task: Task[a]) a {
            Semaphore.withPermits(gate, 1, task).await
        }
        async fn run() String {
            let gate = Semaphore.make(2).await
            let number = protect(gate, async { 42 }).await
            let text = protect(gate, async { "text" }).await
            text
        }
    "#},
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn ref_captured_function_cell_cannot_be_specialized_through_an_alias() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        async fn bad() String {
            let cell = Ref.make(value -> value).await
            let read = () -> async { Ref.get(cell).await }
            Ref.set(cell, (value: Number) -> value + 1).await
            let operation = read().await
            operation("wrong")
        }
    "#},
    )
    .unwrap_err();
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::Mismatch { .. },
                ..
            })
        )),
        "{errors:?}"
    );
}

#[test]
fn ref_reusable_allocation_task_keeps_shared_array_payload_monomorphic() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        let allocation = Ref.make([])
        async fn write() {
            let cell = allocation.await
            Array.push(Ref.get(cell).await, 42)
        }
        async fn read() String {
            let cell = allocation.await
            let values: Array[String] = Ref.get(cell).await
            values[0]
        }
    "#},
    )
    .unwrap_err();
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::Mismatch { .. },
                ..
            })
        )),
        "{errors:?}"
    );
}

#[test]
fn ref_fresh_array_factory_retains_independent_instantiations() {
    let bump = Bump::new();
    let result = solve_input(
        &bump,
        indoc! {r#"
        async fn fresh() { Ref.make([]).await }
        async fn run() String {
            let numbers = fresh().await
            let strings = fresh().await
            Ref.set(numbers, [42]).await
            Ref.set(strings, ["text"]).await
            Ref.get(strings).await[0]
        }
    "#},
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn ref_operations_preserve_payload_and_callback_result_types() {
    let bump = Bump::new();
    let result = solve_input(
        &bump,
        indoc! {r#"
        async fn fresh(value: a) Ref[a] { Ref.make(value).await }
        async fn run() String {
            let number = fresh(1).await
            let text = fresh("text").await
            Ref.set(number, 2).await
            Ref.update(number, value -> value + 1).await
            Ref.modify(number, value -> (Ref.same(number, number), value + 1)).await
            Ref.get(text).await
        }
    "#},
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn ref_shared_payload_cannot_be_instantiated_incompatibly() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        async fn bad() String {
            let cell = Ref.make([]).await
            let alias = cell
            Ref.set(cell, [42]).await
            let strings: Array[String] = Ref.get(alias).await
            strings[0]
        }
    "#},
    )
    .unwrap_err();
    assert!(
        errors.iter().any(|error| matches!(
            error,
            alder_solve::SolveError::Core(Error {
                kind: ErrorKind::Mismatch { .. },
                ..
            })
        )),
        "{errors:?}"
    );
}

#[test]
fn shared_array_cannot_be_instantiated_at_incompatible_types() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        let shared = []
        fn bad() {
            Array.push(shared, 42)
            let strings: Array[String] = shared
            String.length(strings[0])
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn shared_map_cannot_be_instantiated_at_incompatible_types() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        let shared = Map.new()
        fn bad() {
            Map.set(shared, "key", 42)
            let strings: Map[String, String] = shared
            Map.get(strings, "key")
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn function_cannot_generalize_captured_shared_state() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        let shared = []
        fn values() { shared }
        fn bad() {
            Array.push(values(), 42)
            let strings: Array[String] = values()
            String.length(strings[0])
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn shared_alias_cannot_regeneralize_state() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        let shared = []
        let alias = shared
        fn bad() {
            Array.push(shared, 42)
            let strings: Array[String] = alias
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn shared_record_keeps_nested_state_monomorphic() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        let shared = { items: [] }
        fn bad() {
            Array.push(shared.items, 42)
            let strings: Array[String] = shared.items
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn shared_set_keeps_its_element_type() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        let shared = Set.new()
        fn bad() {
            Set.add(shared, 42)
            Set.add(shared, "text")
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn shared_task_cannot_regeneralize_captured_state() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        let shared = []
        async fn operation() {
            Task.sleep(0).await
            shared
        }
        let task = operation()
        async fn bad() {
            let numbers: Array[Number] = task.await
            let strings: Array[String] = task.await
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn exported_shared_state_requires_a_determined_type() {
    let bump = Bump::new();
    assert!(solve_input(&bump, "pub let shared = []").is_err());
}

#[test]
fn exported_closure_cannot_hide_an_unresolved_shared_type() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        let shared = []
        pub fn values() { shared }
    "#}
        )
        .is_err()
    );
}

#[test]
fn exported_shared_state_can_have_a_concrete_type() {
    let bump = Bump::new();
    solve_input(&bump, "pub let shared: Array[Number] = []")
        .expect("a concrete shared contract is safe across modules");
}

#[test]
fn array_factories_remain_polymorphic() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn empty() { [] }
        let factory = () -> []
        fn good() {
            let numbers: Array[Number] = empty()
            let strings: Array[String] = empty()
            let more_numbers: Array[Number] = factory()
            let more_strings: Array[String] = factory()
        }
    "#},
    )
    .expect("each call allocates independent state");
}

#[test]
fn builtin_string_length_rejects_a_number() {
    let bump = Bump::new();
    assert!(solve_input(&bump, "fn bad() Number { String.length(42) }").is_err());
}

#[test]
fn builtin_array_push_checks_the_element_type() {
    let bump = Bump::new();
    assert!(solve_input(&bump, r#"fn bad() { Array.push(["text"], 42) }"#).is_err());
}

#[test]
fn builtin_array_filter_requires_a_boolean_callback() {
    let bump = Bump::new();
    assert!(solve_input(&bump, "fn bad() { Array.filter([1], x -> 42) }").is_err());
}

#[test]
fn builtin_fiber_join_requires_a_fiber() {
    let bump = Bump::new();
    assert!(solve_input(&bump, "fn bad() { Fiber.join(42) }").is_err());
}

#[test]
fn builtin_argument_count_is_checked() {
    let bump = Bump::new();
    assert!(solve_input(&bump, "fn bad() { Array.length([1], 2) }").is_err());
}

#[test]
fn builtin_signature_survives_an_indirect_reference() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn bad() {
            let length = String.length
            length(42)
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn builtin_calls_publish_their_actual_result_types() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
        fn numbers() { Array.map([1], x -> x) }
        fn strings() { Array.map(["text"], x -> x) }
    "#},
    )
    .expect("each builtin call independently instantiates its signature");
    assert_eq!(
        render_annotations(&solved.schemes),
        "numbers: fn() Array[Number]\nstrings: fn() Array[String]"
    );
}

#[test]
fn lambda_annotation_cannot_specialize_an_enclosing_generic() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn keep(value: a) a {
            let ignored = (other: a) a -> 42
            value
        }
    "#}
        )
        .is_err(),
        "the lambda's a is the enclosing universal, even when unused"
    );
}

#[test]
fn lambda_annotation_can_use_an_enclosing_bound_without_a_call() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Describe[a] { fn describe(value: a) String }
        fn keep(value: a) a where a: Describe {
            let ignored = (other: a) String -> describe(other)
            value
        }
    "#},
    )
    .expect("the enclosing dictionary describes the lambda's annotated argument");
}

#[test]
fn sibling_lambda_annotation_variables_are_independent() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn pair() (Number, String) {
            let number = (value: b) b -> value
            let text = (value: b) b -> value
            (number(42), text("hello"))
        }
    "#},
    )
    .expect("fresh lambda variables do not leak into sibling annotations");
}

#[test]
fn nested_lambda_annotation_reuses_its_parent_lambda_variable() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn bad() (String, Number) {
            let outer = (value: b) -> {
                let inner = (other: b) -> other
                (value, inner(42))
            }
            outer("hello")
        }
    "#}
        )
        .is_err(),
        "the inner annotation must constrain the parent lambda's b"
    );
}

#[test]
fn lambda_annotation_does_not_leak_between_named_functions() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn identity(value: a) a { value }
        fn number() Number {
            let convert = (value: a) a -> 42
            convert(0)
        }
        fn text() String { identity("hello") }
    "#},
    )
    .expect("a fresh annotation in another function does not specialize identity");
}

#[test]
fn lambda_annotation_preserves_enclosing_higher_kinded_variables() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn keep(value: f[a]) f[a] {
            let identity = (other: f[a]) f[a] -> other
            identity(value)
        }
    "#},
    )
    .expect("higher-kinded lambda annotations share the enclosing constructor and argument");
}

#[test]
fn generic_contract_rejects_a_specialized_method_body() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        trait Convert[a] { fn convert(value: a, other: b) b }
        impl Convert[Number] {
            fn convert(value: Number, other: b) b { 42 }
        }
        fn use_string() String { convert(0, "hello") }
    "#}
        )
        .is_err()
    );
}

#[test]
fn generic_contract_rejects_specialization_across_explicit_async_boundaries() {
    for source in [
        indoc! {"
            trait Convert[a] { async fn convert(value: a, other: b) b }
            impl Convert[Number] {
                async fn convert(value: Number, other: b) b { 42 }
            }
        "},
        indoc! {"
            trait Convert[a] {
                async fn convert(value: a, other: b) b { return 42 }
            }
        "},
        indoc! {"
            fn invalid(value: a) Task[a] { async { return 42 } }
        "},
        indoc! {"
            async fn invalid(value: a) Task[a] { async { 42 } }
        "},
    ] {
        let bump = Bump::new();
        let errors =
            solve_input(&bump, source).expect_err("async wrappers cannot hide specialization");
        assert!(
            errors.iter().any(|error| matches!(
                error,
                alder_solve::SolveError::Core(Error {
                    kind: ErrorKind::GenericSpecialization { .. },
                    ..
                })
            )),
            "{source}\n{errors:?}"
        );
    }
}

#[test]
fn generic_contract_preserves_universals_across_explicit_async_boundaries() {
    let source = indoc! {r#"
        trait Keep[a] {
            async fn keep(value: a, other: b) b
            async fn fallback(value: a, other: b) b { return other }
        }
        impl Keep[Number] {
            async fn keep(value: Number, other: b) b { other }
        }
        fn deferred(value: a) Task[a] { async { return value } }
        async fn nested(value: a) Task[a] { async { value } }
        pub async fn check() {
            let number: Number = keep(0, 42).await
            let text: String = keep(0, "text").await
            let flag: Bool = fallback(0, true).await
            let preserved: String = deferred(text).await
            let inner: Task[Number] = nested(number).await
            let result: Number = inner.await
            (result, preserved, flag)
        }
    "#};
    let bump = Bump::new();
    solve_input(&bump, source)
        .expect("task boundaries preserve universality and exactly one layer");
}

#[test]
fn generic_contract_rejects_a_specialized_method_signature() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        trait Convert[a] { fn convert(value: a, other: b) b }
        impl Convert[Number] {
            fn convert(value: Number, other: Number) Number { other }
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn generic_contract_rejects_a_specialized_function() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn constant(value: a) a { 42 }
    "#}
        )
        .is_err()
    );
}

#[test]
fn generic_contract_keeps_independent_parameters_distinct() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn invalid(first: a, second: b) a { second }
    "#}
        )
        .is_err()
    );
}

#[test]
fn generic_contract_rejects_a_specialized_default() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        trait Convert[a] { fn convert(value: a, other: b) b { 42 } }
    "#}
        )
        .is_err()
    );
}

#[test]
fn generic_contract_rejects_higher_kinded_specialization() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn specialize(values: f[a]) Array[a] { values }
    "#}
        )
        .is_err()
    );
}

#[test]
fn generic_contract_rejects_escape_into_shared_mutable_state() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        #[extern("alder:kernel", "$arrayPush")]
        fn push(values: Array[a], value: a) ()
        let stored = []
        fn store(value: a) { push(stored, value) }
    "#}
        )
        .is_err()
    );
}

#[test]
fn generic_contract_checks_specialization_through_recursive_peers() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn first(value: a) a { second(value) }
        fn second(value) { if true { first(value) } else { 42 } }
    "#}
        )
        .is_err()
    );
}

#[test]
fn generic_contract_accepts_mutually_recursive_universal_functions() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn first(value: a) a { second(value) }
        fn second(value: b) b { if true { value } else { first(value) } }
        fn strings() String { first("hello") }
        fn numbers() Number { first(42) }
    "#},
    )
    .expect("each call independently instantiates the recursive group's contract");
}

#[test]
fn generic_contract_accepts_universal_methods_and_functions() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Convert[a] { fn convert(value: a, other: b) b }
        impl Convert[Number] {
            fn convert(value: Number, other: b) b { other }
        }
        fn identity(value: a) a { value }
        fn use_string() String { identity(convert(0, "hello")) }
        fn use_number() Number { identity(convert(0, 42)) }
    "#},
    )
    .expect("universal implementations remain independently instantiable");
}

#[test]
fn implementation_cannot_add_a_method_bound() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        trait Describe[a] { fn describe(value: a) String }
        trait Convert[a] { fn convert(value: a, other: b) String }
        impl Convert[Number] {
            fn convert(value: Number, other: b) String where b: Describe {
                describe(other)
            }
        }
    "#}
        )
        .is_err(),
        "callers do not supply an implementation-only dictionary"
    );
}

#[test]
fn implementation_bound_order_uses_the_trait_dictionary_abi() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
        trait First[a] { fn first(value: a) String }
        trait Second[a] { fn second(value: a) String }
        trait Convert[a] {
            fn convert(value: a, other: b) String where b: First + Second
        }
        impl Convert[Number] {
            fn convert(value: Number, other: c) String where c: Second + First {
                first(other)
            }
        }
    "#},
    )
    .expect("reordered implementation bounds do not reorder dictionary arguments");
    assert!(solved.uses.values().any(|action| matches!(action,
        alder_solve::UseAction::Reference { dictionaries, method: Some(method) }
            if method.name == "first"
                && matches!(dictionaries.as_slice(), [alder_solve::Evidence::Param(0)])
    )));
}

#[test]
fn implementation_cannot_add_an_unused_method_bound() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        trait Describe[a] { fn describe(value: a) String }
        trait Convert[a] { fn convert(value: a, other: b) b }
        impl Convert[Number] {
            fn convert(value: Number, other: b) b where b: Describe { other }
        }
    "#}
        )
        .is_err(),
        "even unused implementation constraints must follow from the trait contract"
    );
}

#[test]
fn implementation_cannot_add_a_projection_assumption() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        trait Source[a] {
            type Item
            fn get(value: a) Item
        }
        trait Convert[a] {
            fn convert(value: a, source: b) Number where b: Source
        }
        impl Convert[Number] {
            fn convert(value: Number, source: b) Number
                where b: Source, b.Item == Number
            { get(source) }
        }
    "#}
        )
        .is_err(),
        "implementation-only equalities cannot narrow a method contract"
    );
}

#[test]
fn implementation_inherits_a_method_projection_equality() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Source[a] {
            type Item
            fn get(value: a) Item
        }
        trait Convert[a] {
            fn convert(value: a, source: b) Number
                where b: Source, b.Item == Number
        }
        impl Convert[Number] {
            fn convert(value: Number, source: c) Number { get(source) }
        }
    "#},
    )
    .expect("the method contract supplies the projection equality");
}

#[test]
fn implementation_signature_uses_the_method_projection_equality() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Source[a] {
            type Item
            fn get(value: a) Item where a.Item == Number
        }
        impl Source[String] {
            type Item = Number
            fn get(value: String) Number { 42 }
        }
    "#},
    )
    .expect("signature checking can use the declared associated equality");
}

#[test]
fn implementation_inherits_the_declared_method_bound() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Describe[a] { fn describe(value: a) String }
        trait Convert[a] {
            fn convert(value: a, other: b) String where b: Describe
        }
        impl Convert[Number] {
            fn convert(value: Number, other: c) String { describe(other) }
        }
    "#},
    )
    .expect("the trait contract supplies the dictionary, regardless of local variable names");
}

#[test]
fn direct_trait_method_selects_the_unique_impl() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) String }
            impl Show[Number] { fn show(value: Number) String { "number" } }
            fn render() String { show(1) }
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
            trait Show[a] { fn show(value: a) String }
            fn describe(value: a) String where a: Show { show(value) }
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
            trait Show[a] { fn show(value: a) String }
            impl Show[Number] {
                fn show(value: Number) String { show(value) }
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
            trait Show[a] { fn show(value: a) String }
            impl Show[Array[a]] where a: Show {
                fn show(values: Array[a]) String { show(values[0]) }
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
                fn show(value: a) String
                fn render(value: a) String { show(value) }
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
            trait Equal[a] { fn equal(left: a, right: a) Bool }
            trait Ordered[a] where a: Equal {
                fn compare(left: a, right: a) Number
                fn same(left: a, right: a) Bool { equal(left, right) }
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
fn generic_bounds_expose_transitive_superclass_dictionaries() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            trait Equal[a] { fn equal(left: a, right: a) Bool }
            trait Ordered[a] where a: Equal { fn less(left: a, right: a) Bool }
            trait Ranked[a] where a: Ordered { fn rank(value: a) Number }
            fn same(left: a, right: a) Bool where a: Ranked { equal(left, right) }
        "#},
    )
    .expect("Ranked exposes Equal through its Ordered superclass");
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Reference {
            dictionaries,
            method: Some(method),
        } if method.name == "equal"
            && matches!(
                dictionaries.as_slice(),
                [alder_solve::Evidence::ParamSuperPath { param: 0, path }]
                    if path == &[0, 0]
            )
    )));
}

#[test]
fn implementation_must_supply_each_superclass_dictionary() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            trait Equal[a] { fn equal(left: a, right: a) Bool }
            trait Ordered[a] where a: Equal {
                fn less(left: a, right: a) Bool
            }
            impl Ordered[Number] {
                fn less(left: Number, right: Number) Bool { left < right }
            }
        "#},
    )
    .expect_err("an Ordered implementation without Equal cannot construct its dictionary");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Trait(alder_solve::SolveTraitError::MissingInstance {
            trait_, subject, ..
        }) if trait_.0.name == "Equal" && *subject == "Number"
    )));
}

#[test]
fn implementation_records_resolved_superclass_evidence() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            trait Equal[a] { fn equal(left: a, right: a) Bool }
            trait Ordered[a] where a: Equal {
                fn less(left: a, right: a) Bool
            }
            impl Equal[Number] {
                fn equal(left: Number, right: Number) Bool { left == right }
            }
            impl Ordered[Number] {
                fn less(left: Number, right: Number) Bool { left < right }
            }
        "#},
    )
    .expect("the sibling Equal implementation satisfies Ordered's superclass");
    assert!(
        solved
            .impl_superclasses
            .iter()
            .any(|((implementation, slot), evidence)| {
                *slot == 0
                    && matches!(
                        evidence,
                        alder_solve::Evidence::Impl { impl_id, .. }
                            if impl_id != implementation
                    )
            })
    );
}

#[test]
fn declared_bounds_are_preserved_in_the_binding_abi() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) String }
            fn describe(value: a) String where a: Show { show(value) }
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
            trait Show[a] { fn show(value: a) String }
            impl Show[Number] { fn show(value: Number) String { "number" } }
            fn describe(value: a) String where a: Show { show(value) }
            fn render() String { describe(1) }
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
            trait Show[a] { fn show(value: a) String }
            impl Show[Number] { fn show(value: Number) String { "number" } }
            fn render() String { show("nope") }
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
                fn next(value: i) Item
            }
            fn increment(value: i) Number
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
fn conflicting_associated_equalities_are_structured() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            trait Iterator[i] {
                type Item
                fn next(value: i) Item
            }
            fn impossible(value: i) ()
                where i: Iterator, i.Item == Number, i.Item == String
            {}
        "#},
    )
    .expect_err("one associated type cannot equal Number and String");
    assert!(matches!(
        &errors[0],
        alder_solve::SolveError::Core(Error {
            kind: ErrorKind::AssocTypeMismatch {
                assoc,
                expected,
                actual,
            },
            ..
        }) if assoc == "Item" && expected == "Number" && actual == "String"
    ));
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
                fn next(value: i) Item
            }
            impl Iterator[Counter] {
                type Item = Number
                fn next(value: Counter) Number { 1 }
            }
            fn increment(value: Counter) Number { next(value) + 1 }
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
                fn next(value: i) Item
            }
            impl Iterator[Counter] {
                type Item = Number
                fn next(value: Counter) String { "wrong" }
            }
        "#},
    )
    .expect_err("the method result must equal the impl's Item binding");
    assert!(matches!(
        &errors[0],
        alder_solve::SolveError::Core(Error {
            kind: ErrorKind::Mismatch { actual, expected },
            ..
        }) if (*actual == DiagnosticType::Named("String".into()) && *expected == DiagnosticType::Named("Number".into()))
            || (*actual == DiagnosticType::Named("Number".into()) && *expected == DiagnosticType::Named("String".into()))
    ));
}

#[test]
fn cyclic_associated_binding_is_rejected() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            enum Counter { Counter }
            trait Iterator[i] {
                type Item
                fn next(value: i) Item
            }
            impl Iterator[Counter] {
                type Item = Item
                fn next(value: Counter) Item { next(value) }
            }
        "#},
    )
    .expect_err("an associated type cannot contain its own projection");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Coherence(alder_solve::CoherenceError::ProjectionCycle {
            chain,
            ..
        }) if chain.iter().map(|assoc| assoc.name).collect::<Vec<_>>() == ["Item"]
    )));
}

#[test]
fn indirect_associated_binding_cycle_is_rejected() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            enum Counter { Counter }
            trait Pair[i] {
                type Left
                type Right
            }
            impl Pair[Counter] {
                type Left = Right
                type Right = Left
            }
        "#},
    )
    .expect_err("associated bindings cannot form an indirect cycle");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Coherence(alder_solve::CoherenceError::ProjectionCycle {
            chain,
            ..
        }) if chain.iter().map(|assoc| assoc.name).collect::<Vec<_>>() == ["Left", "Right"]
    )));
}

#[test]
fn trait_method_projection_equalities_are_instantiated_at_use_sites() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            trait NumericIterator[i] {
                type Item
                fn next(value: i) Item where i.Item == Number
            }
            fn increment(value: i) Number where i: NumericIterator {
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
            trait Show[a] { fn show(value: a) String }
            impl Show[Number] { fn show(value: Number) String { "one" } }
            impl Show[Number] { fn show(value: Number) String { "two" } }
            fn render() String { show(1) }
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
    let implementation_module = ModuleId {
        package: PackageId::Application,
        path: &["Main"],
    };
    let number = bump.alloc(Located::at_zero(Type::Named {
        reference: alder_ast::QualifiedName {
            module: ModuleId {
                package: PackageId::Builtin,
                path: &[],
            },
            name: "Number",
        },
        args: &[],
    }));
    let trait_ref = alder_ast::TraitRef {
        trait_: trait_id,
        args: bump.alloc_slice_copy(&[number as alder_ast::Node<'_, Type<'_>>]),
    };
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
        instances: bump.alloc_slice_copy(&[alder_ast::InterfaceImpl {
            id: alder_ast::ImplId {
                module: implementation_module,
                origin: alder_ast::ImplOrigin::Source { item_ordinal: 0 },
            },
            source_uri: Some("file:///dependency/Foreign.ald"),
            region: Some(alder_region::Region::one()),
            params: &[],
            trait_ref,
            trait_predicates: &[],
            projection_equalities: &[],
            assoc_bindings: &[],
            dictionary_symbol: "$dict$ForeignEq$0",
            dictionary_kind: alder_ast::DictionaryKind::Singleton,
            methods: &[],
        }]),
        modules: &[],
        private_names: &[],
    };
    let module = alder_ast::Module {
        id: implementation_module,
        imports: &[],
        items: &[],
        value_sccs: &[],
        assigned_bindings: &[],
    };
    let interfaces = bump.alloc_slice_copy(&[interface]);
    let database = alder_solve::TraitDatabase::build(&bump, &module, interfaces);
    let errors = database.validate(&bump);
    assert!(matches!(
        &errors[0],
        alder_solve::CoherenceError::OrphanImpl {
            trait_, subject, ..
        } if trait_.0.name == "ForeignEq" && *subject == "Number"
    ));
}

#[test]
fn generic_and_concrete_heads_overlap() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            trait Show[a] { fn show(value: a) String }
            impl Show[a] { fn show(value: a) String { "any" } }
            impl Show[Number] { fn show(value: Number) String { "number" } }
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
            trait Show[a] { fn show(value: a) String }
            impl Show[a] where a: Show { fn show(value: a) String { "loop" } }
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
            trait Show[a] { fn show(value: a) String }
            impl Show[Array[a]] where a: Show {
                fn show(value: Array[a]) String { "array" }
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
            trait A[a] where a: B { fn a(value: a) a }
            trait B[a] where a: A { fn b(value: a) a }
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
            fn number() Number { 1 + 2 }
            fn bigint() BigInt { 1n + 2n }
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
fn builtin_hash_and_num_bounds_expose_their_superclasses() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            fn hash_equal(left: a, right: a) Bool where a: Hash { left == right }
            fn num_equal(left: a, right: a) Bool where a: Num { left == right }
            fn num_greater(left: a, right: a) Bool where a: Num { left > right }
        "#},
    )
    .expect("Hash and Num dictionaries expose their declared superclasses");
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Operator {
            dictionary: alder_solve::Evidence::ParamSuper { param: 0, slot: 0 },
        }
    )));
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Operator {
            dictionary: alder_solve::Evidence::ParamSuper { param: 0, slot: 1 },
        }
    )));
}

#[test]
fn functions_have_no_structural_eq_instance() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            fn identity(value: a) a { value }
            fn bad() Bool { identity == identity }
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

#[test]
fn closed_records_have_fieldwise_structural_eq_evidence() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            fn same(
                left: { name: String, score: Number },
                right: { name: String, score: Number },
            ) Bool {
                left == right
            }
        "#},
    )
    .expect("closed records have structural equality when every field does");

    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Operator {
            dictionary: alder_solve::Evidence::StructuralEq {
                shape: alder_solve::StructuralEqShape::Record(fields),
                fields: dictionaries,
            },
        } if fields == &["name", "score"] && dictionaries.len() == 2
    )));
}

#[test]
fn open_record_rows_have_no_structural_eq_instance() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            fn same(left: { r | name: String }, right: { r | name: String }) Bool {
                left == right
            }
        "#},
    )
    .expect_err("an open row can hide fields without Eq instances");

    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Trait(
            alder_solve::SolveTraitError::MissingInstance { trait_, .. }
                | alder_solve::SolveTraitError::UnsatisfiedBound { trait_, .. }
        ) if trait_.0.name == "Eq"
    )));
}

#[test]
fn trait_errors_preserve_structured_missing_evidence() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            trait Display[a] { fn display(value: a) String }
            fn missing(value: Number) String { display(value) }
            fn generic(value: a) String { display(value) }
        "#},
    )
    .expect_err("both calls require unavailable Display evidence");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Trait(alder_solve::SolveTraitError::MissingInstance {
            trait_,
            subject: "Number",
            ..
        }) if trait_.0.name == "Display"
    )));
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Trait(alder_solve::SolveTraitError::UnsatisfiedBound {
            trait_,
            ..
        }) if trait_.0.name == "Display"
    )));
}

#[test]
fn nested_instance_failure_retains_the_obligation_chain() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            fn missing() String {
                show([(value: Number) Number -> value])
            }
        "#},
    )
    .expect_err("Show cannot be derived for an array of functions");
    let chain = errors
        .iter()
        .find_map(|error| match error {
            alder_solve::SolveError::Trait(alder_solve::SolveTraitError::MissingInstance {
                chain,
                ..
            }) => Some(*chain),
            _ => None,
        })
        .expect("the nested missing instance is retained");
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].trait_.0.name, "Show");
    assert_eq!(chain[0].subject, "Array[fn(Number) Number]");
    assert!(chain[0].required_by.is_none());
    assert_eq!(chain[1].trait_.0.name, "Show");
    assert_eq!(chain[1].subject, "fn(Number) Number");
    assert!(chain[1].required_by.is_some());
}

#[test]
fn builtin_containers_require_equality_for_every_type_argument() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            fn same_arrays(left: Array[a], right: Array[a]) Bool where a: Eq {
                left == right
            }
            fn same_options(left: Option[a], right: Option[a]) Bool where a: Eq {
                left == right
            }
            fn same_results(left: Result[a, e], right: Result[a, e]) Bool
                where a: Eq, e: Eq {
                left == right
            }
        "#},
    )
    .expect("container equality is structural when all contained types implement Eq");
    assert_eq!(
        solved
            .uses
            .values()
            .filter(|action| matches!(
                action,
                alder_solve::UseAction::Operator {
                    dictionary: alder_solve::Evidence::StructuralEq { .. },
                }
            ))
            .count(),
        3
    );

    let errors = solve_input(
        &bump,
        indoc! {r#"
            fn invalid(left: Array[fn(Number) Number], right: Array[fn(Number) Number]) Bool {
                left == right
            }
        "#},
    )
    .expect_err("container equality cannot hide a function-valued element");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Trait(alder_solve::SolveTraitError::MissingInstance {
            trait_, subject, ..
        }) if trait_.0.name == "Eq" && subject.starts_with("fn(")
    )));
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

macro_rules! assert_solve_error_snapshot {
    ($source:expr) => {{
        let source = indoc!($source);
        let bump = Bump::new();
        let errors = solve_input(&bump, source).expect_err("solving fails");
        insta::with_settings!({ description => source, omit_expression => true }, {
            insta::assert_debug_snapshot!(errors);
        });
    }};
}

#[test]
fn cross_trait_non_decreasing_instance_prerequisite_is_rejected() {
    assert_solve_error_snapshot! {r#"
        trait Display[a] { fn display(value: a) String }
        trait Render[a] { fn render(value: a) String }

        impl Display[a] where a: Render {
            fn display(value: a) String { "display" }
        }
    "#};
}

#[test]
fn explicit_equality_overlaps_automatic_enum_equality() {
    assert_solve_error_snapshot! {r#"
        enum Token { Token }

        impl Eq[Token] {
            fn eq(left: Token, right: Token) Bool { true }
        }
    "#};
}

#[test]
fn opaque_types_may_define_explicit_equality() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            #[extern]
            type Secret

            impl Eq[Secret] {
                fn eq(left: Secret, right: Secret) Bool { true }
            }
        "#},
    )
    .expect("opaque types have no automatic Eq and may define their own implementation");
}

#[test]
fn polymorphic_identity() {
    assert_inference_snapshot!("fn identity(value) { value }");
}

#[test]
fn partial_annotations_share_variables_with_inferred_positions() {
    assert_inference_snapshot! {r#"
        fn apply(value: a, transform) { transform(value) }
    "#};
}

#[test]
fn unresolved_local_trait_obligations_are_not_generalized() {
    assert_solve_error_snapshot! {r#"
        fn ambiguous() {
            let equal = (left, right) -> { left == right }
            ()
        }
    "#};
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
        "identity: forall a. fn(a) a\npair: fn() (Number, String)"
    );
}

#[test]
fn mutable_top_level_bindings_do_not_generalize() {
    assert_inference_error_snapshot! {r#"
        let identity = (value) -> { value }
        fn number() { identity(1) }
        fn text() { identity("text") }
        fn replace() { identity = value -> value }
    "#};
}

#[test]
fn generalization_subtracts_mutable_environment_variables() {
    assert_inference_error_snapshot! {r#"
        let identity = (value) -> { value }
        fn forward(value) { identity(value) }
        fn number() { forward(1) }
        fn text() { forward("text") }
        fn replace() { identity = value -> value }
    "#};
}

#[test]
fn local_let_bindings_remain_monomorphic() {
    assert_inference_error_snapshot! {r#"
        fn invalid() {
            let identity = (value) -> { value }
            let number = identity(1)
            identity("text")
        }
    "#};
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
        "first: forall a, b. fn(a) b\nsecond: forall a, b. fn(a) b"
    );
}

#[test]
fn mutually_recursive_calls_receive_preseeded_dictionary_arguments() {
    let bump = Bump::new();
    let output = solve_input(
        &bump,
        indoc! {r#"
            trait Display[a] { fn display(value: a) String }
            impl Display[Number] {
                fn display(value: Number) String { "number" }
            }
            fn first(value: a) String where a: Display { second(value) }
            fn second(value: a) String where a: Display {
                if true { display(value) } else { first(value) }
            }
            fn main() String { first(1) }
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
fn three_member_predicate_fixpoint_is_source_order_independent() {
    fn solve_order(source: &str) {
        let bump = Bump::new();
        let output = solve_input(&bump, source).expect("all recursive peers share the bound ABI");
        let calls = output
            .uses
            .values()
            .filter_map(|action| match action {
                alder_solve::UseAction::DirectCall {
                    dictionaries,
                    target: Some(alder_solve::DirectTarget::Binding(name)),
                    ..
                } if matches!(name.name, "first" | "second" | "third") => {
                    Some((name.name, dictionaries.len()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 3);
        assert!(calls.iter().all(|(_, dictionaries)| *dictionaries == 1));
    }

    solve_order(indoc! {r#"
        trait Display[a] { fn display(value: a) String }
        fn first(value: a) String where a: Display { second(value) }
        fn second(value: a) String where a: Display { third(value) }
        fn third(value: a) String where a: Display {
            if true { display(value) } else { first(value) }
        }
    "#});
    solve_order(indoc! {r#"
        trait Display[a] { fn display(value: a) String }
        fn third(value: a) String where a: Display {
            if true { display(value) } else { first(value) }
        }
        fn first(value: a) String where a: Display { second(value) }
        fn second(value: a) String where a: Display { third(value) }
    "#});
}

#[test]
fn recursive_peers_cannot_hide_mismatched_bounds() {
    assert_solve_error_snapshot! {r#"
        trait Display[a] { fn display(value: a) String }
        trait Hashable[a] { fn hash(value: a) BigInt }

        fn first(value: a) String where a: Display { second(value) }
        fn second(value: a) String where a: Hashable { first(value) }
    "#};
}

#[test]
fn higher_kinded_application_is_preserved_and_specialized() {
    let bump = Bump::new();
    let annotations = infer(
        &bump,
        indoc! {r#"
            fn adapt(value: f[a]) f[a] { value }
            fn specialize(value: Result[Number, [:failed]]) { adapt(value) }
        "#},
    )
    .unwrap();

    assert_eq!(
        render_annotations(&annotations),
        concat!(
            "adapt: forall a, b. fn(a[b]) a[b]\n",
            "specialize: fn(Result[Number, [:failed]]) Result[Number, [:failed]]"
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
fn higher_kinded_unification_recovers_partial_result() {
    assert_inference_snapshot! {r#"
        fn adapt(value: f[a]) f[a] { value }
        fn specialize(value: Result[Number, [:failed]]) { adapt(value) }
    "#};
}

#[test]
fn higher_kinded_unification_preserves_two_hole_order() {
    assert_inference_snapshot! {r#"
        fn adapt(value: f[a, b], first: a, second: b) f[a, b] { value }
        enum Pair[a, b] { Pair(a, b) }
        fn specialize(value: Pair[Number, String]) {
            adapt(value, 1, "second")
        }
    "#};
}

#[test]
fn higher_kinded_unification_rejects_inconsistent_partial_sections() {
    assert_inference_error_snapshot! {r#"
        fn combine(left: f[a], right: f[b]) f[a] { left }
        fn invalid(
            left: Result[Number, [:left]],
            right: Result[Bool, [:right]],
        ) {
            combine(left, right)
        }
    "#};
}

#[test]
fn higher_kinded_unification_rejects_an_occurs_cycle() {
    assert_inference_error_snapshot! {r#"
        fn impossible(value: f[a]) a { value }
    "#};
}

#[test]
fn higher_kinded_unification_expands_transparent_aliases() {
    assert_inference_snapshot! {r#"
        type Wrapped[a, e] = Result[a, e]

        fn adapt(value: f[a]) f[a] { value }
        fn specialize(value: Wrapped[Number, [:failed]]) { adapt(value) }
    "#};
}

#[test]
fn builtin_functor_instances_cover_array_option_and_partial_result() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            fn array(value: Array[Number]) Array[Number] {
                map(value, (item) -> { item + 1 })
            }
            fn option(value: Option[Number]) Option[Number] {
                map(value, (item) -> { item + 1 })
            }
            fn result(value: Result[Number, [:failure(String)]]) Result[Number, [:failure(String)]] {
                map(value, (item) -> { item + 1 })
            }
        "#},
    )
    .expect("all three first-party Functor instances resolve");
    let mut found = [false; 3];
    for action in solved.uses.values() {
        let alder_solve::UseAction::Reference { dictionaries, .. } = action else {
            continue;
        };
        match dictionaries.first() {
            Some(alder_solve::Evidence::Intrinsic(alder_solve::Intrinsic::FunctorArray)) => {
                found[0] = true;
            }
            Some(alder_solve::Evidence::Intrinsic(alder_solve::Intrinsic::FunctorOption)) => {
                found[1] = true;
            }
            Some(alder_solve::Evidence::Intrinsic(alder_solve::Intrinsic::FunctorResult)) => {
                found[2] = true;
            }
            _ => {}
        }
    }
    assert_eq!(found, [true, true, true]);
}

#[test]
fn builtin_applicative_and_monad_instances_preserve_the_hkt_hierarchy() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            fn option_pure(value: Number) Option[Number] { pure(value) }
            fn array_apply(
                functions: Array[fn(Number) String],
                values: Array[Number],
            ) Array[String] { apply(functions, values) }
            fn result_bind(
                value: Result[Number, [:failure(String)]],
            ) Result[String, [:failure(String)]] {
                flat_map(value, (item) -> { Result.ok("done") })
            }
            fn monad_map(value: f[Number]) f[Number] where f: Monad {
                map(value, (item) -> { item + 1 })
            }
        "#},
    )
    .expect("Applicative and Monad instances resolve with transitive Functor evidence");
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Reference { dictionaries, .. }
            if matches!(
                dictionaries.first(),
                Some(alder_solve::Evidence::Intrinsic(
                    alder_solve::Intrinsic::ApplicativeOption
                ))
            )
    )));
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Reference { dictionaries, .. }
            if matches!(
                dictionaries.first(),
                Some(alder_solve::Evidence::Intrinsic(
                    alder_solve::Intrinsic::MonadResult
                ))
            )
    )));
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Reference { dictionaries, method: Some(method) }
            if method.name == "map"
                && matches!(
                    dictionaries.as_slice(),
                    [alder_solve::Evidence::ParamSuperPath { path, .. }]
                        if path == &[0, 0]
                )
    )));
}

#[test]
fn builtin_traversable_passes_method_level_applicative_evidence() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            fn traverse_array(value: Array[Number]) Option[Array[String]] {
                traverse(value, (item) -> { Option.some("item") })
            }
            fn traverse_option(value: Option[Number]) Array[Option[String]] {
                traverse(value, (item) -> { ["item"] })
            }
            fn traverse_result(value: Result[Number, [:failed]]) Option[Result[String, [:failed]]] {
                traverse(value, (item) -> { Option.some("item") })
            }
        "#},
    )
    .expect("Traversable resolves both its subject and method-level Applicative instance");
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Reference { dictionaries, method: Some(method) }
            if method.name == "traverse"
                && dictionaries.len() == 2
                && matches!(
                    dictionaries[0],
                    alder_solve::Evidence::Intrinsic(alder_solve::Intrinsic::TraversableArray)
                )
                && matches!(
                    dictionaries[1],
                    alder_solve::Evidence::Intrinsic(alder_solve::Intrinsic::ApplicativeOption)
                )
    )));
}

#[test]
fn builtin_array_iterator_normalizes_its_item_projection() {
    let bump = Bump::new();
    let solved = solve_input(
        &bump,
        indoc! {r#"
            fn first(values: Array[Number]) Option[Number] { next(Array.iter(values)) }
            fn generic(value: i) Option[Number]
                where i: Iterator, i.Item == Number
            {
                next(value)
            }
        "#},
    )
    .expect("ArrayIterator's Item projection normalizes to its element type");
    assert!(solved.uses.values().any(|action| matches!(
        action,
        alder_solve::UseAction::Reference { dictionaries, method: Some(method) }
            if method.name == "next"
                && matches!(
                    dictionaries.as_slice(),
                    [alder_solve::Evidence::Intrinsic(alder_solve::Intrinsic::IteratorArray)]
                )
    )));
}

#[test]
fn repeated_higher_kinded_pattern_argument_is_rejected() {
    let bump = Bump::new();
    let errors = infer(
        &bump,
        indoc! {r#"
            fn adapt(value: f[a, a]) f[a, a] { value }
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
fn concrete_type_cannot_fill_a_higher_kinded_trait_parameter() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            trait Mapper[f] {
                fn map(value: f[a], transform: fn(a) b) f[b]
            }
            impl Mapper[Number] {
                fn map(value: Number, transform: fn(a) b) Number { value }
            }
        "#},
    )
    .expect_err("Number has kind Type, not Type -> Type");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Coherence(alder_solve::CoherenceError::KindMismatch {
            parameter: 0,
            expected_arity: 1,
            actual_arity: 0,
            ..
        })
    )));
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
fn pipe_forwards_into_first_call_argument() {
    assert_inference_snapshot!(
        "fn subtract(left: Number, right: Number) Number { left - right }\nlet answer = 44 |> subtract(2)"
    );
}

#[test]
fn optional_record_field_annotation() {
    assert_inference_snapshot!("fn name(user: { name?: String }) { user.name }");
}

#[test]
fn mismatch_reports_new_type_syntax() {
    assert_inference_error_snapshot!("fn bad() Number { \"nope\" }");
}

#[test]
fn mutable_loop_and_assignment() {
    assert_inference_snapshot!(
        r#"
        fn sum(values: Array[Number]) Number {
            let total = 0
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
        fn choose(flag: Bool) Number {
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
        fn unwrap(
            value: Result[Number, [:failure(String)]],
        ) Result[Number, [:failure(String)]] {
            Result.ok(value? + 1)
        }
    "#
    );
}

#[test]
fn result_shorthand_infers_an_open_tag_row_with_payloads() {
    assert_inference_snapshot! {r#"
        fn fail(id: Number) Result[String] {
            Err(:not_found(id))
        }
    "#};
}

#[test]
fn try_merges_three_error_rows_into_the_enclosing_result() {
    assert_inference_snapshot! {r#"
        fn read() Result[Number, [:storage(String)]] {
            Err(:storage("disk"))
        }

        fn decode() Result[Number, [:invalid(String)]] {
            Err(:invalid("number"))
        }

        fn authorize() Result[Number, [:forbidden]] {
            Err(:forbidden)
        }

        fn load() Result[Number] {
            let first = read()?
            let second = decode()?
            let third = authorize()?
            Ok(first + second + third)
        }
    "#};
}

#[test]
fn repeated_tag_payloads_must_have_the_same_type() {
    assert_inference_error_snapshot! {r#"
        fn invalid(flag: Bool) Result[()] {
            if flag {
                Err(:invalid("text"))
            } else {
                Err(:invalid(1))
            }
        }
    "#};
}

#[test]
fn named_error_group_closes_the_result_row() {
    assert_inference_error_snapshot! {r#"
        error Failure {
            :known(String)
        }

        fn invalid() Result[(), Failure] {
            Err(:other)
        }
    "#};
}

#[test]
fn try_rejects_a_tag_missing_from_the_closed_result_row() {
    assert_inference_error_snapshot! {r#"
        fn read() Result[Number, [:storage]] {
            Err(:storage)
        }

        fn load() Result[Number, [:invalid]] {
            read()?
        }
    "#};
}

#[test]
fn repeated_tag_payloads_must_have_the_same_arity() {
    assert_inference_error_snapshot! {r#"
        fn invalid(flag: Bool) Result[()] {
            if flag {
                Err(:invalid("text"))
            } else {
                Err(:invalid("text", 1))
            }
        }
    "#};
}

#[test]
fn closed_error_match_is_exhaustive_with_every_result_case() {
    assert_inference_snapshot! {r#"
        error Failure {
            :invalid(String),
            :missing
        }

        fn render(value: Result[Number, Failure]) String {
            match value {
                Ok(number) => "ok",
                Err(:invalid(message)) => message,
                Err(:missing) => "missing",
            }
        }
    "#};
}

#[test]
fn closed_error_match_reports_missing_cases() {
    assert_inference_error_snapshot! {r#"
        error Failure {
            :invalid(String),
            :missing
        }

        fn render(value: Result[Number, Failure]) String {
            match value {
                Ok(number) => "ok",
                Err(:invalid(message)) => message,
            }
        }
    "#};
}

#[test]
fn wildcard_makes_a_closed_error_match_exhaustive() {
    assert_inference_snapshot! {r#"
        error Failure {
            :invalid(String),
            :missing
        }

        fn render(value: Result[Number, Failure]) String {
            match value {
                Ok(number) => "ok",
                Err(:invalid(message)) => message,
                _ => "other",
            }
        }
    "#};
}

#[test]
fn open_error_match_requires_an_error_catch_all() {
    assert_inference_error_snapshot! {r#"
        fn render(value: Result[Number]) String {
            match value {
                Ok(number) => "ok",
                Err(:invalid(message)) => message,
            }
        }
    "#};
}

#[test]
fn open_error_match_accepts_an_error_catch_all() {
    assert_inference_snapshot! {r#"
        fn render(value: Result[Number]) String {
            match value {
                Ok(number) => "ok",
                Err(:invalid(message)) => message,
                Err(_) => "other",
            }
        }
    "#};
}

#[test]
fn guarded_error_arm_does_not_complete_coverage() {
    assert_inference_error_snapshot! {r#"
        error Failure {
            :invalid(String)
        }

        fn render(value: Result[Number, Failure]) String {
            match value {
                Ok(number) => "ok",
                Err(:invalid(message)) if message == "special" => message,
            }
        }
    "#};
}

#[test]
fn closed_error_match_rejects_an_impossible_tag() {
    assert_inference_error_snapshot! {r#"
        error Failure {
            :known
        }

        fn render(value: Result[Number, Failure]) String {
            match value {
                Ok(number) => "ok",
                Err(:other) => "other",
                Err(:known) => "known",
            }
        }
    "#};
}

#[test]
fn error_tag_cannot_be_bound_as_an_ordinary_value() {
    assert_inference_error_snapshot! {r#"
        fn invalid() {
            let failure = :not_found(42)
            failure
        }
    "#};
}

#[test]
fn await_unwraps_task_inside_task_function() {
    assert_inference_snapshot!(
        r#"
        async fn wait() () {
            Task.sleep(1).await
        }
    "#
    );
}

#[test]
fn explicit_async_infers_completed_value() {
    assert_inference_snapshot! {r#"
        #[extern("alder:kernel", "$taskSleep")]
        fn sleep(milliseconds: Number) Task[()]

        async fn wait() {
            sleep(1).await
        }
    "#};
}

#[test]
fn tuple_projections_accumulate_before_fixing_arity() {
    assert_inference_snapshot! {r#"
        fn sum(pair) Number { pair.0 + pair.1 }
        fn run() Number { sum((20, 22)) }
    "#};
}

#[test]
fn recursive_tuple_projections_collect_across_the_whole_group() {
    for source in [
        indoc! {r#"
            fn first(pair, remaining: Number) Number {
                if remaining == 0 { pair.0 } else { last(pair, remaining - 1) }
            }
            fn last(pair, remaining: Number) Number {
                if remaining == 0 { pair.3 } else { first(pair, remaining - 1) }
            }
            fn check() Number { first((20, false, "kept", 22), 1) }
        "#},
        indoc! {r#"
            fn last(pair, remaining: Number) Number {
                if remaining == 0 { pair.3 } else { first(pair, remaining - 1) }
            }
            fn first(pair, remaining: Number) Number {
                if remaining == 0 { pair.0 } else { last(pair, remaining - 1) }
            }
            fn check() Number { first((20, false, "kept", 22), 1) }
        "#},
    ] {
        let bump = Bump::new();
        solve_input(&bump, source)
            .expect("all recursive projections contribute before fixing exact tuple length");
    }
}

#[test]
fn recursive_tuple_projections_preserve_whole_value_relationships() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn first(pair, remaining: Number) {
                let value: Number = pair.0
                if remaining == 0 { pair } else { last(pair, remaining - 1) }
            }
            fn last(pair, remaining: Number) {
                let value: Number = pair.3
                if remaining == 0 { pair } else { first(pair, remaining - 1) }
            }
            fn strings() (Number, Bool, String, Number) {
                first((20, false, "kept", 22), 1)
            }
            fn numbers() (Number, String, Number, Number) {
                last((20, "independent", 7, 22), 2)
            }
        "#},
    )
    .expect("recursive schemes independently instantiate untouched slots and preserve returns");
}

#[test]
fn recursive_tuple_projections_reject_incompatible_lengths() {
    let bump = Bump::new();
    let result = solve_input(
        &bump,
        indoc! {r#"
            fn first(pair, remaining: Number) Number {
                if remaining == 0 { pair.0 } else { last(pair, remaining - 1) }
            }
            fn last(pair, remaining: Number) Number {
                if remaining == 0 { pair.3 } else { first(pair, remaining - 1) }
            }
            fn invalid() Number { first((20, 22), 1) }
        "#},
    );
    assert!(
        result.is_err(),
        "recursive calls cannot drop later projection requirements"
    );
}

#[test]
fn tuple_projections_cannot_specialize_an_explicit_generic_contract() {
    for source in [
        "fn invalid(value: a) { value.0 }",
        "fn invalid(value: a) { value.0 = 42 }",
    ] {
        let bump = Bump::new();
        assert!(solve_input(&bump, source).is_err(), "must reject: {source}");
    }
}

#[test]
fn tuple_projections_preserve_whole_tuple_return_relationships() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn inspect(value) { let ignored = value.0
            value
        }
        fn check() (Number, String) { inspect((42, "kept")) }
    "#},
    )
    .expect("unprojected elements retain their types through return values");
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn inspect(value) { let ignored = value.0
            value
        }
        fn invalid() (Number, Bool) { inspect((42, "kept")) }
    "#}
        )
        .is_err(),
        "unprojected slots cannot change type at a call"
    );
}

#[test]
fn tuple_projections_through_record_overlays_accumulate() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn read(left, right) Number {
            let merged = { ..left, ..right }
            merged.value.0 + merged.value.3
        }
        fn run() Number { read({}, { value: (20, false, "unused", 22) }) }
    "#},
    )
    .expect("record overlays must retain tuple projection relationships");
}

#[test]
fn tuple_shapes_survive_optional_argument_lifting_in_both_orders() {
    for source in [
        indoc! {r#"
            fn consume(value?: a) {}
            fn relay(pair) {
                consume(pair)
                let first: Number = pair.0
                pair
            }
            fn numbers() (Number, Number) { relay((42, 7)) }
            fn strings() (Number, String) { relay((42, "kept")) }
        "#},
        indoc! {r#"
            fn consume(value?: a) {}
            fn relay(pair) {
                let first: Number = pair.0
                consume(pair)
                pair
            }
            fn numbers() (Number, Number) { relay((42, 7)) }
            fn strings() (Number, String) { relay((42, "kept")) }
        "#},
    ] {
        let bump = Bump::new();
        solve_input(&bump, source)
            .expect("lifting preserves a fixed tuple shape and independent unobserved slots");
    }
}

#[test]
fn optional_argument_lifting_cannot_erase_tuple_shape_requirements() {
    for source in [
        indoc! {r#"
            fn consume(value?: a) {}
            fn relay(pair) {
                consume(pair)
                let first: Number = pair.0
                pair
            }
            fn invalid() { relay((42, false, "extra")) }
        "#},
        indoc! {r#"
            fn consume(value?: a) {}
            fn relay(pair) {
                let first: Number = pair.0
                consume(pair)
                pair
            }
            fn invalid() { relay(("wrong", false)) }
        "#},
        indoc! {r#"
            fn consume(value?: a) {}
            fn relay(pair) {
                consume(pair)
                let first: Number = pair.0
                pair
            }
            fn invalid() (Number, Bool) { relay((42, "kept")) }
        "#},
    ] {
        let bump = Bump::new();
        assert!(solve_input(&bump, source).is_err(), "must reject: {source}");
    }
}

#[test]
fn tuple_projections_share_nested_aliases_before_fixing_arity() {
    assert_inference_snapshot! {r#"
        fn read(value) {
            let first = value.0
            let again = value.0
            (first.0, again.3)
        }
        fn run() { read(((1, false, "unused", 42), ())) }
    "#};
}

#[test]
fn tuple_projections_accumulate_read_and_write_constraints() {
    assert_inference_snapshot! {r#"
        fn update(value) {
            value.0 = 20
            value.3 = 22
            value.0 + value.3
        }
        fn run() Number { update((0, false, "unused", 0)) }
    "#};
}

#[test]
fn tuple_projections_infer_at_least_two_elements() {
    assert_inference_snapshot!("fn first(value) { value.0 }");
}

#[test]
fn tuple_projections_reverse_order_has_same_contract() {
    assert_inference_snapshot! {r#"
        fn sum(pair) Number { pair.1 + pair.0 }
        fn run() Number { sum((20, 22)) }
    "#};
}

#[test]
fn explicit_async_without_await_adds_task() {
    assert_inference_snapshot!("async fn answer() Number { 42 }");
}

#[test]
fn explicit_async_extern_has_task_result() {
    assert_inference_snapshot! {r#"
        #[extern("./client.js", "answer")]
        async fn answer() Number
        fn start() Task[Number] { answer() }
    "#};
}

#[test]
fn explicit_async_trait_contract_adds_task() {
    assert_inference_snapshot! {r#"
        trait Load[a] { async fn load(value: a) Number }
        impl Load[Number] { async fn load(value: Number) Number { value } }
        fn start() Task[Number] { load(42) }
    "#};
}

#[test]
fn explicit_async_does_not_flatten_task_annotation() {
    assert_inference_snapshot!("async fn nested() Task[Number] { async { 42 } }");
}

#[test]
fn explicit_async_block_return_does_not_constrain_outer_function() {
    assert_inference_snapshot! {r#"
        fn answer() String {
            let task = async { return 42 }
            "outer"
        }
    "#};
}

#[test]
fn explicit_async_lambda_annotation_is_actual_task_type() {
    assert_inference_snapshot!("let worker = (x: Number) Task[Number] -> async { x }");
}

#[test]
fn explicit_async_nested_blocks_preserve_both_layers() {
    assert_inference_snapshot!("fn nested() { async { async { 42 } } }");
}

#[test]
fn await_wraps_an_explicit_body_result_in_task() {
    assert_inference_snapshot! {r#"
        async fn load(value: Number) Result[Number] {
            Task.sleep(1).await
            Ok(value)
        }
    "#};
}

#[test]
fn await_inside_a_lambda_belongs_to_the_lambda() {
    assert_inference_snapshot! {r#"
        fn makeWorker() fn(Number) Task[Number] {
            value -> async {
                Task.sleep(1).await
                value
            }
        }

        fn staysSynchronous() Number {
            let worker = value -> async {
                Task.sleep(1).await
                value
            }
            42
        }
    "#};
}

#[test]
fn pipe_forwarding_precedes_await_and_try() {
    assert_inference_snapshot! {r#"
        async fn load(value: Number) Result[Number] {
            Task.sleep(1).await
            Ok(value)
        }

        async fn run() Result[Number] {
            let value = 42 |> load().await?
            Ok(value)
        }
    "#};
}

#[test]
fn constructor_call_arity_is_checked() {
    assert_inference_error_snapshot!(
        "enum Maybe[a] { Just(a) }\nfn invalid() { Maybe::Just(1, 2) }"
    );
}

#[test]
fn derive_rejects_a_payload_without_the_required_instance() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
            enum Payload { Payload }
            #[derive(Show)]
            enum Wrapper { Wrapper(Payload) }
        "#},
    )
    .expect_err("Payload does not have a Show instance");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Trait(alder_solve::SolveTraitError::MissingInstance {
            trait_,
            ..
        }) if trait_.0.name == "Show"
    )));
}
