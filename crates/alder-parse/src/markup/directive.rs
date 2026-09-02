//! `@if` / `@for` / `@match` directives and `child_block`s.
//!
//! See docs/parser-internals.md §5.16 and §6.2.
//!
//! Directive heads are code mode (`chomp` between tokens) and are parsed
//! under `with_record_ctor(false, …)` like `if` / `for` / `match` heads
//! (§2.3), so `@if s == Shape::Empty { … }` reads the `{` as the body.
//! A `child_block` is `{` items `}`: at an item start `let` / `let mut` and
//! `use` are statements (setup, not rendered; §10.23); everything else is a
//! child in text mode. `@else` / `@empty` may follow the previous block on
//! a later line: the parser looks past whitespace for them
//! (`peek_directive`) and otherwise leaves that whitespace to the next
//! text run.
//!
//! `@match` arm bodies (§10.24) are a `child_block` when they start with
//! `{` (always — a `{` after `=>` is never a hole), an element, a fragment
//! or a directive; bare text is `DirMatch::BareText` because it would run
//! into the next arm. Arms are code mode between bodies: an optional `,`
//! or a line break separates them.
//!
//! Error positions: `StrayElse` / `StrayEmpty` / `UnknownDirective` at the
//! `@`; the wrapping `Child::If` / `For` / `Match` at the `@` too;
//! `ElseBranchStart` after `@else`; `DirFor::In` / `Key` where the word was
//! expected; `BareText` at the body start; `ChildBlock::Open` where `{` was
//! expected; `ChildBlock::Item` at the failing item; `ChildBlock::End` at
//! EOF.
// OWNER: markup/directive.rs (Wave 3)

use alder_region::{Located, Position, Region};
use alder_source::{Child, ChildBlock, ChildIfBranch, ChildItem, ChildMatchArm, Stmt};
use bumpalo::collections::Vec as BumpVec;

use super::ChildTerminator;
use crate::keyword::is_ident_byte;
use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// At `@`.
    pub(crate) fn directive(&mut self) -> Result<&'a Located<Child<'a>>, error::Child<'a>> {
        let start = self.get_position();
        let (row, col) = self.position();
        self.word1(b'@', error::Child::UnknownDirective)?;
        // `peek_word` takes the whole identifier run, so a match here means
        // the word is followed by a non-identifier byte (§2).
        let word = self.peek_word();
        match word {
            "if" => {
                self.advance_by(2);
                self.dir_if(start)
                    .map_err(|e| error::Child::If(self.alloc(e), row, col))
            }
            "for" => {
                self.advance_by(3);
                self.dir_for(start)
                    .map_err(|e| error::Child::For(self.alloc(e), row, col))
            }
            "match" => {
                self.advance_by(5);
                self.dir_match(start)
                    .map_err(|e| error::Child::Match(self.alloc(e), row, col))
            }
            "else" => Err(error::Child::StrayElse(row, col)),
            "empty" => Err(error::Child::StrayEmpty(row, col)),
            _ => Err(error::Child::UnknownDirective(row, col)),
        }
    }

    /// After `@if`: `cond child_block { @else if cond child_block } [ @else child_block ]`.
    fn dir_if(&mut self, start: Position) -> Result<&'a Located<Child<'a>>, error::DirIf<'a>> {
        let mut branches = BumpVec::new_in(self.bump);
        let mut final_else = None;
        let mut end;
        loop {
            self.chomp();
            let condition = self.specialize(
                |bump, e, row, col| error::DirIf::Condition(bump.alloc(e), row, col),
                |p| p.with_record_ctor(false, |p| p.expression()),
            )?;
            // `expression()` chomped; the cursor is on the `{`.
            let body = self.specialize(
                |bump, e, row, col| error::DirIf::Body(bump.alloc(e), row, col),
                |p| p.child_block(),
            )?;
            end = body.region.end;
            branches.push(ChildIfBranch { condition, body });
            if !self.peek_directive(b"else") {
                break;
            }
            self.eat_directive(b"else");
            self.chomp();
            if self.peek_keyword(b"if") {
                self.advance_by(2);
                continue;
            }
            if self.peek() != Some(b'{') {
                let (row, col) = self.position();
                return Err(error::DirIf::ElseBranchStart(row, col));
            }
            let block = self.specialize(
                |bump, e, row, col| error::DirIf::Else(bump.alloc(e), row, col),
                |p| p.child_block(),
            )?;
            end = block.region.end;
            final_else = Some(block);
            break;
        }
        Ok(self.child_at(
            start,
            end,
            Child::If {
                branches: branches.into_bump_slice(),
                final_else,
            },
        ))
    }

    /// After `@for`: `pattern in expr [ ; key expr ] child_block [ @empty child_block ]`.
    fn dir_for(&mut self, start: Position) -> Result<&'a Located<Child<'a>>, error::DirFor<'a>> {
        self.chomp();
        // `pattern()` chomps.
        let pattern = self.specialize(
            |bump, e, row, col| error::DirFor::Pattern(bump.alloc(e), row, col),
            |p| p.pattern(),
        )?;
        self.keyword(b"in", error::DirFor::In)?;
        self.chomp();
        let iter = self.specialize(
            |bump, e, row, col| error::DirFor::Iter(bump.alloc(e), row, col),
            |p| p.with_record_ctor(false, |p| p.expression()),
        )?;
        let key = if self.peek() == Some(b';') {
            self.advance();
            self.chomp();
            self.keyword(b"key", error::DirFor::Key)?;
            self.chomp();
            Some(self.specialize(
                |bump, e, row, col| error::DirFor::KeyExpr(bump.alloc(e), row, col),
                |p| p.with_record_ctor(false, |p| p.expression()),
            )?)
        } else {
            None
        };
        let body = self.specialize(
            |bump, e, row, col| error::DirFor::Body(bump.alloc(e), row, col),
            |p| p.child_block(),
        )?;
        let mut end = body.region.end;
        let empty = if self.peek_directive(b"empty") {
            self.eat_directive(b"empty");
            self.chomp();
            let block = self.specialize(
                |bump, e, row, col| error::DirFor::Empty(bump.alloc(e), row, col),
                |p| p.child_block(),
            )?;
            end = block.region.end;
            Some(block)
        } else {
            None
        };
        Ok(self.child_at(
            start,
            end,
            Child::For {
                pattern,
                iter,
                key,
                body,
                empty,
            },
        ))
    }

    /// After `@match`: `expr { arms }`.
    fn dir_match(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Child<'a>>, error::DirMatch<'a>> {
        self.chomp();
        let scrutinee = self.specialize(
            |bump, e, row, col| error::DirMatch::Scrutinee(bump.alloc(e), row, col),
            |p| p.with_record_ctor(false, |p| p.expression()),
        )?;
        // `expression()` chomped.
        self.word1(b'{', error::DirMatch::Open)?;
        self.chomp();
        let arms = self.with_record_ctor(true, |p| p.child_match_arms())?;
        Ok(self.child_at(start, self.get_position(), Child::Match { scrutinee, arms }))
    }

    /// Arms until the closing `}` (consumed). Mirrors `match_arms`.
    fn child_match_arms(&mut self) -> Result<&'a [ChildMatchArm<'a>], error::DirMatch<'a>> {
        let mut arms = BumpVec::new_in(self.bump);
        // After `{` or `,` an arm (or `}`) is required; after a comma-less
        // body anything that is not a pattern start is `DirMatch::End`.
        let mut expect_arm = true;
        loop {
            if self.peek() == Some(b'}') {
                self.advance();
                return Ok(arms.into_bump_slice());
            }
            let (row, col) = self.position();
            match self.child_match_arm() {
                Ok(arm) => arms.push(arm),
                Err(error::DirMatch::Pattern(error::Pattern::Start(..), r, c))
                    if !expect_arm && (r, c) == (row, col) =>
                {
                    return Err(error::DirMatch::End(r, c));
                }
                Err(e) => return Err(e),
            }
            // The body ended in text mode (right after `}` or `>`).
            self.chomp();
            expect_arm = self.peek() == Some(b',');
            if expect_arm {
                self.advance();
                self.chomp();
            }
        }
    }

    /// One arm: head, then a `child_block`, an element / fragment, or a
    /// directive (§10.24). A bare child is stored as a one-item block.
    fn child_match_arm(&mut self) -> Result<ChildMatchArm<'a>, error::DirMatch<'a>> {
        let (patterns, guard) = self.arm_head(
            error::DirMatch::Pattern,
            error::DirMatch::Guard,
            error::DirMatch::Arrow,
        )?;
        // `arm_head` chomped; the cursor is on the body.
        let (row, col) = self.position();
        let body = match self.peek() {
            Some(b'{') => self.specialize(
                |bump, e, row, col| error::DirMatch::Block(bump.alloc(e), row, col),
                |p| p.child_block(),
            )?,
            // `</` is the terminator `child(CloseTag)` must never be
            // called on (its debug assertion): a close tag after `=>`
            // (`A => </b>`) is reported as `child()` reports a stray one
            // elsewhere, `Child::Element(Markup::CloseName)`.
            Some(b'<') if self.peek_at(1) == Some(b'/') => {
                let close = self.alloc(error::Markup::CloseName(row, col + 2));
                let child = self.alloc(error::Child::Element(close, row, col));
                return Err(error::DirMatch::Body(child, row, col));
            }
            Some(b'<') | Some(b'@') => {
                let child = self.specialize(
                    |bump, e, row, col| error::DirMatch::Body(bump.alloc(e), row, col),
                    |p| p.child(ChildTerminator::CloseTag),
                )?;
                // `child` returns None only for a whitespace run, which
                // cannot start at `<` or `@`; `@` + non-letter is text.
                let Some(child) = child else {
                    return Err(error::DirMatch::BareText(row, col));
                };
                if matches!(child.value, Child::Text(_)) {
                    return Err(error::DirMatch::BareText(row, col));
                }
                let items = self.alloc_slice_copy(&[ChildItem::Child(child)]);
                self.alloc(Located::at(child.region, ChildBlock { items }))
            }
            _ => return Err(error::DirMatch::BareText(row, col)),
        };
        Ok(ChildMatchArm {
            patterns,
            guard,
            body,
        })
    }

    /// At `{`: `let` / `let mut` / `use` statements and children until `}`.
    pub(crate) fn child_block(
        &mut self,
    ) -> Result<&'a Located<ChildBlock<'a>>, error::ChildBlock<'a>> {
        let start = self.get_position();
        self.word1(b'{', error::ChildBlock::Open)?;
        // Brackets reset the record-constructor restriction (§2.3): a
        // `let x = Shape::Rect { … }` inside the block is a constructor.
        let items = self.with_record_ctor(true, |p| p.child_items())?;
        Ok(self.add_end(start, ChildBlock { items }))
    }

    /// Items until `}` (consumed). An item starts at the first
    /// non-whitespace byte: `let` / `use` there is a statement; anything
    /// else (including that whitespace) is a child. This is the
    /// `children(Brace)` loop with the statement case added (its result is
    /// `ChildItem`s, which `children` cannot produce).
    fn child_items(&mut self) -> Result<&'a [ChildItem<'a>], error::ChildBlock<'a>> {
        let mut items = BumpVec::new_in(self.bump);
        loop {
            if self.at_item_stmt() {
                self.skip_whitespace();
                let stmt = self.specialize(
                    |bump, e, row, col| {
                        let child = error::Child::Stmt(bump.alloc(e), row, col);
                        error::ChildBlock::Item(bump.alloc(child), row, col)
                    },
                    |p| p.child_stmt(),
                )?;
                items.push(ChildItem::Stmt(stmt));
                continue;
            }
            let (row, col) = self.position();
            if self.at_terminator(ChildTerminator::Brace) {
                // EOF or `}`.
                if self.is_eof() {
                    return Err(error::ChildBlock::End(row, col));
                }
                self.advance();
                return Ok(items.into_bump_slice());
            }
            match self.child(ChildTerminator::Brace) {
                Ok(Some(child)) => items.push(ChildItem::Child(child)),
                Ok(None) => {}
                Err(e) => return Err(error::ChildBlock::Item(self.alloc(e), row, col)),
            }
        }
    }

    /// Does `let` / `use` (+ non-identifier byte) follow the layout whitespace?
    fn at_item_stmt(&mut self) -> bool {
        self.lookahead(|p| {
            p.skip_whitespace();
            p.peek_keyword(b"let") || p.peek_keyword(b"use")
        })
    }

    /// At `let` or `use`: the setup statements a child block recognizes
    /// (§10.23). `let_decl` chomps after its value; `use_stmt` stops right
    /// after the path, leaving the line break to the next text run.
    fn child_stmt(&mut self) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>> {
        let start = self.get_position();
        if self.peek_keyword(b"let") {
            let decl = self.specialize(
                |bump, e, row, col| error::Stmt::Let(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(3);
                    p.let_decl()
                },
            )?;
            let region = Region::new(start, decl.value.region.end);
            Ok(self.alloc(Located::at(region, Stmt::Let(decl))))
        } else {
            self.advance_by(3);
            self.use_stmt(start)
        }
    }

    /// Lookahead past whitespace for `@else` / `@empty` (does not consume).
    pub(crate) fn peek_directive(&mut self, word: &[u8]) -> bool {
        self.lookahead(|p| {
            p.skip_whitespace();
            if p.peek() != Some(b'@') {
                return false;
            }
            let rest = &p.remaining()[1..];
            rest.starts_with(word) && !rest.get(word.len()).copied().is_some_and(is_ident_byte)
        })
    }

    /// Consume the whitespace and the `@word` that `peek_directive` found.
    fn eat_directive(&mut self, word: &[u8]) {
        self.skip_whitespace();
        self.advance_by(1 + word.len());
    }
}

#[cfg(test)]
mod tests {
    use super::super::{assert_markup_error_snapshot, assert_markup_snapshot};

    // ---- @if --------------------------------------------------------------

    #[test]
    fn directive_if() {
        assert_markup_snapshot!(
            r#"
            <div>
                @if ready {
                    <p>ok</p>
                }
            </div>
            "#
        );
    }

    #[test]
    fn directive_if_else() {
        assert_markup_snapshot!("<div>@if a { <p>a</p> } @else { <p>b</p> }</div>");
    }

    #[test]
    fn directive_if_else_if() {
        assert_markup_snapshot!(
            r#"
            <div>
                @if status.loading {
                    <Spinner />
                } @else if status.failed {
                    <p>Something went wrong</p>
                } @else {
                    <p>{count} items</p>
                }
            </div>
            "#
        );
    }

    #[test]
    fn directive_if_else_next_line() {
        assert_markup_snapshot!(
            r#"
            <div>
                @if a {
                    <p>a</p>
                }
                @else {
                    <p>b</p>
                }
            </div>
            "#
        );
    }

    #[test]
    fn directive_if_condition_path_no_record_ctor() {
        assert_markup_snapshot!("<div>@if s == Shape::Empty { <p>e</p> }</div>");
    }

    #[test]
    fn directive_if_empty_body() {
        assert_markup_snapshot!("<div>@if a {}</div>");
    }

    #[test]
    fn directive_if_text_body() {
        assert_markup_snapshot!("<p>@if a { yes } @else { no }</p>");
    }

    #[test]
    fn directive_if_mid_text() {
        assert_markup_snapshot!("<p>count: @if a { <b>1</b> } done</p>");
    }

    // ---- @for -------------------------------------------------------------

    #[test]
    fn directive_for() {
        assert_markup_snapshot!("<ul>@for item in items { <li>{item}</li> }</ul>");
    }

    #[test]
    fn directive_for_key() {
        assert_markup_snapshot!(
            r#"
            <ul>
                @for item in items; key item.id {
                    <li>{item.name}</li>
                }
            </ul>
            "#
        );
    }

    #[test]
    fn directive_for_empty() {
        assert_markup_snapshot!(
            r#"
            <ul>
                @for item in items {
                    <li>{item}</li>
                } @empty {
                    <li>Nothing here</li>
                }
            </ul>
            "#
        );
    }

    #[test]
    fn directive_for_empty_next_line() {
        assert_markup_snapshot!(
            r#"
            <ul>
                @for item in items {
                    <li>{item}</li>
                }
                @empty {
                    <li>none</li>
                }
            </ul>
            "#
        );
    }

    #[test]
    fn directive_for_tuple_pattern() {
        assert_markup_snapshot!(
            r#"
            <box>
                @for (task, i) in tasks; key task {
                    <text inverse={i == selected}>{task}</text>
                }
            </box>
            "#
        );
    }

    #[test]
    fn directive_for_iter_call() {
        assert_markup_snapshot!("<ul>@for x in Array.range(0, n) { <li>{x}</li> }</ul>");
    }

    // ---- @match -----------------------------------------------------------

    #[test]
    fn directive_match() {
        assert_markup_snapshot!(
            r#"
            <div>
                @match status {
                    Loading => <Spinner />,
                    Ready(n) => <span>{n}</span>,
                }
            </div>
            "#
        );
    }

    #[test]
    fn directive_match_child_body() {
        assert_markup_snapshot!("<div>@match s { Loading => <Spinner /> }</div>");
    }

    #[test]
    fn directive_match_block_body() {
        assert_markup_snapshot!(
            r#"
            <div>
                @match s {
                    Ready(n) => {
                        let double = n * 2
                        <span>{double}</span>
                    }
                    _ => {}
                }
            </div>
            "#
        );
    }

    #[test]
    fn directive_match_guard() {
        assert_markup_snapshot!(
            "<div>@match n { n if n > 0 => <p>pos</p>, _ => <p>other</p> }</div>"
        );
    }

    #[test]
    fn directive_match_alternatives() {
        assert_markup_snapshot!("<div>@match n { 1 | 2 => <p>small</p> }</div>");
    }

    #[test]
    fn directive_match_directive_body() {
        assert_markup_snapshot!(
            "<div>@match s { Some(x) => @if x { <b>x</b> }, None => <></> }</div>"
        );
    }

    #[test]
    fn directive_match_fragment_body() {
        assert_markup_snapshot!("<div>@match s { _ => <>a</> }</div>");
    }

    #[test]
    fn directive_match_newline_separated_arms() {
        assert_markup_snapshot!(
            r#"
            <div>
                @match s {
                    A => <p>a</p>
                    B => <p>b</p>
                }
            </div>
            "#
        );
    }

    #[test]
    fn directive_match_empty() {
        assert_markup_snapshot!("<div>@match s {}</div>");
    }

    #[test]
    fn directive_match_scrutinee_path_no_record_ctor() {
        assert_markup_snapshot!("<div>@match Shape::Empty { Shape::Empty => <p>e</p> }</div>");
    }

    // ---- child blocks -----------------------------------------------------

    #[test]
    fn child_block_let() {
        assert_markup_snapshot!(
            r#"
            <div>
                @if x {
                    let y = x * 2
                    <p>{y}</p>
                }
            </div>
            "#
        );
    }

    #[test]
    fn child_block_let_mut() {
        assert_markup_snapshot!(
            r#"
            <div>
                @if x {
                    let mut n = 0
                    <p>{n}</p>
                }
            </div>
            "#
        );
    }

    #[test]
    fn child_block_use() {
        assert_markup_snapshot!(
            r#"
            <div>
                @if x {
                    use Theme
                    <p>t</p>
                }
            </div>
            "#
        );
    }

    #[test]
    fn child_block_let_record_ctor() {
        assert_markup_snapshot!(
            r#"
            <div>
                @if x {
                    let r = Shape::Rect { width: 1 }
                    <p>{r.width}</p>
                }
            </div>
            "#
        );
    }

    #[test]
    fn child_block_text_only() {
        assert_markup_snapshot!("<p>@if x { just text }</p>");
    }

    #[test]
    fn child_block_let_same_line() {
        assert_markup_snapshot!("<p>@if x { let y = 1\n<b>{y}</b> }</p>");
    }

    // `let` mid-run is text: only an item start is code.
    #[test]
    fn child_block_let_after_text_is_text() {
        assert_markup_snapshot!("<p>@if x { so let it be }</p>");
    }

    // The `let` value is code mode: an operator on the next line continues
    // it (§2.1 rule 2), so `- 2` is part of the value, not text.
    #[test]
    fn child_block_let_value_continues_on_operator_line() {
        assert_markup_snapshot!(
            r#"
            <p>
                @if x {
                    let y = 1
                     - 2
                }
            </p>
            "#
        );
    }

    // `(` on a new line does not continue the value (§2.1 rule 1): text.
    #[test]
    fn child_block_let_then_paren_text() {
        assert_markup_snapshot!(
            r#"
            <p>
                @if x {
                    let y = 1
                    (2)
                }
            </p>
            "#
        );
    }

    #[test]
    fn nested_directives() {
        assert_markup_snapshot!(
            r#"
            <ul>
                @for item in items {
                    @if item.visible {
                        <li>{item.name}</li>
                    }
                }
            </ul>
            "#
        );
    }

    #[test]
    fn directive_in_fragment() {
        assert_markup_snapshot!("<>@if a { <p>a</p> }</>");
    }

    // ---- errors -----------------------------------------------------------

    #[test]
    fn error_directive_unknown() {
        assert_markup_error_snapshot!("<p>@foo</p>");
    }

    // After a hole the cursor is at a child start again (see the module
    // doc of `markup/mod.rs`; `text_at_word_after_space` is the text case).
    #[test]
    fn error_directive_unknown_after_hole() {
        assert_markup_error_snapshot!("<p>{x}@iffy</p>");
    }

    #[test]
    fn error_else_without_if() {
        assert_markup_error_snapshot!("<p>@else { <b>x</b> }</p>");
    }

    // A `//` comment is text in child position (§6.2: nothing is chomped),
    // so the `@else` after it no longer follows the `@if` block.
    #[test]
    fn error_else_after_comment_text() {
        assert_markup_error_snapshot!("<p>@if x { a } // c\n@else { b }</p>");
    }

    #[test]
    fn error_empty_without_for() {
        assert_markup_error_snapshot!("<p>@empty { <b>x</b> }</p>");
    }

    #[test]
    fn error_if_condition() {
        assert_markup_error_snapshot!("<p>@if ) { <b>x</b> }</p>");
    }

    #[test]
    fn error_if_missing_block() {
        assert_markup_error_snapshot!("<p>@if x }</p>");
    }

    // `x <b>x</b>` is the comparison chain `x < b > x < /b…` (§11: `<`
    // after an operand is an operator), so the missing `{` surfaces as a
    // missing operand inside the condition.
    #[test]
    fn error_if_missing_block_before_element() {
        assert_markup_error_snapshot!("<p>@if x <b>x</b></p>");
    }

    #[test]
    fn error_if_else_branch_start() {
        assert_markup_error_snapshot!("<p>@if x { } @else <b>x</b></p>");
    }

    #[test]
    fn error_for_missing_in() {
        assert_markup_error_snapshot!("<ul>@for x items { <li>{x}</li> }</ul>");
    }

    #[test]
    fn error_for_key_keyword() {
        assert_markup_error_snapshot!("<ul>@for x in xs; foo x { <li>{x}</li> }</ul>");
    }

    #[test]
    fn error_for_pattern() {
        assert_markup_error_snapshot!("<ul>@for +x in xs { }</ul>");
    }

    #[test]
    fn error_match_bare_text() {
        assert_markup_error_snapshot!("<p>@match x { A => hi }</p>");
    }

    /// A close tag as an arm body: `child(CloseTag)` must not be entered on
    /// its own terminator (debug assertion; the release build reported the
    /// same error by accident).
    #[test]
    fn error_match_close_tag_body() {
        assert_markup_error_snapshot!("<p>@match x { A => </p>");
    }

    #[test]
    fn error_match_close_tag_body_multiline() {
        assert_markup_error_snapshot!(
            r#"
            <div>
                @match status {
                    Loading => </pinner />,
                    Ready(n) => <span>{n}</span>,
                }
            </div>
            "#
        );
    }

    #[test]
    fn error_match_open() {
        assert_markup_error_snapshot!("<p>@match x A => <b>a</b> }</p>");
    }

    #[test]
    fn error_match_arrow() {
        assert_markup_error_snapshot!("<p>@match x { A -> <b>a</b> }</p>");
    }

    #[test]
    fn error_match_end() {
        assert_markup_error_snapshot!("<p>@match x { A => <b>a</b> ) }</p>");
    }

    #[test]
    fn error_match_unclosed() {
        assert_markup_error_snapshot!("<p>@match x { A => <b>a</b>");
    }

    #[test]
    fn error_child_block_end() {
        assert_markup_error_snapshot!("<p>@if x { <b>y</b>");
    }

    #[test]
    fn error_child_block_close_tag_inside() {
        assert_markup_error_snapshot!("<p>@if x { </p> }");
    }

    #[test]
    fn error_child_block_let() {
        assert_markup_error_snapshot!("<p>@if x { let y }</p>");
    }

    #[test]
    fn error_child_block_use_member() {
        assert_markup_error_snapshot!("<p>@if x { use Db.run }</p>");
    }
}
