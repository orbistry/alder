# Parser diagnostic parity audit

Status: completed and validated on `diagnostic-ux`, 2026-09-06. The acceptance checklist
is `plans/parser-diagnostic-parity.md`; the earlier type/pattern-checking diagnostic
goal did not establish parser-message parity.

## Active boundary

`alder-parse/src/lib.rs` declares the parser modules; `error.rs` supplies their
nested errors. The driver calls `Parser::module` and passes its `Module` error to
`report::parse`, implemented in the source-aware `report/syntax.rs` module.
The `Error::ParseError` wrapper belongs to the public convenience
parsing APIs and is not an additional driver failure family. No inactive Elm-port
file is used as evidence of implementation or test coverage.

Deferred execution does not exempt syntax: tables, schemas, macros, comptime,
styles, queries, components, and markup all have active parsing paths. Their
rendering is included below, independently of later codegen rejection.

## Variant inventory

This inventories the definitions, including nested wrappers. A defined variant
is not necessarily emitted today: for example, lambda parameter/return parsing
backtracks rather than constructing `Lambda::Params` or `Lambda::Ret`. The final
coverage audit distinguishes source-reachable cases from defensive handlers below.

### Declarations, types, and shared leaves

```text
Module: Item, SameLine, BadEnd
Attribute: Open, Name, Arg, ArgEnd, End, Dangling
Import: Path, Tail, Name, NameAlias, NamesEnd, Alias, PubNeedsNames, ReservedBinding, RootOnly
ModulePath: Start, Author, Slash, Package, Segment
Fn: Keyword, Name, Params, Ret, Where, Body
Params: Open, Pattern, Type, OptionalAnnotation, End
Where: Var, Colon, Bound, AssocName, AssocEq, Type
Let: Pattern, Type, Equals, Body
TypeAlias: Name, Params, Body
TypeParams: Open, Var, End, Empty
Enum: Name, Params, Open, Variant, VariantArg, VariantArgEnd, VariantRecord, VariantRecordExt, End
Trait: Name, Params, Where, Open, Item, SameLine, Semicolon, AssocType, AssocTypeHasBody, Fn
Impl: Trait, PathMember, Open, Arg, ArgEnd, Where, BodyOpen, Item, SameLine, Semicolon, AssocType, AssocEquals, AssocBody, Fn
ErrorDecl: Name, Open, Tag, End
TagVariant: Name, Arg, ArgEnd
Component: Name, Params, Body
Block: Open, Stmt, SameLine, LooksLikeRecord, End, TooDeep
Type: Start, Reserved, PathMember, Args, Fn, Tuple, Record, ErrorRow, TooDeep
TArgs: Type, Empty, End
TFn: Open, Param, ParamEnd, Ret
TTuple: Type, End
TRecord: Field, Colon, Type, ExtField, End
TErrorRow: Start, Tag, Ext, End
Number: End, Dot, Exponent, HexDigit, NoLeadingZero, BigIntFraction
StringError: Endless, Newline, Escape
BadOperator: Arrow, Bar, PlusPlus, DoubleColon, DotDot, PipeLeft, ComposeRight, ComposeLeft, Caret
```

Nested causes delegate through these handlers, with the nearest useful enclosing
context retained. `Type::Reserved` names and highlights the entire
keyword. String escapes have dedicated detailed handlers, described below.
The shared “I got stuck here” fallback has been removed: entry-point leaves now
label the expected syntax class. Eight source-based entry fixtures cover invalid
module input, `pub` at EOF, missing types/patterns/expressions, reserved type words,
and lowercase type path members. The generic expression-entry expectation does
not prescribe a value: the parser has no evidence of which expression was intended.

### Initial core implementation

The type-family pass adds 19 malformed-source rendered fixtures, covering every
`TArgs`, `TFn`, `TTuple`, `TRecord`, `TErrorRow`, and `TagVariant` variant together
with the existing empty-argument fixture. Nested type failures keep the nearest
type delimiter instead of only the outer declaration. Separator/closer advice is
also present in help text so it remains visible at zero-width EOF. The record-field
colon message now correctly asks for punctuation, not a type before the colon.
The `Type::TooDeep` guard is covered by the generated-source nesting tests below.

Another 23 rendered declaration fixtures cover every `Enum`, `ErrorDecl`, and
`Component` variant, plus alias names and type-parameter errors. Reviewing these
against the parser corrected three misleading claims: component names are not
uppercase-only; a malformed error tag may be missing its colon entirely; and an
empty type-parameter list is invalid without making type parameters mandatory for
all enums and aliases. Enum record payloads and component parameter lists now
retain their nearer opening context.

Nineteen additional fixtures cover every `Attribute`, `Import`, and `ModulePath`
variant. Attribute failures retain the enclosing attribute sequence; reserved
module bindings highlight the complete keyword. The selected-name alias diagnostic
no longer incorrectly requires lowercase (module aliases still do). Public import
help uses a syntactically valid `~/module` path rather than a bare placeholder path.

Thirty trait/impl/`where` fixtures cover every variant of those families, including
associated types, same-line and semicolon rejection, missing type-parameter openers,
and nested method failures. A missing bound after `+` no longer claims to follow
`:`. Trait/impl method wrappers preserve their start when there is no nearer
delimiter or local-binding context. The `where` parser's existing decision to stop
before a non-lowercase first constraint is unchanged; the variable-error fixture
uses a reserved word to exercise the actual `Where::Var` path.

The direct-leaf audit found that `Expr::OperatorReserved` still discarded its
`BadOperator` payload despite the earlier inventory grouping it with dedicated
handlers. It now exhaustively distinguishes all nine reserved operators, highlights
the entire symbol, and explains the relevant Alder syntax or alternative. Nine
source-based snapshots cover this correction, including Elm application/composition
habits, boolean OR, path syntax, rest/spread syntax, and pins versus exponentiation.

Twenty-two direct expression/pattern/number fixtures cover reserved/query words,
missing path members and accessors, unary/binary operands, pins, placeholders,
closing tags, pattern aliases and wildcard-like names, and all six `Number`
variants. Reserved words and wildcard-like names now retain their full widths.
Wildcard guidance asks for a name beginning with a letter rather than assuming
that removing one underscore always repairs the name.

Six generated-source rendered tests reach all six parser nesting guards:
`Expr`, `Block`, `Pattern`, `Type`, `Style`, and markup `Child`. These use the
actual public nesting limit and parse multiline source rather than constructing
errors directly. The snapshots retain the nearest useful opener and the actual
failure location. This exposed and fixed discarded `Expr::Loop` context; ordinary
expression blocks now retain their opener too. Pattern depth guidance explains how
to split the match rather than suggesting a syntax change or raising the limit.

```text
Array: Expr, End
Tuple: Expr, End
Record: Field, Spread, Expr, End, EqualsNotColon
Lambda: Params, Ret, Body, Block, AssignTarget, AssignValue
If: Condition, Then, ThenKeyword, ElseBranchStart, Else
Match: Scrutinee, Open, Arm, End
Arm: Pattern, Guard, Arrow, Body, Block
Call: Arg, End
Index: Expr, End
Tag: Name, Arg, End
State: Open, Expr, End
Provide: Name, Equals, Value, Body
For: Pattern, In, Iter, Body
While: Condition, Body
PCtor: Arg, End, Record
PTuple: Pattern, End
PArray: Pattern, RestNotLast, RestName, End
PRecord: Field, Pattern, RestNotLast, End
```

These now have exhaustive handlers instead of enclosing `invalid ...` messages.
Nested errors preserve the actual leaf position. The nearest useful enclosing
construct supplies one secondary label, avoiding a stack of redundant wrappers.
Specific repair advice covers separators, rest placement, record `=`, branch
syntax, and match arrows. Coverage is family-based with representative nested
failures; not every combination of wrappers has its own snapshot.

### Expanded declaration and literal implementation

```text
Table: Name, Open, Column, Colon, Builder, ModifierArg, ModifierArgEnd, End
Schema: Name, From, Open, Item, PickName, Colon, Type, Rule, RuleArg, RuleArgEnd, End
Macro: Name, ParamsOpen, Param, ParamEnd, Body
Test: Name, NameString, Body
Tests: Open, Item, SameLine, End
Template: Endless, Escape, HoleEmpty, HoleExpr, HoleEnd
Escape: Unknown, BadUnicodeFormat, BadUnicodeCode, BadUnicodeLength
RawTokens: Unbalanced, Endless, String, Open
```

These now have exhaustive handlers as well. The `syntax_case!` tests in
`report/syntax_tests.rs` parse actual malformed modules, assert an expected nested
error variant, and render each fixture into its own snapshot. The initial pass
adds 59 cases: 31 literal/macro/comptime cases and 28 table/schema/test cases.
All new outputs were inspected, along with the three changed raw-error parser
snapshots. `test_` fixture names lose that prefix in insta's snapshot filenames.

The reporter reads source text where needed, including distinguishing an
unfinished raw backtick template from an unfinished quoted string. It labels full
escape widths, explains four-to-six-digit Unicode syntax and the exclusion of
surrogate values, and finds ordinary strings' opening quotes while skipping
escaped quotes. A raw template's escape failure does not manufacture a string
opener from quotes in template text. Four additional span/context regressions
verify those cases and innermost macro delimiters.

`RawTokens::Unbalanced` and `RawTokens::Endless` now retain the innermost opening
position and expected closer from the scanner's existing stack. This is necessary
information previously discarded by the parser; accepted token text is unchanged.
The source-aware reporter renders EOF at its actual insertion point and labels
the innermost unmatched opener, rather than guessing which bracket is missing.

Local Elm `toStringReport`/`toEscapeReport` distinguishes unknown escapes, Unicode
format, code, and length errors and labels their widths. Alder now does the same,
but recommends its own backtick templates instead of Elm's triple-quoted strings,
and explicitly excludes UTF-16 surrogate values. New declaration messages follow
Alder's actual modifier/rule/item separation rules; table columns are not claimed
to take comma or semicolon separators.

### Expanded style, query, and markup implementation

```text
Style: Open, Key, KeyString, Colon, Value, Dimension, Nested, End, TooDeep
Query: Open, Verb, Select, Insert, Update, Delete, ClauseOrder, End
Select: Projection, ProjectionExpr, ProjectionEnd, From, Table, Join, Where, GroupBy, OrderBy, Limit, Offset
TableRef: Name, Alias
Join: Keyword, Table, On, Condition
Insert: Into, Table, Values, Pin, Value
Update: Table, Set, RecordOpen, Record, Where
Delete: From, Table, Where
Markup: Name, Attr, TagEnd, Child, CloseName, CloseMismatch, CloseEnd, Unclosed, FragmentUnclosed
Attr: Value, String, Expr, ExprEnd
Child: HoleEmpty, Hole, HoleEnd, StrayBrace, Element, If, For, Match, UnknownDirective, StrayElse, StrayEmpty, Stmt, TooDeep
DirIf: Condition, Body, ElseBranchStart, Else
DirFor: Pattern, In, Iter, Key, KeyExpr, Body, Empty
DirMatch: Scrutinee, Open, Pattern, Guard, Arrow, Body, BareText, Block, End
ChildBlock: Open, Item, End
Clause: From, Join, Where, GroupBy, OrderBy, Limit, Offset
```

These families now have exhaustive handlers that preserve nested causes and
positions. Source-based rendered fixtures cover style keys, values, dimensions,
nested rules, query verbs and clauses, and markup attributes, interpolation,
closing tags, and directives. The markup pass adds 43 reviewed fixtures, including
nested match patterns/guards and directive bodies. Closing-tag mismatches retain
the found name's width; unclosed elements report the actual EOF and their opening
tag. A closing tag encountered inside an unfinished directive block is not
misreported as an ordinary child element.

`Clause` is the ordered payload for `Query::ClauseOrder`, not a separate parser
failure. `Escape` is a shared leaf payload for strings and templates. All six
nesting-limit paths have source-based rendered fixtures; exhaustive Rust matches
alone are not treated as rendered-coverage evidence.

### Dispatchers and direct leaves

```text
Error: ParseError
Item: Start, AfterPub, Semicolon, Attribute, Import, Fn, Let, TypeAlias, Enum, Trait, Impl, ErrorDecl, Component, Table, Schema, Macro, Comptime, Test, Tests
Stmt: Let, Use, UseMember, For, While, Return, Break, Assert, Expr, AssignTarget, AssignValue, Semicolon
Expr: Start, Reserved, SqlKeyword, Number, String, Template, TaggedTemplate, Array, Tuple, Record, RecordCtor, Block, Lambda, If, Match, Loop, Provide, Call, Index, Tag, State, Style, Query, Markup, MacroCall, PathMember, Access, TupleIndexOverflow, Unary, PinOutsideQuery, Placeholder, OperatorReserved, OperatorRight, UnexpectedClose, TooDeep
Pattern: Start, Reserved, SqlKeyword, Number, String, Pin, PathMember, PathVar, Ctor, Tag, TagName, Tuple, Array, Record, Alias, WildcardNotVar, TooDeep
```

Wrappers route to the family entries above. Direct leaves have dedicated messages
and representative rendered fixtures. Nine final statement/query-pattern fixtures
cover `use`, returns, breaks, assertions, assignment targets/values, semicolons, and
a query keyword inside a match pattern. Missing initializers, assertion conditions,
and assignment values name their role; a more specific nested cause takes priority.

## Evidence and Elm comparison

The `compile::tests::parser_*` fixtures exercise the actual driver path and render
colorless diagnostics, asserting that the failure is a syntax diagnostic rather
than a later-phase rejection. The first 32 fixtures cover missing separators, EOF,
record assignment syntax, if/else branches, nested calls, match arrows/guards/
bodies, pattern rest/constructor/tuple errors, for/while, lambda assignment/body,
index, tag, state, provide, and Unicode failures. Their snapshots were inspected after
each family change, including nested primary/secondary positions.

Five `report::syntax_tests` regressions check byte offsets, whole Unicode scalar
labels, unchanged source identities/text, zero-width EOF positions including CRLF,
and nearest-context selection. These initially reproduced a one-byte Unicode
label and missing EOF explanation. Parser labels now cover a whole scalar, and
EOF messages explicitly name the end of the file. Miette may omit a zero-width
EOF underline, so the true primary header location and enclosing source label are
retained without fabricating or appending source characters.

Local Elm `toIfReport` distinguishes missing keywords and nested branch failures;
`toListReport` distinguishes delimiters and nested entries, retains surrounding
source, and explains corrections. Alder's initial core handlers now retain those
distinctions and nearby context. Elm's list trailing-comma prohibition and
indentation advice do not apply: Alder permits trailing commas and uses braces.
The CLI integration test
`parser_diagnostics_preserve_cli_links_and_editor_context` now verifies the
same nested array failure through `check`, `build`, `test`, and a real LSP session.
It checks canonical file hyperlinks from a different working directory (including
a project path with spaces), readable relative paths, specific labels and help,
UTF-16 ranges after an emoji, related-label source URIs, unsaved multiline EOF
insertion points, and clearing after a valid edit. This test passes without changes
to the CLI or LSP conversion code.

The entry-point comparison also inspected Elm's `PStart` and `TStart` branches
in `Reporting/Error/Syntax.hs`. Elm distinguishes a reserved token from an
otherwise unknown pattern/type start, shows enclosing context, and gives examples
without pretending to know the intended value. Alder now provides that distinction
and Alder-specific type examples (`Number`, `Array[Number]`, `a`). A
context gap identified in that comparison was that declaration wrappers discarded
their starting positions, leaving an EOF initializer without a secondary label.
Declaration wrappers now preserve those positions, with parameter-list and function
body openers taking priority when closer to the failure. Twelve additional rendered
fixtures cover function names/`async`, parameter syntax, return types, binding
initializers/equals, and type aliases. Nested local bindings retain their own start
rather than the outer function's start. Binding, assignment, assertion, query, and
markup entry errors specialize missing-expression wording by role.

## Inventory exceptions and limitations

- The named families above map to exhaustive family handlers in `report/syntax.rs`.
  Wrappers delegate to the child handler; there is no catch-all `invalid ...`
  enclosing-construct diagnostic. The inventory includes every enum in the active
  `error.rs`, including the shared `Clause` and `BadOperator` payloads.
- `Error::ParseError` wraps the public convenience API result. The driver receives
  `Module` directly, so it is not a second rendered failure path.
- `Lambda::Params` and `Lambda::Ret` are defensive definitions, not errors emitted
  by `try_lambda`: parameter/return-type failures restore parser state and are
  diagnosed by the ordinary expression path. Their handlers still delegate to the
  detailed parameter/type reporters. This work does not redesign that backtracking.
- `Module` translates an initial `Item::Start` into `Module::BadEnd`. The item
  handler remains useful for errors after consumed attributes and direct item APIs;
  both explain that a declaration or import is expected.
- `Expr::Start`, `Pattern::Start`, and `Type::Start` identify an expected syntax
  class when no more specific cause exists. They are deliberate leaf expectations,
  not discarded nested failures. Examples and enclosing labels provide context;
  the reporter does not guess an arbitrary replacement value.
- Only the nearest useful context label is retained, not the entire error-wrapper
  stack. This keeps nested snippets readable. Miette can omit zero-width EOF
  underlines; the canonical source, true primary position, EOF wording, and related
  context are retained, with no fabricated source characters.
- This is parity in specificity, source context, and justified repair advice,
  not a verbatim port of Elm prose or recovery. Alder permits trailing commas and
  uses braces; Elm-only indentation, list-comma, and module-header guidance is not
  applicable. No parser recovery or unrelated type-system behavior was changed.

The implementation uses the existing report/source and CLI/LSP conversion layers.
The only parser error-data extension is the raw-token scanner's retained expected
closer and innermost opening position; existing stack information was previously
discarded. The parser's accepted syntax is unchanged. Workspace tests include the
existing valid-program parser fixtures and executable end-to-end projects.

## Final validation

- `cargo fmt --all --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo test`: passed across the workspace, including 598 driver tests, 10 CLI
  diagnostic integration tests, and the standalone executable projects.
- No pending `.snap.new` files; rendered snapshots were inspected by family and
  rerun without snapshot-update mode. `git diff --check` passed.
- No new ignored tests. Existing ignored doctests are the schematic
  `Parser::one_of` example and the upstream `deno_core`-generated runtime extension
  example; neither was changed by this work.

The plan and SPEC checklist are complete. The Sampo changeset records the driver
diagnostic improvement and the parser error-payload API change independently.
