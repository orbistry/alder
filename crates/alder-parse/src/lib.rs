//! Parser for Alder source code.
//!
//! A scannerless, byte-level recursive-descent parser in the style of the
//! Elm compiler's `Parse/Primitives.hs`. See docs/parser-internals.md.

use alder_region::{Located, Position, Region};
use alder_source::Module;
use bumpalo::Bump;

pub mod error;
mod expression;
mod item;
mod keyword;
mod markup;
mod module;
mod name;
mod number;
mod pattern;
mod query;
mod raw;
mod space;
mod statement;
mod string;
mod style;
mod symbol;
mod template;
mod type_;

pub type Row = u16;
pub type Col = u16;

/// Saved parser state for backtracking.
#[derive(Clone, Copy)]
pub(crate) struct ParserState {
    pos: usize,
    row: Row,
    col: Col,
}

/// Parser for Alder source code.
///
/// Combines the arena allocator with parsing state for a unified API.
/// All parsed AST nodes are allocated in the provided bump arena.
///
/// The source bytes should already be allocated in the arena (via `bump.alloc_str`),
/// so all string slices in the resulting AST share the `'a` lifetime.
pub struct Parser<'a> {
    /// Arena allocator for AST nodes
    bump: &'a Bump,
    /// Source bytes (UTF-8, already in arena)
    src: &'a [u8],
    /// Current byte position
    pos: usize,
    /// Current row (1-indexed)
    row: Row,
    /// Current column (1-indexed)
    col: Col,
    /// Inside `query { }`: `in` is a binop, `^` pins, SQL words are not identifiers.
    in_query: bool,
    /// Set in if/while/for/match/provide/@directive heads: `Path {` is not a record constructor.
    #[allow(unused)]
    no_record_ctor: bool,
}

/// Entry point used by the driver and by tests.
pub fn parse_module<'a>(bump: &'a Bump, src: &'a str) -> Result<Module<'a>, error::Error<'a>> {
    let mut parser = Parser::new(bump, src.as_bytes());
    parser
        .module()
        .map_err(|e| error::Error::ParseError(bump.alloc(e)))
}

impl<'a> Parser<'a> {
    /// Create a new parser for the given source bytes.
    ///
    /// The source should already be allocated in the arena.
    pub fn new(bump: &'a Bump, src: &'a [u8]) -> Self {
        Parser {
            bump,
            src,
            pos: 0,
            row: 1,
            col: 1,
            in_query: false,
            no_record_ctor: false,
        }
    }

    // -------------------------------------------------------------------------
    // Position & State
    // -------------------------------------------------------------------------

    /// Current position as (row, col).
    #[inline]
    pub fn position(&self) -> (Row, Col) {
        (self.row, self.col)
    }

    /// Get the current position as a `Position`.
    #[inline]
    pub fn get_position(&self) -> Position {
        Position::new(self.row, self.col)
    }

    /// Create a `Located` value spanning from `start` to the current position,
    /// allocated directly in the arena.
    #[inline]
    pub fn add_end<T>(&self, start: Position, value: T) -> &'a Located<T> {
        self.alloc(self.located(start, value))
    }

    /// Current row (1-indexed).
    #[inline]
    pub fn row(&self) -> Row {
        self.row
    }

    /// Current column (1-indexed).
    #[inline]
    pub fn col(&self) -> Col {
        self.col
    }

    /// Check if we've reached the end of input.
    #[inline]
    pub fn is_eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    /// Save current parser state for backtracking.
    #[inline]
    pub(crate) fn save_state(&self) -> ParserState {
        ParserState {
            pos: self.pos,
            row: self.row,
            col: self.col,
        }
    }

    /// Restore parser state for backtracking.
    #[inline]
    pub(crate) fn restore_state(&mut self, state: ParserState) {
        self.pos = state.pos;
        self.row = state.row;
        self.col = state.col;
    }

    /// Inline `Located` spanning `start`..current (for names and other Copy leaves).
    #[inline]
    pub(crate) fn located<T>(&self, start: Position, value: T) -> Located<T> {
        Located::at(Region::new(start, self.get_position()), value)
    }
}

// Newline / adjacency / mode helpers. Callers arrive with Waves 1-3; the
// `allow` goes away in Wave 4 (docs/parser-internals.md §9 step 4.2).
#[allow(unused)]
impl<'a> Parser<'a> {
    /// Has a newline been crossed between `end` (a node's `region.end`) and here?
    #[inline]
    pub(crate) fn newline_since(&self, end: Position) -> bool {
        end.line != self.row
    }

    /// Nothing (not even whitespace) has been consumed since `end`.
    #[inline]
    pub(crate) fn adjacent_to(&self, end: Position) -> bool {
        end == self.get_position()
    }

    /// Run `f`, then restore position regardless of its result (lookahead).
    pub(crate) fn lookahead<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.save_state();
        let result = f(self);
        self.restore_state(saved);
        result
    }

    /// Run `f` with query mode set to `on`, restoring the previous mode afterwards.
    pub(crate) fn with_query<T, E>(
        &mut self,
        on: bool,
        f: impl FnOnce(&mut Self) -> Result<T, E>,
    ) -> Result<T, E> {
        let old = self.in_query;
        self.in_query = on;
        let result = f(self);
        self.in_query = old;
        result
    }

    /// Run `f` with record constructors allowed or not, restoring the previous
    /// setting afterwards.
    pub(crate) fn with_record_ctor<T, E>(
        &mut self,
        allowed: bool,
        f: impl FnOnce(&mut Self) -> Result<T, E>,
    ) -> Result<T, E> {
        let old = self.no_record_ctor;
        self.no_record_ctor = !allowed;
        let result = f(self);
        self.no_record_ctor = old;
        result
    }

    /// Are we inside `query { }`?
    #[inline]
    pub(crate) fn in_query(&self) -> bool {
        self.in_query
    }

    /// May `Path {` start a record constructor here?
    #[inline]
    pub(crate) fn record_ctor_allowed(&self) -> bool {
        !self.no_record_ctor
    }
}

impl<'a> Parser<'a> {
    // -------------------------------------------------------------------------
    // Combinators
    // -------------------------------------------------------------------------

    /// Try multiple parsers in order, returning the first success.
    ///
    /// Mirrors Elm's `oneOf`:
    /// ```haskell
    /// oneOf :: (Row -> Col -> x) -> [Parser x a] -> Parser x a
    /// ```
    ///
    /// Key semantics:
    /// - If a parser fails without consuming input, try the next one
    /// - If a parser fails after consuming input, propagate the error (committed)
    /// - If all parsers fail without consuming, call `to_error(row, col)`
    ///
    /// # Example
    /// ```ignore
    /// parser.one_of(
    ///     error::Expr::Start,
    ///     vec![
    ///         Box::new(|p: &mut Parser| p.string(start)),
    ///         Box::new(|p| p.number(start)),
    ///     ],
    /// )
    /// ```
    #[allow(clippy::type_complexity)]
    pub fn one_of<T, E>(
        &mut self,
        to_error: impl FnOnce(Row, Col) -> E,
        parsers: Vec<Box<dyn FnOnce(&mut Self) -> Result<T, E> + '_>>,
    ) -> Result<T, E> {
        let initial_state = self.save_state();

        for parser in parsers {
            let before = self.save_state();
            match parser(self) {
                Ok(value) => return Ok(value),
                Err(e) => {
                    // Did we consume any input?
                    if self.pos != before.pos {
                        // Committed - propagate error
                        return Err(e);
                    }
                    // No input consumed - restore and try next
                    self.restore_state(before);
                }
            }
        }

        // All parsers failed without consuming - restore to initial and return error
        self.restore_state(initial_state);
        let (row, col) = self.position();
        Err(to_error(row, col))
    }

    /// Like `one_of` but returns a fallback value if nothing matches.
    ///
    /// Mirrors Elm's `oneOfWithFallback`:
    /// ```haskell
    /// oneOfWithFallback :: [Parser x a] -> a -> Parser x a
    /// ```
    #[allow(clippy::type_complexity)]
    pub fn one_of_with_fallback<T, E>(
        &mut self,
        parsers: Vec<Box<dyn FnOnce(&mut Self) -> Result<T, E> + '_>>,
        fallback: T,
    ) -> Result<T, E> {
        let initial_state = self.save_state();

        for parser in parsers {
            let before = self.save_state();
            match parser(self) {
                Ok(value) => return Ok(value),
                Err(e) => {
                    // Did we consume any input?
                    if self.pos != before.pos {
                        // Committed - propagate error
                        return Err(e);
                    }
                    // No input consumed - restore and try next
                    self.restore_state(before);
                }
            }
        }

        // All parsers failed without consuming - return fallback
        self.restore_state(initial_state);
        Ok(fallback)
    }

    /// Parse with error context wrapping.
    ///
    /// Mirrors Elm's `inContext`:
    /// ```haskell
    /// inContext :: (x -> Row -> Col -> y) -> Parser y start -> Parser x a -> Parser y a
    /// ```
    ///
    /// 1. Saves the starting position
    /// 2. Runs `start_parser` - if it fails without consuming, returns that error
    /// 3. If start succeeds, runs `body_parser`
    /// 4. If body fails, wraps the error using `add_context` at the original position
    ///
    /// The `add_context` closure receives the bump allocator so it can allocate wrapped errors.
    ///
    /// This is used to provide better error context, e.g., "error in list expression".
    pub fn in_context<T, StartErr, BodyErr, ContextErr>(
        &mut self,
        add_context: impl FnOnce(&'a Bump, BodyErr, Row, Col) -> ContextErr,
        start_parser: impl FnOnce(&mut Self) -> Result<(), StartErr>,
        body_parser: impl FnOnce(&mut Self) -> Result<T, BodyErr>,
    ) -> Result<T, ContextErr>
    where
        StartErr: Into<ContextErr>,
    {
        let (start_row, start_col) = self.position();

        // Try to parse start token
        match start_parser(self) {
            Ok(()) => {
                // Start succeeded, now parse body
                match body_parser(self) {
                    Ok(value) => Ok(value),
                    Err(body_err) => {
                        // Wrap body error with context at original position
                        Err(add_context(self.bump, body_err, start_row, start_col))
                    }
                }
            }
            Err(start_err) => {
                // Start failed - convert to context error type
                Err(start_err.into())
            }
        }
    }

    /// Transform errors from one type to another with position context.
    ///
    /// Mirrors Elm's `specialize`:
    /// ```haskell
    /// specialize :: (x -> Row -> Col -> y) -> Parser x a -> Parser y a
    /// ```
    ///
    /// Runs the parser and wraps any error with the context at the starting position.
    /// The `add_context` closure receives the bump allocator so it can allocate wrapped errors.
    pub fn specialize<T, InnerErr, OuterErr>(
        &mut self,
        add_context: impl FnOnce(&'a Bump, InnerErr, Row, Col) -> OuterErr,
        parser: impl FnOnce(&mut Self) -> Result<T, InnerErr>,
    ) -> Result<T, OuterErr> {
        let (start_row, start_col) = self.position();

        match parser(self) {
            Ok(value) => Ok(value),
            Err(inner_err) => Err(add_context(self.bump, inner_err, start_row, start_col)),
        }
    }

    // -------------------------------------------------------------------------
    // Single-byte parsing
    // -------------------------------------------------------------------------

    /// Parse a single expected byte.
    ///
    /// Mirrors Elm's `word1`:
    /// ```haskell
    /// word1 :: Word8 -> (Row -> Col -> x) -> Parser x ()
    /// ```
    ///
    /// Returns `Ok(())` and advances if the byte matches.
    /// Returns `Err` without consuming if it doesn't match.
    #[inline]
    pub fn word1<E>(
        &mut self,
        expected: u8,
        to_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<(), E> {
        if self.peek() == Some(expected) {
            self.advance();
            Ok(())
        } else {
            let (row, col) = self.position();
            Err(to_error(row, col))
        }
    }

    /// Parse two expected consecutive bytes.
    ///
    /// Mirrors Elm's `word2`:
    /// ```haskell
    /// word2 :: Word8 -> Word8 -> (Row -> Col -> x) -> Parser x ()
    /// ```
    #[inline]
    pub fn word2<E>(
        &mut self,
        b1: u8,
        b2: u8,
        to_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<(), E> {
        if self.peek() == Some(b1) && self.peek_at(1) == Some(b2) {
            self.advance();
            self.advance();
            Ok(())
        } else {
            let (row, col) = self.position();
            Err(to_error(row, col))
        }
    }

    // -------------------------------------------------------------------------
    // Peeking
    // -------------------------------------------------------------------------

    /// Peek at the current byte without consuming it.
    #[inline]
    pub fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    /// Peek at a byte at the given offset from current position.
    #[inline]
    pub fn peek_at(&self, offset: usize) -> Option<u8> {
        self.src.get(self.pos + offset).copied()
    }

    /// Get the remaining bytes from current position.
    #[inline]
    pub fn remaining(&self) -> &'a [u8] {
        &self.src[self.pos..]
    }

    // -------------------------------------------------------------------------
    // Advancing
    // -------------------------------------------------------------------------

    /// Advance by one byte, updating row/col for newlines.
    #[inline]
    pub fn advance(&mut self) {
        if let Some(b) = self.peek() {
            self.pos += 1;
            if b == b'\n' {
                self.row += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
    }

    /// Advance by n bytes, tracking newlines.
    #[inline]
    pub fn advance_by(&mut self, n: usize) {
        for _ in 0..n {
            self.advance();
        }
    }

    // -------------------------------------------------------------------------
    // Allocation helpers
    // -------------------------------------------------------------------------

    /// Allocate a value in the arena.
    #[inline]
    pub fn alloc<T>(&self, value: T) -> &'a T {
        self.bump.alloc(value)
    }

    /// Allocate a slice in the arena by copying.
    #[inline]
    pub fn alloc_slice_copy<T: Copy>(&self, slice: &[T]) -> &'a [T] {
        self.bump.alloc_slice_copy(slice)
    }

    /// Allocate a string in the arena (for constructed strings like escape sequences).
    #[inline]
    pub fn alloc_str(&self, s: &str) -> &'a str {
        self.bump.alloc_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_new() {
        let bump = Bump::new();
        let src = bump.alloc_str("hello");
        let parser = Parser::new(&bump, src.as_bytes());

        assert_eq!(parser.row(), 1);
        assert_eq!(parser.col(), 1);
        assert_eq!(parser.peek(), Some(b'h'));
        assert!(!parser.is_eof());
        assert!(!parser.in_query());
        assert!(parser.record_ctor_allowed());
    }

    #[test]
    fn test_parser_advance() {
        let bump = Bump::new();
        let src = bump.alloc_str("ab\ncd");
        let mut parser = Parser::new(&bump, src.as_bytes());

        assert_eq!(parser.position(), (1, 1));
        parser.advance(); // 'a'
        assert_eq!(parser.position(), (1, 2));
        parser.advance(); // 'b'
        assert_eq!(parser.position(), (1, 3));
        parser.advance(); // '\n'
        assert_eq!(parser.position(), (2, 1));
        parser.advance(); // 'c'
        assert_eq!(parser.position(), (2, 2));
    }

    #[test]
    fn test_parser_eof() {
        let bump = Bump::new();
        let src = bump.alloc_str("x");
        let mut parser = Parser::new(&bump, src.as_bytes());

        assert!(!parser.is_eof());
        parser.advance();
        assert!(parser.is_eof());
        assert_eq!(parser.peek(), None);
    }

    #[test]
    fn newline_since_and_adjacent_to() {
        let bump = Bump::new();
        let src = bump.alloc_str("a \nb");
        let mut parser = Parser::new(&bump, src.as_bytes());

        parser.advance(); // 'a'
        let end = parser.get_position();
        assert!(parser.adjacent_to(end));
        assert!(!parser.newline_since(end));
        parser.advance(); // ' '
        assert!(!parser.adjacent_to(end));
        assert!(!parser.newline_since(end));
        parser.advance(); // '\n'
        assert!(parser.newline_since(end));
    }

    #[test]
    fn lookahead_restores_position() {
        let bump = Bump::new();
        let src = bump.alloc_str("abc");
        let mut parser = Parser::new(&bump, src.as_bytes());

        let seen = parser.lookahead(|p| {
            p.advance();
            p.advance();
            p.peek()
        });
        assert_eq!(seen, Some(b'c'));
        assert_eq!(parser.position(), (1, 1));
    }

    #[test]
    fn mode_flags_are_scoped() {
        let bump = Bump::new();
        let src = bump.alloc_str("");
        let mut parser = Parser::new(&bump, src.as_bytes());

        let r: Result<bool, ()> = parser.with_query(true, |p| {
            assert!(p.in_query());
            p.with_query(false, |p| {
                assert!(!p.in_query());
                Ok(())
            })?;
            assert!(p.in_query());
            Ok(p.in_query())
        });
        assert_eq!(r, Ok(true));
        assert!(!parser.in_query());

        let r: Result<(), ()> = parser.with_record_ctor(false, |p| {
            assert!(!p.record_ctor_allowed());
            p.with_record_ctor(true, |p| {
                assert!(p.record_ctor_allowed());
                Ok(())
            })?;
            assert!(!p.record_ctor_allowed());
            Err(())
        });
        assert_eq!(r, Err(()));
        assert!(parser.record_ctor_allowed());
    }
}
