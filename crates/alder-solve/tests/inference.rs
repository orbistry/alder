//! End-to-end Alder inference tests: parse → canonicalize → constrain → solve.

use alder_ast::{Annotation, FieldPresence, ModuleId, PackageId, RowExtension, Type};
use alder_can::{Annotations, Context};
use alder_constrain::{Error, UnionFind};
use alder_region::Located;
use bumpalo::Bump;
use indoc::indoc;

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
    let mut uf = UnionFind::new();
    let constraints = alder_constrain::constrain(bump, &mut uf, can_result.module);
    alder_solve::run(bump, &mut uf, &constraints)
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
    if annotation.free_vars.is_empty() {
        typ
    } else {
        format!("forall {}. {typ}", annotation.free_vars.join(", "))
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
