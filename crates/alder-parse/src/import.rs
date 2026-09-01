//! Import statement parsing for Alder.
//!
//! Ported from Elm's `Parse/Module.hs` (chompImport, chompAs, chompExposing).
//!
//! Parses import statements like:
//! - `import List`
//! - `import Json.Decode as Decode`
//! - `import Html exposing (div, span)`
//! - `import Platform.Cmd as Cmd exposing (Cmd)`

use alder_region::Located;
use alder_source::{Exposing, Import};

use crate::Parser;
use crate::error;

impl<'a> Parser<'a> {
    /// Parse an import statement.
    ///
    /// Mirrors Elm's `chompImport`:
    /// ```text
    /// import = 'import' module_name [ 'as' alias ] [ 'exposing' exposing_list ]
    /// ```
    pub fn import(&mut self) -> Result<&'a Import<'a>, error::Module<'a>> {
        // Match 'import' keyword
        self.keyword_import(error::Module::ImportStart)?;

        self.chomp_and_check_indent(error::Module::Space, error::Module::ImportIndentName)?;

        // Parse module name (e.g., "Json.Decode")
        let start = self.get_position();
        let name = self.module_name(error::Module::ImportName)?;
        let module_name = self.add_end(start, name);

        // Chomp whitespace (not indent-checked, could be end of line)
        self.chomp(error::Module::Space)?;

        // Check what comes next: fresh line, or continuation (as/exposing)
        self.import_help(module_name)
    }

    /// Parse the continuation of an import after the module name.
    ///
    /// Handles: fresh line (done), `as Alias`, or `exposing (...)`.
    fn import_help(
        &mut self,
        module_name: &'a Located<&'a str>,
    ) -> Result<&'a Import<'a>, error::Module<'a>> {
        // Try fresh line first (import done, no alias or exposing)
        if self.col == 1 {
            let default_exposing = self.alloc(Exposing::Explicit(&[]));
            return Ok(self.alloc(Import {
                import: module_name,
                alias: None,
                exposing: default_exposing,
            }));
        }

        // Check that we're indented past the start
        self.one_of(
            error::Module::ImportAs,
            vec![
                // `as Alias [exposing (...)]`
                Box::new(|p: &mut Parser<'a>| p.import_as(module_name)),
                // `exposing (...)`
                Box::new(|p: &mut Parser<'a>| p.import_exposing(module_name, None)),
            ],
        )
    }

    /// Parse `as Alias` part of an import.
    ///
    /// Mirrors Elm's `chompAs`.
    fn import_as(
        &mut self,
        module_name: &'a Located<&'a str>,
    ) -> Result<&'a Import<'a>, error::Module<'a>> {
        self.keyword_as(error::Module::ImportAs)?;

        self.chomp_and_check_indent(error::Module::Space, error::Module::ImportIndentAlias)?;

        let alias = self.upper_name(error::Module::ImportAlias)?;

        // Chomp whitespace
        self.chomp(error::Module::Space)?;

        // Check for exposing or end
        if self.col == 1 {
            // Fresh line - done
            let default_exposing = self.alloc(Exposing::Explicit(&[]));
            Ok(self.alloc(Import {
                import: module_name,
                alias: Some(alias),
                exposing: default_exposing,
            }))
        } else {
            // Must be exposing
            self.import_exposing(module_name, Some(alias))
        }
    }

    /// Parse `exposing (...)` part of an import.
    ///
    /// Mirrors Elm's `chompExposing`.
    fn import_exposing(
        &mut self,
        module_name: &'a Located<&'a str>,
        alias: Option<&'a str>,
    ) -> Result<&'a Import<'a>, error::Module<'a>> {
        self.keyword_exposing(error::Module::ImportExposing)?;

        self.chomp_and_check_indent(
            error::Module::Space,
            error::Module::ImportIndentExposingList,
        )?;

        // Parse the exposing list, wrapping errors
        let exposing = self.specialize(
            |bump, err, row, col| error::Module::ImportExposingList(bump.alloc(err), row, col),
            |p| p.exposing(),
        )?;

        // Check for fresh line (end of import)
        self.chomp(error::Module::Space)?;
        self.check_fresh_line(error::Module::ImportEnd)?;

        Ok(self.alloc(Import {
            import: module_name,
            alias,
            exposing: self.alloc(exposing),
        }))
    }

    /// Parse a module name like "Json.Decode".
    ///
    /// Mirrors Elm's `Var.moduleName`:
    /// ```text
    /// module_name = upper_var { '.' upper_var }
    /// ```
    pub(crate) fn module_name<E>(
        &mut self,
        to_error: impl FnOnce(u16, u16) -> E,
    ) -> Result<&'a str, E> {
        let (row, col) = self.position();
        let start_pos = self.pos;

        // Must start with uppercase
        match self.peek() {
            Some(b) if b.is_ascii_uppercase() => {
                self.advance();
                self.chomp_inner_chars();
            }
            _ => return Err(to_error(row, col)),
        }

        // Continue with .Upper segments
        loop {
            if self.peek() == Some(b'.') {
                // Check if followed by uppercase
                if let Some(next) = self.peek_at(1)
                    && next.is_ascii_uppercase()
                {
                    self.advance(); // consume '.'
                    self.advance(); // consume first upper char
                    self.chomp_inner_chars();
                    continue;
                }
            }
            // Not a dot or not followed by uppercase - done
            break;
        }

        Ok(self.slice_from(start_pos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;
    use indoc::indoc;

    macro_rules! assert_import_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let src = bump.alloc_str(input);
            let mut parser = Parser::new(&bump, src.as_bytes());
            let result = parser.import();
            match result {
                Ok(ref import) => {
                    insta::with_settings!({
                        description => format!("Code:\n\n{}", input),
                        omit_expression => true,
                    }, {
                        insta::assert_debug_snapshot!(import);
                    });
                }
                Err(e) => {
                    panic!("Expected successful parse, got error: {:?}", e);
                }
            }
        }};
    }

    #[test]
    fn import_simple() {
        assert_import_snapshot!("import Foo\n");
    }

    #[test]
    fn import_dotted() {
        assert_import_snapshot!("import Json.Decode\n");
    }

    #[test]
    fn import_deeply_nested() {
        assert_import_snapshot!("import Platform.Cmd.Extra\n");
    }

    #[test]
    fn import_with_alias() {
        assert_import_snapshot!("import Json.Decode as Decode\n");
    }

    #[test]
    fn import_exposing_open() {
        assert_import_snapshot!("import List exposing (..)\n");
    }

    #[test]
    fn import_exposing_explicit() {
        assert_import_snapshot!("import Html exposing (div, span)\n");
    }

    #[test]
    fn import_full() {
        assert_import_snapshot!("import Platform.Cmd as Cmd exposing (Cmd)\n");
    }

    #[test]
    fn import_exposing_types() {
        assert_import_snapshot!("import Maybe exposing (Maybe(..))\n");
    }
}
