//! Reserved words, contextual SQL words, and keyword matching.
//!
//! See docs/parser-internals.md §4.1 and §5.3.

use crate::{Col, Parser, Row};

/// Reserved words: SPEC list plus `assert` and `await`.
pub const RESERVED: &[&str] = &[
    "as",
    "assert",
    "await",
    "break",
    "comptime",
    "component",
    "continue",
    "else",
    "enum",
    "error",
    "false",
    "fn",
    "for",
    "if",
    "impl",
    "import",
    "in",
    "let",
    "loop",
    "macro",
    "match",
    "mut",
    "pub",
    "provide",
    "query",
    "return",
    "schema",
    "state",
    "style",
    "table",
    "test",
    "tests",
    "trait",
    "true",
    "type",
    "use",
    "where",
    "while",
];

/// Contextual keywords inside `query { }` only.
pub const SQL_WORDS: &[&str] = &[
    "select", "insert", "update", "delete", "from", "join", "on", "set", "into", "values",
    "orderBy", "groupBy", "limit", "offset", "asc", "desc", "left", "inner",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyword {
    As,
    Assert,
    Await,
    Break,
    Comptime,
    Component,
    Continue,
    Else,
    Enum,
    Error,
    False,
    Fn,
    For,
    If,
    Impl,
    Import,
    In,
    Let,
    Loop,
    Macro,
    Match,
    Mut,
    Pub,
    Provide,
    Query,
    Return,
    Schema,
    State,
    Style,
    Table,
    Test,
    Tests,
    Trait,
    True,
    Type,
    Use,
    Where,
    While,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqlWord {
    Select,
    Insert,
    Update,
    Delete,
    From,
    Join,
    On,
    Set,
    Into,
    Values,
    OrderBy,
    GroupBy,
    Limit,
    Offset,
    Asc,
    Desc,
    Left,
    Inner,
}

impl Keyword {
    /// Named `from_word`, not `from_str`: clippy's `should_implement_trait`
    /// rejects the latter under `-D warnings`.
    pub fn from_word(s: &str) -> Option<Keyword> {
        Some(match s {
            "as" => Keyword::As,
            "assert" => Keyword::Assert,
            "await" => Keyword::Await,
            "break" => Keyword::Break,
            "comptime" => Keyword::Comptime,
            "component" => Keyword::Component,
            "continue" => Keyword::Continue,
            "else" => Keyword::Else,
            "enum" => Keyword::Enum,
            "error" => Keyword::Error,
            "false" => Keyword::False,
            "fn" => Keyword::Fn,
            "for" => Keyword::For,
            "if" => Keyword::If,
            "impl" => Keyword::Impl,
            "import" => Keyword::Import,
            "in" => Keyword::In,
            "let" => Keyword::Let,
            "loop" => Keyword::Loop,
            "macro" => Keyword::Macro,
            "match" => Keyword::Match,
            "mut" => Keyword::Mut,
            "pub" => Keyword::Pub,
            "provide" => Keyword::Provide,
            "query" => Keyword::Query,
            "return" => Keyword::Return,
            "schema" => Keyword::Schema,
            "state" => Keyword::State,
            "style" => Keyword::Style,
            "table" => Keyword::Table,
            "test" => Keyword::Test,
            "tests" => Keyword::Tests,
            "trait" => Keyword::Trait,
            "true" => Keyword::True,
            "type" => Keyword::Type,
            "use" => Keyword::Use,
            "where" => Keyword::Where,
            "while" => Keyword::While,
            _ => return None,
        })
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Keyword::As => "as",
            Keyword::Assert => "assert",
            Keyword::Await => "await",
            Keyword::Break => "break",
            Keyword::Comptime => "comptime",
            Keyword::Component => "component",
            Keyword::Continue => "continue",
            Keyword::Else => "else",
            Keyword::Enum => "enum",
            Keyword::Error => "error",
            Keyword::False => "false",
            Keyword::Fn => "fn",
            Keyword::For => "for",
            Keyword::If => "if",
            Keyword::Impl => "impl",
            Keyword::Import => "import",
            Keyword::In => "in",
            Keyword::Let => "let",
            Keyword::Loop => "loop",
            Keyword::Macro => "macro",
            Keyword::Match => "match",
            Keyword::Mut => "mut",
            Keyword::Pub => "pub",
            Keyword::Provide => "provide",
            Keyword::Query => "query",
            Keyword::Return => "return",
            Keyword::Schema => "schema",
            Keyword::State => "state",
            Keyword::Style => "style",
            Keyword::Table => "table",
            Keyword::Test => "test",
            Keyword::Tests => "tests",
            Keyword::Trait => "trait",
            Keyword::True => "true",
            Keyword::Type => "type",
            Keyword::Use => "use",
            Keyword::Where => "where",
            Keyword::While => "while",
        }
    }
}

impl SqlWord {
    pub fn from_word(s: &str) -> Option<SqlWord> {
        Some(match s {
            "select" => SqlWord::Select,
            "insert" => SqlWord::Insert,
            "update" => SqlWord::Update,
            "delete" => SqlWord::Delete,
            "from" => SqlWord::From,
            "join" => SqlWord::Join,
            "on" => SqlWord::On,
            "set" => SqlWord::Set,
            "into" => SqlWord::Into,
            "values" => SqlWord::Values,
            "orderBy" => SqlWord::OrderBy,
            "groupBy" => SqlWord::GroupBy,
            "limit" => SqlWord::Limit,
            "offset" => SqlWord::Offset,
            "asc" => SqlWord::Asc,
            "desc" => SqlWord::Desc,
            "left" => SqlWord::Left,
            "inner" => SqlWord::Inner,
            _ => return None,
        })
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            SqlWord::Select => "select",
            SqlWord::Insert => "insert",
            SqlWord::Update => "update",
            SqlWord::Delete => "delete",
            SqlWord::From => "from",
            SqlWord::Join => "join",
            SqlWord::On => "on",
            SqlWord::Set => "set",
            SqlWord::Into => "into",
            SqlWord::Values => "values",
            SqlWord::OrderBy => "orderBy",
            SqlWord::GroupBy => "groupBy",
            SqlWord::Limit => "limit",
            SqlWord::Offset => "offset",
            SqlWord::Asc => "asc",
            SqlWord::Desc => "desc",
            SqlWord::Left => "left",
            SqlWord::Inner => "inner",
        }
    }

    /// The `select` clause a word opens, for `Query::ClauseOrder`: `From`, `Join`
    /// (also for `left` / `inner`), `GroupBy`, `OrderBy`, `Limit`, `Offset`;
    /// `None` for every other word.
    pub const fn clause(self) -> Option<crate::error::Clause> {
        use crate::error::Clause;
        match self {
            SqlWord::From => Some(Clause::From),
            SqlWord::Join | SqlWord::Left | SqlWord::Inner => Some(Clause::Join),
            SqlWord::GroupBy => Some(Clause::GroupBy),
            SqlWord::OrderBy => Some(Clause::OrderBy),
            SqlWord::Limit => Some(Clause::Limit),
            SqlWord::Offset => Some(Clause::Offset),
            SqlWord::Select
            | SqlWord::Insert
            | SqlWord::Update
            | SqlWord::Delete
            | SqlWord::On
            | SqlWord::Set
            | SqlWord::Into
            | SqlWord::Values
            | SqlWord::Asc
            | SqlWord::Desc => None,
        }
    }
}

/// Is `name` a reserved word?
#[inline]
pub fn is_reserved(name: &str) -> bool {
    RESERVED.contains(&name)
}

/// Is `name` a contextual SQL word (refused as an identifier only inside `query { }`)?
#[inline]
pub fn is_sql_word(name: &str) -> bool {
    SQL_WORDS.contains(&name)
}

/// Is `b` an identifier continuation byte (`[A-Za-z0-9_]`)?
#[inline]
pub(crate) fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

impl<'a> Parser<'a> {
    /// Exact bytes followed by a non-identifier byte; fails without consuming.
    pub(crate) fn keyword<E>(
        &mut self,
        kw: &[u8],
        to_error: impl FnOnce(Row, Col) -> E,
    ) -> Result<(), E> {
        if self.peek_keyword(kw) {
            self.advance_by(kw.len());
            Ok(())
        } else {
            let (row, col) = self.position();
            Err(to_error(row, col))
        }
    }

    /// Is `kw` at the cursor, followed by a non-identifier byte? Does not consume.
    pub(crate) fn peek_keyword(&self, kw: &[u8]) -> bool {
        let rest = self.remaining();
        rest.starts_with(kw) && !rest.get(kw.len()).copied().is_some_and(is_ident_byte)
    }

    /// The identifier-shaped word at the cursor (no consume), for dispatch tables.
    /// Empty when the cursor is not on an identifier byte.
    pub(crate) fn peek_word(&self) -> &'a str {
        let rest = self.remaining();
        let len = rest.iter().take_while(|b| is_ident_byte(**b)).count();
        // Identifier bytes are ASCII, so the prefix is always valid UTF-8.
        std::str::from_utf8(&rest[..len]).expect("identifier bytes are ASCII")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;

    fn parser<'a>(bump: &'a Bump, src: &str) -> Parser<'a> {
        let src = bump.alloc_str(src);
        Parser::new(bump, src.as_bytes())
    }

    #[test]
    fn reserved_round_trip() {
        for word in RESERVED {
            let kw = Keyword::from_word(word).expect("every reserved word maps to a Keyword");
            assert_eq!(kw.as_str(), *word);
            assert!(is_reserved(word));
        }
        assert!(!is_reserved("foo"));
        assert_eq!(Keyword::from_word("foo"), None);
    }

    #[test]
    fn sql_round_trip() {
        for word in SQL_WORDS {
            let sql = SqlWord::from_word(word).expect("every SQL word maps to a SqlWord");
            assert_eq!(sql.as_str(), *word);
            assert!(is_sql_word(word));
            assert!(!is_reserved(word));
        }
        assert!(!is_sql_word("where"));
        assert_eq!(SqlWord::from_word("where"), None);
    }

    #[test]
    fn sql_clause_mapping() {
        use crate::error::Clause;
        assert_eq!(SqlWord::From.clause(), Some(Clause::From));
        assert_eq!(SqlWord::Join.clause(), Some(Clause::Join));
        assert_eq!(SqlWord::Left.clause(), Some(Clause::Join));
        assert_eq!(SqlWord::Inner.clause(), Some(Clause::Join));
        assert_eq!(SqlWord::GroupBy.clause(), Some(Clause::GroupBy));
        assert_eq!(SqlWord::OrderBy.clause(), Some(Clause::OrderBy));
        assert_eq!(SqlWord::Limit.clause(), Some(Clause::Limit));
        assert_eq!(SqlWord::Offset.clause(), Some(Clause::Offset));
        assert_eq!(SqlWord::Select.clause(), None);
        assert_eq!(SqlWord::Asc.clause(), None);
        assert!(Clause::From < Clause::Join && Clause::Join < Clause::Where);
        assert!(Clause::Where < Clause::GroupBy && Clause::GroupBy < Clause::OrderBy);
        assert!(Clause::OrderBy < Clause::Limit && Clause::Limit < Clause::Offset);
    }

    #[test]
    fn keyword_matches_whole_word() {
        let bump = Bump::new();
        let mut p = parser(&bump, "if x");
        assert!(p.keyword(b"if", |r, c| (r, c)).is_ok());
        assert_eq!(p.position(), (1, 3));
    }

    #[test]
    fn keyword_rejects_prefix_without_consuming() {
        let bump = Bump::new();
        let mut p = parser(&bump, "iffy");
        assert_eq!(p.keyword(b"if", |r, c| (r, c)), Err((1, 1)));
        assert_eq!(p.position(), (1, 1));
        assert!(!p.peek_keyword(b"if"));
    }

    #[test]
    fn keyword_at_eof() {
        let bump = Bump::new();
        let mut p = parser(&bump, "if");
        assert!(p.peek_keyword(b"if"));
        assert!(p.keyword(b"if", |r, c| (r, c)).is_ok());
        assert!(p.is_eof());
    }

    #[test]
    fn peek_word_reads_identifier_shape() {
        let bump = Bump::new();
        let p = parser(&bump, "orderBy x");
        assert_eq!(p.peek_word(), "orderBy");
        let p = parser(&bump, "(x");
        assert_eq!(p.peek_word(), "");
        let p = parser(&bump, "");
        assert_eq!(p.peek_word(), "");
    }
}
