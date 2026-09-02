//! Names and paths: Var, Path, PathVar, RecordCtor, macro call dispatch.
//!
//! `primary` has already screened reserved and SQL words before a lowercase
//! name reaches `name_or_path`, so the only lowercase failure left is the
//! adjacent `!(` of a macro call, which `loop_.rs::macro_call` handles.
//! `Upper::lower` is `PathVar`; `path()` stops before the `::lower` (§5.8),
//! and a member that `lower_name` refuses (`Foo::type`, or `Foo::limit` in
//! query mode) is reported as `PathMember` after the `::`, like a dangling
//! one (§10.42).
//!
//! `Shape::Rect { … }` is dispatched by the postfix loop, which checks the
//! same-line rule (§2.1 rule 5) and `record_ctor_allowed()` (§2.3) before
//! calling `record_ctor`.
//!
//! See docs/parser-internals.md §5.13.
// OWNER: expression/path.rs (Wave 2)

use alder_region::{Located, Position};
use alder_source::{Expr, Path};

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// At a letter: `Var` (adjacent `!(` → `macro_call`), `Path`, or `PathVar`.
    pub(crate) fn name_or_path(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        if self.peek_lower() {
            let name = self.located_lower(error::Expr::Start)?;
            if self.peek() == Some(b'!') && self.peek_at(1) == Some(b'(') {
                return self.macro_call(start, name);
            }
            return Ok(self.add_end(start, Expr::Var(name.value)));
        }
        let path = self.path(error::Expr::Start, error::Expr::PathMember)?;
        if self.peek() == Some(b':') && self.peek_at(1) == Some(b':') {
            self.advance_by(2);
            let name = self.located_lower(error::Expr::PathMember)?;
            return Ok(self.add_end(start, Expr::PathVar { path, name }));
        }
        Ok(self.add_end(start, Expr::Path(path)))
    }

    /// At `{` after a `Path` (same line, `record_ctor_allowed()`).
    pub(crate) fn record_ctor(
        &mut self,
        start: Position,
        path: Path<'a>,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let fields = self.specialize(
            |bump, e, row, col| error::Expr::RecordCtor(bump.alloc(e), row, col),
            |p| {
                p.advance();
                p.with_record_ctor(true, |p| p.record_fields())
            },
        )?;
        Ok(self.add_end(start, Expr::RecordCtor { path, fields }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_expression_error_snapshot, assert_expression_snapshot};

    #[test]
    fn var_simple() {
        assert_expression_snapshot!("x");
    }

    #[test]
    fn var_camel() {
        assert_expression_snapshot!("fetchPrefs");
    }

    #[test]
    fn var_underscore_inside() {
        assert_expression_snapshot!("snake_case_1");
    }

    #[test]
    fn path_bare() {
        assert_expression_snapshot!("None");
    }

    #[test]
    fn path_qualified() {
        assert_expression_snapshot!("Option::Some");
    }

    #[test]
    fn path_deep() {
        assert_expression_snapshot!("Ui::Button::Primary");
    }

    #[test]
    fn path_var() {
        assert_expression_snapshot!("Show::show");
    }

    #[test]
    fn path_dot_access() {
        assert_expression_snapshot!("Array.map");
    }

    #[test]
    fn record_ctor() {
        assert_expression_snapshot!("Shape::Rect { width: 1, height: 2 }");
    }

    #[test]
    fn record_ctor_shorthand() {
        assert_expression_snapshot!("Point { x, y }");
    }

    #[test]
    fn record_ctor_empty() {
        assert_expression_snapshot!("Empty {}");
    }

    /// Under `no_record_ctor` (an `if` head, §2.3) the `{` is left alone and
    /// the path is the whole expression.
    #[test]
    fn record_ctor_disabled_in_head() {
        let bump = bumpalo::Bump::new();
        let code = "Shape::Empty { }";
        let src = bump.alloc_str(code);
        let mut parser = crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .with_record_ctor(false, |p| p.expression())
            .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
        assert_eq!(
            parser.position(),
            (1, 14),
            "the `{{` must be left unconsumed"
        );
        insta::with_settings!({
            description => code,
            omit_expression => true,
        }, {
            insta::assert_debug_snapshot!(result);
        });
    }

    #[test]
    fn error_reserved_word() {
        assert_expression_error_snapshot!("type");
    }

    #[test]
    fn error_path_dangling_colons() {
        assert_expression_error_snapshot!("Foo::");
    }

    #[test]
    fn error_path_reserved_member() {
        assert_expression_error_snapshot!("Foo::type");
    }

    #[test]
    fn error_sql_word_outside_query_is_var() {
        assert_expression_snapshot!("select");
    }
}
