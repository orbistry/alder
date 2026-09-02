//! `query { }` blocks (docs/parser-internals.md §6.3).
//!
//! ```ebnf
//! query_expr  = select_expr | insert_expr | update_expr | delete_expr ;
//! query_value = '^' postfix ;
//! select_expr = 'select' ( '{' expression { ',' expression } [ ',' ] '}' | '*' )
//!               'from' table_ref { join } [ 'where' expression ]
//!               [ 'groupBy' expression { ',' expression } ] [ 'orderBy' order { ',' order } ]
//!               [ 'limit' expression ] [ 'offset' expression ] ;
//! table_ref   = lower_ident [ 'as' lower_ident ] ;
//! join        = [ 'left' | 'inner' ] 'join' table_ref 'on' expression ;
//! order       = expression [ 'asc' | 'desc' ] ;
//! insert_expr = 'insert' 'into' lower_ident 'values' query_value ;
//! update_expr = 'update' lower_ident 'set' record [ 'where' expression ] ;
//! delete_expr = 'delete' 'from' lower_ident [ 'where' expression ] ;
//! ```
//!
//! The body runs under `with_query(true, …)`: `lower_name` refuses SQL
//! words, so every clause operand (`expression()`) stops cleanly at the
//! next clause word; `binop()` returns `BinOp::In`; `unary()` routes `^` to
//! `pinned_value`, whose operand is a whole postfix chain parsed with query
//! mode off again (`^user.id` pins `user.id`, `^{ a, b }` and `^select`
//! work; §10.20). The braces of a `query { }` are a bracket context, so
//! record constructors are re-enabled inside (§2.3).
//!
//! Clause order (§10.21). `select()` can only report `error::Select`, while
//! a misplaced clause is `error::Query::ClauseOrder`, so the two share the
//! work: the clause loop in `select()` accepts a clause only when it is
//! strictly later than the last one accepted (`Clause`'s derived `Ord`), or
//! a `join` after a `join`, and otherwise stops before the word; `query_body`
//! then finds a clause word where `}` should be and reports
//! `ClauseOrder(clause)` at it. That covers both `where` after `orderBy`
//! (`ClauseOrder(Where)`) and `limit 1 limit 2` (`ClauseOrder(Limit)`).
//! After `insert` / `update` / `delete` a stray clause word is a plain
//! `Query::End`: those verbs have no clause list to be out of order in.
//!
//! See docs/parser-internals.md §5.17.
// OWNER: query.rs (Wave 3)

use alder_region::{Located, Position, Region};
use alder_source::{Expr, Join, JoinKind, Order, OrderDir, Projection, Query, Select, TableRef};
use bumpalo::collections::Vec as BumpVec;

use crate::error::Clause;
use crate::{Col, Parser, Row, SqlWord, error};

impl<'a> Parser<'a> {
    /// After `query`: `{` … `}` under `with_query(true, …)`.
    pub(crate) fn query(
        &mut self,
        start: Position,
    ) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let (row, col) = (start.line, start.column);
        let query = self
            .with_query(true, |p| p.with_record_ctor(true, |p| p.query_block()))
            .map_err(|e| error::Expr::Query(self.alloc(e), row, col))?;
        Ok(self.add_end(start, Expr::Query(query)))
    }

    /// `{` `query_body` `}` — the `}` is consumed here so the expression's
    /// region ends past it; nothing is chomped afterwards (`primary` rule).
    fn query_block(&mut self) -> Result<&'a Query<'a>, error::Query<'a>> {
        self.chomp();
        self.word1(b'{', error::Query::Open)?;
        let query = self.query_body()?;
        self.chomp();
        if self.peek() == Some(b'}') {
            self.advance();
            return Ok(query);
        }
        let (row, col) = self.position();
        // Only a `select` has a clause list that a clause word can be out
        // of order in; after the other verbs the word is simply unexpected.
        if let (Query::Select(_), Some(clause)) = (query, self.peek_clause()) {
            return Err(error::Query::ClauseOrder(clause, row, col));
        }
        Err(error::Query::End(row, col))
    }

    /// Verb dispatch on the word after `{`; each verb's parser starts after
    /// its word and its errors are wrapped at the word's position.
    fn query_body(&mut self) -> Result<&'a Query<'a>, error::Query<'a>> {
        self.chomp();
        let (row, col) = self.position();
        let word = self.peek_word();
        let query = match word {
            "select" => {
                self.advance_by(word.len());
                let select = self
                    .select()
                    .map_err(|e| error::Query::Select(self.alloc(e), row, col))?;
                Query::Select(select)
            }
            "insert" => {
                self.advance_by(word.len());
                self.insert()
                    .map_err(|e| error::Query::Insert(self.alloc(e), row, col))?
            }
            "update" => {
                self.advance_by(word.len());
                self.update()
                    .map_err(|e| error::Query::Update(self.alloc(e), row, col))?
            }
            "delete" => {
                self.advance_by(word.len());
                self.delete()
                    .map_err(|e| error::Query::Delete(self.alloc(e), row, col))?
            }
            _ => return Err(error::Query::Verb(row, col)),
        };
        Ok(self.alloc(query))
    }

    /// After `select`: projection, `from`, then the ordered clause list.
    fn select(&mut self) -> Result<&'a Select<'a>, error::Select<'a>> {
        self.chomp();
        let projection = self.projection()?;
        self.chomp();
        self.keyword(b"from", error::Select::From)?;
        self.chomp();
        let from = self.specialize(
            |_, e, row, col| error::Select::Table(e, row, col),
            |p| p.table_ref(),
        )?;

        let mut joins = BumpVec::new_in(self.bump);
        let mut where_ = None;
        let mut group_by: &'a [&'a Located<Expr<'a>>] = &[];
        let mut order_by: &'a [Order<'a>] = &[];
        let mut limit = None;
        let mut offset = None;
        let mut last = Clause::From;
        loop {
            self.chomp();
            let Some(clause) = self.peek_clause() else {
                break;
            };
            // Strictly later than the last clause, except that joins repeat.
            if !(clause > last || (clause == Clause::Join && last == Clause::Join)) {
                break;
            }
            match clause {
                Clause::From => unreachable!("`from` is never later than the last clause"),
                Clause::Join => {
                    let join = self.specialize(
                        |bump, e, row, col| error::Select::Join(bump.alloc(e), row, col),
                        |p| p.join(),
                    )?;
                    joins.push(join);
                }
                Clause::Where => {
                    self.advance_by(b"where".len());
                    where_ = Some(self.clause_expr(error::Select::Where)?);
                }
                Clause::GroupBy => {
                    self.advance_by(b"groupBy".len());
                    group_by = self.clause_exprs(error::Select::GroupBy)?;
                }
                Clause::OrderBy => {
                    self.advance_by(b"orderBy".len());
                    order_by = self.orders()?;
                }
                Clause::Limit => {
                    self.advance_by(b"limit".len());
                    limit = Some(self.clause_expr(error::Select::Limit)?);
                }
                Clause::Offset => {
                    self.advance_by(b"offset".len());
                    offset = Some(self.clause_expr(error::Select::Offset)?);
                }
            }
            last = clause;
        }
        Ok(self.alloc(Select {
            projection,
            from,
            joins: joins.into_bump_slice(),
            where_,
            group_by,
            order_by,
            limit,
            offset,
        }))
    }

    /// `*` or `{ expression { ',' expression } [ ',' ] }` — the `{` here is
    /// the query parser's, never subject to the record-vs-block rule.
    fn projection(&mut self) -> Result<Projection<'a>, error::Select<'a>> {
        let (row, col) = self.position();
        match self.peek() {
            Some(b'*') => {
                let start = self.get_position();
                self.advance();
                Ok(Projection::Star(Region::new(start, self.get_position())))
            }
            Some(b'{') => {
                self.advance();
                let mut fields = BumpVec::new_in(self.bump);
                loop {
                    self.chomp();
                    let expr = self.specialize(
                        |bump, e, row, col| error::Select::ProjectionExpr(bump.alloc(e), row, col),
                        |p| p.expression(),
                    )?;
                    fields.push(expr);
                    match self.peek() {
                        Some(b',') => {
                            self.advance();
                            self.chomp();
                            if self.peek() == Some(b'}') {
                                self.advance();
                                break;
                            }
                        }
                        Some(b'}') => {
                            self.advance();
                            break;
                        }
                        _ => {
                            let (row, col) = self.position();
                            return Err(error::Select::ProjectionEnd(row, col));
                        }
                    }
                }
                Ok(Projection::Fields(fields.into_bump_slice()))
            }
            _ => Err(error::Select::Projection(row, col)),
        }
    }

    /// `[ 'left' | 'inner' ] 'join' table_ref 'on' expression`, at the first word.
    fn join(&mut self) -> Result<Join<'a>, error::Join<'a>> {
        let start = self.get_position();
        let word = self.peek_word();
        let kind = match word {
            "left" => JoinKind::Left,
            "inner" => JoinKind::Inner,
            _ => JoinKind::Plain,
        };
        if kind != JoinKind::Plain {
            self.advance_by(word.len());
            self.chomp();
        }
        self.keyword(b"join", error::Join::Keyword)?;
        let kind = self.located(start, kind);
        self.chomp();
        let table = self.specialize(
            |_, e, row, col| error::Join::Table(e, row, col),
            |p| p.table_ref(),
        )?;
        self.chomp();
        self.keyword(b"on", error::Join::On)?;
        self.chomp();
        let on = self.specialize(
            |bump, e, row, col| error::Join::Condition(bump.alloc(e), row, col),
            |p| p.expression(),
        )?;
        Ok(Join { kind, table, on })
    }

    /// One clause operand after its word.
    fn clause_expr(
        &mut self,
        to_error: impl FnOnce(&'a error::Expr<'a>, Row, Col) -> error::Select<'a>,
    ) -> Result<&'a Located<Expr<'a>>, error::Select<'a>> {
        self.chomp();
        self.specialize(
            |bump, e, row, col| to_error(bump.alloc(e), row, col),
            |p| p.expression(),
        )
    }

    /// `expression { ',' expression }` after `groupBy`. No trailing comma:
    /// there is no closing bracket, so a comma always announces another
    /// expression.
    fn clause_exprs(
        &mut self,
        to_error: impl Fn(&'a error::Expr<'a>, Row, Col) -> error::Select<'a> + Copy,
    ) -> Result<&'a [&'a Located<Expr<'a>>], error::Select<'a>> {
        let mut exprs = BumpVec::new_in(self.bump);
        loop {
            exprs.push(self.clause_expr(to_error)?);
            if self.peek() != Some(b',') {
                break;
            }
            self.advance();
        }
        Ok(exprs.into_bump_slice())
    }

    /// `order { ',' order }` after `orderBy`, `order = expression [ 'asc' | 'desc' ]`.
    fn orders(&mut self) -> Result<&'a [Order<'a>], error::Select<'a>> {
        let mut orders = BumpVec::new_in(self.bump);
        loop {
            let expr = self.clause_expr(error::Select::OrderBy)?;
            // `expression()` chomped, so a direction word sits at the cursor.
            let direction = self.order_direction();
            orders.push(Order { expr, direction });
            self.chomp();
            if self.peek() != Some(b',') {
                break;
            }
            self.advance();
        }
        Ok(orders.into_bump_slice())
    }

    /// `asc` / `desc` at the cursor, or nothing.
    fn order_direction(&mut self) -> Option<Located<OrderDir>> {
        let word = self.peek_word();
        let direction = match word {
            "asc" => OrderDir::Asc,
            "desc" => OrderDir::Desc,
            _ => return None,
        };
        let start = self.get_position();
        self.advance_by(word.len());
        Some(self.located(start, direction))
    }

    /// After `insert`: `into <table> values ^<value>`.
    fn insert(&mut self) -> Result<Query<'a>, error::Insert<'a>> {
        self.chomp();
        self.keyword(b"into", error::Insert::Into)?;
        self.chomp();
        let table = self.located_lower(error::Insert::Table)?;
        self.chomp();
        self.keyword(b"values", error::Insert::Values)?;
        self.chomp();
        // `values` takes exactly a pinned value; anything else (a bare
        // record, a name, `}`) is refused before an expression is attempted.
        if self.peek() != Some(b'^') {
            let (row, col) = self.position();
            return Err(error::Insert::Pin(row, col));
        }
        let values = self.specialize(
            |bump, e, row, col| error::Insert::Value(bump.alloc(e), row, col),
            |p| p.pinned_value(),
        )?;
        Ok(Query::Insert { table, values })
    }

    /// After `update`: `<table> set { … } [ where expression ]`.
    fn update(&mut self) -> Result<Query<'a>, error::Update<'a>> {
        self.chomp();
        let table = self.located_lower(error::Update::Table)?;
        self.chomp();
        self.keyword(b"set", error::Update::Set)?;
        self.chomp();
        // TODO(wave0): a dedicated `Update::RecordOpen` variant for a missing
        // `{` after `set`; `Update::Set` ("expected `set { … }`") stands in.
        self.word1(b'{', error::Update::Set)?;
        let set = self.specialize(
            |bump, e, row, col| error::Update::Record(bump.alloc(e), row, col),
            |p| p.record_fields(),
        )?;
        let where_ = self.optional_where(error::Update::Where)?;
        Ok(Query::Update { table, set, where_ })
    }

    /// After `delete`: `from <table> [ where expression ]`.
    fn delete(&mut self) -> Result<Query<'a>, error::Delete<'a>> {
        self.chomp();
        self.keyword(b"from", error::Delete::From)?;
        self.chomp();
        let table = self.located_lower(error::Delete::Table)?;
        let where_ = self.optional_where(error::Delete::Where)?;
        Ok(Query::Delete { table, where_ })
    }

    /// `[ 'where' expression ]` for `update` and `delete`.
    fn optional_where<E>(
        &mut self,
        to_error: impl FnOnce(&'a error::Expr<'a>, Row, Col) -> E,
    ) -> Result<Option<&'a Located<Expr<'a>>>, E> {
        self.chomp();
        if !self.peek_keyword(b"where") {
            return Ok(None);
        }
        self.advance_by(b"where".len());
        self.chomp();
        let expr = self.specialize(
            |bump, e, row, col| to_error(bump.alloc(e), row, col),
            |p| p.expression(),
        )?;
        Ok(Some(expr))
    }

    /// `lower_ident [ 'as' lower_ident ]`. Chomps before looking for `as`.
    fn table_ref(&mut self) -> Result<TableRef<'a>, error::TableRef> {
        let name = self.located_lower(error::TableRef::Name)?;
        self.chomp();
        if !self.peek_keyword(b"as") {
            return Ok(TableRef { name, alias: None });
        }
        self.advance_by(2);
        self.chomp();
        let alias = self.located_lower(error::TableRef::Alias)?;
        Ok(TableRef {
            name,
            alias: Some(alias),
        })
    }

    /// The `select` clause the word at the cursor opens, if any: `where`
    /// (a reserved word, so not a `SqlWord`) or a SQL word with a clause.
    fn peek_clause(&self) -> Option<Clause> {
        let word = self.peek_word();
        if word == "where" {
            return Some(Clause::Where);
        }
        SqlWord::from_word(word).and_then(SqlWord::clause)
    }

    /// `^` + postfix parsed with `with_query(false, …)` so `^select` and `^{ a, b }` work.
    /// The operand is the whole postfix chain (`^user.id` pins `user.id`; §10.20).
    ///
    /// The operand must be adjacent to the `^`, as in patterns; a missing
    /// operand (`^}`) is `Expr::Unary` at the operand position, like `-` and
    /// `!`, while any other operand error propagates unchanged (§6.0).
    pub(crate) fn pinned_value(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>> {
        let start = self.get_position();
        self.advance();
        let operand = match self.with_query(false, |p| p.postfix()) {
            Err(error::Expr::Start(row, col)) => return Err(error::Expr::Unary(row, col)),
            other => other?,
        };
        // `postfix()` chomped trailing whitespace; the pin ends with its operand.
        Ok(self.alloc(Located::at(
            Region::new(start, operand.region.end),
            Expr::Pin(operand),
        )))
    }
}

/// Snapshot test macro for successful query parsing.
#[cfg(test)]
macro_rules! assert_query_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .expression()
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

/// Snapshot test macro for query parse errors.
#[cfg(test)]
macro_rules! assert_query_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .expression()
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

// Re-exported for child modules per §7.1; `query.rs` has none, so the
// re-export itself is unused here (same as `statement.rs` / `type_.rs`).
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_query_error_snapshot;
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_query_snapshot;

#[cfg(test)]
mod tests {
    // ---- select -------------------------------------------------------------

    #[test]
    fn select_star() {
        assert_query_snapshot!("query { select * from users }");
    }

    #[test]
    fn select_fields() {
        assert_query_snapshot!("query { select { u.name, u.email } from users as u }");
    }

    #[test]
    fn select_fields_trailing_comma() {
        assert_query_snapshot!("query { select { name, } from users }");
    }

    #[test]
    fn select_alias() {
        assert_query_snapshot!("query { select * from users as u }");
    }

    #[test]
    fn select_join() {
        assert_query_snapshot!(
            r#"
            query {
                select * from users as u
                join posts as p on p.author == u.id
            }
        "#
        );
    }

    #[test]
    fn select_left_join() {
        assert_query_snapshot!(
            r#"
            query {
                select * from users as u
                left join posts as p on p.author == u.id
            }
        "#
        );
    }

    #[test]
    fn select_inner_join() {
        assert_query_snapshot!(
            r#"
            query {
                select * from users as u
                inner join posts as p on p.author == u.id
            }
        "#
        );
    }

    #[test]
    fn select_multiple_joins() {
        assert_query_snapshot!(
            r#"
            query {
                select * from users as u
                join posts as p on p.author == u.id
                left join likes as l on l.post == p.id
            }
        "#
        );
    }

    #[test]
    fn select_where() {
        assert_query_snapshot!("query { select * from users where users.active }");
    }

    #[test]
    fn select_where_pin() {
        assert_query_snapshot!("query { select * from users where users.id == ^id }");
    }

    #[test]
    fn select_where_pin_access() {
        assert_query_snapshot!("query { select * from users where users.id == ^user.id }");
    }

    #[test]
    fn select_where_pin_parens() {
        assert_query_snapshot!("query { select * from users where users.age > ^(min + 1) }");
    }

    #[test]
    fn select_where_pin_call() {
        assert_query_snapshot!("query { select * from users where users.id == ^current() }");
    }

    #[test]
    fn select_where_pin_sql_word() {
        assert_query_snapshot!("query { select * from users where users.id == ^select }");
    }

    #[test]
    fn select_in() {
        assert_query_snapshot!("query { select * from users where users.id in ^ids }");
    }

    #[test]
    fn select_group_by() {
        assert_query_snapshot!("query { select { p.author } from posts as p groupBy p.author }");
    }

    #[test]
    fn select_group_by_multiple() {
        assert_query_snapshot!(
            "query { select { p.author, p.year } from posts as p groupBy p.author, p.year }"
        );
    }

    #[test]
    fn select_order_by_asc() {
        assert_query_snapshot!("query { select * from users orderBy users.name asc }");
    }

    #[test]
    fn select_order_by_desc() {
        assert_query_snapshot!("query { select * from users orderBy users.created desc }");
    }

    #[test]
    fn select_order_by_default() {
        assert_query_snapshot!("query { select * from users orderBy users.name }");
    }

    #[test]
    fn select_order_by_multiple() {
        assert_query_snapshot!(
            "query { select * from users orderBy users.name asc, users.created desc }"
        );
    }

    #[test]
    fn select_limit_offset() {
        assert_query_snapshot!("query { select * from users limit 10 offset 20 }");
    }

    #[test]
    fn select_limit_pin() {
        assert_query_snapshot!("query { select * from users limit ^pageSize }");
    }

    #[test]
    fn select_multiline_clauses() {
        assert_query_snapshot!(
            r#"
            query {
                select *
                from users
                where users.active
                    && users.age > 18
                limit 1
            }
        "#
        );
    }

    #[test]
    fn select_full_docs_example() {
        assert_query_snapshot!(
            r#"
            query {
                select { u.name, p.title, p.created }
                from users as u
                join posts as p on p.author == u.id
                where u.active && p.created > ^since && u.id in ^ids
                orderBy p.created desc
                limit ^pageSize
            }
        "#
        );
    }

    #[test]
    fn docs_web_select_star_where() {
        assert_query_snapshot!("query { select * from users where users.id == event.params.id }");
    }

    // ---- insert / update / delete ------------------------------------------

    #[test]
    fn insert() {
        assert_query_snapshot!("query { insert into users values ^{ email, name } }");
    }

    #[test]
    fn insert_pin_var() {
        assert_query_snapshot!("query { insert into users values ^rows }");
    }

    #[test]
    fn update() {
        assert_query_snapshot!("query { update users set { name: ^newName } }");
    }

    #[test]
    fn update_where() {
        assert_query_snapshot!(
            "query { update users set { name: ^newName } where users.id == ^user.id }"
        );
    }

    #[test]
    fn update_set_multiple() {
        assert_query_snapshot!("query { update users set { name: ^n, active: true } }");
    }

    #[test]
    fn delete() {
        assert_query_snapshot!("query { delete from posts }");
    }

    #[test]
    fn delete_where() {
        assert_query_snapshot!("query { delete from posts where posts.author == ^user.id }");
    }

    // ---- mode boundaries ----------------------------------------------------

    #[test]
    fn sql_words_are_identifiers_outside_query() {
        assert_query_snapshot!("update(select, from, limit)");
    }

    #[test]
    fn query_in_call() {
        assert_query_snapshot!("db.run(query { select * from users })");
    }

    #[test]
    fn nested_query_in_pin_leaves_mode() {
        assert_query_snapshot!("query { select * from users where users.id == ^select.id }");
    }

    // ---- errors -------------------------------------------------------------

    #[test]
    fn error_open() {
        assert_query_error_snapshot!("query select * from users");
    }

    #[test]
    fn error_verb() {
        assert_query_error_snapshot!("query { fetch * from users }");
    }

    #[test]
    fn error_verb_empty() {
        assert_query_error_snapshot!("query { }");
    }

    #[test]
    fn error_projection() {
        assert_query_error_snapshot!("query { select from users }");
    }

    #[test]
    fn error_projection_expr() {
        assert_query_error_snapshot!("query { select { } from users }");
    }

    #[test]
    fn error_projection_end() {
        assert_query_error_snapshot!("query { select { a b } from users }");
    }

    #[test]
    fn error_missing_from() {
        assert_query_error_snapshot!("query { select * users }");
    }

    #[test]
    fn error_table_name() {
        assert_query_error_snapshot!("query { select * from Users }");
    }

    #[test]
    fn error_table_name_sql_word() {
        assert_query_error_snapshot!("query { select * from limit }");
    }

    #[test]
    fn error_alias() {
        assert_query_error_snapshot!("query { select * from users as }");
    }

    #[test]
    fn error_join_keyword() {
        assert_query_error_snapshot!("query { select * from users left posts on x }");
    }

    #[test]
    fn error_join_table() {
        assert_query_error_snapshot!("query { select * from users join on x }");
    }

    #[test]
    fn error_join_missing_on() {
        assert_query_error_snapshot!("query { select * from users join posts }");
    }

    #[test]
    fn error_join_condition() {
        assert_query_error_snapshot!("query { select * from users join posts on }");
    }

    #[test]
    fn error_where_expr() {
        assert_query_error_snapshot!("query { select * from users where }");
    }

    #[test]
    fn error_pin_no_operand() {
        assert_query_error_snapshot!("query { select * from users where users.id == ^ }");
    }

    #[test]
    fn error_pin_bad_operand_propagates() {
        assert_query_error_snapshot!("query { select * from users where users.id == ^007 }");
    }

    #[test]
    fn error_clause_order() {
        assert_query_error_snapshot!(
            "query { select * from users orderBy users.name where users.active }"
        );
    }

    #[test]
    fn error_clause_repeated() {
        assert_query_error_snapshot!("query { select * from users limit 1 limit 2 }");
    }

    #[test]
    fn error_clause_join_after_where() {
        assert_query_error_snapshot!(
            "query { select * from users where users.active join posts on x }"
        );
    }

    #[test]
    fn error_clause_from_repeated() {
        assert_query_error_snapshot!("query { select * from users from posts }");
    }

    #[test]
    fn error_sql_keyword_as_operand() {
        assert_query_error_snapshot!("query { select * from users where limit > 3 }");
    }

    #[test]
    fn error_end() {
        assert_query_error_snapshot!("query { select * from users on x }");
    }

    #[test]
    fn error_insert_into() {
        assert_query_error_snapshot!("query { insert users values ^rows }");
    }

    #[test]
    fn error_insert_values() {
        assert_query_error_snapshot!("query { insert into users ^rows }");
    }

    #[test]
    fn error_insert_not_pinned() {
        assert_query_error_snapshot!("query { insert into users values { email, name } }");
    }

    #[test]
    fn error_insert_end() {
        assert_query_error_snapshot!("query { insert into users values ^rows where x }");
    }

    #[test]
    fn error_update_set() {
        assert_query_error_snapshot!("query { update users { name: ^n } }");
    }

    #[test]
    fn error_update_record_open() {
        assert_query_error_snapshot!("query { update users set name: ^n }");
    }

    #[test]
    fn error_update_record() {
        assert_query_error_snapshot!("query { update users set { name = ^n } }");
    }

    #[test]
    fn error_delete_from() {
        assert_query_error_snapshot!("query { delete posts }");
    }

    #[test]
    fn error_delete_table() {
        assert_query_error_snapshot!("query { delete from }");
    }

    #[test]
    fn error_unclosed() {
        assert_query_error_snapshot!("query { select * from users");
    }
}
