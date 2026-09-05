//! End-to-end Alder inference tests: parse → canonicalize → constrain → solve.

use alder_ast::{Annotation, FieldPresence, Kind, ModuleId, PackageId, RowExtension, Type};
use alder_can::{Annotations, Context};
use alder_constrain::{Error, ErrorKind};
use alder_region::Located;
use bumpalo::Bump;
use indoc::indoc;

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
            if flag { { name: "present" } } else { record }
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
                    if flag { break { value: 42 } }
                    break record
                }
            }
            fn run() Option[Number] { choose(false, {}).value }
        "#},
        indoc! {r#"
            fn choose(flag: Bool, record: { value?: Number }) {
                loop {
                    if flag { break record }
                    break { value: 42 }
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
fn optional_record_assignment_stores_the_declared_payload() {
    let source = indoc! {r#"
        fn run() Option[Number] {
            let mut record: { value?: Number } = {}
            record.value = 42
            record.value
        }
    "#};
    let bump = Bump::new();
    let result = infer(&bump, source);
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn optional_record_assignment_rejects_an_option_instead_of_the_payload() {
    let source = indoc! {r#"
        fn run() {
            let mut record: { value?: Number } = {}
            record.value = Option.some(42)
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn optional_record_assignment_cannot_read_an_absent_compound_target() {
    let source = indoc! {r#"
        fn run() {
            let mut record: { value?: Number } = {}
            record.value += 1
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn optional_record_assignment_cannot_traverse_an_absent_parent() {
    let source = indoc! {r#"
        fn run() {
            let mut record: { child?: { value: Number } } = {}
            record.child.value = 42
        }
    "#};
    assert!(infer(&Bump::new(), source).is_err());
}

#[test]
fn branch_results_preserve_optional_field_presence() {
    for source in [
        indoc! {r#"
            fn choose(flag: Bool, user: { name?: String }) {
                if flag { { name: "present" } } else { user }
            }
            fn run() Option[String] { choose(false, {}).name }
        "#},
        indoc! {r#"
            fn choose(flag: Bool, user: { name?: String }) {
                if flag { user } else { { name: "present" } }
            }
            fn run() Option[String] { choose(false, {}).name }
        "#},
        indoc! {r#"
            fn choose(flag: Bool, user: { name?: String }) {
                match flag {
                    true => ({ name: "present" }),
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
        fn suspended() Number {
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
            fn missing(flag: Bool) Number {
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
        fn operation() {
            Task.sleep(0).await
            shared
        }
        let task = operation()
        fn bad() {
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
        let mut stored = []
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
        }) if (actual == "String" && expected == "Number")
            || (actual == "Number" && expected == "String")
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
        let mut identity = (value) -> { value }
        fn number() { identity(1) }
        fn text() { identity("text") }
    "#};
}

#[test]
fn generalization_subtracts_mutable_environment_variables() {
    assert_inference_error_snapshot! {r#"
        let mut identity = (value) -> { value }
        fn forward(value) { identity(value) }
        fn number() { forward(1) }
        fn text() { forward("text") }
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
            fn specialize(value: Result[Number, String]) { adapt(value) }
        "#},
    )
    .unwrap();

    assert_eq!(
        render_annotations(&annotations),
        concat!(
            "adapt: forall a, b. fn(a[b]) a[b]\n",
            "specialize: fn(Result[Number, String]) Result[Number, String]"
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
        fn specialize(value: Result[Number, String]) { adapt(value) }
    "#};
}

#[test]
fn higher_kinded_unification_preserves_two_hole_order() {
    assert_inference_snapshot! {r#"
        fn adapt(value: f[a, b], first: a, second: b) f[a, b] { value }
        fn specialize(value: Result[Number, String]) {
            adapt(value, 1, "second")
        }
    "#};
}

#[test]
fn higher_kinded_unification_rejects_inconsistent_partial_sections() {
    assert_inference_error_snapshot! {r#"
        fn combine(left: f[a], right: f[b]) f[a] { left }
        fn invalid(
            left: Result[Number, String],
            right: Result[Bool, Bool],
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
        fn specialize(value: Wrapped[Number, String]) { adapt(value) }
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
            fn traverse_result(value: Result[Number, String]) Option[Result[String, String]] {
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
            fn first(values: Array[Number]) Option[Number] { next(values) }
            fn generic(value: i) Option[Number]
                where i: Iterator, i.Item == Number
            {
                next(value)
            }
        "#},
    )
    .expect("Array's Iterator Item projection normalizes to its element type");
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
        fn wait() Task[()] {
            Task.sleep(1).await
        }
    "#
    );
}

#[test]
fn await_infers_a_task_return_without_an_explicit_wrapper() {
    assert_inference_snapshot! {r#"
        #[extern("alder:kernel", "$taskSleep")]
        fn sleep(milliseconds: Number) Task[()]

        fn wait() {
            sleep(1).await
        }
    "#};
}

#[test]
fn await_wraps_an_explicit_body_result_in_task() {
    assert_inference_snapshot! {r#"
        fn load(value: Number) Result[Number] {
            Task.sleep(1).await
            Ok(value)
        }
    "#};
}

#[test]
fn await_inside_a_lambda_belongs_to_the_lambda() {
    assert_inference_snapshot! {r#"
        fn makeWorker() fn(Number) Task[Number] {
            value -> {
                Task.sleep(1).await
                value
            }
        }

        fn staysSynchronous() Number {
            let worker = value -> {
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
        fn load(value: Number) Result[Number] {
            Task.sleep(1).await
            Ok(value)
        }

        fn run() Result[Number] {
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
