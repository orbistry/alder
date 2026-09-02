//! `component` declarations.
//!
//! See docs/parser-internals.md §5.11 and §10.16.
//!
//! Grammar (SPEC.md "Items", with §10.16's lowercase names):
//!
//! ```ebnf
//! component_decl = 'component' ( upper_ident | lower_ident ) '(' [ params ] ')' block ;
//! ```
//!
//! The name is `Counter` for an ordinary component or `page` for a
//! route-file component (web.md's `pub component page(props: …)`); a
//! lowercase name goes through `lower_name`, so a reserved word (`component
//! for()`) is `Component::Name`. Parameters reuse `params()` and the body is
//! always a `block()` (§2.2: `component` bodies never consult the
//! record-vs-block heuristic).
//!
//! Conventions: `component_decl` runs after the `component` keyword and
//! leaves the cursor where `block()` left it (past trailing whitespace).
// OWNER: item/component.rs (Wave 3)

use alder_source::ComponentDecl;

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// After `component`.
    pub(crate) fn component_decl(&mut self) -> Result<&'a ComponentDecl<'a>, error::Component<'a>> {
        self.chomp();
        let name = if self.peek_upper() {
            self.located_upper(error::Component::Name)?
        } else {
            self.located_lower(error::Component::Name)?
        };
        self.chomp();
        let params = self.specialize(
            |bump, e, row, col| error::Component::Params(bump.alloc(e), row, col),
            |p| p.params(),
        )?;
        self.chomp();
        let body = self.specialize(
            |bump, e, row, col| error::Component::Body(bump.alloc(e), row, col),
            |p| p.block(),
        )?;
        Ok(self.alloc(ComponentDecl { name, params, body }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::assert_item_snapshot;

    // Deviation from §7.1, following item/fn_.rs: the pair below drives
    // `component_decl()` directly (the input starts at the `component`
    // keyword, which the macro consumes). The `pub` form goes through
    // `item()`.

    /// Snapshot test macro for a successful `component_decl()` parse (input starts at `component`).
    macro_rules! assert_component_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            if let Err((row, col)) = parser.keyword(b"component", |row, col| (row, col)) {
                panic!("input must start with `component` ({row}:{col})\n\nSource:\n{code}");
            }
            let result = parser
                .component_decl()
                .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
            assert!(
                parser.is_eof(),
                "unconsumed input at {:?}\n\nSource:\n{code}",
                parser.position()
            );
            insta::with_settings!({
                description => code,
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(result);
            });
        }};
    }

    /// Snapshot test macro for a `component_decl()` parse error (input starts at `component`).
    macro_rules! assert_component_error_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            if let Err((row, col)) = parser.keyword(b"component", |row, col| (row, col)) {
                panic!("input must start with `component` ({row}:{col})\n\nSource:\n{code}");
            }
            let err = parser
                .component_decl()
                .err()
                .unwrap_or_else(|| panic!("expected Err, got Ok\n\nSource:\n{code}"));
            insta::with_settings!({
                description => code,
                omit_expression => true,
            }, {
                insta::assert_debug_snapshot!(err);
            });
        }};
    }

    #[test]
    fn component_simple() {
        assert_component_snapshot!("component App() { render() }");
    }

    #[test]
    fn component_props() {
        assert_component_snapshot!("component UserCard(props: { id: Id }) { props.id }");
    }

    #[test]
    fn component_lowercase_page() {
        assert_component_snapshot!("component page(props: { data: PageData }) { props.data }");
    }

    #[test]
    fn component_state_body() {
        assert_component_snapshot!(
            r#"
            component App() {
                let mut selected = state(0)
                selected
            }
        "#
        );
    }

    /// web.md "Routing" (`pub component page`).
    #[test]
    fn component_pub() {
        assert_item_snapshot!(
            r#"
            pub component page(props: { data: PageData }) {
                <h1>{props.data.user.name}</h1>
            }
        "#
        );
    }

    /// language.md "Components and state" (`pub` dropped: the direct macro
    /// starts at `component`).
    #[test]
    fn component_with_state_and_markup() {
        assert_component_snapshot!(
            r#"
            component Counter(props: { start?: Number, label: String }) {
                let mut count = state(props.start ?? 0)
                let double = count * 2                     // memoized automatically

                <button onClick={fn() count += 1}>
                    {props.label}: {count} ({double})
                </button>
            }
        "#
        );
    }

    #[test]
    fn error_no_body() {
        assert_component_error_snapshot!("component App()");
    }

    #[test]
    fn error_name() {
        assert_component_error_snapshot!("component 1() {}");
    }

    #[test]
    fn error_name_reserved() {
        assert_component_error_snapshot!("component for() {}");
    }

    #[test]
    fn error_params() {
        assert_component_error_snapshot!("component App {}");
    }

    #[test]
    fn error_params_end() {
        assert_component_error_snapshot!("component App(a b) {}");
    }

    #[test]
    fn error_body() {
        assert_component_error_snapshot!("component App() { let }");
    }
}
