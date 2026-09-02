//! Number literal scanning for Alder (JS semantics: decimal, `0x` hex,
//! fraction, exponent, `n` BigInt suffix).
//!
//! See docs/parser-internals.md §2 and §5.5.
//!
//! Hand-off: the pre-rewrite scanner is largely reusable — recover it with
//! `git show 95c298e:crates/alder-parse/src/number.rs`.
// OWNER: number.rs (Wave 1)

use alder_region::Located;
use alder_source::NumberLit;

use crate::error;
use crate::{Col, Parser, Row};

#[allow(unused)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum NumberLiteral<'a> {
    Number(NumberLit<'a>),
    BigInt(&'a str),
}

#[allow(unused)]
impl<'a> Parser<'a> {
    /// Digit-led literal with Elm's dirty-end check (`123abc` → Number::End).
    pub(crate) fn number_literal<E>(
        &mut self,
        to_expectation: impl FnOnce(Row, Col) -> E,
        to_error: impl FnOnce(error::Number, Row, Col) -> E,
    ) -> Result<NumberLiteral<'a>, E> {
        todo!()
    }

    /// Committed numeric prefix without the dirty-end check (style dimensions read the unit after it).
    pub(crate) fn chomp_number(&mut self) -> Result<NumberLiteral<'a>, error::Number> {
        todo!()
    }

    /// Bare digit run for tuple indices (`t.0`). None without consuming if no digit.
    pub(crate) fn digits(&mut self) -> Option<Located<u32>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
