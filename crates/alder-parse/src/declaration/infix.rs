//! Infix declaration parsing for Alder.
//!
//! Ported from Elm's `Parse/Declaration.hs` (infix_).
//!
//! Parses declarations like: `infix left 6 (|>) = apR`

use alder_region::Located;
use alder_source::{Associativity, Infix, Precedence};

use crate::Parser;
use crate::error::Module as ModuleErr;

impl<'a> Parser<'a> {
    /// Parse an infix declaration.
    ///
    /// Parses: `infix left 6 (|>) = apR`
    ///
    /// Mirrors Elm's `infix_`:
    /// ```haskell
    /// infix_ :: Parser E.Module (A.Located Src.Infix)
    /// infix_ =
    ///   do  start <- getPosition
    ///       Keyword.infix_ err
    ///       Space.chompAndCheckIndent _err err
    ///       associativity <- oneOf err [ left, right, non ]
    ///       Space.chompAndCheckIndent _err err
    ///       precedence <- Number.precedence err
    ///       Space.chompAndCheckIndent _err err
    ///       word1 0x28 {-(-} err
    ///       op <- Symbol.operator err _err
    ///       word1 0x29 {-)-} err
    ///       Space.chompAndCheckIndent _err err
    ///       word1 0x3D {-=-} err
    ///       Space.chompAndCheckIndent _err err
    ///       name <- Var.lower err
    ///       end <- getPosition
    ///       Space.chomp _err
    ///       Space.checkFreshLine err
    ///       return (A.at start end (Src.Infix op associativity precedence name))
    /// ```
    pub fn infix_decl(&mut self) -> Result<&'a Located<Infix<'a>>, ModuleErr<'a>> {
        let start = self.get_position();
        let err = ModuleErr::Infix;

        self.keyword_infix(err)?;
        self.chomp_and_check_indent(|_, r, c| err(r, c), err)?;

        // Parse associativity
        let associativity = self.one_of(
            err,
            vec![
                Box::new(|p: &mut Parser<'a>| {
                    p.keyword_left(err)?;
                    Ok(Associativity::Left)
                }),
                Box::new(|p| {
                    p.keyword_right(err)?;
                    Ok(Associativity::Right)
                }),
                Box::new(|p| {
                    p.keyword_non(err)?;
                    Ok(Associativity::None)
                }),
            ],
        )?;

        self.chomp_and_check_indent(|_, r, c| err(r, c), err)?;

        // Parse precedence (single digit 0-9)
        let precedence = self.precedence(err)?;

        self.chomp_and_check_indent(|_, r, c| err(r, c), err)?;

        // Parse operator in parentheses
        self.word1(b'(', err)?;
        let op = self.operator(err, |_, r, c| err(r, c))?;
        self.word1(b')', err)?;

        self.chomp_and_check_indent(|_, r, c| err(r, c), err)?;

        // Parse equals
        self.word1(b'=', err)?;

        self.chomp_and_check_indent(|_, r, c| err(r, c), err)?;

        // Parse function name
        let name = self.lower_name(err)?;

        // Consume trailing whitespace and check for fresh line
        self.chomp(|_, r, c| err(r, c))?;
        self.check_fresh_line(err)?;

        let infix = Infix {
            op,
            associativity,
            precedence,
            name,
        };

        Ok(self.add_end(start, infix))
    }

    /// Parse a precedence digit (0-9).
    ///
    /// Mirrors Elm's `Number.precedence`.
    fn precedence<E>(&mut self, to_error: impl FnOnce(u16, u16) -> E) -> Result<Precedence, E> {
        match self.peek() {
            Some(b) if b.is_ascii_digit() => {
                let value = (b - b'0') as u16;
                self.advance();
                Ok(Precedence(value))
            }
            _ => {
                let (row, col) = self.position();
                Err(to_error(row, col))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Parser;

    /// Macro for testing successful infix declaration parsing.
    /// Note: infix declarations require a trailing newline.
    macro_rules! assert_infix_snapshot {
        ($src:expr) => {{
            let bump = bumpalo::Bump::new();
            let src = concat!($src, "\n");
            let src_in_arena = bump.alloc_str(src);
            let mut parser = Parser::new(&bump, src_in_arena.as_bytes());
            match parser.infix_decl() {
                Ok(infix) => {
                    insta::with_settings!({
                        description => $src,
                        omit_expression => true,
                    }, {
                        insta::assert_debug_snapshot!(infix);
                    });
                }
                Err(e) => panic!("Expected Ok, got Err: {:?}\n\nSource:\n{}", e, src),
            }
        }};
    }

    #[test]
    fn infix_left() {
        assert_infix_snapshot!("infix left 6 (|>) = apR");
    }

    #[test]
    fn infix_right() {
        assert_infix_snapshot!("infix right 5 (::) = cons");
    }

    #[test]
    fn infix_non() {
        assert_infix_snapshot!("infix non 4 (==) = eq");
    }
}
