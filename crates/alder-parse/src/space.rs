//! Whitespace and comment handling for Alder.
//!
//! Ported from Elm's `Parse/Space.hs`.
//!
//! Handles:
//! - Spaces and newlines
//! - Line comments (`--`)
//! - Multi-line comments (`{- ... -}`) with nesting
//! - Doc comments (`{-| ... -}`)
//! - Indentation checking
//! - Tab detection (tabs are not allowed)

use crate::error::Space;
use crate::{Col, Parser, Row};
use alder_source::{Comment, Snippet};

/// Result of eating spaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceStatus {
    /// Successfully consumed whitespace.
    Good,
    /// Encountered a tab character (not allowed).
    HasTab,
    /// Encountered an unclosed multi-line comment.
    EndlessMultiComment,
}

impl<'a> Parser<'a> {
    /// Consume whitespace and comments.
    ///
    /// Returns an error if tabs or unclosed comments are found.
    /// Mirrors Elm's `Space.chomp`.
    pub fn chomp<E>(&mut self, to_error: impl FnOnce(Space, Row, Col) -> E) -> Result<(), E> {
        let (status, new_row, new_col) = self.eat_spaces();

        match status {
            SpaceStatus::Good => Ok(()),
            SpaceStatus::HasTab => Err(to_error(Space::HasTab, new_row, new_col)),
            SpaceStatus::EndlessMultiComment => {
                Err(to_error(Space::EndlessMultiComment, new_row, new_col))
            }
        }
    }

    /// Consume whitespace and check that we're indented past the current indent level.
    ///
    /// Mirrors Elm's `Space.chompAndCheckIndent`.
    pub fn chomp_and_check_indent<E>(
        &mut self,
        to_space_error: impl FnOnce(Space, Row, Col) -> E,
        to_indent_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<(), E> {
        let (row_before, col_before) = self.position();
        let (status, new_row, new_col) = self.eat_spaces();

        match status {
            SpaceStatus::Good => {
                if new_col > self.indent && new_col > 1 {
                    Ok(())
                } else {
                    Err(to_indent_error(row_before, col_before))
                }
            }
            SpaceStatus::HasTab => Err(to_space_error(Space::HasTab, new_row, new_col)),
            SpaceStatus::EndlessMultiComment => {
                Err(to_space_error(Space::EndlessMultiComment, new_row, new_col))
            }
        }
    }

    /// Check that the parser's CURRENT column is past the indent level and
    /// not at column 1 (which starts a fresh top-level declaration).
    ///
    /// Mirrors Elm's `Space.checkIndent`: the passed position is only the
    /// location to report on failure; the check itself is `col > indent
    /// && col > 1` on the current state.
    pub fn check_indent<E>(
        &self,
        end_row: Row,
        end_col: Col,
        to_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<(), E> {
        if self.col > self.indent && self.col > 1 {
            Ok(())
        } else {
            Err(to_error(end_row, end_col))
        }
    }

    /// Check that current column equals indent level (for alignment).
    ///
    /// Mirrors Elm's `Space.checkAligned`.
    pub fn check_aligned<E>(&self, to_error: impl FnOnce(u16, Row, Col) -> E) -> Result<(), E> {
        if self.col == self.indent {
            Ok(())
        } else {
            Err(to_error(self.indent, self.row, self.col))
        }
    }

    /// Check that we're at column 1 (start of a fresh line).
    ///
    /// Mirrors Elm's `Space.checkFreshLine`.
    pub fn check_fresh_line<E>(&self, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E> {
        if self.col == 1 {
            Ok(())
        } else {
            Err(to_error(self.row, self.col))
        }
    }

    /// Parse a doc comment `{-| ... -}`.
    ///
    /// Returns the Comment containing a Snippet of the content.
    /// Mirrors Elm's `Space.docComment`.
    pub fn doc_comment<E>(
        &mut self,
        to_expectation: impl FnOnce(Row, Col) -> E,
        to_space_error: impl FnOnce(Space, Row, Col) -> E,
    ) -> Result<&'a Comment<'a>, E> {
        // Check for {-| at current position
        if self.peek() == Some(0x7B)
            && self.peek_at(1) == Some(0x2D)
            && self.peek_at(2) == Some(0x7C)
        {
            let start_row = self.row;
            let start_col = self.col + 3; // Column after {-|

            // Skip {-|
            self.advance();
            self.advance();
            self.advance();

            let content_start = self.pos;

            // Use the existing multi-comment helper with nesting=1
            let status = self.eat_multi_comment_help(1);

            match status {
                SpaceStatus::Good => {
                    // Content ends 2 bytes before current position (before -})
                    let content_end = self.pos - 2;
                    let content = &self.src[content_start..content_end];

                    let snippet = self.alloc(Snippet {
                        data: content,
                        off_row: start_row,
                        off_col: start_col,
                    });
                    let comment = self.alloc(Comment(snippet));

                    Ok(comment)
                }
                SpaceStatus::HasTab => Err(to_space_error(Space::HasTab, self.row, self.col)),
                SpaceStatus::EndlessMultiComment => Err(to_space_error(
                    Space::EndlessMultiComment,
                    self.row,
                    self.col,
                )),
            }
        } else {
            Err(to_expectation(self.row, self.col))
        }
    }

    /// Core function to eat spaces, newlines, and comments.
    ///
    /// Updates parser position and returns status.
    fn eat_spaces(&mut self) -> (SpaceStatus, Row, Col) {
        loop {
            match self.peek() {
                // Space
                Some(0x20) => {
                    self.advance();
                }

                // Newline
                Some(0x0A) => {
                    self.advance();
                }

                // Carriage return (skip)
                Some(0x0D) => {
                    self.advance();
                }

                // Potential comment start: { or -
                Some(0x7B) => {
                    // Check for {-
                    if self.peek_at(1) == Some(0x2D) {
                        // Check for {-| (doc comment marker - don't consume, let caller handle)
                        if self.peek_at(2) == Some(0x7C) {
                            return (SpaceStatus::Good, self.row, self.col);
                        }
                        // Multi-line comment
                        match self.eat_multi_comment() {
                            SpaceStatus::Good => {
                                // Continue eating spaces
                            }
                            status => return (status, self.row, self.col),
                        }
                    } else {
                        return (SpaceStatus::Good, self.row, self.col);
                    }
                }

                Some(0x2D) => {
                    // Check for --
                    if self.peek_at(1) == Some(0x2D) {
                        self.eat_line_comment();
                        // Continue eating spaces
                    } else {
                        return (SpaceStatus::Good, self.row, self.col);
                    }
                }

                // Tab (not allowed)
                Some(0x09) => {
                    return (SpaceStatus::HasTab, self.row, self.col);
                }

                // Anything else (including EOF)
                _ => {
                    return (SpaceStatus::Good, self.row, self.col);
                }
            }
        }
    }

    /// Eat a line comment (from -- to end of line).
    fn eat_line_comment(&mut self) {
        // Skip the --
        self.advance();
        self.advance();

        loop {
            match self.peek() {
                Some(0x0A) => {
                    // Newline ends the comment
                    self.advance();
                    return;
                }
                Some(_) => {
                    self.advance();
                }
                None => {
                    // EOF ends the comment
                    return;
                }
            }
        }
    }

    /// Eat a multi-line comment ({- ... -}).
    ///
    /// Supports nested comments.
    fn eat_multi_comment(&mut self) -> SpaceStatus {
        // Skip the {-
        self.advance();
        self.advance();

        self.eat_multi_comment_help(1)
    }

    /// Helper for eating multi-line comments with nesting.
    fn eat_multi_comment_help(&mut self, open_comments: u16) -> SpaceStatus {
        loop {
            match self.peek() {
                // Newline
                Some(0x0A) => {
                    self.advance();
                }

                // Tab (not allowed even in comments)
                Some(0x09) => {
                    return SpaceStatus::HasTab;
                }

                // Potential close: -}
                Some(0x2D) => {
                    if self.peek_at(1) == Some(0x7D) {
                        self.advance();
                        self.advance();
                        if open_comments == 1 {
                            return SpaceStatus::Good;
                        } else {
                            return self.eat_multi_comment_help(open_comments - 1);
                        }
                    } else {
                        self.advance();
                    }
                }

                // Potential nested open: {-
                Some(0x7B) => {
                    if self.peek_at(1) == Some(0x2D) {
                        self.advance();
                        self.advance();
                        return self.eat_multi_comment_help(open_comments + 1);
                    } else {
                        self.advance();
                    }
                }

                // Any other character
                Some(_) => {
                    self.advance();
                }

                // EOF without closing
                None => {
                    return SpaceStatus::EndlessMultiComment;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;

    fn parse_and_chomp(input: &str) -> (SpaceStatus, usize, Row, Col) {
        let bump = Bump::new();
        let src = bump.alloc_str(input);
        let mut parser = Parser::new(&bump, src.as_bytes());

        let (status, row, col) = parser.eat_spaces();
        (status, parser.pos, row, col)
    }

    #[test]
    fn empty() {
        let (status, pos, _, _) = parse_and_chomp("");
        assert_eq!(status, SpaceStatus::Good);
        assert_eq!(pos, 0);
    }

    #[test]
    fn spaces_only() {
        let (status, pos, _, _) = parse_and_chomp("   ");
        assert_eq!(status, SpaceStatus::Good);
        assert_eq!(pos, 3);
    }

    #[test]
    fn newlines() {
        let (status, pos, row, col) = parse_and_chomp("  \n  \n  ");
        assert_eq!(status, SpaceStatus::Good);
        assert_eq!(pos, 8);
        assert_eq!(row, 3);
        assert_eq!(col, 3);
    }

    #[test]
    fn line_comment() {
        let (status, pos, _, _) = parse_and_chomp("-- comment\nfoo");
        assert_eq!(status, SpaceStatus::Good);
        assert_eq!(pos, 11); // After the newline
    }

    #[test]
    fn multi_comment() {
        let (status, pos, _, _) = parse_and_chomp("{- comment -}foo");
        assert_eq!(status, SpaceStatus::Good);
        assert_eq!(pos, 13);
    }

    #[test]
    fn nested_multi_comment() {
        let (status, pos, _, _) = parse_and_chomp("{- outer {- inner -} outer -}foo");
        assert_eq!(status, SpaceStatus::Good);
        assert_eq!(pos, 29);
    }

    #[test]
    fn tab_error() {
        let (status, _, _, _) = parse_and_chomp("  \t  ");
        assert_eq!(status, SpaceStatus::HasTab);
    }

    #[test]
    fn endless_multi_comment() {
        let (status, _, _, _) = parse_and_chomp("{- never closed");
        assert_eq!(status, SpaceStatus::EndlessMultiComment);
    }

    #[test]
    fn doc_comment_not_consumed() {
        // {-| should not be consumed - it's a doc comment for the caller
        let (status, pos, _, _) = parse_and_chomp("{-| doc -}");
        assert_eq!(status, SpaceStatus::Good);
        assert_eq!(pos, 0); // Not consumed
    }

    #[test]
    fn stops_at_content() {
        let (status, pos, _, _) = parse_and_chomp("  foo");
        assert_eq!(status, SpaceStatus::Good);
        assert_eq!(pos, 2); // Stopped at 'f'
    }

    #[test]
    fn doc_comment_simple() {
        let bump = Bump::new();
        let src = bump.alloc_str("{-| hello -}");
        let mut parser = Parser::new(&bump, src.as_bytes());

        let result = parser.doc_comment(|_, _| "expected", |_, _, _| "space error");
        assert!(result.is_ok());
        let comment = result.unwrap();
        // Content is " hello " (between {-| and -})
        assert_eq!(comment.0.data, b" hello ");
        assert_eq!(comment.0.off_row, 1);
        assert_eq!(comment.0.off_col, 4); // Column after {-|
    }

    #[test]
    fn doc_comment_multiline() {
        let bump = Bump::new();
        let src = bump.alloc_str("{-| line one\nline two -}");
        let mut parser = Parser::new(&bump, src.as_bytes());

        let result = parser.doc_comment(|_, _| "expected", |_, _, _| "space error");
        assert!(result.is_ok());
        let comment = result.unwrap();
        assert_eq!(comment.0.data, b" line one\nline two ");
    }

    #[test]
    fn doc_comment_not_doc() {
        let bump = Bump::new();
        let src = bump.alloc_str("{- not a doc comment -}");
        let mut parser = Parser::new(&bump, src.as_bytes());

        let result = parser.doc_comment(|_, _| "expected", |_, _, _| "space error");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "expected");
    }
}
