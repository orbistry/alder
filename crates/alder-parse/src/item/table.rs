//! `table` declarations and the modifier lists shared with `schema` rules.
//!
//! See docs/parser-internals.md §5.11 and §10.28.
//!
//! Grammar (SPEC.md "Items"):
//!
//! ```ebnf
//! table_decl = 'table' lower_ident '{' { column } '}' ;
//! column     = lower_ident ':' expression { modifier } ;
//! modifier   = lower_ident [ '(' [ expression { ',' expression } [ ',' ] ')' ] ] ;
//! ```
//!
//! Columns are line-separated only by convention: nothing separates them
//! in the grammar. After the builder expression, every lowercase word is a
//! modifier unless it is followed (after whitespace) by `:`, in which case
//! it starts the next column (§10.28). A reserved word after the builder
//! is neither (`Table::Column` at the word). Modifier arguments require
//! the `(` on the modifier's line, like a call (§2.1 rule 1); a `(` on a
//! later line ends the column and is reported by the column loop.
//!
//! `table_decl` stops right after the closing `}` without chomping, so
//! `item()` computes the item region before its own chomp.
// OWNER: item/table.rs (Wave 3)

use alder_region::Located;
use alder_source::{Column, Expr, Modifier, TableDecl};
use bumpalo::collections::Vec as BumpVec;

use crate::keyword::is_reserved;
use crate::{Col, Parser, Row, error};

impl<'a> Parser<'a> {
    /// After `table`.
    // Called by `item()` (item/mod.rs, Wave 3); the allow goes away with the
    // Wave 4 sweep (docs/parser-internals.md §9 step 4.2).
    #[allow(unused)]
    pub(crate) fn table_decl(&mut self) -> Result<&'a TableDecl<'a>, error::Table<'a>> {
        self.chomp();
        let name = self.located_lower(error::Table::Name)?;
        self.chomp();
        self.word1(b'{', error::Table::Open)?;
        self.chomp();
        let mut columns = BumpVec::new_in(self.bump);
        loop {
            match self.peek() {
                Some(b'}') => {
                    self.advance();
                    break;
                }
                Some(b) if b.is_ascii_lowercase() => columns.push(self.column()?),
                _ => {
                    let (row, col) = self.position();
                    return Err(if columns.is_empty() {
                        error::Table::Column(row, col)
                    } else {
                        error::Table::End(row, col)
                    });
                }
            }
        }
        Ok(self.alloc(TableDecl {
            name,
            columns: columns.into_bump_slice(),
        }))
    }

    /// `name ':' expression { modifier }`. Chomps trailing whitespace
    /// (`expression()` and `modifier()` do).
    fn column(&mut self) -> Result<Column<'a>, error::Table<'a>> {
        let name = self.located_lower(error::Table::Column)?;
        self.chomp();
        self.word1(b':', error::Table::Colon)?;
        self.chomp();
        let builder = self.specialize(
            |bump, e, row, col| error::Table::Builder(bump.alloc(e), row, col),
            |p| p.expression(),
        )?;
        let mut modifiers = BumpVec::new_in(self.bump);
        while self.peek_modifier() {
            modifiers.push(self.modifier(error::Table::ModifierArg, error::Table::ModifierArgEnd)?);
        }
        Ok(Column {
            name,
            builder,
            modifiers: modifiers.into_bump_slice(),
        })
    }

    /// Is the cursor on a modifier / rule name: a non-reserved lowercase
    /// word that is **not** followed by `:` (which would make it the next
    /// column or schema field, §10.28)? Does not consume.
    pub(super) fn peek_modifier(&mut self) -> bool {
        self.peek_lower() && !is_reserved(self.peek_word()) && !self.peek_ident_colon()
    }

    /// Is the cursor on a lowercase word followed, after whitespace, by `:`?
    /// The next-column / next-field lookahead of §10.28. Does not consume.
    pub(super) fn peek_ident_colon(&mut self) -> bool {
        self.lookahead(|p| {
            if !p.peek_lower() {
                return false;
            }
            let len = p.peek_word().len();
            p.advance_by(len);
            p.chomp();
            p.peek() == Some(b':')
        })
    }

    /// `name [ '(' expr { ',' expr } [ ',' ] ')' ]` — shared with schema rules.
    ///
    /// Precondition: the cursor is on a lowercase letter (callers check
    /// `peek_modifier`). The signature (§5.11) has no name-error callback,
    /// so a caller that skips the check gets `to_end_error` at the cursor
    /// rather than a panic. The `(` must be on the name's line; otherwise
    /// the modifier is bare and the `(` is left for the caller. Chomps
    /// trailing whitespace.
    pub(crate) fn modifier<E>(
        &mut self,
        to_arg_error: impl Fn(&'a error::Expr<'a>, Row, Col) -> E + Copy,
        to_end_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<Modifier<'a>, E> {
        let name = match self.raw_lower(|row, col| (row, col)) {
            Ok(name) => name,
            Err((row, col)) => return Err(to_end_error(row, col)),
        };
        self.chomp();
        let args = if self.peek() == Some(b'(') && !self.newline_since(name.region.end) {
            self.advance();
            self.chomp();
            let args =
                self.with_record_ctor(true, |p| p.modifier_args(to_arg_error, to_end_error))?;
            self.chomp();
            args
        } else {
            &[]
        };
        Ok(Modifier { name, args })
    }

    /// After `(` and whitespace; consumes through the `)`.
    fn modifier_args<E>(
        &mut self,
        to_arg_error: impl Fn(&'a error::Expr<'a>, Row, Col) -> E + Copy,
        to_end_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<&'a [&'a Located<Expr<'a>>], E> {
        let mut args = BumpVec::new_in(self.bump);
        loop {
            if self.peek() == Some(b')') {
                self.advance();
                break;
            }
            let arg = self.specialize(
                |bump, e, row, col| to_arg_error(bump.alloc(e), row, col),
                |p| p.expression(),
            )?;
            args.push(arg);
            // `expression()` chomps, so the cursor is on the next token.
            match self.peek() {
                Some(b',') => {
                    self.advance();
                    self.chomp();
                }
                Some(b')') => {
                    self.advance();
                    break;
                }
                _ => {
                    let (row, col) = self.position();
                    return Err(to_end_error(row, col));
                }
            }
        }
        Ok(args.into_bump_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::super::assert_item_snapshot;

    // Deviation from §7.1 (one `assert_<thing>` pair per module): like
    // item/fn_.rs, the two macros below drive `table_decl()` directly
    // because the §7.2 tests go through `item()`, which stays a stub until
    // item/mod.rs lands. The input is a complete `table …` declaration; the
    // macro consumes the keyword and hands the rest to `table_decl()`.
    // Private to this `mod tests`. Wave 4 decides whether to keep or fold
    // them; recorded for §10.

    /// Snapshot test macro for a successful `table_decl()` parse (input starts at `table`).
    macro_rules! assert_table_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            parser
                .keyword(b"table", |row, col| format!("input must start with `table` ({row}:{col})"))
                .unwrap();
            let result = parser
                .table_decl()
                .unwrap_or_else(|e| panic!("expected Ok, got Err: {e:#?}\n\nSource:\n{code}"));
            // `table_decl()` stops at the `}`; `item()` chomps.
            parser.chomp();
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

    /// Snapshot test macro for a `table_decl()` parse error (input starts at `table`).
    macro_rules! assert_table_error_snapshot {
        ($code:expr) => {{
            let bump = bumpalo::Bump::new();
            let code = indoc::indoc!($code);
            let src = bump.alloc_str(code);
            let mut parser = $crate::Parser::new(&bump, src.as_bytes());
            parser
                .keyword(b"table", |row, col| format!("input must start with `table` ({row}:{col})"))
                .unwrap();
            let err = parser
                .table_decl()
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

    // ---- table_decl() directly ---------------------------------------------

    #[test]
    fn table_single_column() {
        assert_table_snapshot!("table users { id: integer() }");
    }

    #[test]
    fn table_empty() {
        assert_table_snapshot!("table users {}");
    }

    #[test]
    fn table_modifiers() {
        assert_table_snapshot!(
            r#"
            table users {
                id: integer() primaryKey autoIncrement
            }
        "#
        );
    }

    #[test]
    fn table_modifier_args() {
        assert_table_snapshot!(
            r#"
            table posts {
                author: integer() notNull references(users.id)
            }
        "#
        );
    }

    #[test]
    fn table_modifier_args_multiple() {
        assert_table_snapshot!("table t { n: integer() between(1, 10) }");
    }

    #[test]
    fn table_modifier_args_trailing_comma() {
        assert_table_snapshot!("table t { n: integer() between(1, 10,) }");
    }

    #[test]
    fn table_modifier_args_empty() {
        assert_table_snapshot!("table t { n: integer() unique() }");
    }

    #[test]
    fn table_modifier_args_record() {
        assert_table_snapshot!("table t { n: integer() default({ x: 1 }) }");
    }

    #[test]
    fn table_multiple_columns() {
        assert_table_snapshot!(
            r#"
            table users {
                id: integer() primaryKey
                email: text() notNull unique
            }
        "#
        );
    }

    #[test]
    fn table_columns_same_line() {
        assert_table_snapshot!("table t { a: integer() b: text() }");
    }

    #[test]
    fn table_modifiers_on_next_line() {
        assert_table_snapshot!(
            r#"
            table users {
                created: timestamp()
                    notNull
                    default(now)
                id: integer()
            }
        "#
        );
    }

    #[test]
    fn table_colon_spaced() {
        assert_table_snapshot!("table t { id : integer() }");
    }

    #[test]
    fn table_builder_with_args() {
        assert_table_snapshot!("table t { name: varchar(255) notNull }");
    }

    #[test]
    fn table_builder_access() {
        assert_table_snapshot!("table t { id: sqlite.integer() primaryKey }");
    }

    #[test]
    fn table_with_comments() {
        assert_table_snapshot!(
            r#"
            table users {
                // the key
                id: integer() primaryKey // modifiers
                email: text()
            }
        "#
        );
    }

    #[test]
    fn docs_data_users() {
        assert_table_snapshot!(
            r#"
            table users {
                id: integer() primaryKey autoIncrement
                email: text() notNull unique
                name: text() notNull
                created: timestamp() notNull default(now)
            }
        "#
        );
    }

    #[test]
    fn docs_data_posts() {
        assert_table_snapshot!(
            r#"
            table posts {
                id: integer() primaryKey autoIncrement
                author: integer() notNull references(users.id)
                title: text() notNull
                body: text()
            }
        "#
        );
    }

    #[test]
    fn error_no_name() {
        assert_table_error_snapshot!("table { }");
    }

    #[test]
    fn error_name_reserved() {
        assert_table_error_snapshot!("table type { }");
    }

    #[test]
    fn error_name_uppercase() {
        assert_table_error_snapshot!("table Users { }");
    }

    #[test]
    fn error_open() {
        assert_table_error_snapshot!("table users ( )");
    }

    #[test]
    fn error_column_start() {
        assert_table_error_snapshot!("table users { 5 }");
    }

    #[test]
    fn error_column_reserved() {
        assert_table_error_snapshot!("table users { type: text() }");
    }

    #[test]
    fn error_unclosed_empty() {
        assert_table_error_snapshot!("table users {");
    }

    #[test]
    fn error_missing_colon() {
        assert_table_error_snapshot!("table users { id integer() }");
    }

    #[test]
    fn error_builder() {
        assert_table_error_snapshot!("table users { id: }");
    }

    #[test]
    fn error_builder_propagates() {
        assert_table_error_snapshot!("table users { id: text(\"x }");
    }

    #[test]
    fn error_modifier_reserved() {
        assert_table_error_snapshot!("table users { id: integer() match }");
    }

    #[test]
    fn error_modifier_arg() {
        assert_table_error_snapshot!("table users { id: integer() default( }");
    }

    #[test]
    fn error_modifier_arg_end() {
        assert_table_error_snapshot!("table users { id: integer() default(now 1) }");
    }

    #[test]
    fn error_modifier_arg_unclosed() {
        assert_table_error_snapshot!("table users { id: integer() default(now");
    }

    #[test]
    fn error_modifier_paren_next_line() {
        assert_table_error_snapshot!(
            r#"
            table users {
                id: integer() default
                (now)
            }
        "#
        );
    }

    #[test]
    fn error_comma_between_modifiers() {
        assert_table_error_snapshot!("table users { id: integer() notNull, unique }");
    }

    #[test]
    fn error_end() {
        assert_table_error_snapshot!("table users { id: integer() 5 }");
    }

    #[test]
    fn error_unclosed() {
        assert_table_error_snapshot!("table users { id: integer()");
    }

    // ---- through `item()` (§7.2 names) -------------------------------------

    #[test]
    #[ignore = "waits for item/mod.rs"]
    fn table_pub() {
        assert_item_snapshot!(
            r#"
            pub table users {
                id: integer() primaryKey autoIncrement
            }
        "#
        );
    }
}
