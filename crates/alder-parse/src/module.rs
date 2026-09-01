//! Module parsing for Alder.
//!
//! Ported from Elm's `Parse/Module.hs`.
//!
//! Parses full modules including:
//! - Module header: `module Main exposing (main)`
//! - Imports: `import List exposing (map)`
//! - Declarations: values, types, aliases

use alder_region::{Located, Region};
use alder_source::{Alias, Docs, Exposing, Import, Infix, Module, Union, Value};

use crate::Parser;
use crate::declaration::Decl;
use crate::error;

impl<'a> Parser<'a> {
    /// Parse a module header.
    ///
    /// Mirrors Elm's `chompHeader` (simplified - no port/effect modules):
    /// ```text
    /// module_header = 'module' module_name 'exposing' exposing_list
    /// ```
    ///
    /// Returns the module name and exports as a tuple.
    pub fn module_header(
        &mut self,
    ) -> Result<(&'a Located<&'a str>, &'a Located<Exposing<'a>>), error::Module<'a>> {
        // Match 'module' keyword
        self.keyword_module(error::Module::Problem)?;

        self.chomp_and_check_indent(error::Module::Space, error::Module::Name)?;

        // Parse module name (e.g., "Json.Decode")
        let start = self.get_position();
        let name = self.module_name(error::Module::Name)?;
        let module_name = self.add_end(start, name);

        // Consume whitespace
        self.chomp(error::Module::Space)?;

        // Must have 'exposing' keyword
        self.keyword_exposing(|row, col| {
            error::Module::Exposing(self.bump.alloc(error::Exposing::Start(row, col)), row, col)
        })?;

        self.chomp_and_check_indent(error::Module::Space, |row, col| {
            error::Module::Exposing(
                self.bump.alloc(error::Exposing::IndentValue(row, col)),
                row,
                col,
            )
        })?;

        // Parse the exposing list, wrapping errors
        let exposing_start = self.get_position();
        let exposing = self.specialize(
            |bump, err, row, col| error::Module::Exposing(bump.alloc(err), row, col),
            |p| p.exposing(),
        )?;
        let exposing_located = self.add_end(exposing_start, exposing);

        Ok((module_name, exposing_located))
    }

    /// Parse zero or more import statements.
    ///
    /// Mirrors Elm's `chompImports`:
    /// ```haskell
    /// chompImports :: [Src.Import] -> Parser E.Module [Src.Import]
    /// chompImports imports =
    ///   oneOfWithFallback
    ///     [ do  i <- chompImport
    ///           chompImports (i:imports)
    ///     ]
    ///     (reverse imports)
    /// ```
    fn imports(&mut self) -> Result<&'a [&'a Import<'a>], error::Module<'a>> {
        let mut imports = Vec::new();

        loop {
            // Save state in case import keyword doesn't match
            let state = self.save_state();

            match self.import() {
                Ok(import) => {
                    imports.push(import);
                    // import() already ensures fresh line at the end
                }
                Err(_) => {
                    // If we didn't consume input, we're done with imports
                    if self.pos == state.pos {
                        self.restore_state(state);
                        break;
                    }
                    // Otherwise propagate the error
                    return Err(error::Module::ImportStart(self.row, self.col));
                }
            }
        }

        Ok(self.alloc_slice_copy(&imports))
    }

    /// Parse zero or more declarations.
    ///
    /// Mirrors Elm's `chompDecls`:
    /// ```haskell
    /// chompDecls :: [Decl.Decl] -> Parser E.Decl [Decl.Decl]
    /// chompDecls decls =
    ///   do  (decl, _) <- Decl.declaration
    ///       oneOfWithFallback
    ///         [ do  Space.checkFreshLine E.DeclStart
    ///               chompDecls (decl:decls)
    ///         ]
    ///         (reverse (decl:decls))
    /// ```
    fn declarations(&mut self) -> Result<Vec<Decl<'a>>, error::Module<'a>> {
        let mut decls = Vec::new();

        loop {
            // Save state in case no declaration starts
            let state = self.save_state();

            // Try to parse a declaration
            match self.specialize(
                |bump, err, row, col| error::Module::Declarations(bump.alloc(err), row, col),
                |p| p.declaration(),
            ) {
                Ok((decl, _end)) => {
                    decls.push(decl);

                    // Chomp any trailing whitespace
                    self.chomp(error::Module::Space)?;

                    // Check for fresh line (another declaration might follow)
                    if self.is_eof() {
                        break;
                    }

                    // If not at fresh line, we're done
                    if self.col != 1 {
                        break;
                    }
                }
                Err(_) => {
                    // If we didn't consume input, we're done with declarations
                    if self.pos == state.pos {
                        self.restore_state(state);
                        break;
                    }
                    // Otherwise propagate the error
                    return Err(error::Module::Declarations(
                        self.bump.alloc(error::Decl::Start(self.row, self.col)),
                        self.row,
                        self.col,
                    ));
                }
            }
        }

        Ok(decls)
    }

    /// Parse zero or more infix declarations.
    ///
    /// Infixes are parsed at module level (before regular declarations).
    fn infixes(&mut self) -> Result<Vec<&'a Located<Infix<'a>>>, error::Module<'a>> {
        let mut infixes = Vec::new();

        loop {
            let state = self.save_state();

            match self.infix_decl() {
                Ok(infix) => {
                    infixes.push(infix);
                    // infix_decl already ensures fresh line
                }
                Err(_) => {
                    // If we didn't consume input, we're done
                    if self.pos == state.pos {
                        self.restore_state(state);
                        break;
                    }
                    return Err(error::Module::Infix(self.row, self.col));
                }
            }
        }

        Ok(infixes)
    }

    /// Parse a complete module.
    ///
    /// Mirrors Elm's `chompModule`:
    /// ```text
    /// module = [ module_header ] { import } { infix } { declaration }
    /// ```
    pub fn module(&mut self) -> Result<Module<'a>, error::Module<'a>> {
        // Consume initial whitespace
        self.chomp(error::Module::Space)?;

        let start_pos = self.get_position();

        // Try to parse module header (optional)
        let (name, exports) = {
            let state = self.save_state();
            match self.module_header() {
                Ok((n, e)) => {
                    // Consume whitespace after header and check fresh line
                    self.chomp(error::Module::Space)?;
                    self.check_fresh_line(error::Module::FreshLine)?;
                    (Some(n), e)
                }
                Err(_) => {
                    // No header - restore and use defaults
                    if self.pos == state.pos {
                        self.restore_state(state);
                    }
                    // Default: name = None, exports = Open
                    let default_exports = self.alloc(Located::at(Region::one(), Exposing::Open));
                    (None, default_exports)
                }
            }
        };

        // Parse imports
        let imports = self.imports()?;

        // Parse infixes
        let infix_vec = self.infixes()?;
        let binops = self.alloc_slice_copy(&infix_vec);

        // Parse declarations
        let decls = self.declarations()?;

        // Categorize declarations into values, unions, aliases
        let (values, unions, aliases) = self.categorize_decls(decls);

        // Build docs (simplified: no module-level docs for now)
        let docs = self.alloc(Docs::NoDocs(Region::new(start_pos, self.get_position())));

        Ok(Module {
            name,
            exports,
            docs,
            imports,
            values,
            unions,
            aliases,
            binops,
        })
    }

    /// Categorize declarations into separate slices by type.
    #[allow(clippy::type_complexity)]
    fn categorize_decls(
        &self,
        decls: Vec<Decl<'a>>,
    ) -> (
        &'a [&'a Located<Value<'a>>],
        &'a [&'a Located<Union<'a>>],
        &'a [&'a Located<Alias<'a>>],
    ) {
        let mut values = Vec::new();
        let mut unions = Vec::new();
        let mut aliases = Vec::new();

        for decl in decls {
            match decl {
                Decl::Value(_doc, value) => values.push(value),
                Decl::Union(_doc, union) => unions.push(union),
                Decl::Alias(_doc, alias) => aliases.push(alias),
            }
        }

        (
            self.alloc_slice_copy(&values),
            self.alloc_slice_copy(&unions),
            self.alloc_slice_copy(&aliases),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;
    use indoc::indoc;

    macro_rules! assert_module_header_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let src = bump.alloc_str(input);
            let mut parser = Parser::new(&bump, src.as_bytes());
            let result = parser.module_header();
            match result {
                Ok((name, exposing)) => {
                    insta::with_settings!({
                        description => format!("Code:\n\n{}", input),
                        omit_expression => true,
                    }, {
                        insta::assert_debug_snapshot!((name, exposing));
                    });
                }
                Err(e) => {
                    panic!("Expected successful parse, got error: {:?}", e);
                }
            }
        }};
    }

    #[test]
    fn module_header_simple_open() {
        assert_module_header_snapshot!("module Foo exposing (..)");
    }

    #[test]
    fn module_header_dotted() {
        assert_module_header_snapshot!("module Foo.Bar exposing (baz)");
    }

    #[test]
    fn module_header_mixed_exposing() {
        assert_module_header_snapshot!("module Main exposing (main, Msg(..))");
    }

    #[test]
    fn module_header_deeply_nested() {
        assert_module_header_snapshot!("module Platform.Cmd.Extra exposing (batch, none)");
    }

    // =========================================================================
    // Full module parsing tests
    // =========================================================================

    macro_rules! assert_module_snapshot {
        ($input:expr) => {{
            let input = indoc!($input);
            let bump = Bump::new();
            let src = bump.alloc_str(input);
            let mut parser = Parser::new(&bump, src.as_bytes());
            let result = parser.module();
            match result {
                Ok(module) => {
                    insta::with_settings!({
                        description => format!("Code:\n\n{}", input),
                        omit_expression => true,
                    }, {
                        insta::assert_debug_snapshot!(module);
                    });
                }
                Err(e) => {
                    panic!("Expected successful parse, got error: {:?}", e);
                }
            }
        }};
    }

    #[test]
    fn module_header_only() {
        assert_module_snapshot!("module Main exposing (..)\n");
    }

    #[test]
    fn module_with_imports() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            import List
            import Maybe exposing (Maybe(..))
        "#
        );
    }

    #[test]
    fn module_with_value() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            main = 42
        "#
        );
    }

    #[test]
    fn module_with_type() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            type Maybe a
                = Just a
                | Nothing
        "#
        );
    }

    #[test]
    fn module_with_union_then_value() {
        // A definition at column 1 must end the union's variant list, not
        // be swallowed as extra constructor arguments.
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            type Wrap a
                = Wrap a

            f w = w
        "#
        );
    }

    #[test]
    fn module_with_alias() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            type alias Point = { x : Int, y : Int }
        "#
        );
    }

    #[test]
    fn module_full() {
        assert_module_snapshot!(
            r#"
            module Main exposing (main, Model, Msg(..))

            import Html exposing (div)
            import Platform.Cmd as Cmd

            type alias Model = { count : Int }

            type Msg
                = Increment
                | Decrement

            main = 0
        "#
        );
    }

    #[test]
    fn module_no_header() {
        // Without header, defaults to name=None, exports=Open
        assert_module_snapshot!("x = 1\n");
    }

    #[test]
    fn module_with_infix() {
        assert_module_snapshot!(
            r#"
            module Main exposing (..)

            infix left 6 (|>) = apR

            apR f x = f x
        "#
        );
    }
}
