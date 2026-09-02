//! Whitespace and comment handling for Alder.
//!
//! Whitespace is ` `, `\t`, `\r`, `\n`; the only comment form is `//` to end
//! of line (`///` and `//!` doc comments are skipped like any other comment in
//! M1). Nothing here can fail, so `chomp` is infallible.
//! See docs/parser-internals.md §2 and §5.2.

use crate::Parser;

impl<'a> Parser<'a> {
    /// Spaces, tabs, CR/LF and `//…` comments (including `///`, `//!`). Infallible.
    pub fn chomp(&mut self) {
        self.eat_spaces();
    }

    /// Core loop: eat whitespace bytes and line comments until something else.
    fn eat_spaces(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => self.advance(),
                Some(b'/') if self.peek_at(1) == Some(b'/') => self.eat_line_comment(),
                _ => return,
            }
        }
    }

    /// Eat a line comment (from `//` to end of line, consuming the newline).
    fn eat_line_comment(&mut self) {
        // Skip the `//`
        self.advance();
        self.advance();

        loop {
            match self.peek() {
                Some(b'\n') => {
                    self.advance();
                    return;
                }
                Some(_) => self.advance(),
                None => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;

    fn chomped(input: &str) -> (usize, u16, u16) {
        let bump = Bump::new();
        let src = bump.alloc_str(input);
        let mut parser = Parser::new(&bump, src.as_bytes());
        parser.chomp();
        let (row, col) = parser.position();
        (parser.pos, row, col)
    }

    #[test]
    fn empty() {
        assert_eq!(chomped(""), (0, 1, 1));
    }

    #[test]
    fn spaces_only() {
        assert_eq!(chomped("   "), (3, 1, 4));
    }

    #[test]
    fn tabs_allowed() {
        assert_eq!(chomped(" \t\tx"), (3, 1, 4));
    }

    #[test]
    fn newlines() {
        assert_eq!(chomped("  \n  \r\n  "), (9, 3, 3));
    }

    #[test]
    fn line_comment() {
        // After the newline that ends the comment.
        assert_eq!(chomped("// comment\nfoo"), (11, 2, 1));
    }

    #[test]
    fn doc_line_comment_skipped() {
        assert_eq!(chomped("/// doc\n//! inner\nfoo"), (18, 3, 1));
    }

    #[test]
    fn comment_at_eof() {
        assert_eq!(chomped("  // trailing"), (13, 1, 14));
    }

    #[test]
    fn stops_at_content() {
        assert_eq!(chomped("  foo"), (2, 1, 3));
        // A lone `/` is content, not a comment.
        assert_eq!(chomped(" / 2"), (1, 1, 2));
    }
}
