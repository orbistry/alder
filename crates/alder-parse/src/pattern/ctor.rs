//! Constructor and tag patterns.
//!
//! `None`, `Some(x)`, `Option::Some(x)`, `Rect { width, .. }` and
//! `:timeout`, `:not_found(id)`. An argument list or record body may be
//! separated from the name by whitespace but must start on the same line
//! (the `Path {` rule of docs/parser-internals.md §2.1 rule 5).
//!
//! See docs/parser-internals.md §5.14.
// OWNER: pattern/ctor.rs (Wave 1)

use alder_region::{Located, Position};
use alder_source::Pattern;
use bumpalo::collections::Vec as BumpVec;

use crate::error::PCtor;
use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// At an uppercase letter: `None`, `Some(x)`, `Option::Some(x)`, `Rect { .. }`.
    pub(super) fn pattern_ctor(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        let path = self.path(error::Pattern::Start, error::Pattern::PathMember)?;
        if self.peek() == Some(b':') && self.peek_at(1) == Some(b':') {
            // `path()` stops before `::lower` (§5.8). `Foo::bar` names a value,
            // which a pattern can only match through `^Foo::bar`; consume the
            // `::` like a dangling one (§10.42) and report after it.
            // TODO(wave0): a `Pattern::PathVar` variant would carry that hint;
            // `PathMember` is the nearest existing one.
            self.advance_by(2);
            let (row, col) = self.position();
            return Err(error::Pattern::PathMember(row, col));
        }
        let end = self.get_position();
        self.chomp();
        let same_line = !self.newline_since(end);
        match self.peek() {
            Some(b'(') if same_line => {
                let args = self.specialize(
                    |bump, e, _, _| error::Pattern::Ctor(bump.alloc(e), start.line, start.column),
                    |p| p.pattern_ctor_args(),
                )?;
                Ok(self.add_end(start, Pattern::Ctor { path, args }))
            }
            Some(b'{') if same_line => {
                let (fields, rest) = self.specialize(
                    |bump, e, row, col| {
                        error::Pattern::Ctor(
                            bump.alloc(PCtor::Record(bump.alloc(e), row, col)),
                            start.line,
                            start.column,
                        )
                    },
                    |p| {
                        p.advance();
                        p.pattern_record_fields()
                    },
                )?;
                Ok(self.add_end(start, Pattern::CtorRecord { path, fields, rest }))
            }
            _ => Ok(self.pattern_at(start, end, Pattern::Ctor { path, args: &[] })),
        }
    }

    /// At `:`: `:tag` / `:tag(p, …)`.
    pub(super) fn pattern_tag(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>> {
        let name = self.tag_name(error::Pattern::Start, error::Pattern::TagName)?;
        let end = self.get_position();
        self.chomp();
        if self.peek() == Some(b'(') && !self.newline_since(end) {
            let args = self.specialize(
                |bump, e, _, _| error::Pattern::Tag(bump.alloc(e), start.line, start.column),
                |p| p.pattern_ctor_args(),
            )?;
            Ok(self.add_end(start, Pattern::Tag { name, args }))
        } else {
            Ok(self.pattern_at(start, end, Pattern::Tag { name, args: &[] }))
        }
    }

    /// At `(`: `( pattern { ',' pattern } [','] )` — at least one argument,
    /// as SPEC's `variant` has no zero-type tuple form either, so `Foo()`
    /// is `Arg(Start)` at `)`. Consumes the closing `)` and nothing after it.
    fn pattern_ctor_args(&mut self) -> Result<&'a [&'a Located<Pattern<'a>>], PCtor<'a>> {
        self.advance();
        self.chomp();
        let mut args = BumpVec::new_in(self.bump);
        loop {
            let arg = self.specialize(
                |bump, e, row, col| PCtor::Arg(bump.alloc(e), row, col),
                |p| p.pattern(),
            )?;
            args.push(arg);
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                    if self.peek() == Some(b')') {
                        self.advance();
                        break;
                    }
                }
                Some(b')') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(PCtor::End(row, col));
                }
            }
        }
        Ok(args.into_bump_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_pattern_error_snapshot, assert_pattern_snapshot};

    #[test]
    fn ctor_bare() {
        assert_pattern_snapshot!("None");
    }

    #[test]
    fn ctor_qualified() {
        assert_pattern_snapshot!("Option::Some(x)");
    }

    #[test]
    fn ctor_one_arg() {
        assert_pattern_snapshot!("Some(x)");
    }

    #[test]
    fn ctor_many_args() {
        assert_pattern_snapshot!("Node(left, value, right)");
    }

    #[test]
    fn ctor_nested() {
        assert_pattern_snapshot!("Some(Some(x))");
    }

    #[test]
    fn ctor_record() {
        assert_pattern_snapshot!("Rect { width, height }");
    }

    #[test]
    fn ctor_record_rename() {
        assert_pattern_snapshot!("Rect { width: w, height: h }");
    }

    #[test]
    fn ctor_record_rest() {
        assert_pattern_snapshot!("Rect { width, .. }");
    }

    #[test]
    fn tag_bare() {
        assert_pattern_snapshot!(":timeout");
    }

    #[test]
    fn tag_args() {
        assert_pattern_snapshot!(":not_found(id)");
    }

    #[test]
    fn error_ctor_unclosed() {
        assert_pattern_error_snapshot!("Some(x");
    }

    #[test]
    fn error_ctor_record_field() {
        assert_pattern_error_snapshot!("Rect { 1 }");
    }

    #[test]
    fn error_tag_name() {
        assert_pattern_error_snapshot!(":Foo");
    }

    #[test]
    fn error_path_dangling() {
        assert_pattern_error_snapshot!("Foo::");
    }

    #[test]
    fn error_path_lower_member() {
        assert_pattern_error_snapshot!("Foo::bar");
    }

    #[test]
    fn error_ctor_empty_args() {
        assert_pattern_error_snapshot!("Foo()");
    }
}
