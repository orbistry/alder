//! Mutable captures must stay monomorphic, including across recursive groups.

use alder_ast::{ModuleId, PackageId};
use alder_can::Context;
use alder_constrain::{Error, ErrorKind};
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

fn assert_type_mismatch(source: &str) {
    let bump = Bump::new();
    let errors = solve_input(&bump, source).expect_err("shared payload cannot change type");
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
fn array_iterator_keeps_its_mutable_source_payload_monomorphic() {
    assert_type_mismatch(indoc! {r#"
        let values = []
        let iterator = Array.iter(values)
        let alias = iterator
        fn invalid() Option[String] {
            Array.push(values, 42)
            next(alias)
        }
    "#});
}

#[test]
fn independent_array_iterators_preserve_factory_polymorphism() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
            fn fresh(values: Array[a]) ArrayIterator[a] { Array.iter(values) }
            fn valid() {
                let numbers: Option[Number] = next(fresh([42]))
                let strings: Option[String] = next(fresh(["text"]))
            }
        "#},
    )
    .expect("independent cursors may have independent payload types");
}

#[test]
fn allocated_closure_cannot_regeneralize_hidden_mutable_state() {
    assert_type_mismatch(indoc! {r#"
        fn make() {
            let items = []
            value -> {
                Array.push(items, value)
                items
            }
        }
        let store = make()
        let alias = store
        fn invalid() {
            store(42)
            alias("text")
        }
    "#});
}

#[test]
fn independent_closure_allocations_preserve_function_polymorphism() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn make() {
            let items = []
            value -> {
                Array.push(items, value)
                items
            }
        }
        fn valid() {
            let numbers = make()
            let strings = make()
            let first: Array[Number] = numbers(42)
            let second: Array[String] = strings("text")
        }
    "#},
    )
    .expect("separate factory calls allocate separate captured arrays");
}

#[test]
fn recursive_functions_cannot_regeneralize_captured_array() {
    assert_type_mismatch(indoc! {r#"
        let shared = []
        fn first(count: Number) {
            if count == 0 { shared } else { second(count - 1) }
        }
        fn second(count: Number) {
            if count == 0 { shared } else { first(count - 1) }
        }
        let alias = second
        fn invalid() {
            Array.push(first(0), 42)
            let strings: Array[String] = alias(1)
        }
    "#});
}

#[test]
fn recursive_group_restricts_state_without_restricting_independent_arguments() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        let shared = [42]
        fn first(value, count: Number) {
            if count == 0 { (shared, value) } else { second(value, count - 1) }
        }
        fn second(value, count: Number) {
            if count == 0 { (shared, value) } else { first(value, count - 1) }
        }
        fn valid() {
            let numbers: (Array[Number], Number) = first(42, 0)
            let strings: (Array[Number], String) = second("text", 1)
        }
    "#},
    )
    .expect("unrelated arguments remain universal while the captured array stays Number");
}

#[test]
fn restricted_record_in_same_recursive_group_protects_its_array() {
    assert_type_mismatch(indoc! {r#"
        let shared = { items: [], read: () -> read() }
        fn read() { shared.items }
        fn invalid() {
            Array.push(read(), 42)
            let strings: Array[String] = shared.read()
        }
    "#});
}

#[test]
fn reusable_async_closure_cannot_regeneralize_captured_state() {
    assert_type_mismatch(indoc! {r#"
        fn make() {
            let items = []
            value -> async {
                Task.sleep(0).await
                Array.push(items, value)
                items
            }
        }
        let store = make()
        async fn invalid() {
            store(42).await
            store("text").await
        }
    "#});
}

#[test]
fn tuple_overlay_error_constraints_preserve_captured_payload_identity() {
    assert_type_mismatch(indoc! {r#"
        let shared = []
        fn expose(pair, patch) {
            pair.0 = { ..{ value: Err(:saved(shared)) }, ..patch }
            pair
        }
        fn propagate(pair, patch) {
            let value = expose(pair, patch).0.value?
            Ok(value)
        }
        fn write() { Array.push(shared, 42) }
        fn invalid() Result[Number, [:saved(Array[String])]] {
            propagate(({ value: Err(:saved([])) }, ()), {})
        }
    "#});
}

#[test]
fn tuple_overlay_error_constraints_allow_independent_allocations() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn expose(pair, patch) {
            pair.0 = { ..{ value: Err(:saved([])) }, ..patch }
            pair
        }
        fn propagate(pair, patch) {
            let value = expose(pair, patch).0.value?
            Ok(value)
        }
        fn numbers() Result[Number, [:saved(Array[Number])]] {
            propagate(({ value: Err(:saved([])) }, ()), {})
        }
        fn strings() Result[String, [:saved(Array[String])]] {
            propagate(({ value: Err(:saved([])) }, ()), {})
        }
    "#},
    )
    .expect("connected deferred constraints freshen together for independent allocations");
}

#[test]
fn uncalled_export_retains_connected_tuple_overlay_error_constraints() {
    let bump = Bump::new();
    let output = solve_input(
        &bump,
        indoc! {r#"
        let shared = [42]
        fn expose(pair, patch) {
            pair.0 = { ..{ value: Err(:saved(shared)) }, ..patch }
            pair
        }
        pub fn propagate(pair, patch) {
            let value = expose(pair, patch).0.value?
            Ok(value)
        }
    "#},
    )
    .expect("an uncalled export can retain constraints and a concrete captured payload");
    let annotation = output
        .annotations
        .iter()
        .find(|(name, _)| name.name == "propagate")
        .unwrap()
        .1;
    assert!(!annotation.tuple_shapes.is_empty());
    assert!(!annotation.record_overlays.is_empty());
    assert!(!annotation.error_row_inclusions.is_empty());
}
