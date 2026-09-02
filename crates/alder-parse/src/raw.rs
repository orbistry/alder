//! Raw balanced token text for macro bodies and `name!( … )` calls
//! (docs/parser-internals.md §6.5).
//!
//! See docs/parser-internals.md §5.9.
// OWNER: raw.rs (Wave 1)

use alder_region::Located;

use crate::error::RawTokens;
use crate::{Col, Parser, Row};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// At `open`. Consumes through the matching `close`, honoring nested
    /// `()[]{}`, strings, templates and `//` comments. Returns the interior text.
    pub(crate) fn raw_balanced<E>(
        &mut self,
        open: u8,
        close: u8,
        to_error: impl FnOnce(RawTokens, Row, Col) -> E,
    ) -> Result<Located<&'a str>, E> {
        todo!()
    }
}

#[cfg(test)]
mod tests {}
