//! Local annotation scope and generic-contract regressions.

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
            imports: alder_can::resolve_imports(bump, &parsed, PackageId::Application),
            interfaces: &[],
        },
        &parsed,
    )
    .expect("source canonicalizes");
    let constraints = alder_constrain::constrain(bump, canonical.module);
    let database = alder_solve::TraitDatabase::build(bump, canonical.module, &[]);
    alder_solve::solve(bump, &constraints, &database)
}

#[test]
fn default_method_scope_preserves_unmentioned_higher_kinded_parameters() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Container[f] where f: Functor {
            fn check() Number {
                let identity = (value: f[Number]) f[Number] -> {
                    let local: f[Number] = map(value, number -> number)
                    local
                }
                42
            }
        }
    "#},
    )
    .expect("the full higher-kinded trait head and its Functor evidence scope over defaults");
}

#[test]
fn default_method_scope_rejects_unmentioned_constructor_specialization() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Container[f] where f: Functor {
            fn check() Number {
                let local: f[Number] = [42]
                42
            }
        }
    "#},
    )
    .expect_err("a default body cannot specialize the trait constructor to Array");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Core(Error {
            kind: ErrorKind::GenericSpecialization { .. },
            ..
        })
    )));
}

#[test]
fn default_method_local_annotations_use_unmentioned_trait_bounds() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Describe[a] where a: Show {
            fn formatter() Number {
                let render = (value: a) String -> {
                    let local: a = value
                    show(local)
                }
                42
            }
        }
    "#},
    )
    .expect("default bodies inherit the full trait head and its superclasses");
}

#[test]
fn local_annotations_in_default_methods_cannot_specialize_trait_parameters() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        trait Marker[a] {
            fn value() Number {
                let unused: a = 42
                42
            }
        }
    "#},
    )
    .expect_err("a trait parameter remains universal even when absent from method arguments");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Core(Error {
            kind: ErrorKind::GenericSpecialization { .. },
            ..
        })
    )));
}

#[test]
fn recursive_local_annotations_preserve_independent_contracts() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn first(value: a, remaining: Number) a {
            let local: a = value
            if remaining == 0 { local } else { second(local, remaining - 1) }
        }
        fn second(value: b, remaining: Number) b {
            let local: b = value
            if remaining == 0 { local } else { first(local, remaining - 1) }
        }
        fn use_it() {
            let number: Number = first(42, 3)
            let text: String = second("text", 4)
        }
    "#},
    )
    .expect("SCC peers retain their own universal names and independent calls");
}

#[test]
fn recursive_local_annotations_cannot_hide_peer_specialization() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        fn first(value: a, flag: Bool) a {
            let local: a = value
            if flag { second(local, false) } else { local }
        }
        fn second(value: b, flag: Bool) b {
            let specialized: b = 42
            if flag { first(value, false) } else { value }
        }
    "#},
    )
    .expect_err("a local annotation cannot specialize a recursive peer's universal");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Core(Error {
            kind: ErrorKind::GenericSpecialization { .. },
            ..
        })
    )));
}

#[test]
fn local_annotation_preserves_trait_method_contracts_and_bounds() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        trait Copy[a] {
            fn copy(value: a, other: b) b {
                let result: b = other
                result
            }
        }
        impl Copy[Number] {}
        impl Copy[String] {
            fn copy(value: String, other: b) b {
                let result: b = other
                result
            }
        }
        fn render(value: a) String where a: Show {
            let local: a = value
            show(local)
        }
        fn use_it() {
            let number: Number = copy("override", 42)
            let text: String = copy(42, "default")
        }
    "#},
    )
    .expect("local annotations preserve default, override, and bound evidence");
}

#[test]
fn local_annotation_rejects_specialization_inside_async_blocks() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        import task
        fn invalid(value: a) Task[a] {
            async {
                task.sleep(0).await
                let local: a = 42
                value
            }
        }
    "#},
    )
    .expect_err("async bodies retain enclosing universal variables");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Core(Error {
            kind: ErrorKind::GenericSpecialization { .. },
            ..
        })
    )));
}

#[test]
fn local_annotation_fresh_names_do_not_leak_to_siblings() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn values() {
            let number: a = 42
            let text: a = "text"
            let checked_number: Number = number
            let checked_text: String = text
        }
    "#},
    )
    .expect("fresh names belong to their individual local annotations");
}

#[test]
fn local_annotation_does_not_generalize_a_shared_array() {
    let bump = Bump::new();
    assert!(
        solve_input(
            &bump,
            indoc! {r#"
        fn invalid() {
            let values: Array[a] = []
            array.push(values, 42)
            let texts: Array[String] = values
        }
    "#}
        )
        .is_err()
    );
}

#[test]
fn local_annotation_tracks_lambda_and_async_scopes() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn copy(value: a) Task[a] {
            let make = (other: a) Task[a] -> async {
                let result: a = other
                result
            }
            make(value)
        }
        fn make() {
            let number = (value: a) a -> {
                let local: a = value
                local
            }
            let text = (value: a) a -> {
                let local: a = value
                local
            }
            let n: Number = number(42)
            let s: String = text("text")
        }
    "#},
    )
    .expect("local annotations inherit the innermost callable scope across async blocks");
}

#[test]
fn local_annotation_reuses_enclosing_generic_variables() {
    let bump = Bump::new();
    solve_input(
        &bump,
        indoc! {r#"
        fn copy(value: a) a {
            let copied: a = value
            copied
        }
        fn transform(values: f[a], map: fn(f[a]) f[b]) f[b] {
            let result: f[b] = map(values)
            result
        }
        fn use_it() {
            let number: Number = copy(42)
            let text: String = copy("text")
        }
    "#},
    )
    .expect("local annotations must share their enclosing generic contract");
}

#[test]
fn local_annotation_cannot_specialize_an_enclosing_generic() {
    let bump = Bump::new();
    let errors = solve_input(
        &bump,
        indoc! {r#"
        fn unchanged(value: a) a {
            let unused: a = 42
            value
        }
    "#},
    )
    .expect_err("an unused local annotation still refers to the declared universal");
    assert!(errors.iter().any(|error| matches!(
        error,
        alder_solve::SolveError::Core(Error {
            kind: ErrorKind::GenericSpecialization { .. },
            ..
        })
    )));
}
