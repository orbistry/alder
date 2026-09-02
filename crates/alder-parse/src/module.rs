//! Module parsing: a flat, line-break separated item list.
//!
//! See docs/parser-internals.md §5.10.
// OWNER: module.rs (Wave 4)

use alder_source::Module;

use crate::{Parser, error};

#[allow(unused)]
impl<'a> Parser<'a> {
    /// chomp; items until EOF; a non-item → Module::BadEnd. After each item the
    /// next one must start on a later line (`newline_since(item.region.end)`),
    /// otherwise Module::SameLine (§2.1 rule 3).
    pub fn module(&mut self) -> Result<Module<'a>, error::Module<'a>> {
        todo!()
    }
}

/// Snapshot test macro for successful module parsing.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_module_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .module()
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

/// Snapshot test macro for module parse errors.
#[cfg(test)]
#[allow(unused)]
macro_rules! assert_module_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .module()
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

#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_module_error_snapshot;
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_module_snapshot;

#[cfg(test)]
mod tests {}
