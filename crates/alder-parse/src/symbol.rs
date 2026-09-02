//! Operator scanning for Alder.
//!
//! A fixed longest-match table (custom operators are out of scope). Elm-habit
//! tokens (`->`, `|`, `++`, `::`, `..`, `<|`, `>>`, `<<`, `^`) are recognized
//! only to produce a `BadOperator` hint; `= => += -= *= /=` terminate a chain
//! and are never consumed here. See docs/parser-internals.md §2 and §5.4.

use alder_region::Located;
use alder_source::{AssignOp, BinOp};

use crate::error::BadOperator;
use crate::keyword::is_ident_byte;
use crate::{Col, Parser, Row};

/// What the longest-match scan found at the cursor.
enum Scan {
    /// A binary operator of the given byte length.
    Op(BinOp, usize),
    /// An Elm-habit token; reported, never consumed.
    Bad(BadOperator),
    /// A chain terminator or a non-operator; nothing consumed.
    None,
}

#[allow(unused)]
impl<'a> Parser<'a> {
    /// Longest-match over the fixed table. `Ok(None)` (nothing consumed) for
    /// chain terminators (`=`, `=>`, `+=`, `-=`, `*=`, `/=`) and non-operators.
    /// `in` is returned as `BinOp::In` only when `in_query()`.
    /// Elm-habit tokens produce `to_error(BadOperator, …)`.
    pub(crate) fn binop<E>(
        &mut self,
        to_error: impl FnOnce(BadOperator, Row, Col) -> E,
    ) -> Result<Option<Located<BinOp>>, E> {
        match self.scan_operator() {
            Scan::Op(op, len) => {
                let start = self.get_position();
                self.advance_by(len);
                Ok(Some(self.located(start, op)))
            }
            Scan::Bad(bad) => {
                let (row, col) = self.position();
                Err(to_error(bad, row, col))
            }
            Scan::None => Ok(None),
        }
    }

    /// `=` (not `==`/`=>`), `+=`, `-=`, `*=`, `/=`. None without consuming otherwise.
    pub(crate) fn assign_op(&mut self) -> Option<Located<AssignOp>> {
        let (op, len) = match (self.peek()?, self.peek_at(1)) {
            (b'=', Some(b'=' | b'>')) => return None,
            (b'=', _) => (AssignOp::Set, 1),
            (b'+', Some(b'=')) => (AssignOp::Add, 2),
            (b'-', Some(b'=')) => (AssignOp::Sub, 2),
            (b'*', Some(b'=')) => (AssignOp::Mul, 2),
            (b'/', Some(b'=')) => (AssignOp::Div, 2),
            _ => return None,
        };
        let start = self.get_position();
        self.advance_by(len);
        Some(self.located(start, op))
    }

    /// Longest match at the cursor without consuming.
    fn scan_operator(&self) -> Scan {
        let Some(b0) = self.peek() else {
            return Scan::None;
        };
        let b1 = self.peek_at(1);
        match (b0, b1) {
            (b'|', Some(b'>')) => Scan::Op(BinOp::Pipe, 2),
            (b'|', Some(b'|')) => Scan::Op(BinOp::Or, 2),
            (b'|', _) => Scan::Bad(BadOperator::Bar),
            (b'?', Some(b'?')) => Scan::Op(BinOp::Coalesce, 2),
            (b'&', Some(b'&')) => Scan::Op(BinOp::And, 2),
            (b'=', Some(b'=')) => Scan::Op(BinOp::Eq, 2),
            (b'!', Some(b'=')) => Scan::Op(BinOp::NotEq, 2),
            (b'<', Some(b'=')) => Scan::Op(BinOp::LtEq, 2),
            (b'<', Some(b'|')) => Scan::Bad(BadOperator::PipeLeft),
            (b'<', Some(b'<')) => Scan::Bad(BadOperator::ComposeLeft),
            (b'<', _) => Scan::Op(BinOp::Lt, 1),
            (b'>', Some(b'=')) => Scan::Op(BinOp::GtEq, 2),
            (b'>', Some(b'>')) => Scan::Bad(BadOperator::ComposeRight),
            (b'>', _) => Scan::Op(BinOp::Gt, 1),
            (b'+', Some(b'=')) => Scan::None,
            (b'+', Some(b'+')) => Scan::Bad(BadOperator::PlusPlus),
            (b'+', _) => Scan::Op(BinOp::Add, 1),
            (b'-', Some(b'=')) => Scan::None,
            (b'-', Some(b'>')) => Scan::Bad(BadOperator::Arrow),
            (b'-', _) => Scan::Op(BinOp::Sub, 1),
            (b'*', Some(b'=')) => Scan::None,
            (b'*', _) => Scan::Op(BinOp::Mul, 1),
            // `/=` is compound assignment; `//` is a comment.
            (b'/', Some(b'=' | b'/')) => Scan::None,
            (b'/', _) => Scan::Op(BinOp::Div, 1),
            (b'%', _) => Scan::Op(BinOp::Rem, 1),
            (b':', Some(b':')) => Scan::Bad(BadOperator::DoubleColon),
            (b'.', Some(b'.')) => Scan::Bad(BadOperator::DotDot),
            (b'^', _) => Scan::Bad(BadOperator::Caret),
            (b'i', Some(b'n'))
                if self.in_query() && !self.peek_at(2).is_some_and(is_ident_byte) =>
            {
                Scan::Op(BinOp::In, 2)
            }
            _ => Scan::None,
        }
    }
}

/// Can `b` begin an operator or chain-terminator token?
#[allow(unused)]
#[inline]
pub fn is_operator_char(b: u8) -> bool {
    matches!(
        b,
        b'+' | b'-'
            | b'*'
            | b'/'
            | b'%'
            | b'='
            | b'!'
            | b'<'
            | b'>'
            | b'&'
            | b'|'
            | b'?'
            | b'^'
            | b':'
            | b'.'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;

    /// `Err` carries the `BadOperator` rendered with `Debug` (error types are not `PartialEq`).
    fn binop_at(src: &str, in_query: bool) -> (Result<Option<BinOp>, String>, u16) {
        let bump = Bump::new();
        let text = bump.alloc_str(src);
        let mut parser = Parser::new(&bump, text.as_bytes());
        let result = parser.with_query(in_query, |p| p.binop(|bad, _, _| format!("{bad:?}")));
        let (_, col) = parser.position();
        (result.map(|op| op.map(|l| l.value)), col)
    }

    fn assign_at(src: &str) -> (Option<AssignOp>, u16) {
        let bump = Bump::new();
        let text = bump.alloc_str(src);
        let mut parser = Parser::new(&bump, text.as_bytes());
        let result = parser.assign_op().map(|l| l.value);
        let (_, col) = parser.position();
        (result, col)
    }

    #[test]
    fn binop_longest_match_each() {
        let cases = [
            ("+ 1", BinOp::Add, 2),
            ("- 1", BinOp::Sub, 2),
            ("* 1", BinOp::Mul, 2),
            ("/ 1", BinOp::Div, 2),
            ("% 1", BinOp::Rem, 2),
            ("== 1", BinOp::Eq, 3),
            ("!= 1", BinOp::NotEq, 3),
            ("< 1", BinOp::Lt, 2),
            ("<= 1", BinOp::LtEq, 3),
            ("> 1", BinOp::Gt, 2),
            (">= 1", BinOp::GtEq, 3),
            ("&& 1", BinOp::And, 3),
            ("|| 1", BinOp::Or, 3),
            ("?? 1", BinOp::Coalesce, 3),
            ("|> 1", BinOp::Pipe, 3),
            // Longest match: `==-1` is `==` then `-1`, `<-1` is `<` then `-1`.
            ("==-1", BinOp::Eq, 3),
            ("<-1", BinOp::Lt, 2),
        ];
        for (src, op, col) in cases {
            assert_eq!(binop_at(src, false), (Ok(Some(op)), col), "{src}");
        }
    }

    #[test]
    fn binop_terminators_not_consumed() {
        for src in ["= 1", "=> 1", "+= 1", "-= 1", "*= 1", "/= 1"] {
            assert_eq!(binop_at(src, false), (Ok(None), 1), "{src}");
        }
    }

    #[test]
    fn binop_non_operators() {
        for src in ["x", ") ", "? x", "! x", "& x", ". x", ": x", "// c", ""] {
            assert_eq!(binop_at(src, false), (Ok(None), 1), "{src}");
        }
    }

    #[test]
    fn binop_bad_each() {
        let cases = [
            ("-> x", BadOperator::Arrow),
            ("| x", BadOperator::Bar),
            ("++ x", BadOperator::PlusPlus),
            (":: x", BadOperator::DoubleColon),
            (".. x", BadOperator::DotDot),
            ("<| x", BadOperator::PipeLeft),
            (">> x", BadOperator::ComposeRight),
            ("<< x", BadOperator::ComposeLeft),
            ("^ x", BadOperator::Caret),
        ];
        for (src, bad) in cases {
            assert_eq!(binop_at(src, false), (Err(format!("{bad:?}")), 1), "{src}");
        }
    }

    #[test]
    fn binop_in_only_in_query() {
        assert_eq!(binop_at("in xs", true), (Ok(Some(BinOp::In)), 3));
        assert_eq!(binop_at("in xs", false), (Ok(None), 1));
        // `index` is an identifier, not `in`.
        assert_eq!(binop_at("index", true), (Ok(None), 1));
    }

    #[test]
    fn binop_region() {
        let bump = Bump::new();
        let text = bump.alloc_str("|> f");
        let mut parser = Parser::new(&bump, text.as_bytes());
        let op = parser.binop(|bad, _, _| bad).unwrap().unwrap();
        assert_eq!((op.region.start.line, op.region.start.column), (1, 1));
        assert_eq!((op.region.end.line, op.region.end.column), (1, 3));
    }

    #[test]
    fn assign_op_each() {
        assert_eq!(assign_at("= 1"), (Some(AssignOp::Set), 2));
        assert_eq!(assign_at("+= 1"), (Some(AssignOp::Add), 3));
        assert_eq!(assign_at("-= 1"), (Some(AssignOp::Sub), 3));
        assert_eq!(assign_at("*= 1"), (Some(AssignOp::Mul), 3));
        assert_eq!(assign_at("/= 1"), (Some(AssignOp::Div), 3));
    }

    #[test]
    fn assign_op_not_double_equals() {
        assert_eq!(assign_at("== 1"), (None, 1));
        assert_eq!(assign_at("=> 1"), (None, 1));
        assert_eq!(assign_at("+ 1"), (None, 1));
        assert_eq!(assign_at(""), (None, 1));
    }

    #[test]
    fn operator_chars() {
        for b in b"+-*/%=!<>&|?^:." {
            assert!(is_operator_char(*b));
        }
        assert!(!is_operator_char(b'a'));
        assert!(!is_operator_char(b' '));
        assert!(!is_operator_char(b'('));
    }
}
