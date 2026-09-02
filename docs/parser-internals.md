# Parser internals — M1 rewrite of `alder-source` and `alder-parse`

**Status: the contract for milestone M1.** This document fixes the source
AST, the syntax-error hierarchy, the `Parser<'a>` surface, every parse
function's signature, the test-macro shape, file ownership, and the
implementation order. Grammar authority is `SPEC.md` plus
`docs/language.md`; where they are silent the choice is recorded in
§10 (Decisions). The code blocks are the contract; prose is commentary.

Scope: `crates/alder-source` and `crates/alder-parse` are rewritten in
place. `alder-ast` (imports `alder_source::{Associativity, Docs,
Precedence}`), `alder-can`, `alder-constrain`, `alder-solve`,
`alder-driver`, `alder-cli` and `alder-language-server` stop compiling
until M2; the workspace `default-members` is narrowed for the duration
(§8, step 0.6).

---

## 1. What stays, what goes

Stays exactly as today unless a section below says otherwise: the
scannerless byte-level `Parser<'a>`; `one_of` / `one_of_with_fallback`
(committed-choice: an alternative that fails after consuming input
propagates its error, one that fails without consuming lets the next run,
and `to_error` fires at the start position when nothing consumed);
`in_context`; `specialize`; `word1`; `word2`; `peek*`; `advance*`;
`alloc*`; `add_end`; bumpalo arenas; `Located<T>`; nested error enums with
trailing `Row, Col`; insta snapshot macros per module; the
"chomp trailing whitespace after success, region computed before the
chomp" discipline.

Goes (forced by the grammar): the `indent` field and every `*Indent*` /
`check_aligned` / `check_fresh_line` / `with_indent` /
`with_backset_indent` function and error variant; `{- -}`, `--`, `{-| -}`
comments; `Docs` / `Comment` / `Snippet`; module headers; `exposing`;
ports; `infix`; `case/of`; `let/in`; `\x ->` lambdas; juxtaposition
application; operator sections; `.field` accessor functions; `::` cons;
`"""` strings; the `Space` error enum (tabs are whitespace and the only
comment is `//`, so `chomp` cannot fail); the `(node, end)` return tuple
(`end` is `node.region.end`, see §4.1); `test_support.rs` and every
`assert_indented_*` macro (no layout rule remains).

---

## 2. Lexical layer

There is no token stream. "Tokens" are recognized by small `Parser`
methods; modes (§6) are just different loops reading bytes.

| Lexeme                                 | File                            | Rule                                                                                                                                                                                                                                                                                                                                                               |
| -------------------------------------- | ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| whitespace                             | `space.rs`                      | ` `, `\t`, `\r`, `\n`. `chomp()` is infallible.                                                                                                                                                                                                                                                                                                                    |
| comment                                | `space.rs`                      | `//` to end of line. `///` and `//!` are skipped like any comment in M1 (doc attachment deferred, §10).                                                                                                                                                                                                                                                            |
| newline crossing                       | callers                         | `self.newline_since(node.region.end)` — `end.line != self.row`. No parser state.                                                                                                                                                                                                                                                                                   |
| `lower_ident`                          | `name.rs`                       | `[a-z][A-Za-z0-9_]*`, not reserved, not a SQL word while `in_query`. `_x` is not an identifier (`Pattern::WildcardNotVar`). ASCII only. The same shape **without** the reserved/SQL check is `raw_lower` (module-path segments, element/attribute names — §2.4).                                                                                                   |
| `upper_ident`                          | `name.rs`                       | `[A-Z][A-Za-z0-9_]*`.                                                                                                                                                                                                                                                                                                                                              |
| `path`                                 | `name.rs`                       | `Upper { '::' Upper }`; stops before `::lower` (consumed by the expression layer as `PathVar`). `Foo::` + nothing → `PathMember` error.                                                                                                                                                                                                                            |
| `:tag`                                 | `name.rs`                       | `:` immediately followed by a lowercase letter. Never ambiguous: record fields, annotations, constraints and style keys consume their `:` before an expression/type starts.                                                                                                                                                                                        |
| dashed name                            | `name.rs`                       | `raw_lower { '-' raw_lower }` for element, attribute and close-tag names. **Keyword-insensitive** (§2.4): `<table>`, `<style>`, `<select>`, `type=`, `for=`, `style=` are all legal.                                                                                                                                                                               |
| reserved words                         | `keyword.rs`                    | SPEC list **plus `assert` and `await`** (§10). `true`/`false` → `Expr::Bool` / `Pattern::Bool`. SQL words are contextual (only refused as identifiers inside `query { }`).                                                                                                                                                                                         |
| numbers                                | `number.rs`                     | JS semantics: decimal, `0x` hex, `.digits` fraction, `e[+-]digits` exponent, `123n` / `0xFFn` BigInt. Value `f64` plus source text. `007` → `NoLeadingZero`, `1.` → `Dot`, `1e` → `Exponent`, `0x` → `HexDigit`, `123abc` → `End`, `1.5n` → `BigIntFraction`. Tuple indices `t.0.1` are bare digit runs read by the postfix layer, never through `number_literal`. |
| strings                                | `string.rs`                     | `"..."` single-line; escapes `\n \r \t \0 \" \' \\ \u{…}`. Newline inside → `StringError::Newline`.                                                                                                                                                                                                                                                                |
| templates                              | `template.rs`                   | `` `…${expr}…` ``; raw text (newlines allowed, `\r\n` → `\n`), escapes as strings plus `` \` `` and `\$`. Tagged form is a postfix op.                                                                                                                                                                                                                             |
| operators                              | `symbol.rs`                     | **Fixed longest-match table** (custom operators are out of scope): `                                                                                                                                                                                                                                                                                               | > ?? |     | && == != <= >= < > + - * / %`, plus `in`(query mode only). Elm-habit tokens`-> | ++ :: .. < | >> << ^`also longest-match and produce`BadOperator`with a hint.`= => += -= *= /=`are **chain terminators** (not consumed). Anything else ends the chain. Longest match means`a==-1`is`a == -1`and`x<-1`is`x < -1`(there is no`<-` token). |
| assignment ops                         | `symbol.rs`                     | `=` (not followed by `=` or `>`), `+=`, `-=`, `*=`, `/=`.                                                                                                                                                                                                                                                                                                          |
| postfix marks                          | `expression/postfix.rs`         | `(` call, `[` index, `.name`, `.digits`, `.await`, `?` (when the next byte is not `?`), adjacent `` ` `` tagged template, `{` record constructor after a path, adjacent `!(` macro call after a lowercase name.                                                                                                                                                    |
| `<` markup                             | `markup/`                       | Primary position only: `<` followed by a letter or `>`; `</` → `Expr::UnexpectedClose`.                                                                                                                                                                                                                                                                            |
| `@if` `@for` `@match` `@else` `@empty` | `markup/directive.rs`           | Child position only. Text stops at `@` only when one of these words follows and the byte after it is not an identifier byte; any other `@` is text.                                                                                                                                                                                                                |
| `#[`                                   | `item/attribute.rs`             | Attributes. `#` not followed by `[` → `Attribute::Open`.                                                                                                                                                                                                                                                                                                           |
| `_`                                    | patterns, call args             | Wildcard / placeholder.                                                                                                                                                                                                                                                                                                                                            |
| `;`                                    | `statement.rs`, `item/`, `@for` | Only legal as `@for … ; key …`. Elsewhere an error with a "separate with a line break" hint: `Stmt::Semicolon` in blocks, `Item::Semicolon` in module / `tests` bodies, `Trait::Semicolon` / `Impl::Semicolon` in trait and impl bodies.                                                                                                                           |

### 2.1 Newline rules (the grammar has no `;` and no layout)

All derived from `newline_since(end)`:

1. **Postfix.** After a newline only `.` (`.field`, `.0`, `.await`)
   continues a postfix chain. `(`, `[`, `` ` ``, `?`, `{`, `!(` on a new
   line start a new statement (`x\n(y)` is two statements; Kotlin/Swift
   rule).
2. **Binary operators.** An operator on a new line continues the chain
   (leading `|>` style), **except** `-` not followed by whitespace
   (unary minus starts a statement) and `<` followed by a letter or `>`
   (markup starts a statement). Implemented by
   `continues_line(op, next_byte)` in `expression/mod.rs`.
3. **Statement and item separation.** Inside a block, after a statement
   the next token must be `}` or start on a later line; otherwise
   `Block::SameLine`. So `let x = 1 2` and `foo() bar()` are errors, not
   two statements. **The same rule applies to items**: in a module, a
   `tests { }` body, a `trait { }` body and an `impl { }` body, the item
   after an item must be EOF / `}` or start on a later line; otherwise
   `Module::SameLine` / `Tests::SameLine` / `Trait::SameLine` /
   `Impl::SameLine`. So `fn a() {} fn b() {}` is an error, and so is
   language.md's one-line `trait Iterator[i] { type Item; fn next(it: i)
-> Option[Item] }` (it first hits `Trait::Semicolon`; §10.38). Items
   and statements are never `;`-separated. Comma-separated members (enum
   variants, match arms, record fields, params) are unaffected: commas,
   not line breaks, separate them.
4. `return` / `break` take a value only when it starts on the same line
   and is not `}`.
5. Record constructor `Path {` and tagged template `` tag` `` require the
   opener on the same line; the tagged template and `name!(` additionally
   require **adjacency** (no whitespace at all).

### 2.2 `{`: record or block

Positions that grammatically demand a block always parse a block and
never consult the heuristic: `fn`/`component`/`test`/`comptime`/`loop`/
`for`/`while`/`provide` bodies, `if`/`else` branches, **lambda bodies
starting with `{`**, **match-arm bodies starting with `{`**, and
`child_block`. In every other expression position `{` is a **record**
iff, after whitespace, the next token is `}`, `..`, or a `lower_ident`
followed (after whitespace) by `:`, `,` or `}`; otherwise a **block**.
So `{ user, prefs }`, `{ ..r, x }`, `{ x: 1 }`, `{}` are records in
argument position and `fn() { x }` / `=> { x }` are blocks returning
`x`. A block whose first statement is a bare name followed by `:` gets
`Block::LooksLikeRecord` ("wrap the record in parentheses").

### 2.3 Record constructors and the `no_record_ctor` flag

`Shape::Rect { width: 1 }` is a postfix `{` after a `Path`. Inside the
head of `if` / `else if` / `while` / `for … in` / `match` /
`provide … =` / `@if` / `@for` / `@match` the flag is set (Rust's rule)
so `if s == Shape::Empty { … }` parses; it is cleared inside `( )`,
`[ ]`, record `{ }`, `${ }`, and markup holes.

### 2.4 Keyword-insensitive names

Reserved words (§4.1) are refused only where a name becomes a **binding
or a reference in code**: `lower_name` (variables, fields, params, record
keys, style keys, `as` aliases, import names). Three positions read the
identifier shape with `raw_lower` and never consult `RESERVED` /
`SQL_WORDS`:

- **Element, attribute and close-tag names** in markup (`dashed_name`).
  HTML needs `<table>`, `<style>`, `<select>`, `<form>`, `type=`, `for=`,
  `style=`; web.md writes `<Field name="password" type="password" />`.
  Attribute names become record labels only in canonicalization, which
  is where any clash with a reserved word would be handled (none is
  planned: `type` as a prop name is legal).
- **Module-path segments** (`@alder/test`, `@alder/http`, `~/db/users`):
  author, package and every `/` segment. A path is never a binding by
  itself; what the import _binds_ is checked separately (§5.11: a bare
  `import @alder/test` is `Import::ReservedBinding(Test)`).
- **Tuple indices** (`t.0`) and **directive words** (`@if`, `@else`),
  which are matched as bytes, not as identifiers.

Everything else (`match` as a variable, `let type = …`, `fn for()`) is
`Reserved(kw)` as before.

---

## 3. `crates/alder-source/src/lib.rs`

Conventions: identifiers are `Name<'a> = Located<&'a str>` inline (24
bytes, `Copy`); small `Copy` structs/enums (`Path`, `BinOp`, `Region`,
`Visibility`, `NumberLit`, …) are inline; `Expr`, `Pattern`, `Type`,
`Block`, `Item`, `Stmt`, `Child` nodes live behind `&'a Located<…>` and
their lists are `&'a [&'a Located<T>]`; `Copy` leaf structs are stored
in by-value slices `&'a [T]`; `Option<&'a T>`, never `&'a Option<T>`.

```rust
//! Source AST for Alder — what `alder-parse` produces before canonicalization.
//! See docs/parser-internals.md §3 for the layout conventions.

use alder_region::{Located, Region};

/// An identifier with its region. Stored inline everywhere.
pub type Name<'a> = Located<&'a str>;

// ============================================================================
// Module and items
// ============================================================================

#[derive(Debug)]
pub struct Module<'a> {
    pub items: &'a [&'a Located<Item<'a>>],
}

impl<'a> Module<'a> {
    /// Top-level imports (including `pub import` re-exports). Imports inside
    /// `tests { }` are only reachable through `ItemKind::Tests`.
    pub fn imports(&self) -> impl Iterator<Item = &'a Import<'a>> + '_ {
        self.items.iter().filter_map(|item| match &item.value.kind {
            ItemKind::Import(import) => Some(*import),
            _ => None,
        })
    }
}

#[derive(Debug)]
pub struct Item<'a> {
    pub attributes: &'a [Located<Attribute<'a>>],
    pub visibility: Visibility,
    pub kind: ItemKind<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Pub(Region),
}

/// `#[derive(Show, Eq)]` → args `[Path(Show), Path(Eq)]`; `#[extern("m", "n")]` → two `Str`s.
#[derive(Clone, Copy, Debug)]
pub struct Attribute<'a> {
    pub name: Name<'a>,
    pub args: &'a [&'a Located<Expr<'a>>],
}

#[derive(Debug)]
pub enum ItemKind<'a> {
    Import(&'a Import<'a>),
    /// `body: None` is a bodiless declaration; canonicalization requires `#[extern]`.
    Fn(&'a FnDecl<'a>),
    /// Includes `let card = style { … }`.
    Let(&'a LetDecl<'a>),
    TypeAlias(&'a TypeAlias<'a>),
    /// `type Name` with no `=`; canonicalization requires `#[extern]`.
    OpaqueType(Name<'a>),
    Enum(&'a EnumDecl<'a>),
    Trait(&'a TraitDecl<'a>),
    Impl(&'a ImplDecl<'a>),
    Error(&'a ErrorDecl<'a>),
    Component(&'a ComponentDecl<'a>),
    Table(&'a TableDecl<'a>),
    Schema(&'a SchemaDecl<'a>),
    Macro(&'a MacroDecl<'a>),
    Comptime(&'a Located<Block<'a>>),
    Test(&'a TestDecl<'a>),
    Tests(&'a [&'a Located<Item<'a>>]),
}

#[derive(Debug)]
pub struct Import<'a> {
    pub path: Located<ModulePath<'a>>,
    pub tail: ImportTail<'a>,
}

#[derive(Clone, Copy, Debug)]
pub struct ModulePath<'a> {
    pub root: ModuleRoot<'a>,
    /// Segments after the root: `@alder/http/client` → `["client"]`; `~/db/users` → `["db", "users"]`.
    /// Keyword-insensitive, like `author` / `package` (`@alder/test` is legal; §2.4).
    pub segments: &'a [Name<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum ModuleRoot<'a> {
    /// `@author/package`
    Package { author: Name<'a>, package: Name<'a> },
    /// `~`
    Local(Region),
}

#[derive(Clone, Copy, Debug)]
pub enum ImportTail<'a> {
    /// `import @alder/http` — binds the last segment (the package name when there
    /// are no segments). The parser rejects the two forms with nothing legal to
    /// bind: `import ~` (`Import::RootOnly`) and a reserved last segment such as
    /// `import @alder/test` (`Import::ReservedBinding(Test)`).
    Module,
    /// `as h`
    Alias(Name<'a>),
    /// `.{ get, Request as Req }`
    Names(&'a [ImportName<'a>]),
    /// `.*`
    All(Region),
}

#[derive(Clone, Copy, Debug)]
pub struct ImportName<'a> {
    pub name: Name<'a>,
    pub alias: Option<Name<'a>>,
}

#[derive(Debug)]
pub struct FnDecl<'a> {
    pub name: Name<'a>,
    pub params: &'a [Param<'a>],
    pub ret: Option<&'a Located<Type<'a>>>,
    pub where_clause: &'a [Constraint<'a>],
    /// `None` for bodiless functions (extern, trait signatures).
    pub body: Option<&'a Located<Block<'a>>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Param<'a> {
    pub mutable: Option<Region>,
    pub pattern: &'a Located<Pattern<'a>>,
    pub annotation: Option<&'a Located<Type<'a>>>,
}

#[derive(Clone, Copy, Debug)]
pub enum Constraint<'a> {
    /// `a: Show + Eq`
    Bound { var: Name<'a>, bounds: &'a [Path<'a>] },
    /// `i.Item == Number`
    AssocEq { var: Name<'a>, assoc: Name<'a>, typ: &'a Located<Type<'a>> },
}

/// `let [mut] pattern [: Type] = expr` — shared by items and statements.
#[derive(Debug)]
pub struct LetDecl<'a> {
    pub mutable: Option<Region>,
    pub pattern: &'a Located<Pattern<'a>>,
    pub annotation: Option<&'a Located<Type<'a>>>,
    pub value: &'a Located<Expr<'a>>,
}

#[derive(Debug)]
pub struct TypeAlias<'a> {
    pub name: Name<'a>,
    pub params: &'a [Name<'a>],
    pub typ: &'a Located<Type<'a>>,
}

#[derive(Debug)]
pub struct EnumDecl<'a> {
    pub name: Name<'a>,
    pub params: &'a [Name<'a>],
    pub variants: &'a [Variant<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct Variant<'a> {
    pub name: Name<'a>,
    pub payload: VariantPayload<'a>,
}

#[derive(Clone, Copy, Debug)]
pub enum VariantPayload<'a> {
    Unit,
    Tuple(&'a [&'a Located<Type<'a>>]),
    /// `Rect { width: Number }` — no `r |` extension (`Enum::VariantRecordExt`).
    Record(&'a [FieldType<'a>]),
}

#[derive(Debug)]
pub struct TraitDecl<'a> {
    pub name: Name<'a>,
    pub params: &'a [Name<'a>],
    pub where_clause: &'a [Constraint<'a>],
    pub items: &'a [TraitItem<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum TraitItem<'a> {
    AssocType(Name<'a>),
    /// `body: None` = required, `Some` = default body.
    Fn(&'a FnDecl<'a>),
}

#[derive(Debug)]
pub struct ImplDecl<'a> {
    pub trait_: Path<'a>,
    pub args: &'a [&'a Located<Type<'a>>],
    pub where_clause: &'a [Constraint<'a>],
    pub items: &'a [ImplItem<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum ImplItem<'a> {
    AssocType { name: Name<'a>, typ: &'a Located<Type<'a>> },
    Fn(&'a FnDecl<'a>),
}

#[derive(Debug)]
pub struct ErrorDecl<'a> {
    pub name: Name<'a>,
    pub tags: &'a [TagVariant<'a>],
}

/// `:tag` or `:tag(T1, T2)` — `error` groups and error-row types.
#[derive(Clone, Copy, Debug)]
pub struct TagVariant<'a> {
    /// Without the leading `:`.
    pub name: Name<'a>,
    pub args: &'a [&'a Located<Type<'a>>],
}

#[derive(Debug)]
pub struct ComponentDecl<'a> {
    /// `Counter`, or route-file `page` (either case is accepted).
    pub name: Name<'a>,
    pub params: &'a [Param<'a>],
    pub body: &'a Located<Block<'a>>,
}

#[derive(Debug)]
pub struct TableDecl<'a> {
    pub name: Name<'a>,
    pub columns: &'a [Column<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct Column<'a> {
    pub name: Name<'a>,
    pub builder: &'a Located<Expr<'a>>,
    pub modifiers: &'a [Modifier<'a>],
}

/// `primaryKey`, `default(now)`, `min(3)` — table modifiers and schema rules.
#[derive(Clone, Copy, Debug)]
pub struct Modifier<'a> {
    pub name: Name<'a>,
    pub args: &'a [&'a Located<Expr<'a>>],
}

#[derive(Debug)]
pub struct SchemaDecl<'a> {
    pub name: Name<'a>,
    pub from: Option<Name<'a>>,
    pub items: &'a [SchemaItem<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum SchemaItem<'a> {
    Pick(&'a [Name<'a>]),
    Field { name: Name<'a>, typ: Option<&'a Located<Type<'a>>>, rules: &'a [Modifier<'a>] },
}

#[derive(Debug)]
pub struct MacroDecl<'a> {
    pub name: Name<'a>,
    pub params: &'a [Name<'a>],
    /// Raw balanced body text between the braces (M5 defines quote/unquote).
    pub body: Located<&'a str>,
}

#[derive(Debug)]
pub struct TestDecl<'a> {
    pub name: Located<&'a str>,
    pub body: &'a Located<Block<'a>>,
}

// ============================================================================
// Blocks and statements
// ============================================================================

#[derive(Debug)]
pub struct Block<'a> {
    pub stmts: &'a [&'a Located<Stmt<'a>>],
    /// Trailing expression = the block's value.
    pub tail: Option<&'a Located<Expr<'a>>>,
}

#[derive(Debug)]
pub enum Stmt<'a> {
    Let(&'a LetDecl<'a>),
    Use(Path<'a>),
    /// A statement in M1, so a trailing `provide … { … }` leaves the enclosing block
    /// without a `tail` (web.md's `handle` relies on it having one — §10.40, M2 decides).
    Provide { name: Path<'a>, value: &'a Located<Expr<'a>>, body: &'a Located<Block<'a>> },
    Assign { place: &'a Located<Place<'a>>, op: Located<AssignOp>, value: &'a Located<Expr<'a>> },
    For { pattern: &'a Located<Pattern<'a>>, iter: &'a Located<Expr<'a>>, body: &'a Located<Block<'a>> },
    While { condition: &'a Located<Expr<'a>>, body: &'a Located<Block<'a>> },
    Return(Option<&'a Located<Expr<'a>>>),
    Break(Option<&'a Located<Expr<'a>>>),
    Continue,
    /// `assert expr` — compiler-known (power-assert).
    Assert(&'a Located<Expr<'a>>),
    Expr(&'a Located<Expr<'a>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
}

impl AssignOp {
    pub const fn as_str(self) -> &'static str {
        match self {
            AssignOp::Set => "=",
            AssignOp::Add => "+=",
            AssignOp::Sub => "-=",
            AssignOp::Mul => "*=",
            AssignOp::Div => "/=",
        }
    }
}

/// `lower_ident { '.' lower_ident | '.' digits | '[' expr ']' }`
#[derive(Debug)]
pub struct Place<'a> {
    pub root: Name<'a>,
    pub steps: &'a [PlaceStep<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum PlaceStep<'a> {
    Field(Name<'a>),
    TupleIndex(Located<u32>),
    Index(&'a Located<Expr<'a>>),
}

// ============================================================================
// Expressions
// ============================================================================

/// `Upper { '::' Upper }` — enum name, constructor path, trait path, component name,
/// or a module-style receiver (`Array` in `Array.map`); canonicalization decides.
#[derive(Clone, Copy, Debug)]
pub struct Path<'a> {
    /// At least one segment.
    pub segments: &'a [Name<'a>],
}

impl<'a> Path<'a> {
    pub fn region(&self) -> Region {
        let first = &self.segments[0].region;
        let last = &self.segments[self.segments.len() - 1].region;
        Region::span_across(first, last)
    }
}

/// A number literal: the JS value and its source spelling (`0xFF` → 255, "0xFF").
#[derive(Clone, Copy, Debug)]
pub struct NumberLit<'a> {
    pub value: f64,
    pub text: &'a str,
}

#[derive(Debug)]
pub enum Expr<'a> {
    // ---- literals
    Number(NumberLit<'a>),
    /// Digits without the trailing `n`.
    BigInt(&'a str),
    Str(&'a str),
    Bool(bool),
    Template(&'a [TemplatePart<'a>]),
    TaggedTemplate { tag: &'a Located<Expr<'a>>, parts: &'a [TemplatePart<'a>] },
    Unit,
    // ---- names
    Var(&'a str),
    /// `Some`, `Option::Some`, `Shape`, `Array` (in `Array.map`).
    Path(Path<'a>),
    /// `Show::show`
    PathVar { path: Path<'a>, name: Name<'a> },
    /// `:not_found(id)`; `args` empty for a bare `:timeout`.
    Tag { name: Name<'a>, args: &'a [&'a Located<Expr<'a>>] },
    /// `_` as a whole call argument.
    Placeholder,
    // ---- aggregates
    Array(&'a [&'a Located<Expr<'a>>]),
    Tuple { first: &'a Located<Expr<'a>>, second: &'a Located<Expr<'a>>, rest: &'a [&'a Located<Expr<'a>>] },
    Record(&'a [RecordField<'a>]),
    /// `Shape::Rect { width: 1, height: 2 }`
    RecordCtor { path: Path<'a>, fields: &'a [RecordField<'a>] },
    // ---- postfix
    Call { function: &'a Located<Expr<'a>>, arguments: &'a [&'a Located<Expr<'a>>] },
    Access { record: &'a Located<Expr<'a>>, field: Name<'a> },
    TupleAccess { tuple: &'a Located<Expr<'a>>, index: Located<u32> },
    Index { target: &'a Located<Expr<'a>>, index: &'a Located<Expr<'a>> },
    Await(&'a Located<Expr<'a>>),
    /// `expr?`
    Try(&'a Located<Expr<'a>>),
    // ---- prefix
    Negate(&'a Located<Expr<'a>>),
    Not(&'a Located<Expr<'a>>),
    /// `^expr` inside `query { }`. The operand is a whole postfix chain:
    /// `^user.id` pins `user.id`, `^f(x)` pins the call (§10.20).
    Pin(&'a Located<Expr<'a>>),
    // ---- operators: flat chain, precedence resolved in canonicalization (as Elm)
    BinOps { operands: &'a [BinOpOperand<'a>], last: &'a Located<Expr<'a>> },
    // ---- control
    Block(&'a Located<Block<'a>>),
    Lambda(&'a Lambda<'a>),
    If { branches: &'a [IfBranch<'a>], final_else: Option<&'a Located<Block<'a>>> },
    Match { scrutinee: &'a Located<Expr<'a>>, arms: &'a [MatchArm<'a>] },
    Loop(&'a Located<Block<'a>>),
    // ---- framework
    State(&'a Located<Expr<'a>>),
    Style(&'a Style<'a>),
    Query(&'a Query<'a>),
    Markup(&'a Markup<'a>),
    /// `name!( … )` — raw balanced token text until M5.
    MacroCall { name: Name<'a>, tokens: Located<&'a str> },
}

#[derive(Clone, Copy, Debug)]
pub enum TemplatePart<'a> {
    Text(&'a str),
    Expr(&'a Located<Expr<'a>>),
}

#[derive(Clone, Copy, Debug)]
pub enum RecordField<'a> {
    /// `name: value`, or shorthand `name` (`value == None`).
    Field { name: Name<'a>, value: Option<&'a Located<Expr<'a>>> },
    /// `..expr`
    Spread(&'a Located<Expr<'a>>),
}

#[derive(Clone, Copy, Debug)]
pub struct BinOpOperand<'a> {
    pub expr: &'a Located<Expr<'a>>,
    pub op: Located<BinOp>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    /// `??`
    Coalesce,
    /// `|>`
    Pipe,
    /// `in` — query blocks only
    In,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Associativity {
    Left,
    None,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Precedence(pub u16);

impl BinOp {
    /// Fixed table; alder-can's binop resolution reads this instead of `infix`
    /// declarations. Higher binds tighter.
    pub const fn precedence(self) -> (Precedence, Associativity) {
        use Associativity::*;
        match self {
            BinOp::Pipe => (Precedence(0), Left),
            BinOp::Coalesce => (Precedence(1), Right),
            BinOp::Or => (Precedence(2), Left),
            BinOp::And => (Precedence(3), Left),
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq | BinOp::In => {
                (Precedence(4), None)
            }
            BinOp::Add | BinOp::Sub => (Precedence(6), Left),
            BinOp::Mul | BinOp::Div | BinOp::Rem => (Precedence(7), Left),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Eq => "==",
            BinOp::NotEq => "!=",
            BinOp::Lt => "<",
            BinOp::LtEq => "<=",
            BinOp::Gt => ">",
            BinOp::GtEq => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
            BinOp::Coalesce => "??",
            BinOp::Pipe => "|>",
            BinOp::In => "in",
        }
    }
}

#[derive(Debug)]
pub struct Lambda<'a> {
    pub params: &'a [Param<'a>],
    pub ret: Option<&'a Located<Type<'a>>>,
    /// `{ … }` bodies are `Expr::Block`; an assignment body (`fn() count += 1`)
    /// is wrapped as a one-statement block with no tail.
    pub body: &'a Located<Expr<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct IfBranch<'a> {
    pub condition: &'a Located<Expr<'a>>,
    pub body: &'a Located<Block<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct MatchArm<'a> {
    /// `p1 | p2`
    pub patterns: &'a [&'a Located<Pattern<'a>>],
    pub guard: Option<&'a Located<Expr<'a>>>,
    /// `Expr::Block` when braced.
    pub body: &'a Located<Expr<'a>>,
}

// ============================================================================
// Style
// ============================================================================

#[derive(Debug)]
pub struct Style<'a> {
    pub entries: &'a [StyleEntry<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct StyleEntry<'a> {
    pub key: Located<StyleKey<'a>>,
    pub value: StyleValue<'a>,
}

#[derive(Clone, Copy, Debug)]
pub enum StyleKey<'a> {
    Ident(&'a str),
    /// `":hover"`, `"@media (max-width: 600px)"`
    Str(&'a str),
}

#[derive(Clone, Copy, Debug)]
pub enum StyleValue<'a> {
    /// `16px`, `1.5rem`, `100%`, `-8px` — a number immediately followed by a unit.
    /// A leading `-` negates `value` and is kept in `text` (`"-8"`).
    Dimension { number: NumberLit<'a>, unit: &'a str },
    Expr(&'a Located<Expr<'a>>),
    Nested(&'a Style<'a>),
}

// ============================================================================
// Queries
// ============================================================================

#[derive(Debug)]
pub enum Query<'a> {
    Select(&'a Select<'a>),
    /// `values` must be an `Expr::Pin` (checked by the parser: `Insert::Pin`).
    Insert { table: Name<'a>, values: &'a Located<Expr<'a>> },
    Update { table: Name<'a>, set: &'a [RecordField<'a>], where_: Option<&'a Located<Expr<'a>>> },
    Delete { table: Name<'a>, where_: Option<&'a Located<Expr<'a>>> },
}

#[derive(Debug)]
pub struct Select<'a> {
    pub projection: Projection<'a>,
    pub from: TableRef<'a>,
    pub joins: &'a [Join<'a>],
    pub where_: Option<&'a Located<Expr<'a>>>,
    pub group_by: &'a [&'a Located<Expr<'a>>],
    pub order_by: &'a [Order<'a>],
    pub limit: Option<&'a Located<Expr<'a>>>,
    pub offset: Option<&'a Located<Expr<'a>>>,
}

#[derive(Clone, Copy, Debug)]
pub enum Projection<'a> {
    Star(Region),
    Fields(&'a [&'a Located<Expr<'a>>]),
}

#[derive(Clone, Copy, Debug)]
pub struct TableRef<'a> {
    pub name: Name<'a>,
    pub alias: Option<Name<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Join<'a> {
    pub kind: Located<JoinKind>,
    pub table: TableRef<'a>,
    pub on: &'a Located<Expr<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinKind {
    /// bare `join`
    Plain,
    Inner,
    Left,
}

#[derive(Clone, Copy, Debug)]
pub struct Order<'a> {
    pub expr: &'a Located<Expr<'a>>,
    pub direction: Option<Located<OrderDir>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderDir {
    Asc,
    Desc,
}

// ============================================================================
// Markup
// ============================================================================

/// What `<…>` in expression position produces.
#[derive(Debug)]
pub enum Markup<'a> {
    Element(&'a Element<'a>),
    /// `<> … </>`
    Fragment(&'a [&'a Located<Child<'a>>]),
}

#[derive(Debug)]
pub struct Element<'a> {
    pub name: Located<ElementName<'a>>,
    pub attrs: &'a [Attr<'a>],
    pub children: &'a [&'a Located<Child<'a>>],
    pub self_closing: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum ElementName<'a> {
    /// `div`, `box`, `my-widget`, and keyword-shaped names like `table`, `style` (§2.4)
    Tag(&'a str),
    /// `Spinner`, `Ui::Button`
    Component(Path<'a>),
}

#[derive(Clone, Copy, Debug)]
pub struct Attr<'a> {
    /// May contain `-` (`aria-label`) and may be a reserved word (`type`, `for`; §2.4).
    pub name: Name<'a>,
    /// `None` for a boolean attribute (`<text bold>`).
    pub value: Option<AttrValue<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub enum AttrValue<'a> {
    Str(Located<&'a str>),
    Expr(&'a Located<Expr<'a>>),
}

#[derive(Debug)]
pub enum Child<'a> {
    Element(&'a Element<'a>),
    Fragment(&'a [&'a Located<Child<'a>>]),
    /// Raw text; whitespace-only runs containing a newline are dropped by the parser.
    Text(&'a str),
    /// `{expr}`
    Hole(&'a Located<Expr<'a>>),
    If { branches: &'a [ChildIfBranch<'a>], final_else: Option<&'a Located<ChildBlock<'a>>> },
    For {
        pattern: &'a Located<Pattern<'a>>,
        iter: &'a Located<Expr<'a>>,
        key: Option<&'a Located<Expr<'a>>>,
        body: &'a Located<ChildBlock<'a>>,
        empty: Option<&'a Located<ChildBlock<'a>>>,
    },
    Match { scrutinee: &'a Located<Expr<'a>>, arms: &'a [ChildMatchArm<'a>] },
}

#[derive(Clone, Copy, Debug)]
pub struct ChildIfBranch<'a> {
    pub condition: &'a Located<Expr<'a>>,
    pub body: &'a Located<ChildBlock<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct ChildMatchArm<'a> {
    pub patterns: &'a [&'a Located<Pattern<'a>>],
    pub guard: Option<&'a Located<Expr<'a>>>,
    /// A bare `child` body is stored as a one-item block.
    pub body: &'a Located<ChildBlock<'a>>,
}

/// `{ … }` after a directive head: setup statements and children.
#[derive(Debug)]
pub struct ChildBlock<'a> {
    pub items: &'a [ChildItem<'a>],
}

#[derive(Clone, Copy, Debug)]
pub enum ChildItem<'a> {
    /// Only `let` / `let mut` and `use` are recognized here.
    Stmt(&'a Located<Stmt<'a>>),
    Child(&'a Located<Child<'a>>),
}

// ============================================================================
// Patterns
// ============================================================================

#[derive(Debug)]
pub enum Pattern<'a> {
    Anything,
    Var(&'a str),
    /// `^expr` — compare against an existing value.
    Pin(&'a Located<Expr<'a>>),
    /// `-1` allowed; `text` keeps the sign.
    Number(NumberLit<'a>),
    BigInt(&'a str),
    Str(&'a str),
    Bool(bool),
    Unit,
    /// `None`, `Some(x)`, `Option::Some(x)`
    Ctor { path: Path<'a>, args: &'a [&'a Located<Pattern<'a>>] },
    /// `Rect { width, height: h, .. }`
    CtorRecord { path: Path<'a>, fields: &'a [FieldPattern<'a>], rest: Option<Region> },
    /// `:not_found(id)`
    Tag { name: Name<'a>, args: &'a [&'a Located<Pattern<'a>>] },
    Tuple { first: &'a Located<Pattern<'a>>, second: &'a Located<Pattern<'a>>, rest: &'a [&'a Located<Pattern<'a>>] },
    /// `[a, b, ..rest]`, `[a, ..]`
    Array { elements: &'a [&'a Located<Pattern<'a>>], rest: Option<ArrayRest<'a>> },
    Record { fields: &'a [FieldPattern<'a>], rest: Option<Region> },
    Alias { pattern: &'a Located<Pattern<'a>>, name: Name<'a> },
}

#[derive(Clone, Copy, Debug)]
pub struct ArrayRest<'a> {
    /// Region of `..` (plus the name when present).
    pub region: Region,
    pub name: Option<Name<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct FieldPattern<'a> {
    pub name: Name<'a>,
    /// `None` = shorthand binding of the field name.
    pub pattern: Option<&'a Located<Pattern<'a>>>,
}

// ============================================================================
// Types
// ============================================================================

#[derive(Debug)]
pub enum Type<'a> {
    /// `a`, and applied higher-kinded variables `f[a]`, `t[f[a]]`.
    Var { name: &'a str, args: &'a [&'a Located<Type<'a>>] },
    /// `User`, `Map[String, Array[User]]`, `Option::Foo`
    Named { path: Path<'a>, args: &'a [&'a Located<Type<'a>>] },
    Fn { params: &'a [&'a Located<Type<'a>>], ret: &'a Located<Type<'a>> },
    Unit,
    Tuple { first: &'a Located<Type<'a>>, second: &'a Located<Type<'a>>, rest: &'a [&'a Located<Type<'a>>] },
    /// `{ r | name: String, nickname?: String }`
    Record { fields: &'a [FieldType<'a>], ext: Option<Name<'a>> },
    /// `[:not_found(Id) | :timeout | r]`
    ErrorRow { tags: &'a [TagVariant<'a>], ext: Option<Name<'a>> },
}

#[derive(Clone, Copy, Debug)]
pub struct FieldType<'a> {
    pub field: Name<'a>,
    /// Region of the `?`.
    pub optional: Option<Region>,
    pub typ: &'a Located<Type<'a>>,
}
```

Deleted from today's alder-source: `Value`, `Union`, `Ctor`, `Alias`,
`Infix`, `VarType`, `Def`, `CaseArm`, `FieldAssign`, `Docs`, `Comment`,
`Snippet`, `Exposing`, `Exposed`, `Privacy`, `Pattern::{Cons, List,
CtorQual}`, `Type::{Lambda, TypeQual}`, `Expr::{Int, Op, VarQual, Let,
Case, Accessor, Update, List}`. `Associativity` and `Precedence` survive
(alder-ast imports them; alder-ast still breaks on `Docs` until M2).

---

## 4. `crates/alder-parse/src/error.rs`

One enum per construct, nested through `&'a`, positions as trailing
`Row, Col` at the position the failing sub-parser started (Elm's
convention), leaf enums lifetime-free. `Keyword` and `SqlWord` (from
`keyword.rs`) let messages name the misused word.

```rust
//! Syntax error types for the Alder parser.
//!
//! Modeled on Elm's `Reporting/Error/Syntax.hs`: one enum per construct,
//! nested through `&'a` so a leaf error carries its full context.
//! `Expr`, `Pattern`, `Type` … here are ERROR types, not AST types.

use crate::keyword::{Keyword, SqlWord};
use crate::{Col, Row};
use alder_source::{AssignOp, BinOp};

// ============================================================================
// Top level
// ============================================================================

#[derive(Debug)]
pub enum Error<'a> {
    ParseError(&'a Module<'a>),
}

#[derive(Debug)]
pub enum Module<'a> {
    Item(&'a Item<'a>, Row, Col),
    /// A second item on the same line as the previous one (§2.1 rule 3).
    SameLine(Row, Col),
    /// Something that is not an item start after the last item (e.g. a stray `}`).
    BadEnd(Row, Col),
}

// ============================================================================
// Items
// ============================================================================

#[derive(Debug)]
pub enum Item<'a> {
    /// Not an item keyword.
    Start(Row, Col),
    /// `pub` followed by something that is not an item.
    AfterPub(Row, Col),
    /// `;` where an item was expected — items are separated by line breaks, not `;`.
    Semicolon(Row, Col),
    Attribute(&'a Attribute<'a>, Row, Col),
    Import(&'a Import<'a>, Row, Col),
    Fn(&'a Fn<'a>, Row, Col),
    Let(&'a Let<'a>, Row, Col),
    TypeAlias(&'a TypeAlias<'a>, Row, Col),
    Enum(&'a Enum<'a>, Row, Col),
    Trait(&'a Trait<'a>, Row, Col),
    Impl(&'a Impl<'a>, Row, Col),
    ErrorDecl(&'a ErrorDecl<'a>, Row, Col),
    Component(&'a Component<'a>, Row, Col),
    Table(&'a Table<'a>, Row, Col),
    Schema(&'a Schema<'a>, Row, Col),
    Macro(Macro, Row, Col),
    Comptime(&'a Block<'a>, Row, Col),
    Test(&'a Test<'a>, Row, Col),
    Tests(&'a Tests<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Attribute<'a> {
    /// `#` not followed by `[`.
    Open(Row, Col),
    Name(Row, Col),
    Arg(&'a Expr<'a>, Row, Col),
    /// Expected `,` or `)`.
    ArgEnd(Row, Col),
    /// Expected `]`.
    End(Row, Col),
    /// Attribute followed by EOF or `}`.
    Dangling(Row, Col),
}

#[derive(Debug)]
pub enum Import<'a> {
    Path(&'a ModulePath, Row, Col),
    /// After `.`: expected `{` or `*`.
    Tail(Row, Col),
    /// Inside `{ }`: expected a name.
    Name(Row, Col),
    /// `as` inside `{ }` not followed by a name.
    NameAlias(Row, Col),
    /// Expected `,` or `}`.
    NamesEnd(Row, Col),
    /// `as` not followed by a lowercase name.
    Alias(Row, Col),
    /// `pub import @x/y` without `.{ … }` or `.*`.
    PubNeedsNames(Row, Col),
    /// Bare `import @alder/test`: the last segment is a reserved word, so it cannot
    /// be bound — write `as name` or `.{ … }`. Position of the segment.
    ReservedBinding(Keyword, Row, Col),
    /// Bare `import ~`: no segment to bind — write `as name`, `.{ … }` or `.*`.
    RootOnly(Row, Col),
}

/// Segments are keyword-insensitive (`raw_lower`, §2.4): only their shape can fail.
#[derive(Debug)]
pub enum ModulePath {
    /// Expected `@` or `~`.
    Start(Row, Col),
    Author(Row, Col),
    Slash(Row, Col),
    Package(Row, Col),
    /// `/` not followed by a lowercase name.
    Segment(Row, Col),
}

#[derive(Debug)]
pub enum Fn<'a> {
    Name(Row, Col),
    Params(&'a Params<'a>, Row, Col),
    Ret(&'a Type<'a>, Row, Col),
    Where(&'a Where<'a>, Row, Col),
    Body(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Params<'a> {
    /// Expected `(`.
    Open(Row, Col),
    Pattern(&'a Pattern<'a>, Row, Col),
    /// Type after `:`.
    Type(&'a Type<'a>, Row, Col),
    /// Expected `,` or `)`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Where<'a> {
    /// Expected a lowercase type variable.
    Var(Row, Col),
    /// Expected `:` or `.Assoc ==`.
    Colon(Row, Col),
    /// Expected a trait path.
    Bound(Row, Col),
    AssocName(Row, Col),
    AssocEq(Row, Col),
    Type(&'a Type<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Let<'a> {
    Pattern(&'a Pattern<'a>, Row, Col),
    Type(&'a Type<'a>, Row, Col),
    Equals(Row, Col),
    Body(&'a Expr<'a>, Row, Col),
}

#[derive(Debug)]
pub enum TypeAlias<'a> {
    Name(Row, Col),
    Params(&'a TypeParams, Row, Col),
    /// After `=`.
    Body(&'a Type<'a>, Row, Col),
}

#[derive(Debug)]
pub enum TypeParams {
    /// Expected `[` — reported only by callers that require parameters (`trait`).
    Open(Row, Col),
    Var(Row, Col),
    /// Expected `,` or `]`.
    End(Row, Col),
    /// `[]`
    Empty(Row, Col),
}

#[derive(Debug)]
pub enum Enum<'a> {
    Name(Row, Col),
    Params(&'a TypeParams, Row, Col),
    Open(Row, Col),
    /// Expected an uppercase variant name.
    Variant(Row, Col),
    VariantArg(&'a Type<'a>, Row, Col),
    VariantArgEnd(Row, Col),
    VariantRecord(&'a TRecord<'a>, Row, Col),
    /// `Rect { r | width: Number }` — record payloads take no extension. Position of `r`.
    VariantRecordExt(Row, Col),
    /// Expected `,` or `}`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Trait<'a> {
    Name(Row, Col),
    Params(&'a TypeParams, Row, Col),
    Where(&'a Where<'a>, Row, Col),
    Open(Row, Col),
    /// Expected `type`, `fn` or `}`.
    Item(Row, Col),
    /// A second item on the same line as the previous one (§2.1 rule 3).
    SameLine(Row, Col),
    /// `;` after an item — trait items are separated by line breaks, not `;`.
    Semicolon(Row, Col),
    AssocType(Row, Col),
    /// `type Item = …` inside a trait.
    AssocTypeHasBody(Row, Col),
    Fn(&'a Fn<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Impl<'a> {
    /// Expected a trait path.
    Trait(Row, Col),
    /// Expected `[`.
    Open(Row, Col),
    Arg(&'a Type<'a>, Row, Col),
    ArgEnd(Row, Col),
    Where(&'a Where<'a>, Row, Col),
    BodyOpen(Row, Col),
    /// Expected `type`, `fn` or `}`.
    Item(Row, Col),
    /// A second item on the same line as the previous one (§2.1 rule 3).
    SameLine(Row, Col),
    /// `;` after an item — impl items are separated by line breaks, not `;`.
    Semicolon(Row, Col),
    AssocType(Row, Col),
    AssocEquals(Row, Col),
    AssocBody(&'a Type<'a>, Row, Col),
    Fn(&'a Fn<'a>, Row, Col),
}

#[derive(Debug)]
pub enum ErrorDecl<'a> {
    Name(Row, Col),
    Open(Row, Col),
    Tag(&'a TagVariant<'a>, Row, Col),
    /// Expected `,` or `}`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum TagVariant<'a> {
    /// `:` not followed by a lowercase name.
    Name(Row, Col),
    Arg(&'a Type<'a>, Row, Col),
    ArgEnd(Row, Col),
}

#[derive(Debug)]
pub enum Component<'a> {
    Name(Row, Col),
    Params(&'a Params<'a>, Row, Col),
    Body(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Table<'a> {
    Name(Row, Col),
    Open(Row, Col),
    /// Expected a column name.
    Column(Row, Col),
    Colon(Row, Col),
    Builder(&'a Expr<'a>, Row, Col),
    ModifierArg(&'a Expr<'a>, Row, Col),
    ModifierArgEnd(Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum Schema<'a> {
    Name(Row, Col),
    /// `from` not followed by a table name.
    From(Row, Col),
    Open(Row, Col),
    /// Expected `pick`, a field name, or `}`.
    Item(Row, Col),
    PickName(Row, Col),
    Colon(Row, Col),
    Type(&'a Type<'a>, Row, Col),
    Rule(Row, Col),
    RuleArg(&'a Expr<'a>, Row, Col),
    RuleArgEnd(Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum Macro {
    Name(Row, Col),
    Param(Row, Col),
    ParamEnd(Row, Col),
    /// `{` expected, or raw body problem.
    Body(RawTokens, Row, Col),
}

#[derive(Debug)]
pub enum Test<'a> {
    /// `test` not followed by a string.
    Name(Row, Col),
    NameString(StringError, Row, Col),
    Body(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Tests<'a> {
    Open(Row, Col),
    Item(&'a Item<'a>, Row, Col),
    /// A second item on the same line as the previous one (§2.1 rule 3).
    SameLine(Row, Col),
    End(Row, Col),
}

// ============================================================================
// Blocks and statements
// ============================================================================

#[derive(Debug)]
pub enum Block<'a> {
    Open(Row, Col),
    Stmt(&'a Stmt<'a>, Row, Col),
    /// A second statement on the same line as the previous one.
    SameLine(Row, Col),
    /// `{ name: …` in block position — probably a record; wrap it in parentheses.
    LooksLikeRecord(Row, Col),
    /// Expected a statement or `}`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Stmt<'a> {
    Let(&'a Let<'a>, Row, Col),
    /// `use` not followed by a path.
    Use(Row, Col),
    Provide(&'a Provide<'a>, Row, Col),
    For(&'a For<'a>, Row, Col),
    While(&'a While<'a>, Row, Col),
    Return(&'a Expr<'a>, Row, Col),
    Break(&'a Expr<'a>, Row, Col),
    Assert(&'a Expr<'a>, Row, Col),
    Expr(&'a Expr<'a>, Row, Col),
    /// Left side of the assignment operator is not a place
    /// (renderer: for `/=` mention `!=`).
    AssignTarget(AssignOp, Row, Col),
    AssignValue(&'a Expr<'a>, Row, Col),
    /// Statements are not `;`-terminated.
    Semicolon(Row, Col),
}

#[derive(Debug)]
pub enum Provide<'a> {
    Name(Row, Col),
    Equals(Row, Col),
    Value(&'a Expr<'a>, Row, Col),
    Body(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum For<'a> {
    Pattern(&'a Pattern<'a>, Row, Col),
    In(Row, Col),
    Iter(&'a Expr<'a>, Row, Col),
    Body(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum While<'a> {
    Condition(&'a Expr<'a>, Row, Col),
    Body(&'a Block<'a>, Row, Col),
}

// ============================================================================
// Expressions
// ============================================================================

#[derive(Debug)]
pub enum Expr<'a> {
    Start(Row, Col),
    /// A reserved word where an expression was expected (`else`, `match` …).
    Reserved(Keyword, Row, Col),
    /// Inside `query { }`: a SQL word used as a value (`where limit > 3`).
    SqlKeyword(SqlWord, Row, Col),
    Number(Number, Row, Col),
    String(StringError, Row, Col),
    Template(&'a Template<'a>, Row, Col),
    TaggedTemplate(&'a Template<'a>, Row, Col),
    Array(&'a Array<'a>, Row, Col),
    Tuple(&'a Tuple<'a>, Row, Col),
    Record(&'a Record<'a>, Row, Col),
    RecordCtor(&'a Record<'a>, Row, Col),
    Block(&'a Block<'a>, Row, Col),
    Lambda(&'a Lambda<'a>, Row, Col),
    If(&'a If<'a>, Row, Col),
    Match(&'a Match<'a>, Row, Col),
    Loop(&'a Block<'a>, Row, Col),
    Call(&'a Call<'a>, Row, Col),
    Index(&'a Index<'a>, Row, Col),
    Tag(&'a Tag<'a>, Row, Col),
    State(&'a State<'a>, Row, Col),
    Style(&'a Style<'a>, Row, Col),
    Query(&'a Query<'a>, Row, Col),
    Markup(&'a Markup<'a>, Row, Col),
    MacroCall(RawTokens, Row, Col),
    /// `::` not followed by a name.
    PathMember(Row, Col),
    /// `.` not followed by a field name, digits or `await`.
    Access(Row, Col),
    /// Missing operand after `-` or `!`: `postfix()` failed with `Start` at the
    /// operand position. Any other operand error propagates unchanged (§6.0).
    Unary(Row, Col),
    /// `^` outside `query { }` and patterns.
    PinOutsideQuery(Row, Col),
    /// `_` anywhere but as a whole call argument.
    Placeholder(Row, Col),
    OperatorReserved(BadOperator, Row, Col),
    /// Operator with no right operand: `unary()` failed with `Start` at the
    /// operand position. Any other operand error propagates unchanged (§6.0).
    OperatorRight(BinOp, Row, Col),
    /// `</` in expression position.
    UnexpectedClose(Row, Col),
}

#[derive(Debug)]
pub enum Template<'a> {
    /// Position of the opening backtick.
    Endless(Row, Col),
    Escape(Escape, Row, Col),
    /// `${}`
    HoleEmpty(Row, Col),
    HoleExpr(&'a Expr<'a>, Row, Col),
    /// `${ expr` not followed by `}`.
    HoleEnd(Row, Col),
}

#[derive(Debug)]
pub enum Array<'a> {
    Expr(&'a Expr<'a>, Row, Col),
    /// Expected `,` or `]`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Tuple<'a> {
    Expr(&'a Expr<'a>, Row, Col),
    /// Expected `,` or `)`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Record<'a> {
    /// Expected a field name or `..`.
    Field(Row, Col),
    Spread(&'a Expr<'a>, Row, Col),
    Expr(&'a Expr<'a>, Row, Col),
    /// Expected `,` or `}`.
    End(Row, Col),
    /// `{ x = 1 }` (Elm habit).
    EqualsNotColon(Row, Col),
}

#[derive(Debug)]
pub enum Lambda<'a> {
    Params(&'a Params<'a>, Row, Col),
    Ret(&'a Type<'a>, Row, Col),
    Body(&'a Expr<'a>, Row, Col),
    Block(&'a Block<'a>, Row, Col),
    /// `fn() x +=` with no value.
    AssignValue(&'a Expr<'a>, Row, Col),
}

#[derive(Debug)]
pub enum If<'a> {
    Condition(&'a Expr<'a>, Row, Col),
    Then(&'a Block<'a>, Row, Col),
    /// `if x then` (Elm habit).
    ThenKeyword(Row, Col),
    /// `else` not followed by `if` or `{`.
    ElseBranchStart(Row, Col),
    Else(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Match<'a> {
    Scrutinee(&'a Expr<'a>, Row, Col),
    /// Expected `{` (renderer hint if `of` is found).
    Open(Row, Col),
    Arm(&'a Arm<'a>, Row, Col),
    /// Expected `,`, a pattern, or `}`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Arm<'a> {
    Pattern(&'a Pattern<'a>, Row, Col),
    Guard(&'a Expr<'a>, Row, Col),
    /// Expected `=>` (renderer hint if `->` is found).
    Arrow(Row, Col),
    Body(&'a Expr<'a>, Row, Col),
    Block(&'a Block<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Call<'a> {
    Arg(&'a Expr<'a>, Row, Col),
    /// Expected `,` or `)`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Index<'a> {
    Expr(&'a Expr<'a>, Row, Col),
    /// Expected `]`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Tag<'a> {
    /// `:` not followed by a lowercase name.
    Name(Row, Col),
    Arg(&'a Expr<'a>, Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum State<'a> {
    /// `state` not followed by `(`.
    Open(Row, Col),
    Expr(&'a Expr<'a>, Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum Style<'a> {
    Open(Row, Col),
    Key(Row, Col),
    KeyString(StringError, Row, Col),
    Colon(Row, Col),
    Value(&'a Expr<'a>, Row, Col),
    Dimension(Number, Row, Col),
    Nested(&'a Style<'a>, Row, Col),
    /// Expected `,` or `}`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Query<'a> {
    Open(Row, Col),
    /// Expected `select`, `insert`, `update`, or `delete`.
    Verb(Row, Col),
    Select(&'a Select<'a>, Row, Col),
    Insert(&'a Insert<'a>, Row, Col),
    Update(&'a Update<'a>, Row, Col),
    Delete(&'a Delete<'a>, Row, Col),
    /// A clause that is out of order or repeated (`where` after `orderBy`, a second
    /// `limit`). Carries `Clause`, not `SqlWord`: `where` is a `Keyword`, not a SQL word.
    ClauseOrder(Clause, Row, Col),
    /// Expected a clause or `}`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum Select<'a> {
    /// Expected `{` or `*`.
    Projection(Row, Col),
    ProjectionExpr(&'a Expr<'a>, Row, Col),
    ProjectionEnd(Row, Col),
    From(Row, Col),
    Table(TableRef, Row, Col),
    Join(&'a Join<'a>, Row, Col),
    Where(&'a Expr<'a>, Row, Col),
    GroupBy(&'a Expr<'a>, Row, Col),
    OrderBy(&'a Expr<'a>, Row, Col),
    Limit(&'a Expr<'a>, Row, Col),
    Offset(&'a Expr<'a>, Row, Col),
}

#[derive(Debug)]
pub enum TableRef {
    Name(Row, Col),
    /// `as` not followed by a name.
    Alias(Row, Col),
}

#[derive(Debug)]
pub enum Join<'a> {
    /// `left` / `inner` not followed by `join`.
    Keyword(Row, Col),
    Table(TableRef, Row, Col),
    On(Row, Col),
    Condition(&'a Expr<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Insert<'a> {
    Into(Row, Col),
    Table(Row, Col),
    Values(Row, Col),
    /// `values` operand is not `^…`.
    Pin(Row, Col),
    Value(&'a Expr<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Update<'a> {
    Table(Row, Col),
    Set(Row, Col),
    Record(&'a Record<'a>, Row, Col),
    Where(&'a Expr<'a>, Row, Col),
}

#[derive(Debug)]
pub enum Delete<'a> {
    From(Row, Col),
    Table(Row, Col),
    Where(&'a Expr<'a>, Row, Col),
}

// ============================================================================
// Markup
// ============================================================================

#[derive(Debug)]
pub enum Markup<'a> {
    /// `<` not followed by an element name.
    Name(Row, Col),
    Attr(&'a Attr<'a>, Row, Col),
    /// Expected an attribute, `>` or `/>`.
    TagEnd(Row, Col),
    Child(&'a Child<'a>, Row, Col),
    /// `</` not followed by a name.
    CloseName(Row, Col),
    CloseMismatch { expected: &'a str, found: &'a str, row: Row, col: Col },
    /// `</name` not followed by `>`.
    CloseEnd(Row, Col),
    /// EOF before the closing tag; position of the opening tag.
    Unclosed { name: &'a str, row: Row, col: Col },
    /// EOF before `</>`.
    FragmentUnclosed(Row, Col),
}

#[derive(Debug)]
pub enum Attr<'a> {
    /// `=` not followed by a string or `{`.
    Value(Row, Col),
    String(StringError, Row, Col),
    Expr(&'a Expr<'a>, Row, Col),
    /// Expected `}`.
    ExprEnd(Row, Col),
}

#[derive(Debug)]
pub enum Child<'a> {
    /// `{}`
    HoleEmpty(Row, Col),
    Hole(&'a Expr<'a>, Row, Col),
    HoleEnd(Row, Col),
    /// A bare `}` in text (write `{"}"}`).
    StrayBrace(Row, Col),
    Element(&'a Markup<'a>, Row, Col),
    If(&'a DirIf<'a>, Row, Col),
    For(&'a DirFor<'a>, Row, Col),
    Match(&'a DirMatch<'a>, Row, Col),
    /// `@word` that is not if/for/match.
    UnknownDirective(Row, Col),
    /// `@else` with no preceding `@if`.
    StrayElse(Row, Col),
    /// `@empty` with no preceding `@for`.
    StrayEmpty(Row, Col),
    /// `let` / `use` inside a child block.
    Stmt(&'a Stmt<'a>, Row, Col),
}

#[derive(Debug)]
pub enum DirIf<'a> {
    Condition(&'a Expr<'a>, Row, Col),
    Body(&'a ChildBlock<'a>, Row, Col),
    /// `@else` not followed by `if` or `{`.
    ElseBranchStart(Row, Col),
    Else(&'a ChildBlock<'a>, Row, Col),
}

#[derive(Debug)]
pub enum DirFor<'a> {
    Pattern(&'a Pattern<'a>, Row, Col),
    In(Row, Col),
    Iter(&'a Expr<'a>, Row, Col),
    /// `;` not followed by `key`.
    Key(Row, Col),
    KeyExpr(&'a Expr<'a>, Row, Col),
    Body(&'a ChildBlock<'a>, Row, Col),
    Empty(&'a ChildBlock<'a>, Row, Col),
}

#[derive(Debug)]
pub enum DirMatch<'a> {
    Scrutinee(&'a Expr<'a>, Row, Col),
    Open(Row, Col),
    Pattern(&'a Pattern<'a>, Row, Col),
    Guard(&'a Expr<'a>, Row, Col),
    Arrow(Row, Col),
    Body(&'a Child<'a>, Row, Col),
    /// Bare text after `=>` (would swallow the next arm); wrap it in `{ }`.
    BareText(Row, Col),
    Block(&'a ChildBlock<'a>, Row, Col),
    /// Expected `,`, a pattern, or `}`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum ChildBlock<'a> {
    Open(Row, Col),
    Item(&'a Child<'a>, Row, Col),
    End(Row, Col),
}

// ============================================================================
// Patterns
// ============================================================================

#[derive(Debug)]
pub enum Pattern<'a> {
    Start(Row, Col),
    Reserved(Keyword, Row, Col),
    Number(Number, Row, Col),
    String(StringError, Row, Col),
    Pin(&'a Expr<'a>, Row, Col),
    /// `::` not followed by a name.
    PathMember(Row, Col),
    Ctor(&'a PCtor<'a>, Row, Col),
    Tag(&'a PCtor<'a>, Row, Col),
    /// `:` not followed by a lowercase name.
    TagName(Row, Col),
    Tuple(&'a PTuple<'a>, Row, Col),
    Array(&'a PArray<'a>, Row, Col),
    Record(&'a PRecord<'a>, Row, Col),
    /// `as` not followed by a lowercase name.
    Alias(Row, Col),
    /// `_foo` (name, width).
    WildcardNotVar(&'a str, i32, Row, Col),
}

#[derive(Debug)]
pub enum PCtor<'a> {
    Arg(&'a Pattern<'a>, Row, Col),
    /// Expected `,` or `)`.
    End(Row, Col),
    Record(&'a PRecord<'a>, Row, Col),
}

#[derive(Debug)]
pub enum PTuple<'a> {
    Pattern(&'a Pattern<'a>, Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum PArray<'a> {
    Pattern(&'a Pattern<'a>, Row, Col),
    /// `..` must be last.
    RestNotLast(Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum PRecord<'a> {
    /// Expected a field name or `..`.
    Field(Row, Col),
    Pattern(&'a Pattern<'a>, Row, Col),
    RestNotLast(Row, Col),
    End(Row, Col),
}

// ============================================================================
// Types
// ============================================================================

#[derive(Debug)]
pub enum Type<'a> {
    Start(Row, Col),
    Reserved(Keyword, Row, Col),
    PathMember(Row, Col),
    Args(&'a TArgs<'a>, Row, Col),
    Fn(&'a TFn<'a>, Row, Col),
    Tuple(&'a TTuple<'a>, Row, Col),
    Record(&'a TRecord<'a>, Row, Col),
    ErrorRow(&'a TErrorRow<'a>, Row, Col),
}

#[derive(Debug)]
pub enum TArgs<'a> {
    Type(&'a Type<'a>, Row, Col),
    /// `Array[]`
    Empty(Row, Col),
    /// Expected `,` or `]`.
    End(Row, Col),
}

#[derive(Debug)]
pub enum TFn<'a> {
    Open(Row, Col),
    Param(&'a Type<'a>, Row, Col),
    ParamEnd(Row, Col),
    Arrow(Row, Col),
    Ret(&'a Type<'a>, Row, Col),
}

#[derive(Debug)]
pub enum TTuple<'a> {
    Type(&'a Type<'a>, Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum TRecord<'a> {
    Field(Row, Col),
    /// Expected `:` or `?:` (or `|` after the first name).
    Colon(Row, Col),
    Type(&'a Type<'a>, Row, Col),
    /// `{ r | }` with no fields.
    ExtField(Row, Col),
    End(Row, Col),
}

#[derive(Debug)]
pub enum TErrorRow<'a> {
    Tag(&'a TagVariant<'a>, Row, Col),
    /// `|` not followed by a tag or a variable.
    Ext(Row, Col),
    /// Expected `|` or `]`.
    End(Row, Col),
}

// ============================================================================
// Leaves (no lifetimes)
// ============================================================================

#[derive(Debug)]
pub enum StringError {
    Endless,
    Newline,
    Escape(Escape),
}

#[derive(Debug)]
pub enum Escape {
    Unknown,
    BadUnicodeFormat(u16),
    BadUnicodeCode(u16),
    BadUnicodeLength { code: u16, expected: i32, actual: i32 },
}

#[derive(Debug)]
pub enum Number {
    /// `123abc`
    End,
    /// `1.` / `1.x`
    Dot,
    /// `1e` / `1e+`
    Exponent,
    /// `0x` / `0xG`
    HexDigit,
    /// `007`
    NoLeadingZero,
    /// `1.5n`
    BigIntFraction,
}

#[derive(Debug)]
pub enum RawTokens {
    /// Unmatched closer (the byte found).
    Unbalanced(u8),
    /// EOF before the matching closer.
    Endless,
    String(StringError),
}

/// The `select` clauses in their required order (the derived `Ord` is that
/// order); payload of `Query::ClauseOrder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Clause {
    From,
    Join,
    Where,
    GroupBy,
    OrderBy,
    Limit,
    Offset,
}

impl Clause {
    pub const fn as_str(self) -> &'static str {
        match self {
            Clause::From => "from",
            Clause::Join => "join",
            Clause::Where => "where",
            Clause::GroupBy => "groupBy",
            Clause::OrderBy => "orderBy",
            Clause::Limit => "limit",
            Clause::Offset => "offset",
        }
    }
}

#[derive(Debug)]
pub enum BadOperator {
    /// `->` (hint: `=>` in match arms, `fn(A) -> B` in types)
    Arrow,
    /// `|` (hint: `||`, or `|` only between match patterns)
    Bar,
    /// `++` (hint: `Array.concat`, templates)
    PlusPlus,
    /// `::` (hint: paths only, no cons)
    DoubleColon,
    /// `..` (hint: spread only inside records/patterns)
    DotDot,
    /// `<|`
    PipeLeft,
    /// `>>`
    ComposeRight,
    /// `<<`
    ComposeLeft,
    /// `^` (hint: pins only in `query { }` and patterns; no power operator)
    Caret,
}
```

### 4.1 `keyword.rs` types referenced by `error.rs`

```rust
/// Reserved words: SPEC list plus `assert` and `await`.
pub const RESERVED: &[&str] = &[
    "as", "assert", "await", "break", "comptime", "component", "continue", "else", "enum",
    "error", "false", "fn", "for", "if", "impl", "import", "in", "let", "loop", "macro",
    "match", "mut", "pub", "provide", "query", "return", "schema", "state", "style", "table",
    "test", "tests", "trait", "true", "type", "use", "where", "while",
];

/// Contextual keywords inside `query { }` only.
pub const SQL_WORDS: &[&str] = &[
    "select", "insert", "update", "delete", "from", "join", "on", "set", "into", "values",
    "orderBy", "groupBy", "limit", "offset", "asc", "desc", "left", "inner",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyword {
    As, Assert, Await, Break, Comptime, Component, Continue, Else, Enum, Error, False, Fn,
    For, If, Impl, Import, In, Let, Loop, Macro, Match, Mut, Pub, Provide, Query, Return,
    Schema, State, Style, Table, Test, Tests, Trait, True, Type, Use, Where, While,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqlWord {
    Select, Insert, Update, Delete, From, Join, On, Set, Into, Values, OrderBy, GroupBy,
    Limit, Offset, Asc, Desc, Left, Inner,
}

impl Keyword {
    /// Named `from_word`, not `from_str`: clippy's `should_implement_trait` rejects the latter under `-D warnings`.
    pub fn from_word(s: &str) -> Option<Keyword>;
    pub const fn as_str(self) -> &'static str;
}

impl SqlWord {
    pub fn from_word(s: &str) -> Option<SqlWord>;
    pub const fn as_str(self) -> &'static str;
    /// The `select` clause a word opens, for `Query::ClauseOrder`: `From`, `Join`
    /// (also for `left` / `inner`), `GroupBy`, `OrderBy`, `Limit`, `Offset`;
    /// `None` for every other word.
    pub const fn clause(self) -> Option<crate::error::Clause>;
}
```

(`where` is reserved, so it is a `Keyword`, not a `SqlWord`; the `where`
clause is matched with `keyword(b"where")` and reported as
`Clause::Where`. That is why `Query::ClauseOrder` carries `error::Clause`
rather than `SqlWord`, which cannot name it.)

---

## 5. `Parser<'a>` and per-module signatures

### 5.1 `lib.rs`

```rust
pub type Row = u16;
pub type Col = u16;

#[derive(Clone, Copy)]
pub(crate) struct ParserState {
    pos: usize,
    row: Row,
    col: Col,
}

pub struct Parser<'a> {
    bump: &'a Bump,
    src: &'a [u8],
    pos: usize,
    row: Row,
    col: Col,
    /// Inside `query { }`: `in` is a binop, `^` pins, SQL words are not identifiers.
    in_query: bool,
    /// Set in if/while/for/match/provide/@directive heads: `Path {` is not a record constructor.
    no_record_ctor: bool,
}

/// Entry point used by the driver and by tests.
pub fn parse_module<'a>(bump: &'a Bump, src: &'a str) -> Result<Module<'a>, error::Error<'a>>;

impl<'a> Parser<'a> {
    pub fn new(bump: &'a Bump, src: &'a [u8]) -> Self;

    // ---- unchanged ---------------------------------------------------------
    pub fn position(&self) -> (Row, Col);
    pub fn get_position(&self) -> Position;
    pub fn add_end<T>(&self, start: Position, value: T) -> &'a Located<T>;
    pub fn row(&self) -> Row;
    pub fn col(&self) -> Col;
    pub fn is_eof(&self) -> bool;
    pub fn one_of<T, E>(&mut self, to_error: impl FnOnce(Row, Col) -> E, parsers: Vec<Box<dyn FnOnce(&mut Self) -> Result<T, E> + '_>>) -> Result<T, E>;
    pub fn one_of_with_fallback<T, E>(&mut self, parsers: Vec<Box<dyn FnOnce(&mut Self) -> Result<T, E> + '_>>, fallback: T) -> Result<T, E>;
    pub fn in_context<T, StartErr, BodyErr, ContextErr>(&mut self, add_context: impl FnOnce(&'a Bump, BodyErr, Row, Col) -> ContextErr, start_parser: impl FnOnce(&mut Self) -> Result<(), StartErr>, body_parser: impl FnOnce(&mut Self) -> Result<T, BodyErr>) -> Result<T, ContextErr> where StartErr: Into<ContextErr>;
    pub fn specialize<T, InnerErr, OuterErr>(&mut self, add_context: impl FnOnce(&'a Bump, InnerErr, Row, Col) -> OuterErr, parser: impl FnOnce(&mut Self) -> Result<T, InnerErr>) -> Result<T, OuterErr>;
    pub fn word1<E>(&mut self, expected: u8, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E>;
    pub fn word2<E>(&mut self, b1: u8, b2: u8, to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E>;
    pub fn peek(&self) -> Option<u8>;
    pub fn peek_at(&self, offset: usize) -> Option<u8>;
    pub fn remaining(&self) -> &'a [u8];
    pub fn advance(&mut self);
    pub fn advance_by(&mut self, n: usize);
    pub fn alloc<T>(&self, value: T) -> &'a T;
    pub fn alloc_slice_copy<T: Copy>(&self, slice: &[T]) -> &'a [T];
    pub fn alloc_str(&self, s: &str) -> &'a str;

    // ---- visibility change (were private) -----------------------------------
    pub(crate) fn save_state(&self) -> ParserState;
    pub(crate) fn restore_state(&mut self, state: ParserState);

    // ---- new -----------------------------------------------------------------
    /// Inline `Located` spanning `start`..current (for names and other Copy leaves).
    pub(crate) fn located<T>(&self, start: Position, value: T) -> Located<T>;
    /// Has a newline been crossed between `end` (a node's `region.end`) and here?
    pub(crate) fn newline_since(&self, end: Position) -> bool; // end.line != self.row
    /// Nothing (not even whitespace) has been consumed since `end`.
    pub(crate) fn adjacent_to(&self, end: Position) -> bool;   // end == self.get_position()
    /// Run `f`, then restore position regardless of its result (lookahead).
    pub(crate) fn lookahead<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T;
    pub(crate) fn with_query<T, E>(&mut self, on: bool, f: impl FnOnce(&mut Self) -> Result<T, E>) -> Result<T, E>;
    pub(crate) fn with_record_ctor<T, E>(&mut self, allowed: bool, f: impl FnOnce(&mut Self) -> Result<T, E>) -> Result<T, E>;
    pub(crate) fn in_query(&self) -> bool;
    pub(crate) fn record_ctor_allowed(&self) -> bool;
}
```

Removed: `indent()`, `set_indent()`, `with_indent()`,
`with_backset_indent()`, `ParserState::indent`.

Module list in `lib.rs`:

```rust
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
```

**Return convention.** Every parser of a `Located` node returns
`&'a Located<T>`; callers use `node.region.end` where Elm used the
returned `end`. Parsers of chain participants (`expression`, `unary`,
`postfix`, `pattern`, `type_expr`, `block`, `statement`, `item`) chomp
trailing whitespace after success and compute their region before the
chomp. `primary()` and individual postfix-op parsers do **not** chomp
(the postfix loop chomps at its decision point, §6.0).

**Dispatch style.** Where the first byte or word decides the production
(`primary`, `statement`, `item`, `pattern_atom`, `type_term`, query
verbs, markup children), dispatch with `peek()` / `peek_keyword()` and
report `Start` yourself; use `one_of` only for genuinely ambiguous forks
(kept for compatibility with existing code, not as the default).

### 5.2 `space.rs`

```rust
impl<'a> Parser<'a> {
    /// Spaces, tabs, CR/LF and `//…` comments (including `///`, `//!`). Infallible.
    pub fn chomp(&mut self);
    fn eat_spaces(&mut self);
    fn eat_line_comment(&mut self);
}
```

Deleted: `SpaceStatus`, `chomp_and_check_indent`, `check_indent`,
`check_aligned`, `check_fresh_line`, `doc_comment`, `eat_multi_comment*`.

### 5.3 `keyword.rs`

```rust
pub const RESERVED: &[&str];
pub const SQL_WORDS: &[&str];
pub enum Keyword { … }      // §4.1
pub enum SqlWord { … }      // §4.1
pub fn is_reserved(name: &str) -> bool;
pub fn is_sql_word(name: &str) -> bool;

impl<'a> Parser<'a> {
    /// Exact bytes followed by a non-identifier byte; fails without consuming.
    pub(crate) fn keyword<E>(&mut self, kw: &[u8], to_error: impl FnOnce(Row, Col) -> E) -> Result<(), E>;
    pub(crate) fn peek_keyword(&self, kw: &[u8]) -> bool;
    /// The identifier-shaped word at the cursor (no consume), for dispatch tables.
    pub(crate) fn peek_word(&self) -> &'a str;
}
```

The 17 `keyword_if`-style wrappers are deleted.

### 5.4 `symbol.rs`

```rust
impl<'a> Parser<'a> {
    /// Longest-match over the fixed table. `Ok(None)` (nothing consumed) for
    /// chain terminators (`=`, `=>`, `+=`, `-=`, `*=`, `/=`) and non-operators.
    /// `in` is returned as `BinOp::In` only when `in_query()`.
    /// Elm-habit tokens produce `to_error(BadOperator, …)`.
    pub(crate) fn binop<E>(&mut self, to_error: impl FnOnce(BadOperator, Row, Col) -> E) -> Result<Option<Located<BinOp>>, E>;
    /// `=` (not `==`/`=>`), `+=`, `-=`, `*=`, `/=`. None without consuming otherwise.
    pub(crate) fn assign_op(&mut self) -> Option<Located<AssignOp>>;
}
pub fn is_operator_char(b: u8) -> bool;
```

### 5.5 `number.rs`

```rust
pub(crate) enum NumberLiteral<'a> {
    Number(NumberLit<'a>),
    BigInt(&'a str),
}

impl<'a> Parser<'a> {
    /// Digit-led literal with Elm's dirty-end check (`123abc` → Number::End).
    pub(crate) fn number_literal<E>(&mut self, to_expectation: impl FnOnce(Row, Col) -> E, to_error: impl FnOnce(error::Number, Row, Col) -> E) -> Result<NumberLiteral<'a>, E>;
    /// Committed numeric prefix without the dirty-end check (style dimensions read the unit after it).
    pub(crate) fn chomp_number(&mut self) -> Result<NumberLiteral<'a>, error::Number>;
    /// Bare digit run for tuple indices (`t.0`). None without consuming if no digit.
    pub(crate) fn digits(&mut self) -> Option<Located<u32>>;
}
```

### 5.6 `string.rs`

```rust
impl<'a> Parser<'a> {
    /// `"…"` single-line.
    pub(crate) fn string_literal<E>(&mut self, to_expectation: impl FnOnce(Row, Col) -> E, to_error: impl FnOnce(StringError, Row, Col) -> E) -> Result<&'a str, E>;
    pub(crate) fn eat_escape(&self, template: bool) -> EscapeResult;
    pub(crate) fn eat_unicode(&self) -> EscapeResult;
    pub(crate) fn build_escaped_string(&self, start: usize, end: usize, template: bool) -> &'a str;
}
pub(crate) enum EscapeResult { Normal(usize), Unicode(usize), EndOfFile, Problem(Escape) }
pub(crate) fn utf8_char_width(b: u8) -> usize;
```

`chomp_multi_string` is deleted.

### 5.7 `template.rs`

```rust
impl<'a> Parser<'a> {
    /// At the opening backtick. Used by primary and by tagged templates.
    pub(crate) fn template_parts(&mut self) -> Result<&'a [TemplatePart<'a>], error::Template<'a>>;
    /// `Expr::Template` primary.
    pub(crate) fn template(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
}
```

### 5.8 `name.rs` (moved from `expression/variable.rs`)

```rust
impl<'a> Parser<'a> {
    pub(crate) fn lower_name<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<&'a str, E>;
    pub(crate) fn upper_name<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<&'a str, E>;
    pub(crate) fn located_lower<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<Name<'a>, E>;
    pub(crate) fn located_upper<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<Name<'a>, E>;
    /// `Upper { '::' Upper }`; stops before `::lower`.
    pub(crate) fn path<E>(&mut self, to_expectation: impl FnOnce(Row, Col) -> E, to_member_error: impl FnOnce(Row, Col) -> E) -> Result<Path<'a>, E>;
    /// `:lower` — the returned name excludes the colon; region includes it.
    pub(crate) fn tag_name<E>(&mut self, to_expectation: impl FnOnce(Row, Col) -> E, to_bad_name: impl FnOnce(Row, Col) -> E) -> Result<Name<'a>, E>;
    /// `[a-z][A-Za-z0-9_]*` with NO reserved-word / SQL-word check (§2.4): module-path
    /// segments. Fails without consuming when the first byte is not a lowercase letter.
    pub(crate) fn raw_lower<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<Name<'a>, E>;
    /// `raw_lower { '-' raw_lower }` for element, attribute and close-tag names.
    /// Keyword-insensitive: `type`, `for`, `style`, `table`, `select` are names here.
    pub(crate) fn dashed_name<E>(&mut self, to_error: impl FnOnce(Row, Col) -> E) -> Result<Name<'a>, E>;
    pub(crate) fn peek_lower(&self) -> bool;
    pub(crate) fn peek_upper(&self) -> bool;
    pub(crate) fn chomp_inner_chars(&mut self);
    pub(crate) fn slice_from(&self, start_pos: usize) -> &'a str;
}
```

`lower_name` refuses reserved words and, while `in_query()`, SQL words —
both without consuming. `raw_lower` and `dashed_name` never refuse a word
(§2.4); `dashed_name` is built on `raw_lower`, not on `lower_name`.
Deleted: `variable`, `foreign_alpha`, `is_dot_upper`, `is_dot_lower`,
`parse_qualified_lower`, `chomp_qualified_upper`.

### 5.9 `raw.rs`

```rust
impl<'a> Parser<'a> {
    /// At `open`. Consumes through the matching `close`, honoring nested
    /// `()[]{}`, strings, templates and `//` comments. Returns the interior text.
    pub(crate) fn raw_balanced<E>(&mut self, open: u8, close: u8, to_error: impl FnOnce(RawTokens, Row, Col) -> E) -> Result<Located<&'a str>, E>;
}
```

### 5.10 `module.rs`

```rust
impl<'a> Parser<'a> {
    /// chomp; items until EOF; a non-item → Module::BadEnd. After each item the
    /// next one must start on a later line (`newline_since(item.region.end)`),
    /// otherwise Module::SameLine (§2.1 rule 3).
    pub fn module(&mut self) -> Result<Module<'a>, error::Module<'a>>;
}
```

### 5.11 `item/`

```rust
// item/mod.rs
impl<'a> Parser<'a> {
    /// attributes* [pub] item_body. Chomps trailing whitespace.
    pub fn item(&mut self) -> Result<&'a Located<Item<'a>>, error::Item<'a>>;
    /// Items until `}` (for `tests { }`); `}` is consumed. Same line-break rule as
    /// `module()` → Tests::SameLine. `item()` itself reports a `;` as Item::Semicolon.
    pub(crate) fn items_until_close(&mut self) -> Result<&'a [&'a Located<Item<'a>>], error::Tests<'a>>;
}
// item/attribute.rs
pub(crate) fn attributes(&mut self) -> Result<&'a [Located<Attribute<'a>>], error::Attribute<'a>>;
pub(crate) fn attribute(&mut self) -> Result<Located<Attribute<'a>>, error::Attribute<'a>>;   // at `#`
// item/import.rs  (after `import`)
/// The bare tail (`ImportTail::Module`) is validated here: no segments (`import ~`) →
/// Import::RootOnly; reserved last segment (`import @alder/test`) → Import::ReservedBinding(kw).
pub(crate) fn import(&mut self, is_pub: bool) -> Result<&'a Import<'a>, error::Import<'a>>;
/// `@author/package { '/' seg }` | `~ { '/' seg }`; author, package and segments via `raw_lower` (§2.4).
pub(crate) fn module_path(&mut self) -> Result<Located<ModulePath<'a>>, error::ModulePath>;
// item/fn_.rs  (after `fn`; body optional)
pub(crate) fn fn_decl(&mut self) -> Result<&'a FnDecl<'a>, error::Fn<'a>>;
pub(crate) fn params(&mut self) -> Result<&'a [Param<'a>], error::Params<'a>>;                  // at `(`; shared by lambda/component
pub(crate) fn where_clause(&mut self) -> Result<&'a [Constraint<'a>], error::Where<'a>>;         // after `where`; may be empty
// item/let_.rs  (after `let`; shared with Stmt::Let and child blocks)
pub(crate) fn let_decl(&mut self) -> Result<&'a LetDecl<'a>, error::Let<'a>>;
// item/type_alias.rs  (after `type`)
pub(crate) fn type_decl(&mut self) -> Result<ItemKind<'a>, error::TypeAlias<'a>>;               // TypeAlias or OpaqueType
pub(crate) fn type_params(&mut self) -> Result<&'a [Name<'a>], error::TypeParams>;              // expects `[` (else TypeParams::Open); `type`/`enum` peek for `[` first, `trait` calls unconditionally
// item/enum_.rs  (after `enum`)
/// Record payloads reuse `field_types()`; a `Some(ext)` result is Enum::VariantRecordExt.
pub(crate) fn enum_decl(&mut self) -> Result<&'a EnumDecl<'a>, error::Enum<'a>>;
// item/trait_.rs  (after `trait`)
/// `type_params` is required (missing `[` → Trait::Params(TypeParams::Open)). Body items
/// are line-break separated (Trait::SameLine); a `;` after an item → Trait::Semicolon.
pub(crate) fn trait_decl(&mut self) -> Result<&'a TraitDecl<'a>, error::Trait<'a>>;
// item/impl_.rs  (after `impl`)
/// Body items are line-break separated (Impl::SameLine); a `;` after an item → Impl::Semicolon.
pub(crate) fn impl_decl(&mut self) -> Result<&'a ImplDecl<'a>, error::Impl<'a>>;
// item/error_.rs  (after `error`)
pub(crate) fn error_decl(&mut self) -> Result<&'a ErrorDecl<'a>, error::ErrorDecl<'a>>;
// item/component.rs  (after `component`)
pub(crate) fn component_decl(&mut self) -> Result<&'a ComponentDecl<'a>, error::Component<'a>>;
// item/table.rs  (after `table`)
pub(crate) fn table_decl(&mut self) -> Result<&'a TableDecl<'a>, error::Table<'a>>;
pub(crate) fn modifier<E>(&mut self, to_arg_error: impl Fn(&'a Expr<'a>, Row, Col) -> E + Copy, to_end_error: impl FnOnce(Row, Col) -> E) -> Result<Modifier<'a>, E>;  // shared with schema rules
// item/schema.rs  (after `schema`)
pub(crate) fn schema_decl(&mut self) -> Result<&'a SchemaDecl<'a>, error::Schema<'a>>;
// item/macro_.rs
pub(crate) fn macro_decl(&mut self) -> Result<&'a MacroDecl<'a>, error::Macro>;                 // after `macro`
pub(crate) fn comptime_block(&mut self) -> Result<&'a Located<Block<'a>>, error::Block<'a>>;      // after `comptime`
// item/test.rs
pub(crate) fn test_decl(&mut self) -> Result<&'a TestDecl<'a>, error::Test<'a>>;                 // after `test`
pub(crate) fn tests_block(&mut self) -> Result<&'a [&'a Located<Item<'a>>], error::Tests<'a>>;   // after `tests`
```

### 5.12 `statement.rs`

```rust
impl<'a> Parser<'a> {
    /// At `{`. Always a block. Enforces Block::SameLine; the last `Stmt::Expr`
    /// before `}` becomes `tail`.
    pub fn block(&mut self) -> Result<&'a Located<Block<'a>>, error::Block<'a>>;
    /// One statement; dispatch on let/use/provide/for/while/return/break/continue/assert/`;`,
    /// else `expr_or_assign`.
    pub fn statement(&mut self) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>>;
    /// expression, then optional assign_op + value. Shared with lambda bodies.
    pub(crate) fn expr_or_assign(&mut self) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>>;
    /// Var followed by Access/TupleAccess/Index steps → Place; otherwise None.
    pub(crate) fn expr_to_place(&self, expr: &'a Located<Expr<'a>>) -> Option<&'a Located<Place<'a>>>;
    fn for_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::For<'a>>;
    fn while_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::While<'a>>;
    fn provide_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::Provide<'a>>;
    fn use_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>>;
    fn return_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>>;
    fn break_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>>;
    fn assert_stmt(&mut self, start: Position) -> Result<&'a Located<Stmt<'a>>, error::Stmt<'a>>;
}
```

### 5.13 `expression/`

```rust
// expression/mod.rs
impl<'a> Parser<'a> {
    /// Flat binop chain over `unary`. Chomps trailing whitespace.
    pub fn expression(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
    /// `-` / `!` / (query mode) `^` prefix, then `postfix`. A `Start` failure of the
    /// operand becomes `Expr::Unary`; every other operand error propagates (§6.0).
    pub(crate) fn unary(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
    /// `primary` then the postfix loop (§6.0). Chomps trailing whitespace.
    pub(crate) fn postfix(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
    /// Dispatch table on the first byte/word. Does NOT chomp.
    pub(crate) fn primary(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
}
/// May `op` sit at the start of a continuation line? (`-` only if followed by
/// whitespace; `<` only if not followed by a letter or `>`; everything else yes.)
fn continues_line(op: BinOp, next: Option<u8>) -> bool;

// expression/postfix.rs
pub(crate) fn call_args(&mut self) -> Result<&'a [&'a Located<Expr<'a>>], error::Call<'a>>;   // at `(`; accepts `_`
pub(crate) fn index(&mut self, target: &'a Located<Expr<'a>>) -> Result<&'a Located<Expr<'a>>, error::Index<'a>>;   // at `[`
pub(crate) fn dot_suffix(&mut self, target: &'a Located<Expr<'a>>) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;   // at `.`: field / digits / await
pub(crate) fn tagged_template(&mut self, tag: &'a Located<Expr<'a>>) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
// expression/literal.rs
pub(crate) fn number(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
pub(crate) fn string(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
// expression/array.rs
pub(crate) fn array(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
// expression/tuple.rs  (unit / parenthesized / tuple)
pub(crate) fn tuple(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
// expression/record.rs
pub(crate) fn record(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
pub(crate) fn record_fields(&mut self) -> Result<&'a [RecordField<'a>], error::Record<'a>>;   // after `{`; also RecordCtor and query `set`
pub(crate) fn looks_like_record(&mut self) -> bool;   // lookahead at `{` (§2.2)
// expression/path.rs  (Var, Path, PathVar, RecordCtor, macro call dispatch)
pub(crate) fn name_or_path(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
pub(crate) fn record_ctor(&mut self, start: Position, path: Path<'a>) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
// expression/tag.rs
pub(crate) fn tag(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
// expression/lambda.rs  (after `fn`)
pub(crate) fn lambda(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
// expression/if_.rs  (after `if`)
pub(crate) fn if_(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
// expression/match_.rs  (after `match`)
pub(crate) fn match_(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
/// `p | q [if guard] =>` — shared with @match (errors mapped by the caller).
pub(crate) fn arm_head<E>(&mut self, to_pattern: impl FnOnce(&'a error::Pattern<'a>, Row, Col) -> E, to_guard: impl FnOnce(&'a error::Expr<'a>, Row, Col) -> E, to_arrow: impl FnOnce(Row, Col) -> E) -> Result<(&'a [&'a Located<Pattern<'a>>], Option<&'a Located<Expr<'a>>>), E>;
// expression/loop_.rs
pub(crate) fn loop_(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;    // after `loop`
pub(crate) fn state(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;    // after `state`
pub(crate) fn macro_call(&mut self, start: Position, name: Name<'a>) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;   // at `!(`
```

### 5.14 `pattern/`

```rust
// pattern/mod.rs
impl<'a> Parser<'a> {
    /// `pattern_atom [as name]`. Chomps trailing whitespace.
    pub fn pattern(&mut self) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>>;
    /// `p | q | r` (match arms).
    pub(crate) fn pattern_alternatives(&mut self) -> Result<&'a [&'a Located<Pattern<'a>>], error::Pattern<'a>>;
    /// Dispatch on first byte: `_`, lower, upper (ctor), `:`, `^`, `-`/digit, `"`, `(`, `[`, `{`, true/false.
    pub(crate) fn pattern_atom(&mut self) -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>>;
}
// pattern/ctor.rs: fn pattern_ctor(start), fn pattern_tag(start)      -> Result<&'a Located<Pattern<'a>>, error::Pattern<'a>>
// pattern/tuple.rs: fn pattern_tuple(start)                            -> Result<&'a Located<Pattern<'a>>, error::PTuple<'a>>
// pattern/array.rs: fn pattern_array(start)                            -> Result<&'a Located<Pattern<'a>>, error::PArray<'a>>
// pattern/record.rs: fn pattern_record_fields() -> Result<(&'a [FieldPattern<'a>], Option<Region>), error::PRecord<'a>>   // after `{`; shared with CtorRecord
```

### 5.15 `type_.rs`

```rust
impl<'a> Parser<'a> {
    /// `fn` type or term. Chomps trailing whitespace.
    pub fn type_expr(&mut self) -> Result<&'a Located<Type<'a>>, error::Type<'a>>;
    /// path[args] | var[args] | ( ) | tuple | record | error row.
    pub(crate) fn type_term(&mut self) -> Result<&'a Located<Type<'a>>, error::Type<'a>>;
    /// At `[`: `[T, U]`.
    pub(crate) fn type_args(&mut self) -> Result<&'a [&'a Located<Type<'a>>], error::TArgs<'a>>;
    /// `:tag[(T, …)]` — shared by error rows and `error` groups.
    pub(crate) fn tag_variant(&mut self) -> Result<TagVariant<'a>, error::TagVariant<'a>>;
    /// After `{`: fields with `?` and `r |` extension. Shared with enum record variants.
    pub(crate) fn field_types(&mut self) -> Result<(&'a [FieldType<'a>], Option<Name<'a>>), error::TRecord<'a>>;
    fn type_fn(&mut self, start: Position) -> Result<&'a Located<Type<'a>>, error::TFn<'a>>;
    fn type_tuple(&mut self, start: Position) -> Result<&'a Located<Type<'a>>, error::TTuple<'a>>;
    fn type_error_row(&mut self, start: Position) -> Result<&'a Located<Type<'a>>, error::TErrorRow<'a>>;
}
```

### 5.16 `markup/`

```rust
// markup/mod.rs
impl<'a> Parser<'a> {
    /// At `<`. Produces Expr::Markup; does not chomp (postfix loop does).
    pub(crate) fn markup(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
    pub(crate) fn element(&mut self) -> Result<&'a Element<'a>, error::Markup<'a>>;             // at `<name`
    pub(crate) fn fragment(&mut self) -> Result<&'a [&'a Located<Child<'a>>], error::Markup<'a>>; // at `<>`
    pub(crate) fn attrs(&mut self) -> Result<(&'a [Attr<'a>], bool /* self_closing */), error::Markup<'a>>;
    /// Text mode loop until `</` (CloseTag) or `}` (Brace).
    pub(crate) fn children(&mut self, term: ChildTerminator) -> Result<&'a [&'a Located<Child<'a>>], error::Child<'a>>;
    pub(crate) fn child(&mut self, term: ChildTerminator) -> Result<Option<&'a Located<Child<'a>>>, error::Child<'a>>;   // None = droppable whitespace run
    fn closing_tag(&mut self, name: Located<ElementName<'a>>) -> Result<(), error::Markup<'a>>;
}
pub(crate) enum ChildTerminator { CloseTag, Brace }
// markup/directive.rs
pub(crate) fn directive(&mut self) -> Result<&'a Located<Child<'a>>, error::Child<'a>>;        // at `@`
pub(crate) fn child_block(&mut self) -> Result<&'a Located<ChildBlock<'a>>, error::ChildBlock<'a>>;   // at `{`
/// Lookahead past whitespace for `@else` / `@empty` (does not consume).
pub(crate) fn peek_directive(&mut self, word: &[u8]) -> bool;
```

### 5.17 `query.rs`

```rust
impl<'a> Parser<'a> {
    /// After `query`: `{` … `}` under `with_query(true, …)`.
    pub(crate) fn query(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
    fn query_body(&mut self) -> Result<&'a Query<'a>, error::Query<'a>>;
    fn select(&mut self) -> Result<&'a Select<'a>, error::Select<'a>>;
    fn insert(&mut self) -> Result<Query<'a>, error::Insert<'a>>;
    fn update(&mut self) -> Result<Query<'a>, error::Update<'a>>;
    fn delete(&mut self) -> Result<Query<'a>, error::Delete<'a>>;
    fn table_ref(&mut self) -> Result<TableRef<'a>, error::TableRef>;
    /// `^` + postfix parsed with `with_query(false, …)` so `^select` and `^{ a, b }` work.
    /// The operand is the whole postfix chain (`^user.id` pins `user.id`; §10.20).
    pub(crate) fn pinned_value(&mut self) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
}
```

### 5.18 `style.rs`

```rust
impl<'a> Parser<'a> {
    /// After `style`.
    pub(crate) fn style(&mut self, start: Position) -> Result<&'a Located<Expr<'a>>, error::Expr<'a>>;
    pub(crate) fn style_block(&mut self) -> Result<&'a Style<'a>, error::Style<'a>>;   // at `{`
    /// `{` → nested style; digit, or `-` + digit → dimension attempt; otherwise `expression()`.
    fn style_value(&mut self) -> Result<StyleValue<'a>, error::Style<'a>>;
}
```

---

## 6. Modes

### 6.0 Code mode (default)

`primary()` dispatch on the first byte / word:

| first bytes                 | production                                                        |
| --------------------------- | ----------------------------------------------------------------- |
| digit                       | `number` (Number / BigInt)                                        |
| `"`                         | `string`                                                          |
| `` ` ``                     | `template`                                                        |
| `(`                         | `tuple` (unit / paren / tuple)                                    |
| `[`                         | `array`                                                           |
| `{`                         | `record` if `looks_like_record()` else `block`                    |
| `<` + letter or `>`         | `markup`; `</` → `UnexpectedClose`; other `<` → `Start`           |
| `:` + lower                 | `tag`; other `:` → `Tag::Name`                                    |
| `^`                         | `PinOutsideQuery` (query mode handles `^` in `unary`)             |
| `_` + non-ident byte        | `Placeholder` error (`call_args` intercepts the legal case first) |
| `fn`                        | `lambda`                                                          |
| `if` / `match` / `loop`     | `if_` / `match_` / `loop_`                                        |
| `state` / `style` / `query` | `state` / `style` / `query`                                       |
| `true` / `false`            | `Bool`                                                            |
| other reserved word         | `Reserved(kw)`                                                    |
| SQL word while `in_query`   | `SqlKeyword(word)`                                                |
| lowercase                   | `Var`; adjacent `!(` → `macro_call`                               |
| uppercase                   | `path`; `::lower` → `PathVar`                                     |
| anything else               | `Start`                                                           |

`postfix()` loop, per iteration (the node's `region.end` equals the
cursor because nothing has been chomped yet):

1. Before chomping: `` ` `` → `tagged_template` (adjacency); continue.
2. `chomp()`; `nl = newline_since(end)`.
3. `.` (not `..`) → `dot_suffix` on any line.
4. If `!nl`: `(` → call; `[` → index; `?` (next byte not `?`) → `Try`;
   `{` when the node is `Expr::Path` and `record_ctor_allowed()` →
   `record_ctor`.
5. Otherwise return (trailing whitespace already chomped).

`expression()`:

```text
last = unary()
loop:
    saved = save_state()
    match binop(Expr::OperatorReserved)? {
        None => break,
        Some(op) if newline_since(last.region.end) && !continues_line(op.value, peek()) =>
            { restore_state(saved); break }
        Some(op) => {
            chomp()
            operand = match unary() {
                Err(Expr::Start(r, c)) => return Err(Expr::OperatorRight(op.value, r, c)),
                other => other?,   // Reserved, SqlKeyword, Number, String, nested … propagate
            }
            push (last, op); last = operand
        }
    }
build BinOps { operands, last } or return the single operand
```

`OperatorRight` replaces **only** an `Expr::Start` raised at the operand
position — i.e. nothing operand-like was present (`}`, `)`, EOF, another
operator). If something operand-like began and failed, the nested error
propagates unchanged, as in Elm: `a + "unterminated` is `Expr::String`,
`a + else` is `Expr::Reserved(Else)`, `a + 007` is `Expr::Number`.
`unary()` applies the same rule to its own operand: a `Start` after `-` /
`!` becomes `Expr::Unary`; anything else propagates (`-"x` is a string
error, a `-` at EOF is `Unary`).

Brackets reset restrictions: inside `( )`, `[ ]`, record `{ }`, call
args, index, `${ }`, markup holes and attribute holes, sub-expressions run
under `with_record_ctor(true, …)`.

### 6.1 Template mode

At `` ` ``: read raw bytes until `` ` ``, `\`, or `${`. `\` →
`eat_escape(true)` (bad escape → `Template::Escape` at the backslash).
`${` → `chomp`; `}` → `HoleEmpty`; else `with_record_ctor(true,
expression)` (`HoleExpr`), `chomp`, `}` (`HoleEnd`). `\r\n` → `\n`. EOF →
`Template::Endless` at the opening backtick. Text parts are zero-copy
slices unless an escape or CRLF forced a cooked copy.

### 6.2 Markup mode

**Entry** from `primary` on `<`. `<>` → fragment; `<` + lowercase →
`dashed_name` → `ElementName::Tag`; `<` + uppercase → `path` →
`ElementName::Component`; else `Markup::Name`. Tag names are
keyword-insensitive (§2.4): `<table>`, `<style>`, `<select>`, `<form>` are
ordinary elements and never produce `Reserved` / `SqlKeyword` / `Name`.

**Attributes** (code mode; `chomp` between): loop on `peek`: `/` → expect
`/>` (self-closing); `>` → open; lowercase → `dashed_name`
(keyword-insensitive: `type="password"`, `for=`, `style=` are attributes,
never `TagEnd`), optional `=` then `"…"` (`AttrValue::Str`) or `{`
`expression` `}` (`AttrValue::Expr`, restrictions cleared; a missing `}` →
`Attr::ExprEnd`) else `Attr::Value`; anything else → `TagEnd`.

**Children** (`children(term)`), deciding on the current byte **without
chomping** (whitespace is text here):

- `</` → stop if `term == CloseTag`; `Child::Element(Markup::CloseName…)`
  otherwise.
- `<` → nested `element` / `fragment`.
- `{` → hole: `advance`, `chomp`, `}` → `HoleEmpty`, else expression
  (restrictions cleared), `chomp`, `}` (`HoleEnd`).
- `}` → stop if `term == Brace`; else `StrayBrace`.
- `@` + `if`/`for`/`match` + non-ident byte → `directive`; `@else` /
  `@empty` here → `StrayElse` / `StrayEmpty`; `@` + other identifier →
  `UnknownDirective`; any other `@` → text.
- at child start inside a `Brace` block: `let` / `use` + non-ident byte
  → `Child::Stmt` via `let_decl` / `use_stmt`.
- EOF → stop; caller reports `Unclosed` / `FragmentUnclosed`.
- otherwise → text run until `<`, `{`, `}`, EOF, or a directive-starting
  `@`. A run that is whitespace-only **and** contains a newline is
  dropped; every other run is kept verbatim (collapsing is a later pass).

**Close tag**: `</` + name lexed as the open tag was (`dashed_name` or
`path`, so `</table>` and `</style>` lex exactly like their openers);
compare raw slices (`CloseMismatch { expected, found }`); then `>`
(`CloseEnd`).

**Directives** (`@` already seen; heads parsed under
`with_record_ctor(false, …)`):

- `@if cond child_block { @else if cond child_block } [ @else child_block ]`
  — `@else` may follow on the next line (`peek_directive`).
- `@for pattern in expr [ ; key expr ] child_block [ @empty child_block ]`.
- `@match expr { arms }`: `arm_head` (shared with `match_`), then body:
  `{` → `child_block`; `<` → element/fragment; `{`-hole is a block
  (always); `@` → directive; anything else → `DirMatch::BareText`.
  Optional `,`; `}` ends.
- `child_block`: `{` `children(Brace)` `}`.

### 6.3 Query mode

`query` keyword, `chomp`, `{` (`Query::Open`), body under
`with_query(true, …)`. Verb dispatch with `keyword(b"select" | …)`, else
`Query::Verb`. Because `lower_name` refuses SQL words in query mode and
there is no juxtaposition, every clause operand (`expression()`) stops
cleanly at the next clause word; a SQL word in operand position is
`Expr::SqlKeyword`. `binop()` returns `BinOp::In` for the keyword `in`
only here. `^` → `pinned_value` (`Expr::Pin` over `postfix` with query
mode off).

`select`: `*` → `Projection::Star` | `{` expressions `}`; `from`
`table_ref`; joins (`join` | `left join` | `inner join`, `on` expr); then
clauses in the fixed order `where`, `groupBy`, `orderBy`, `limit`,
`offset`. The clause loop remembers the last `error::Clause` accepted
(`From < Join < Where < GroupBy < OrderBy < Limit < Offset`, the enum's
derived `Ord`); each clause word is mapped to a `Clause` (`SqlWord::clause`,
or `keyword(b"where")` → `Clause::Where`) and must be strictly greater
than the last one, otherwise `Query::ClauseOrder(clause)` — this covers
both out-of-order (`where` after `orderBy` → `ClauseOrder(Where)`) and
repeated clauses (`limit 1 limit 2` → `ClauseOrder(Limit)`). `join` is the
one clause that may repeat (joins are a list), so `Join` after `Join` is
accepted.
`order` = expression then optional `asc` / `desc`. `insert into <name>
values <pinned>` (non-pin → `Insert::Pin`). `update <name> set <record>
[where expr]` — `record_fields` in query mode. `delete from <name> [where
expr]`. Then `}` else `Query::End`.

### 6.4 Style mode

`style` then `{` (`Style::Open`); entries `key: value` with optional `,`;
key = `lower_name` or `"string"`; value: `{` → nested `style_block`
(never a record); a digit, or a `-` immediately followed by a digit →
`chomp_number` (the `-` negates `value` and stays in `text`, so
`margin: -8px` is `Dimension { -8 "-8", "px" }`) then, if the next bytes
are ASCII letters or `%`, read them as the unit → `Dimension` (a space
before the unit → `Style::Dimension(Number::End)`); a number with no unit
restores the saved state and falls through; otherwise `expression()`
(`-x` is `Negate`, `16` is `Number`).

### 6.5 Raw mode

`name!(` → `raw_balanced(b'(', b')')` → `Expr::MacroCall`. `macro name(
params ) {` → `raw_balanced(b'{', b'}')` → `MacroDecl.body`. `quote { }`
therefore never reaches the expression parser in M1.

---

## 7. Tests

### 7.1 Macros

Each module defines its pair at module level under `#[cfg(test)]` and
re-exports with `pub(crate) use`, named `assert_<thing>_snapshot!` /
`assert_<thing>_error_snapshot!`. The success macro also asserts the
whole input was consumed (parsers chomp trailing whitespace, so EOF
means nothing was left). No `assert_indented_*` variants exist.

Module-level definition is a **deliberate deviation** from CLAUDE.md's
"macros are defined in each module's test submodule": a `macro_rules!`
defined inside `mod tests` is invisible to child modules, and the
submodules (`expression/if_.rs`, `item/import.rs`, `markup/directive.rs`,
…) must import the parent's pair (`use super::{assert_expression_snapshot,
assert_expression_error_snapshot};`). Defining the pair at module level
under `#[cfg(test)]` and re-exporting it with `pub(crate) use` is what
today's code already does; Wave 4 updates the CLAUDE.md sentence to match
(§10.41).

```rust
/// Snapshot test macro for successful expression parsing.
#[cfg(test)]
macro_rules! assert_expression_snapshot {
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

/// Snapshot test macro for expression parse errors.
#[cfg(test)]
macro_rules! assert_expression_error_snapshot {
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

#[cfg(test)]
pub(crate) use assert_expression_error_snapshot;
#[cfg(test)]
pub(crate) use assert_expression_snapshot;
```

Copy the pair verbatim into each module below, substituting the
method and the `<thing>` name:

| module              | `<thing>`            | method called                               |
| ------------------- | -------------------- | ------------------------------------------- |
| `expression/mod.rs` | `expression`         | `expression()`                              |
| `statement.rs`      | `block`, `statement` | `block()`, `statement()`                    |
| `pattern/mod.rs`    | `pattern`            | `pattern()`                                 |
| `type_.rs`          | `type`               | `type_expr()`                               |
| `item/mod.rs`       | `item`               | `item()`                                    |
| `module.rs`         | `module`             | `module()`                                  |
| `markup/mod.rs`     | `markup`             | `expression()` (input starts at `<`)        |
| `query.rs`          | `query`              | `expression()` (input starts at `query`)    |
| `style.rs`          | `style`              | `expression()` (input starts at `style`)    |
| `template.rs`       | `template`           | `expression()` (input starts at a backtick) |

Submodules (`expression/if_.rs`, `item/import.rs`, `markup/directive.rs`,
…) import the parent's macros; snapshot files land in each submodule's
`snapshots/` directory. `space.rs`, `keyword.rs`, `symbol.rs`, `name.rs`,
`number.rs`, `string.rs`, `raw.rs` keep plain `#[test]` unit tests on the
primitives. Error cases use the `error_` prefix in test names.

### 7.2 Granular test list

One construct per test.

**space.rs**: empty, spaces_only, tabs_allowed, newlines, line_comment, doc_line_comment_skipped, comment_at_eof, stops_at_content.

**keyword.rs / name.rs / symbol.rs**: lower_simple, lower_camel, lower_rejects_reserved, lower_rejects_sql_word_only_in_query, upper_simple, path_single, path_nested, path_stops_before_lower, path_dangling_colons_error, tag_ok, tag_space_rejected, tag_upper_rejected, dashed_name, dashed_name_accepts_reserved (`type`, `style`, `select`), dashed_name_rejects_upper, raw_lower_accepts_reserved (`test`), raw_lower_accepts_sql_word_in_query, binop_longest_match_each (one per operator), binop_terminators_not_consumed (`=`, `=>`, `+=`, `-=`, `*=`, `/=`), binop_bad_each (`->`, `|`, `++`, `::`, `..`, `<|`, `>>`, `<<`, `^`), binop_in_only_in_query, assign_op_each, assign_op_not_double_equals.

**number.rs**: int_simple, int_zero, int_hex, float_simple, float_exponent, float_exponent_sign, bigint, bigint_hex, digits_run, error_leading_zero, error_hex_no_digits, error_trailing_dot, error_bad_exponent, error_dirty_end, error_bigint_fraction.

**string.rs**: simple, empty, with_escape, unicode, error_endless, error_newline, error_bad_escape, error_bad_unicode_length.

**template.rs**: empty, text_only, single_hole, hole_at_start, hole_at_end, adjacent_holes, text_around_holes, nested_template, record_in_hole, escaped_backtick, escaped_dollar, dollar_without_brace, multiline_text, crlf_normalized, error_endless, error_hole_empty, error_hole_unclosed, error_hole_bad_expr, error_bad_escape.

**expression/path.rs**: var_simple, var_camel, var_underscore_inside, path_bare, path_qualified, path_deep, path_var, path_dot_access (`Array.map`), record_ctor, record_ctor_shorthand, record_ctor_empty, error_reserved_word, error_path_dangling_colons, error_sql_word_outside_query_is_var (success: `select` is a plain var outside query).

**expression/tag.rs**: tag_bare, tag_with_arg, tag_with_args, tag_in_call, error_tag_no_name, error_tag_unclosed.

**expression/array.rs**: empty, single, multiple, nested, trailing_comma, multiline, with_comments, error_unclosed, error_double_comma, error_missing_comma.

**expression/tuple.rs**: unit, parenthesized, pair, triple, nested, trailing_comma, multiline, error_unclosed, error_empty_comma.

**expression/record.rs**: empty, single_field, two_fields, shorthand_single, shorthand_multiple, mixed_shorthand, spread_first, spread_with_fields, spread_only, nested_record, trailing_comma, multiline, block_vs_record_let (multiline block), block_vs_record_call, error_unclosed, error_missing_value, error_uppercase_field, error_spread_no_expr, error_equals_not_colon.

**expression/mod.rs**: binop_add, binop_sub, binop_mul, binop_div, binop_rem, binop_eq, binop_neq, binop_lt, binop_lte, binop_gt, binop_gte, binop_and, binop_or, binop_coalesce, binop_pipe, binop_chained, binop_mixed_flat, binop_with_parens, binop_eq_negative_no_space (`a==-1`), binop_lt_negative_no_space (`x<-1`), binop_leading_pipe_newline, binop_leading_plus_newline, negate_var, negate_number, negate_call, not_var, not_parens, double_negate, error_operator_arrow, error_operator_bar, error_operator_plus_plus, error_operator_double_colon, error_operator_caret, error_operator_right_missing (`a +` → `OperatorRight`), error_operator_right_bad_operand_propagates (`a + "x` → `Expr::String`, not `OperatorRight`), error_unary_missing_operand (`-` at EOF → `Expr::Unary`), error_not_missing_operand (`!)`), error_unary_bad_operand_propagates (`-"x` → `Expr::String`), error_pin_outside_query, error_start_reserved, error_unexpected_close_tag, error_placeholder_alone.

**expression/literal.rs**: number_int, number_float, number_hex, number_exponent, number_keeps_text (`0xFF` → value 255, text `"0xFF"`), bigint, bigint_hex, string_simple, string_empty, string_escapes, string_unicode, bool_true, bool_false (`Expr::Bool` is produced by `primary` in `mod.rs`; it is tested here with the other literal primaries), error_number_dirty_end, error_number_leading_zero, error_string_endless, error_string_newline, error_string_bad_escape.

**expression/postfix.rs**: call_no_args, call_one_arg, call_many_args, call_trailing_comma, call_nested, call_chained, call_on_lambda_result, call_placeholder_first, call_placeholder_second, call_placeholder_all, access_field, access_chain, access_newline_continuation, tuple_index, tuple_index_chain, index_simple, index_nested, index_expr, await_simple, await_chain, await_then_try, try_simple, try_after_await, try_then_coalesce, coalesce_not_try, tagged_template_adjacent, tagged_template_after_access, macro_call_simple, macro_call_nested_parens, macro_call_with_string, error_tagged_template_with_space (ends the expression → unconsumed input reported as error via the statement test), error_placeholder_in_binop, error_call_unclosed, error_index_unclosed, error_access_no_name, error_macro_unbalanced.

**expression/lambda.rs**: lambda_expr_body, lambda_no_params, lambda_multiple_params, lambda_typed_params, lambda_return_type, lambda_block_body, lambda_block_single_name_is_block, lambda_assign_body, lambda_pattern_param, lambda_mut_param, error_missing_parens, error_missing_body, error_bad_param, error_assign_no_value.

**expression/if\_.rs**: if_no_else, if_else, if_else_if, if_else_if_else, if_multiline, if_condition_call, if_condition_path_no_record_ctor, if_condition_parenthesized_record_ctor, if_nested, if_block_tail_values, error_missing_block, error_then_keyword, error_else_dangling, error_condition.

**expression/match\_.rs**: match_simple, match_multiple_arms, match_trailing_comma, match_no_trailing_comma, match_newline_separated_arms, match_alternatives, match_guard, match_alternatives_with_guard, match_block_body, match_block_single_name_is_block, match_ctor_args, match_tag_patterns, match_wildcard, match_pin, match_scrutinee_path_no_record_ctor, error_missing_arrow_thin_arrow, error_of_keyword, error_missing_body, error_unclosed, error_bad_pattern.

**expression/loop\_.rs**: loop_simple, loop_break_value, loop_nested, state_simple, state_expr, error_loop_missing_block, error_state_no_parens.

**statement.rs**: block_empty, block_tail_only, block_stmt_and_tail, block_stmts_no_tail, block_nested, block_looks_like_record_hint, let_simple, let_mut, let_annotated, let_pattern_tuple, let_pattern_record, let_multiline_value, assign_var, assign_field, assign_tuple_index, assign_index, assign_add, assign_sub, assign_mul, assign_div, expr_stmt_call, expr_stmt_if_then_stmt, for_simple, for_pattern, for_nested, while_simple, return_bare, return_value, return_newline_no_value, return_before_brace, break_bare, break_value, continue_, use_simple, use_path, provide_simple, provide_nested, assert_simple, assert_comparison, assert_await, style_let, two_calls_on_lines, call_after_newline_is_new_stmt, index_after_newline_is_new_stmt, array_after_newline_is_new_stmt, markup_after_expr_on_next_line, negative_after_newline_is_new_stmt, minus_with_space_after_newline_continues, pipe_after_newline_continues, error_same_line, error_assign_target, error_assign_target_slash_equals, error_semicolon, error_let_missing_equals, error_for_missing_in, error_unclosed_block, error_stmt_start.

**pattern/term (pattern/mod.rs)**: wildcard, variable, number, negative_number, bigint, string, bool_true, bool_false, unit, pin_var, pin_access, pin_call, pin_parens, alias_simple, alias_ctor, alias_tuple, alternatives_two, alternatives_three, error_wildcard_not_var, error_reserved, error_alias_no_name, error_start.

**pattern/ctor.rs**: ctor_bare, ctor_qualified, ctor_one_arg, ctor_many_args, ctor_nested, ctor_record, ctor_record_rename, ctor_record_rest, tag_bare, tag_args, error_ctor_unclosed, error_ctor_record_field, error_tag_name, error_path_dangling.

**pattern/tuple.rs**: pair, triple, nested, parenthesized_single, error_unclosed.

**pattern/array.rs**: empty, single, multiple, rest_anonymous, rest_named, rest_only, error_rest_not_last, error_unclosed.

**pattern/record.rs**: empty, single_shorthand, multiple_shorthand, renamed_field, nested_pattern, rest, error_rest_not_last, error_unclosed.

**type\_.rs**: var, var_applied (`f[a]`), var_applied_nested (`t[f[a]]`), named_simple, named_qualified, app_one_arg, app_many_args, app_nested, app_result_shorthand, fn_no_params, fn_one_param, fn_many_params, fn_returning_fn, fn_hkt (`fn(a) -> f[b]`), unit, tuple_pair, tuple_triple, parenthesized, record_empty, record_fields, record_optional_field, record_extension, record_trailing_comma, record_multiline, error_row_empty, error_row_single, error_row_args, error_row_open, error_row_var_only, error_app_unclosed, error_app_empty, error_fn_missing_arrow, error_record_missing_colon, error_record_ext_no_fields, error_start, error_reserved.

**item/attribute.rs**: attr_bare, attr_args, attr_derive, attr_multiple, error_attr_open, error_attr_unclosed, error_attr_name, error_attr_dangling.

**item/import.rs**: package_root, package_nested, package_alias, package_names, package_names_alias, package_all, package_reserved_segment_names (`import @alder/test.{ fakeDb }` — `test` is a legal segment, §2.4), package_reserved_segment_alias (`import @alder/test as t`), package_reserved_segment_all, local_root (`import ~/db`), local_nested (`import ~/db/users`), local_names, local_root_only_names (`import ~.{ config }`), pub_reexport_names, pub_reexport_all, trailing_comma, error_bad_root, error_missing_slash, error_tail, error_alias_uppercase, error_names_alias_no_name (`.{ x as }` → `Import::NameAlias`), error_pub_needs_names, error_reserved_binding (bare `import @alder/test` → `Import::ReservedBinding(Test)`), error_root_only (bare `import ~` → `Import::RootOnly`).

**item/fn\_.rs**: fn_no_params, fn_params, fn_typed_params, fn_ret, fn_mut_param, fn_pattern_param, fn_where_single, fn_where_multi, fn_where_plus, fn_where_assoc, fn_where_multiline_trailing_comma, fn_pub, fn_bodiless, fn_bodiless_with_extern_attr, fn_trailing_comma_params, error_no_name, error_params_unclosed, error_where_bad_bound, error_body.

**item/let\_.rs**: let_top, let_top_pub, let_top_mut_state, let_top_annotated, let_style.

**item/type_alias.rs**: alias_simple, alias_params, alias_record, alias_fn, opaque_type, opaque_type_with_attr, error_alias_no_body, error_params_unclosed, error_params_empty.

**item/enum\_.rs**: enum_unit_variants, enum_tuple_variant, enum_record_variant, enum_mixed, enum_params, enum_trailing_comma, enum_pub, enum_empty, error_variant_lowercase, error_unclosed, error_variant_arg, error_variant_record_extension (`Rect { r | width: Number }` → `Enum::VariantRecordExt`).

**item/trait\_.rs**: trait_single_fn, trait_hkt, trait_default_body, trait_assoc_type, trait_where, trait_multiple_items (one per line), error_no_params (`trait Show {` → `Trait::Params(TypeParams::Open)`), error_bad_item, error_assoc_type_has_body, error_semicolon_between_items (language.md's one-line `trait Iterator[i] { type Item; fn next(it: i) -> Option[Item] }` → `Trait::Semicolon`), error_same_line_items (`{ type Item fn next(it: i) -> Item }` → `Trait::SameLine`).

**item/impl\_.rs**: impl_simple, impl_hkt, impl_assoc_type, impl_where, impl_multiple_fns, error_no_args, error_bad_item, error_assoc_no_type, error_semicolon_between_items (`Impl::Semicolon`), error_same_line_items (`Impl::SameLine`).

**item/error\_.rs**: error_group_simple, error_group_args, error_group_trailing_comma, error_group_empty, error_bad_tag, error_unclosed.

**item/component.rs**: component_simple, component_props, component_lowercase_page, component_pub, component_with_state_and_markup, error_no_body.

**item/table.rs**: table_single_column, table_modifiers, table_modifier_args, table_multiple_columns, table_pub, error_missing_colon, error_builder.

**item/schema.rs**: schema_simple, schema_from, schema_pick, schema_typed_rules, schema_untyped_rules, error_from_no_table, error_bad_item.

**item/macro\_.rs**: macro_no_params, macro_params, macro_nested_braces, macro_quote_body_raw, comptime_block, error_unbalanced.

**item/test.rs**: test_simple, test_provide, tests_block_empty, tests_block_import_and_tests, error_test_no_name, error_test_name_bad_string (`test "unterminated` → `Test::NameString`), error_tests_bad_item, error_tests_same_line (`Tests::SameLine`).

**item/mod.rs**: pub_fn, pub_enum, attr_then_pub, multiple_attrs, error_pub_alone, error_unknown_start.

**module.rs**: empty_module, single_fn, imports_then_items, leading_comments, docs_counter_component (language.md `Counter`), docs_classify_fn, docs_find_result, docs_load_await, docs_traits_functor (with `->` fixed and the `Iterator` trait written one item per line), docs_tests_block (with the path-first `import @alder/test.{ fakeDb }` — `test` is reserved but a legal path segment, §2.4), docs_web_load_page, docs_web_login_form (`<Field name="password" type="password" />`), docs_web_tui_app, docs_data_tables, docs_data_queries, error_bad_end, error_item, error_same_line_items (`fn a() {} fn b() {}` → `Module::SameLine`), error_semicolon_after_item (`Item::Semicolon`).

**markup/**: element_empty, element_self_closing, element_text, element_hole, element_text_and_holes, element_nested, element_attr_string, element_attr_expr, element_attr_boolean, element_attr_dashed, element_attr_reserved_name (`<Field name="password" type="password" />`), element_attr_reserved_names_html (`<label for="x" style="color: red">`), element_reserved_tag_name (`<table><tr><td>x</td></tr></table>`), element_reserved_tag_name_style (`<style>.a { }</style>`, children are text), element_attr_lambda_assign, element_component, element_component_path, element_custom_dashed, fragment, whitespace_only_lines_dropped, text_keeps_inner_spaces, text_with_at_sign, text_with_punctuation, hole_record, directive_if, directive_if_else, directive_if_else_if, directive_if_else_next_line, directive_for, directive_for_key, directive_for_empty, directive_for_tuple_pattern, directive_match, directive_match_child_body, directive_match_block_body, directive_match_guard, child_block_let, child_block_use, nested_directives, markup_in_match_arm, markup_as_block_tail, markup_after_newline_new_stmt, error_name, error_tag_end, error_close_mismatch, error_unclosed, error_fragment_unclosed, error_stray_close_brace, error_attr_value, error_attr_expr_unclosed (`<div class={x >` → `Attr::ExprEnd`), error_hole_empty, error_hole_unclosed, error_directive_unknown, error_else_without_if, error_empty_without_for, error_for_missing_in, error_for_key_keyword, error_match_bare_text, error_child_block_end.

**query.rs**: select_star, select_fields, select_alias, select_join, select_left_join, select_inner_join, select_where, select_where_pin, select_where_pin_access, select_where_pin_parens, select_in, select_group_by, select_order_by_asc, select_order_by_desc, select_order_by_default, select_limit_offset, select_full_docs_example, insert, update, update_where, delete, delete_where, sql_words_are_identifiers_outside_query, error_open, error_verb, error_missing_from, error_join_missing_on, error_pin_no_operand, error_clause_order (`where` after `orderBy` → `ClauseOrder(Where)`), error_clause_repeated (`limit 1 limit 2` → `ClauseOrder(Limit)`), error_sql_keyword_as_operand, error_insert_not_pinned, error_unclosed.

**style.rs**: style_empty, style_dimension, style_percent, style_float_dimension, style_negative_dimension (`margin: -8px`), style_unitless_number (`opacity: 1` → `Expr`), style_negative_expr (`margin: -x` → `Negate`), style_expr_value, style_string_key_nested, style_media_nested, style_trailing_comma, error_missing_colon, error_unit_space, error_unclosed.

---

## 8. File ownership and build order

Wave 0 is one agent and blocks everything. After it, **no one edits a
shared file** (`alder-source/src/lib.rs`, `error.rs`, `lib.rs`, `space.rs`,
`keyword.rs`, `symbol.rs`, `name.rs`). A needed variant or helper is
filed to the Wave 0 owner, who lands it as one additive commit; the
requester continues with a nearby variant and a `// TODO(wave0)`.

Every owned function exists from Wave 0 as a `todo!()` stub with its
final signature and a `// OWNER: <file>` comment, so the crate compiles
at every merge. A test that transitively hits another owner's stub is
written now and marked `#[ignore = "waits for <file>"]`; the test's
owner removes the attribute when the dependency lands.

| Wave | File(s)                                                                                                                                                     | Runtime deps (all signatures fixed in W0)                          |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| 0    | `alder-source/src/lib.rs`, `error.rs`, `lib.rs`, `space.rs`, `keyword.rs`, `symbol.rs`, `name.rs`, stubs for every file below, root `Cargo.toml`, changeset | —                                                                  |
| 1    | `number.rs`                                                                                                                                                 | —                                                                  |
| 1    | `string.rs`                                                                                                                                                 | —                                                                  |
| 1    | `raw.rs`                                                                                                                                                    | `string.rs` (escape scanning)                                      |
| 1    | `type_.rs`                                                                                                                                                  | —                                                                  |
| 1    | `pattern/` (`mod`, `ctor`, `tuple`, `array`, `record`)                                                                                                      | `number`, `string`; `Pin` needs `postfix` (tests ignored until W2) |
| 2    | `expression/mod.rs`, `postfix.rs`, `literal.rs`, `array.rs`, `tuple.rs`, `record.rs`, `path.rs`, `tag.rs`, `loop_.rs` (state, macro_call)                   | `number`, `string`, `raw`; `block` for `{ }` / `loop`              |
| 2    | `template.rs`                                                                                                                                               | `expression`                                                       |
| 2    | `statement.rs`, `item/let_.rs`                                                                                                                              | `expression`, `pattern`, `type_`                                   |
| 2    | `expression/lambda.rs`, `if_.rs`, `match_.rs`                                                                                                               | `expression`, `block`, `pattern`, `item/fn_.rs::params` (stub)     |
| 3    | `markup/mod.rs`, `markup/directive.rs`                                                                                                                      | `expression`, `pattern`, `let_decl`, `arm_head`                    |
| 3    | `query.rs`                                                                                                                                                  | `expression`, `record_fields`                                      |
| 3    | `style.rs`                                                                                                                                                  | `expression`, `number`                                             |
| 3    | `item/mod.rs`, `attribute.rs`, `import.rs`, `type_alias.rs`, `test.rs`, `macro_.rs`                                                                         | `block`, `type_`, `raw`                                            |
| 3    | `item/fn_.rs`, `enum_.rs`, `trait_.rs`, `impl_.rs`, `error_.rs`, `component.rs`                                                                             | `block`, `pattern`, `type_`                                        |
| 3    | `item/table.rs`, `item/schema.rs`                                                                                                                           | `expression`, `type_`                                              |
| 4    | `module.rs` docs-example tests, SPEC.md, cleanup                                                                                                            | everything                                                         |

Files in the same wave build in parallel; a file may start early once
its runtime deps have landed.

---

## 9. Implementation order (numbered, one step per agent hand-off)

**Wave 0 — foundation (serial, one agent)**

0.1 Rewrite `crates/alder-source/src/lib.rs` to §3 exactly. `cargo build -p alder-source`.
0.2 Rewrite `crates/alder-parse/src/error.rs` to §4; write `keyword.rs` (§4.1, §5.3).
0.3 Rewrite `lib.rs` (§5.1): drop `indent` and its helpers, add the flags and helpers, the `mod` list, and `parse_module`. Rewrite `space.rs` (§5.2), `symbol.rs` (§5.4), `name.rs` (§5.8, moving the surviving helpers out of `expression/variable.rs`).
0.4 Delete: `exposing.rs`, `import.rs`, `test_support.rs`, `declaration/`, `expression/{accessor,case,let_,variable,list,number,string}.rs` (`expression/number.rs` and `expression/string.rs` are superseded by the new `expression/literal.rs`; the primitive scanners stay in the crate-root `number.rs` / `string.rs`), `pattern/list.rs`, every `snapshots/` directory under `crates/alder-parse/src`. Every other existing file whose path coincides with a §5 file (`expression/{mod,if_,lambda,record,tuple}.rs`, `pattern/mod.rs`, `type_.rs`, `module.rs`, …) is overwritten by its 0.5 stub, not kept; nothing left behind may reference a removed AST type, or the 0.8 gate fails.
0.5 Create every file of §5.5–§5.18 with `todo!()` bodies, `#[allow(unused)]` on stubbed impl blocks, and its `#[cfg(test)]` macro pair (§7.1) with an empty `mod tests`.
0.6 Root `Cargo.toml`: add `default-members = ["crates/alder-region", "crates/alder-source", "crates/alder-parse", "crates/alder-config"]` under `[workspace]` (revert in M2).
0.7 Write `.sampo/changesets/parser-rewrite-m1.md` (`cargo/alder-source: minor`, `cargo/alder-parse: minor`).
0.8 Gate: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test` (default members) green. Merge.

**Wave 1 — leaves (parallel, 5 agents)**

1.1 `number.rs` + unit tests.
1.2 `string.rs` + unit tests.
1.3 `raw.rs` + unit tests.
1.4 `type_.rs` + snapshot tests (incl. `tag_variant`, `field_types`).
1.5 `pattern/*` + snapshot tests (`pin_*` tests `#[ignore]`d).

**Wave 2 — expression core and statements (parallel, 4 agents)**

2.1 `expression/{mod,postfix,literal,array,tuple,record,path,tag,loop_}.rs` + tests (block/if/match/lambda-dependent tests ignored).
2.2 `template.rs` + tests.
2.3 `statement.rs` + `item/let_.rs` + tests (SameLine, newline rules, assign/place).
2.4 `expression/{lambda,if_,match_}.rs` + tests (`params` from the `item/fn_.rs` stub — coordinate: this agent also implements `params` and `where_clause` in `item/fn_.rs` if Wave 3 has not started them; otherwise waits).

**Wave 3 — modes and items (parallel, 6 agents)**

3.1 `markup/mod.rs`, `markup/directive.rs` + tests.
3.2 `query.rs` + tests.
3.3 `style.rs` + tests.
3.4 `item/{mod,attribute,import,type_alias,test,macro_}.rs` + tests.
3.5 `item/{fn_,enum_,trait_,impl_,error_,component}.rs` + tests.
3.6 `item/{table,schema}.rs` + tests.

**Wave 4 — integration (serial, one agent)**

4.1 `module.rs` docs-example tests (§7.2 `module.rs` list); fix the doc typos listed in §10.35 (language.md: the missing `->` in `map`, the one-line `trait Iterator` example, the `import { fakeDb } from @alder/test` line; data.md: the `^` precedence sentence) and record the remaining disagreements (§10.40 `provide` tail) for M2.
4.2 Remove every `#[ignore]` and `#[allow(unused)]`; `cargo insta test --unreferenced delete`.
4.3 Update SPEC.md grammar with §10 decisions; tick the M1 checklist.
4.4 Final gate: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test`, plus `cargo build --workspace` to record the known-broken crate list for M2.

Gate at every merge: `cargo fmt --all && cargo clippy -p alder-source -p alder-parse --all-targets -- -D warnings && cargo insta test -p alder-parse`.

---

## 10. Decisions (grammar silent or corrected)

Each item is a proposed SPEC.md / docs change unless marked _(internal)_.

1. **Flat binop chains.** Keep Elm's `BinOps { operands, last }`; precedence and associativity are resolved in canonicalization from `BinOp::precedence()` (`|>` 0 L, `??` 1 R, `||` 2 L, `&&` 3 L, comparisons and `in` 4 non-assoc, `+ -` 6 L, `* / %` 7 L). `%` is added. SPEC's separate `pipe` production is folded into the table.
2. **Fixed operator table with longest match** instead of Elm's maximal munch, so `a==-1` and `x<-1` parse. Elm-habit tokens (`->`, `|`, `++`, `::`, `..`, `<|`, `>>`, `<<`, `^`) are recognized only to produce `BadOperator` hints. `= => += -= *= /=` terminate a chain and are never operators; `/=` is compound assignment only (the `Stmt::AssignTarget(AssignOp::Div, …)` renderer mentions `!=`).
3. **Newline rules** (§2.1): postfix `.` only across lines; operators continue except adjacent `-` and markup-shaped `<`; consecutive statements need a line break (`Block::SameLine`) and so do consecutive items (§10.38); `return`/`break` values on the same line; same-line `Path {`; adjacency for `` tag` `` and `name!(`. SPEC's "whitespace insignificant" must be amended.
4. **`{` record vs block** (§2.2): grammar-block positions (including lambda and match-arm bodies starting with `{`) are always blocks; elsewhere the lookahead rule decides. `Block::LooksLikeRecord` hint for `{ a: 1 }` in block position.
5. **Record constructors** need the `{` on the same line and are disabled under `no_record_ctor` in if/while/for/match/provide/@directive heads; brackets and holes clear the flag (Rust's rule).
6. **Reserved words** gain `assert` (statement `Stmt::Assert`, since juxtaposition does not exist and docs use it) and `await` (so `.await` cannot collide with a field).
7. **`true` / `false`** are literal primaries (`Expr::Bool`, `Pattern::Bool`); SPEC's `primary` omits them.
8. **Trailing commas** are accepted in every comma-separated list; SPEC marks only some.
9. **Tabs** are whitespace; only `//` comments exist; `chomp` is infallible; the `Space` error enum is gone. Doc comments (`///`, `//!`) are skipped as comments in M1; attachment (an `Item.docs` field) is deferred to the documentation-generator milestone.
10. **Numbers** are stored as `NumberLit { value: f64, text }` so codegen/formatter keep the spelling; BigInt keeps its digit text without `n`; `1.`, `1e`, `1.5n` are errors. Tuple indices are bare digit runs (`t.0.1` is two accesses).
11. **Strings** are single-line `"…"` only (`"""` dropped); templates cover multi-line text. Template escapes add `` \` `` and `\$`.
12. **Tagged templates** (`` sql`…` ``, `` css`…` ``) are a postfix op requiring adjacency; SPEC should add `postfix template`.
13. **Lambda bodies** accept `block | assignment | expression` (docs write `fn() count += 1`); an assignment body is a synthetic one-statement block. SPEC: `lambda = 'fn' '(' [params] ')' ['->' type] ( block | assign | expression )`.
14. **HKT type application**: `Type::Var { name, args }` represents `f[a]`, `t[f[a]]`; SPEC's `type_app` gains `lower_ident [ '[' type { ',' type } ']' ]`.
15. **Uppercase module-style access** (`Array.map`, `Http.get`, `Fiber.all`) parses as `Access` on `Expr::Path`; canonicalization decides what the path denotes. Docs conflict with language.md's lowercase-module rule; flagged, not resolved here.
16. **`component` names** may be lowercase (`pub component page(...)` in web.md) or uppercase; SPEC says upper only.
17. **Patterns** accept negative number literals and the unit pattern `()` (for `Ok(())`); SPEC omits both. `_foo` keeps Elm's `WildcardNotVar` (identifiers start with a letter).
18. **`_` placeholders** only as a whole call argument (parsed in `call_args`); elsewhere `Expr::Placeholder` error.
19. **Postfix `?`** applies whenever the next byte is not `?`, so `x? ?? y` and `a ?? b` both work.
20. **`^`** parses only in query mode and in patterns; elsewhere `Expr::PinOutsideQuery`. The pin operand is a **whole postfix chain** parsed with query mode off: `^user.id` pins `user.id`, `^f(x)` pins the call, `^select` and `^{ a, b }` work, and `^(a + b)` pins the sum — so `^` binds looser than `.`, calls and indexing but tighter than every binary operator. This follows data.md's example, not its prose sentence (flagged in §10.35). **`in`** is a binop only in query mode.
21. **Query mode** is the `in_query` flag; `lower_name` refuses SQL words there so clause operands stop cleanly and a misplaced SQL word is `Expr::SqlKeyword`. Clauses must appear in grammar order and (except `join`) at most once; violations are `Query::ClauseOrder(clause)`, where `error::Clause` names the clause — including `Where`, which `SqlWord` cannot express because `where` is a reserved word rather than a SQL word. `select { … }` and `set { … }` are parsed by the query parser, not via the record-vs-block rule. Bare `join` is `JoinKind::Plain`.
22. **Markup text** is kept raw except that whitespace-only runs containing a newline are dropped (JSX rule); text stops at `@` only before `if`/`for`/`match`/`else`/`empty` + non-ident byte, so `a@b.com` is text. Element names accept dashes (custom elements); attribute names accept dashes. Element, attribute and close-tag names are keyword-insensitive (§2.4, §10.36).
23. **`child_block` items** are `let` / `let mut` / `use` statements or children; other statement forms are not recognized there (write `{expr}`). SPEC: `child_block = '{' { let_decl | 'use' path | child } '}'`.
24. **`@match` arm bodies** after `=>` must be an element, fragment, directive, or braced child block; bare text is `DirMatch::BareText` (it would swallow the next arm). A `{` after `=>` is always a child block.
25. **`pub import`** requires `.{ … }` or `.*` (`Import::PubNeedsNames`), per SPEC's `reexport`.
26. **Bodiless `fn`** parses as `FnDecl { body: None }` and `type Name` without `=` as `ItemKind::OpaqueType`; the `#[extern]` requirement (and the extern return type) is validated by canonicalization, not the parser. Trait `type Item = …` is `Trait::AssocTypeHasBody`.
27. **Style values**: a number immediately followed by letters or `%` is `StyleValue::Dimension`; a `-` immediately followed by a digit starts a dimension too (`margin: -8px` → `Dimension` with `value` -8 and `text` `"-8"`), while `-` followed by anything else is an ordinary `expression()` (`Negate`); `{` after `:` in a style block is always a nested style; anything else is an expression. Chosen so the M8 style owner does not inherit a `Number::End` for the most common negative margin. Style bodies are parsed in M1 (SPEC allows deferral).
28. **Table columns**: modifiers are bare identifiers after the builder; the next column is detected by `ident ':'` lookahead. **Schema fields**: a lowercase word after `:` starts the rule list, otherwise a type followed by `,` and rules.
29. **Macro bodies and `name!(…)` arguments** are raw balanced text (`Located<&str>`); `quote`/`unquote`/`stringify` are M5 and never reach the parser in M1.
30. **Module** is a flat ordered item list; `Module::imports()` serves the driver instead of Elm's categorized fields.
31. **AST layout** _(internal)_: names inline as `Name<'a>`; Copy leaf structs in by-value slices; nodes behind `&'a Located<…>`; `Block` is always `&'a Located<Block>` (also inside `Expr::Block`); the `(node, end)` return tuple is replaced by `node.region.end`.
32. **Test macros** _(internal)_: per-module pairs, `description => code`, EOF assertion, no indented variants, `test_support.rs` deleted.
33. **Workspace** _(internal)_: `default-members` narrowed to region/source/parse/config for the duration of M1 so fmt/clippy/test stay green while can/constrain/solve/driver/cli/lsp are knowingly broken.
34. **Import syntax** follows language.md's Modules section and SPEC (path-first `import @alder/test.{ fakeDb }`); the JS-style `import { x } from @pkg` in the Tests section of language.md, in data.md and in web.md is stale and should be corrected.
35. **Docs typos flagged for the docs owner**: `fn map(fa: f[a], g: fn(a) -> b) f[b]` is missing `->`; `error` is reserved but web.md uses `handleError(error: Error, …)`; `web.md` uses `db.run(query { … })` with `event.params.id` unpinned (should be `^event.params.id`); language.md's one-line `trait Iterator[i] { type Item; fn next(it: i) -> Option[Item] }` uses `;` between trait items — items are line-break separated (§10.38), so it must be rewritten one item per line; language.md's tests example imports with the stale `import { fakeDb } from @alder/test` (see §10.34 — the package name `test` itself is fine, §10.37); data.md says "`^` binds tighter than `.` and calls", which contradicts its own example (`^user.id` pins `user.id`) and the parser (§10.20) — the sentence should read "`^` binds looser than `.`, calls and indexing but tighter than every binary operator".
36. **Markup names are keyword-insensitive.** Element names, attribute names and close-tag names are read with `dashed_name` over `raw_lower` and never consult the reserved or SQL word lists (§2.4), so `<table>`, `<style>`, `<select>`, `<form>`, `type="password"`, `for=`, `style=` all parse (web.md's `<Field name="password" type="password" />` requires it). SPEC: annotate `element_name` / `attr_name` with "not subject to reserved words".
37. **Module-path segments are keyword-insensitive.** `@author/package` and every `/` segment use `raw_lower`, so `@alder/test` and `~/db/users` parse (§2.4); language.md keeps the `@alder/test` package name. What a bare `import` binds is validated separately: a reserved last segment (`import @alder/test`) is `Import::ReservedBinding(Test)` and a root-only local path (`import ~`) is `Import::RootOnly`; both are fine with `as name`, `.{ … }` or `.*`. SPEC: `module_path` segments are `raw_lower`, and `import module_path` without a tail requires a bindable (non-reserved, present) last segment.
38. **Item separation.** Items in a module, `tests { }`, `trait { }` and `impl { }` body follow the statement rule (§2.1 rule 3): the next item must start on a later line or be `}` / EOF, else `Module::SameLine` / `Tests::SameLine` / `Trait::SameLine` / `Impl::SameLine`; two items on one line are never legal. `;` is not a separator anywhere: `Item::Semicolon`, `Trait::Semicolon`, `Impl::Semicolon` (and `Stmt::Semicolon`) carry the "separate with a line break" hint. SPEC: add "items and statements are separated by line breaks; `;` is never a separator" next to the whitespace amendment of §10.3.
39. **Enum record variants take no extension.** `enum_decl` reuses `field_types()`; a `Some(ext)` result (`Rect { r | width: Number }`) is `Enum::VariantRecordExt` at the extension name. SPEC: `variant_record = '{' field_type { ',' field_type } [','] '}'` with no `ext`.
40. **`provide` is a statement in M1, and web.md's `handle` needs it to be an expression.** `handle` ends its body with `provide Session = session { resolve(event).await }` and expects that to be the function's `Task[Response]`, but `Stmt::Provide` gives the block no `tail`. The parser is not blocked (the body parses with `tail: None`); the SPEC/docs disagreement is recorded here for M2, which either promotes `provide … { }` to an expression whose value is its body's value (a rename `Stmt::Provide` → `Expr::Provide`, parsed by `primary` at the `provide` keyword; no other parser change) or makes web.md write `return`/a tail. The `no_record_ctor` rule for `provide … =` heads (§2.3) holds either way.
41. **Test-macro placement** _(internal)_: the `assert_<thing>_snapshot!` pair is defined at module level under `#[cfg(test)]` and re-exported with `pub(crate) use`, not inside `mod tests` as CLAUDE.md says, because child modules must import the parent's pair (§7.1). Wave 4 rewords the CLAUDE.md sentence to "macros are defined at module level under `#[cfg(test)]` and re-exported with `pub(crate) use` so submodules can import them".

---

## 11. Risks

- The newline rules and the statement-separation rule are user-visible semantics not yet in SPEC/docs; they must be documented and the M2 formatter should normalize toward them.
- Lambda and match-arm `{` always being a block means `fn(u) { ..u, name }` needs parentheses; the `Block::LooksLikeRecord` hint covers `{ a: 1 }` but not spread-first records — extend the hint to `..` if it bites.
- `<` in operand position followed by a letter after a newline is a new markup statement; `a\n<b` typed as a comparison yields a markup error. Acceptable (JSX has the same), but `Markup::Name` rendering should mention comparisons.
- Contextual SQL words cannot be column names inside `query { }` (`limit`, `set`, `on`); a quoting escape is not designed here (M7).
- Markup whitespace (drop newline-only runs, keep the rest verbatim) is a rendering decision the parser is making; M6 may want JSX-style trimming as a later pass.
- Query, style, table, schema and macro grammars are the least specified parts of SPEC; their AST may churn in M5/M7/M8 and invalidate snapshots.
- alder-ast/can/constrain/solve/driver/cli/lsp are red for all of M1; `cargo build --workspace` should be run occasionally to keep the breakage list known.
- `one_of` with boxed closures allocates per alternative; the peek-dispatch rule (§5.1) keeps it out of the hot postfix/binop loops.
- Cross-owner `todo!()` stubs panic at test time; Wave 4 must grep for `#[ignore` and `#[allow(unused)` and remove every one.
- Deep nesting recurses on the Rust stack as the Elm port does today; no depth guard in M1.
- Items on one line are errors (§2.1 rule 3, §10.38); every one-line docs sample (`trait Iterator[i] { type Item; fn next … }`) must be reformatted, and the M2 formatter must never emit two items on a line.
