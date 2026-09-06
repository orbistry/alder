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
        fn invalid(value: a) Task[a] {
            async {
                Task.sleep(0).await
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
            Array.push(values, 42)
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
