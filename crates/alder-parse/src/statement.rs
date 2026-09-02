//! Blocks and statements (docs/parser-internals.md §2.1 rule 3: statements
//! are separated by line breaks, never `;`).
//!
//! Grammar (SPEC.md "Statements and blocks", with §10.6's `assert`):
//!
//! ```ebnf
//! block     = '{' { statement } [ expression ] '}' ;
//! statement = let_decl
//!           | 'use' path
//!           | 'provide' path '=' expression block
//!           | assign
//!           | 'for' pattern 'in' expression block
//!           | 'while' expression block
//!           | 'return' [ expression ]          (* value on the same line *)
//!           | 'break' [ expression ]           (* value on the same line *)
//!           | 'continue'
//!           | 'assert' expression
//!           | expression ;
//! assign    = place ( '=' | '+=' | '-=' | '*=' | '/=' ) expression ;
//! place     = lower_ident { '.' lower_ident | '.' digits | '[' expression ']' } ;
//! ```
//!
//! A block's trailing expression statement is its `tail`. Inside a block the
//! statement after a statement must start on a later line or be `}`
//! (`Block::SameLine`); a `;` is `Stmt::Semicolon` with the "separate with a
//! line break" hint (§10.38). `if` / `while` / `for … in` / `provide … =`
//! heads run under `no_record_ctor` (§2.3).
//!
//! Error shape in `block()`:
//!
//! - A byte that cannot start a statement (`)`, `,`, `=`, EOF, …) is
//!   `Block::End` ("expected a statement or `}`"): a `Stmt::Expr(Expr::Start)`
//!   raised at the statement's own start position is folded into `End`, the
//!   same rule §6.0 uses for `Expr::OperatorRight`. `Block::SameLine` is
//!   therefore only reported for something that does start a statement.
//! - A first statement shaped like a record start — `..`, or a lowercase name
//!   followed on the same line by `:` (not `::`) or `,` — is
//!   `Block::LooksLikeRecord` (§2.2, §11). The shape must not cross a line
//!   break: `x` then `:timeout` on the next line is a legal block.
//! - An assignment operator must sit on the target's line: `= += -= *= /=`
//!   are chain terminators (§2), not line continuers (§2.1 rule 2), so `x`
//!   then `= 1` on the next line is the statement `x` followed by `End` at
//!   the `=`.
//!
//! Conventions: `block()` and `statement()` chomp trailing whitespace and
//! compute their region before the chomp; the private `*_stmt` helpers and
//! `use_stmt` leave the cursor wherever their last sub-parser left it (after
//! trailing whitespace when that was `expression()` or `block()`, right after
//! the last byte otherwise).
//!
//! See docs/parser-internals.md §5.12.
// OWNER: statement.rs (Wave 2)

use alder_region::{Located, Position, Region};
use alder_source::{Block, Expr, Place, PlaceStep, Stmt};
use bumpalo::collections::Vec as BumpVec;

use crate::keyword::is_reserved;
use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// At `{`. Always a block. Enforces Block::SameLine; the last `Stmt::Expr`
    /// before `}` becomes `tail`. Counts one nesting level (§10.44).
    pub fn block(&mut self) -> Result<&'a Located<Block<'a>>, error::Block<'a>> {
        self.nest(error::Block::TooDeep, Self::block_body)
    }

    fn block_body(&mut self) -> Result<&'a Located<Block<'a>>, error::Block<'a>> {
        let start = self.get_position();
        self.word1(b'{', error::Block::Open)?;
        self.chomp();
        let mut stmts: BumpVec<'a, &'a Located<Stmt<'a>>> = BumpVec::new_in(self.bump);
        let mut last_end: Option<Position> = None;
        loop {
            let (row, col) = self.position();
            // `;` is exempt from the same-line rule: `statement()` reports it
            // as `Stmt::Semicolon` (the more specific hint).
            let mut same_line = false;
            match self.peek() {
                Some(b'}') => {
                    self.advance();
                    break;
                }
                None => return Err(error::Block::End(row, col)),
                Some(b';') => {}
                Some(_) => {
                    if stmts.is_empty() && self.looks_like_record_start() {
                        return Err(error::Block::LooksLikeRecord(row, col));
                    }
                    same_line = last_end.is_some_and(|end| !self.newline_since(end));
                }
            }
            let stmt = match self.statement() {
                Err(error::Stmt::Expr(error::Expr::Start(r, c), _, _))
                    if (*r, *c) == (row, col) =>
                {
                    return Err(error::Block::End(row, col));
                }
                _ if same_line => return Err(error::Block::SameLine(row, col)),
                Err(e) => return Err(error::Block::Stmt(self.alloc(e), row, col)),
                Ok(stmt) => stmt,
            };
            last_end = Some(stmt.region.end);
            stmts.push(stmt);
        }
        let tail = match stmts.last().map(|stmt| &stmt.value) {
            Some(Stmt::Expr(expr)) => {
                let expr = *expr;
                stmts.pop();
                Some(expr)
            }
            _ => None,
        };
        let block = self.add_end(
            start,
            Block {
                stmts: stmts.into_bump_slice(),
                tail,
            },
        );
        self.chomp();
        Ok(block)
    }

    /// Lookahead at the first statement of a block: the record-start shape of
    /// §2.2 — `..`, or a non-reserved lowercase name followed on the same line
    /// by a single `:` or by `,` — so the block was probably meant to be a
    /// record. The shape never crosses a line break: `x` followed by
    /// `:timeout` on the next line is a name statement and a tag tail.
    fn looks_like_record_start(&mut self) -> bool {
        if self.peek() == Some(b'.') && self.peek_at(1) == Some(b'.') {
            return true;
        }
        self.lookahead(|p| {
            if !p.peek_lower() {
                return false;
            }
            let word = p.peek_word();
            if is_reserved(word) {
                return false;
            }
            p.advance_by(word.len());
            let name_end = p.get_position();
            p.chomp();
            if p.newline_since(name_end) {
                return false;
            }
            match (p.peek(), p.peek_at(1)) {
                (Some(b':'), next) => next != Some(b':'),
                (Some(b','), _) => true,
                _ => false,
            }
        })
    }

    /// One statement; dispatch on let/use/provide/for/while/return/break/continue/assert/`;`,
    /// else `expr_or_assign`. Chomps trailing whitespace.
    pub fn statement(&mut self) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>> {
        let start = self.get_position();
        let stmt = match self.peek_word() {
            "let" => self.specialize(
                |bump, e, row, col| error::Stmt::Let(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(3);
                    let decl = p.let_decl()?;
                    Ok(p.stmt_at(start, decl.value.region.end, Stmt::Let(decl)))
                },
            )?,
            "use" => {
                self.advance_by(3);
                self.use_stmt(start)?
            }
            "provide" => self.specialize(
                |bump, e, row, col| error::Stmt::Provide(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(7);
                    p.provide_stmt(start)
                },
            )?,
            "for" => self.specialize(
                |bump, e, row, col| error::Stmt::For(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(3);
                    p.for_stmt(start)
                },
            )?,
            "while" => self.specialize(
                |bump, e, row, col| error::Stmt::While(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(5);
                    p.while_stmt(start)
                },
            )?,
            "return" => {
                self.advance_by(6);
                self.return_stmt(start)?
            }
            "break" => {
                self.advance_by(5);
                self.break_stmt(start)?
            }
            "continue" => {
                self.advance_by(8);
                self.stmt_at(start, self.get_position(), Stmt::Continue)
            }
            "assert" => {
                self.advance_by(6);
                self.assert_stmt(start)?
            }
            _ => {
                if self.peek() == Some(b';') {
                    let (row, col) = self.position();
                    return Err(error::Stmt::Semicolon(row, col));
                }
                self.expr_or_assign()?
            }
        };
        self.chomp();
        Ok(stmt)
    }

    /// expression, then optional assign_op + value. Shared with lambda bodies.
    ///
    /// The assignment operator must sit on the target's line: `=` and the
    /// compound forms are chain terminators (§2), not line continuers, so an
    /// operator on a later line ends this statement and is left for the
    /// caller. The target's start is the position of `Stmt::AssignTarget`.
    pub(crate) fn expr_or_assign(&mut self) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>> {
        let start = self.get_position();
        let expr = self.specialize(
            |bump, e, row, col| error::Stmt::Expr(bump.alloc(e), row, col),
            |p| p.expression(),
        )?;
        if self.newline_since(expr.region.end) {
            return Ok(self.stmt_at(start, expr.region.end, Stmt::Expr(expr)));
        }
        let Some(op) = self.assign_op() else {
            return Ok(self.stmt_at(start, expr.region.end, Stmt::Expr(expr)));
        };
        let Some(place) = self.expr_to_place(expr) else {
            return Err(error::Stmt::AssignTarget(
                op.value,
                start.line,
                start.column,
            ));
        };
        self.chomp();
        let value = self.specialize(
            |bump, e, row, col| error::Stmt::AssignValue(bump.alloc(e), row, col),
            |p| p.expression(),
        )?;
        Ok(self.stmt_at(start, value.region.end, Stmt::Assign { place, op, value }))
    }

    /// Var followed by Access/TupleAccess/Index steps → Place; otherwise None.
    pub(crate) fn expr_to_place(
        &self,
        expr: &'a Located<Expr<'a>>,
    ) -> Option<&'a Located<Place<'a>>> {
        let mut steps: BumpVec<'a, PlaceStep<'a>> = BumpVec::new_in(self.bump);
        let mut current = expr;
        let root = loop {
            match &current.value {
                Expr::Var(name) => break Located::at(current.region, *name),
                Expr::Access { record, field } => {
                    steps.push(PlaceStep::Field(*field));
                    current = record;
                }
                Expr::TupleAccess { tuple, index } => {
                    steps.push(PlaceStep::TupleIndex(*index));
                    current = tuple;
                }
                Expr::Index { target, index } => {
                    steps.push(PlaceStep::Index(index));
                    current = target;
                }
                _ => return None,
            }
        };
        // Collected from the outermost step inwards; a place reads root-first.
        steps.reverse();
        let place = Place {
            root,
            steps: steps.into_bump_slice(),
        };
        Some(self.alloc(Located::at(expr.region, place)))
    }

    /// After `for`: `pattern 'in' expression block`.
    fn for_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::For<'a>> {
        self.chomp();
        let pattern = self.specialize(
            |bump, e, row, col| error::For::Pattern(bump.alloc(e), row, col),
            |p| p.pattern(),
        )?;
        self.keyword(b"in", error::For::In)?;
        self.chomp();
        let iter = self.specialize(
            |bump, e, row, col| error::For::Iter(bump.alloc(e), row, col),
            |p| p.with_record_ctor(false, |p| p.expression()),
        )?;
        let body = self.specialize(
            |bump, e, row, col| error::For::Body(bump.alloc(e), row, col),
            |p| p.block(),
        )?;
        Ok(self.stmt_at(
            start,
            body.region.end,
            Stmt::For {
                pattern,
                iter,
                body,
            },
        ))
    }

    /// After `while`: `expression block`.
    fn while_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::While<'a>> {
        self.chomp();
        let condition = self.specialize(
            |bump, e, row, col| error::While::Condition(bump.alloc(e), row, col),
            |p| p.with_record_ctor(false, |p| p.expression()),
        )?;
        let body = self.specialize(
            |bump, e, row, col| error::While::Body(bump.alloc(e), row, col),
            |p| p.block(),
        )?;
        Ok(self.stmt_at(start, body.region.end, Stmt::While { condition, body }))
    }

    /// After `provide`: `path '=' expression block`.
    fn provide_stmt(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Stmt<'a>>, error::Provide<'a>> {
        self.chomp();
        let name = self.path(error::Provide::Name, error::Provide::Name)?;
        self.chomp();
        let (row, col) = self.position();
        if self.peek() != Some(b'=') || matches!(self.peek_at(1), Some(b'=' | b'>')) {
            return Err(error::Provide::Equals(row, col));
        }
        self.advance();
        self.chomp();
        let value = self.specialize(
            |bump, e, row, col| error::Provide::Value(bump.alloc(e), row, col),
            |p| p.with_record_ctor(false, |p| p.expression()),
        )?;
        let body = self.specialize(
            |bump, e, row, col| error::Provide::Body(bump.alloc(e), row, col),
            |p| p.block(),
        )?;
        Ok(self.stmt_at(start, body.region.end, Stmt::Provide { name, value, body }))
    }

    /// After `use`. `pub(crate)` (not private as §5.12 shows) because
    /// `markup::directive` dispatches child-block `use` through it (§6.2).
    /// Does not chomp: the cursor stops right after the path.
    ///
    /// `path()` stops before `::lower` and never reads `.`, so `use Db::x` and
    /// `use Db.insert(u)` would otherwise leave their tail to the block loop
    /// (a misleading `Block::SameLine`); a `::` or `.` on the path's line is
    /// reported here as `Stmt::UseMember` at its own position.
    pub(crate) fn use_stmt(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>> {
        self.chomp();
        let path = self.path(error::Stmt::Use, error::Stmt::Use)?;
        let path_end = path.region().end;
        let member = self.lookahead(|p| {
            p.chomp();
            if p.newline_since(path_end) {
                return None;
            }
            match (p.peek(), p.peek_at(1)) {
                (Some(b'.'), _) | (Some(b':'), Some(b':')) => Some(p.position()),
                _ => None,
            }
        });
        if let Some((row, col)) = member {
            return Err(error::Stmt::UseMember(row, col));
        }
        Ok(self.stmt_at(start, path_end, Stmt::Use(path)))
    }

    /// After `return`: an optional value on the same line (§2.1 rule 4).
    fn return_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>> {
        let keyword_end = self.get_position();
        let value = self.same_line_value(keyword_end, error::Stmt::Return)?;
        let end = value.map_or(keyword_end, |value| value.region.end);
        Ok(self.stmt_at(start, end, Stmt::Return(value)))
    }

    /// After `break`: an optional value on the same line (§2.1 rule 4).
    fn break_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>> {
        let keyword_end = self.get_position();
        let value = self.same_line_value(keyword_end, error::Stmt::Break)?;
        let end = value.map_or(keyword_end, |value| value.region.end);
        Ok(self.stmt_at(start, end, Stmt::Break(value)))
    }

    /// The value of a `return` / `break`: present only when something that is
    /// not `}` (or `;`, left for `Stmt::Semicolon`, or EOF) follows on the
    /// keyword's line.
    fn same_line_value(
        &mut self,
        keyword_end: Position,
        to_error: impl FnOnce(&'a error::Expr<'a>, crate::Row, crate::Col) -> error::Stmt<'a>,
    ) -> Result<Option<&'a Located<Expr<'a>>>, error::Stmt<'a>> {
        self.chomp();
        if self.newline_since(keyword_end) || matches!(self.peek(), None | Some(b'}' | b';')) {
            return Ok(None);
        }
        let value = self.specialize(
            |bump, e, row, col| to_error(bump.alloc(e), row, col),
            |p| p.expression(),
        )?;
        Ok(Some(value))
    }

    /// After `assert`: `expression`.
    fn assert_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>> {
        self.chomp();
        let expr = self.specialize(
            |bump, e, row, col| error::Stmt::Assert(bump.alloc(e), row, col),
            |p| p.expression(),
        )?;
        Ok(self.stmt_at(start, expr.region.end, Stmt::Assert(expr)))
    }

    /// A statement node spanning `start`..`end` (the end of its last child,
    /// which may already have chomped trailing whitespace).
    fn stmt_at(&self, start: Position, end: Position, value: Stmt<'a>) -> &'a Located<Stmt<'a>> {
        self.alloc(Located::at(Region::new(start, end), value))
    }
}

/// Snapshot test macro for successful block parsing.
#[cfg(test)]
macro_rules! assert_block_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .block()
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

/// Snapshot test macro for block parse errors.
#[cfg(test)]
macro_rules! assert_block_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .block()
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

/// Snapshot test macro for successful statement parsing.
#[cfg(test)]
macro_rules! assert_statement_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .statement()
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

/// Snapshot test macro for statement parse errors.
#[cfg(test)]
macro_rules! assert_statement_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .statement()
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
mod tests {
    // ---- blocks

    #[test]
    fn block_empty() {
        assert_block_snapshot!("{}");
    }

    #[test]
    fn block_tail_only() {
        assert_block_snapshot!("{ x }");
    }

    #[test]
    fn block_stmt_and_tail() {
        assert_block_snapshot!(
            r#"
            {
                let y = x + 1
                y
            }
            "#
        );
    }

    #[test]
    fn block_stmts_no_tail() {
        assert_block_snapshot!(
            r#"
            {
                let y = 1
                count += y
            }
            "#
        );
    }

    #[test]
    fn block_nested() {
        // The inner body must be unambiguously a block: `{ x }` alone sits in
        // expression position, where §2.2 makes it a record.
        assert_block_snapshot!(
            r#"
            {
                {
                    let y = x
                    y
                }
            }
            "#
        );
    }

    #[test]
    fn block_looks_like_record_hint() {
        assert_block_error_snapshot!("{ x: 1 }");
    }

    #[test]
    fn block_shorthand_record_hint() {
        assert_block_error_snapshot!("{ x, y }");
    }

    #[test]
    fn block_spread_record_hint() {
        assert_block_error_snapshot!("{ ..r }");
    }

    #[test]
    fn block_tag_stmt_after_name() {
        // `x` then `:timeout` is a name statement and a tag tail, not the
        // record-field shape: the hint must not cross the line break.
        assert_block_snapshot!(
            r#"
            {
                x
                :timeout
            }
            "#
        );
    }

    /// The record-start lookahead at the first statement position of `{ … }`.
    fn record_start_hint(src: &str) -> bool {
        let bump = bumpalo::Bump::new();
        let src = bump.alloc_str(src);
        let mut parser = crate::Parser::new(&bump, src.as_bytes());
        parser.advance();
        parser.chomp();
        let saved = parser.position();
        let hint = parser.looks_like_record_start();
        assert_eq!(parser.position(), saved, "lookahead must not consume");
        hint
    }

    #[test]
    fn record_start_lookahead() {
        assert!(record_start_hint("{ x: 1 }"));
        assert!(record_start_hint("{ x : 1 }"));
        assert!(record_start_hint("{ x, y }"));
        assert!(record_start_hint("{ ..r }"));
        assert!(!record_start_hint("{ x }"));
        assert!(!record_start_hint("{ x::y }"));
        assert!(!record_start_hint("{ type: 1 }"));
        assert!(!record_start_hint("{ x\n: 1 }"));
        assert!(!record_start_hint("{ x\n, y }"));
        assert!(!record_start_hint("{ x\n:timeout }"));
    }

    // ---- let

    #[test]
    fn let_simple() {
        assert_statement_snapshot!("let x = 1");
    }

    #[test]
    fn let_mut() {
        assert_statement_snapshot!("let mut count = 0");
    }

    #[test]
    fn let_annotated() {
        assert_statement_snapshot!("let x: Number = 1");
    }

    #[test]
    fn let_pattern_tuple() {
        assert_statement_snapshot!("let (a, b) = pair");
    }

    #[test]
    fn let_pattern_record() {
        assert_statement_snapshot!("let { name, age } = user");
    }

    #[test]
    fn let_multiline_value() {
        assert_statement_snapshot!(
            r#"
            let total =
                compute(items)
            "#
        );
    }

    // ---- assignment

    #[test]
    fn assign_var() {
        assert_statement_snapshot!("x = 1");
    }

    /// The statement ends at the value's `)` (§10.43).
    #[test]
    fn assign_parens() {
        assert_statement_snapshot!("x = (a)");
    }

    #[test]
    fn assign_field() {
        assert_statement_snapshot!("user.name = \"Ada\"");
    }

    #[test]
    fn assign_tuple_index() {
        assert_statement_snapshot!("pair.0 = 1");
    }

    #[test]
    fn assign_index() {
        assert_statement_snapshot!("items[i] = 1");
    }

    #[test]
    fn assign_add() {
        assert_statement_snapshot!("count += 1");
    }

    #[test]
    fn assign_sub() {
        assert_statement_snapshot!("count -= 1");
    }

    #[test]
    fn assign_mul() {
        assert_statement_snapshot!("total *= 2");
    }

    #[test]
    fn assign_div() {
        assert_statement_snapshot!("total /= 2");
    }

    // ---- expression statements

    #[test]
    fn expr_stmt_call() {
        assert_statement_snapshot!("process(item)");
    }

    #[test]
    fn expr_stmt_if_then_stmt() {
        assert_block_snapshot!(
            r#"
            {
                if item.skip { continue }
                total += item.price
            }
            "#
        );
    }

    // ---- loops

    #[test]
    fn for_simple() {
        assert_statement_snapshot!("for item in items { total += item }");
    }

    #[test]
    fn for_pattern() {
        assert_statement_snapshot!("for (key, value) in pairs { total += value }");
    }

    #[test]
    fn for_nested() {
        assert_statement_snapshot!(
            r#"
            for row in grid {
                for cell in row {
                    total += cell
                }
            }
            "#
        );
    }

    #[test]
    fn while_simple() {
        assert_statement_snapshot!("while pending.length > 0 { process(pending.pop()) }");
    }

    // ---- return / break / continue

    #[test]
    fn return_bare() {
        assert_statement_snapshot!("return");
    }

    #[test]
    fn return_value() {
        assert_statement_snapshot!("return x + 1");
    }

    /// The statement ends at the value's `)` (§10.43).
    #[test]
    fn return_parens() {
        assert_statement_snapshot!("return (a)");
    }

    #[test]
    fn return_newline_no_value() {
        assert_block_snapshot!(
            r#"
            {
                return
                x
            }
            "#
        );
    }

    #[test]
    fn return_before_brace() {
        assert_block_snapshot!("{ return }");
    }

    #[test]
    fn break_bare() {
        assert_statement_snapshot!("break");
    }

    #[test]
    fn break_value() {
        assert_statement_snapshot!("break next");
    }

    #[test]
    fn continue_() {
        assert_statement_snapshot!("continue");
    }

    // ---- use / provide

    #[test]
    fn use_simple() {
        assert_statement_snapshot!("use Db");
    }

    #[test]
    fn use_path() {
        assert_statement_snapshot!("use App::Db");
    }

    #[test]
    fn error_use_lower_member() {
        assert_statement_error_snapshot!("use Db::x");
    }

    #[test]
    fn error_use_access() {
        assert_statement_error_snapshot!("use Db.insert(u)");
    }

    #[test]
    fn error_use_access_in_block() {
        assert_block_error_snapshot!("{ use Db.insert(u) }");
    }

    #[test]
    fn error_use_path_member() {
        assert_statement_error_snapshot!("use App::Db::x");
    }

    #[test]
    fn provide_simple() {
        assert_statement_snapshot!("provide Db = fakeDb() { run() }");
    }

    #[test]
    fn provide_nested() {
        assert_statement_snapshot!(
            r#"
            provide Db = Sqlite.open("app.db") {
                provide Session = session {
                    saveUser(u).await
                }
            }
            "#
        );
    }

    // ---- assert

    #[test]
    fn assert_simple() {
        assert_statement_snapshot!("assert ok");
    }

    #[test]
    fn assert_comparison() {
        assert_statement_snapshot!("assert add(1, 2) == 3");
    }

    #[test]
    fn assert_await() {
        assert_statement_snapshot!("assert find(1).await == Ok(ada)");
    }

    // ---- style

    #[test]
    fn style_let() {
        assert_statement_snapshot!("let card = style { padding: 16px }");
    }

    // ---- newline rules (§2.1)

    #[test]
    fn two_calls_on_lines() {
        assert_block_snapshot!(
            r#"
            {
                foo()
                bar()
            }
            "#
        );
    }

    #[test]
    fn call_after_newline_is_new_stmt() {
        assert_block_snapshot!(
            r#"
            {
                x
                (y)
            }
            "#
        );
    }

    #[test]
    fn index_after_newline_is_new_stmt() {
        assert_block_snapshot!(
            r#"
            {
                items
                [i]
            }
            "#
        );
    }

    #[test]
    fn array_after_newline_is_new_stmt() {
        assert_block_snapshot!(
            r#"
            {
                foo()
                [1, 2]
            }
            "#
        );
    }

    #[test]
    fn markup_after_expr_on_next_line() {
        assert_block_snapshot!(
            r#"
            {
                x
                <div />
            }
            "#
        );
    }

    #[test]
    fn negative_after_newline_is_new_stmt() {
        assert_block_snapshot!(
            r#"
            {
                x
                -1
            }
            "#
        );
    }

    #[test]
    fn minus_with_space_after_newline_continues() {
        assert_block_snapshot!(
            r#"
            {
                x
                - 1
            }
            "#
        );
    }

    #[test]
    fn pipe_after_newline_continues() {
        assert_block_snapshot!(
            r#"
            {
                x
                |> f
            }
            "#
        );
    }

    // ---- errors

    #[test]
    fn error_same_line() {
        assert_block_error_snapshot!("{ let x = 1 2 }");
    }

    #[test]
    fn error_close_paren_same_line() {
        // `)` cannot start a statement: `End`, not `SameLine`.
        assert_block_error_snapshot!("{ let x = 1 )");
    }

    #[test]
    fn error_close_paren_stmt_start() {
        assert_block_error_snapshot!("{ )");
    }

    #[test]
    fn error_assign_op_on_next_line() {
        // `=` is a chain terminator, not a line continuer: the statement is
        // `x`, and the `=` on the next line cannot start a statement.
        assert_block_error_snapshot!(
            r#"
            {
                x
                = 1
            }
            "#
        );
    }

    #[test]
    fn error_assign_target() {
        assert_statement_error_snapshot!("foo() = 1");
    }

    #[test]
    fn error_assign_target_slash_equals() {
        assert_statement_error_snapshot!("foo() /= 1");
    }

    #[test]
    fn error_semicolon() {
        assert_block_error_snapshot!("{ let x = 1; }");
    }

    #[test]
    fn error_let_missing_equals() {
        assert_statement_error_snapshot!("let x 1");
    }

    #[test]
    fn error_while_condition() {
        assert_statement_error_snapshot!("while ) { }");
    }

    #[test]
    fn error_while_body() {
        assert_statement_error_snapshot!("while ready x");
    }

    #[test]
    fn error_provide_name() {
        assert_statement_error_snapshot!("provide db = x { }");
    }

    #[test]
    fn error_provide_equals() {
        assert_statement_error_snapshot!("provide Db { }");
    }

    #[test]
    fn error_provide_value() {
        assert_statement_error_snapshot!("provide Db = ) { }");
    }

    #[test]
    fn error_provide_body() {
        assert_statement_error_snapshot!("provide Db = db");
    }

    #[test]
    fn error_assert_value() {
        assert_statement_error_snapshot!(r#"assert "x"#);
    }

    #[test]
    fn error_return_value() {
        assert_statement_error_snapshot!("return )");
    }

    #[test]
    fn error_break_value() {
        assert_statement_error_snapshot!("break )");
    }

    #[test]
    fn error_for_missing_in() {
        assert_statement_error_snapshot!("for item of items { }");
    }

    #[test]
    fn error_unclosed_block() {
        assert_block_error_snapshot!("{ let x = 1");
    }

    #[test]
    fn error_stmt_start() {
        assert_statement_error_snapshot!(")");
    }
}
