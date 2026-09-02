//! Items: attributes, visibility and every top-level declaration form.
//!
//! Grammar (SPEC.md): `item = { attribute } [ 'pub' ] item_body ;` where the
//! body is dispatched on its leading keyword (`import`, `fn`, `let`, `type`,
//! `enum`, `trait`, `impl`, `error`, `component`, `table`, `schema`,
//! `macro`, `comptime`, `test`, `tests`). Keyword-led bodies report their
//! errors at the keyword (Elm's `in_context` convention); attributes report
//! at the first `#`.
//!
//! Items are separated by line breaks, never by `;` (§2.1 rule 3, §10.38):
//! `items_until_close` (the `tests { }` body) applies the same rule
//! `module()` does — the item after an item must be `}` or start on a
//! later line, otherwise `Tests::SameLine` — and a `;` where an item was
//! expected is `Item::Semicolon` with the "separate with a line break" hint.
//!
//! The region of a `Located<Item>` runs from its first attribute (or `pub`,
//! or the keyword) to the last significant byte of the body. Most bodies end
//! in a node whose region is exact, but the closers of container bodies
//! (`}` of an `enum`, `)` of a bodiless `fn`, a trailing `,` of a `where`
//! clause) are not recorded in the AST, and some sub-parsers chomp trailing
//! whitespace. `item_end` therefore takes the end of the last located node
//! inside the body as a lower bound and rescans forward from there to the
//! cursor, skipping whitespace and `//` comments exactly as `chomp` does;
//! only punctuation can sit between that bound and the true end, so the
//! rescan never has to know about strings or templates.
//!
//! See docs/parser-internals.md §5.11.
// OWNER: item/mod.rs (Wave 3)

mod attribute;
mod component;
mod enum_;
mod error_;
mod fn_;
mod impl_;
mod import;
mod let_;
mod macro_;
mod schema;
mod table;
mod test;
mod trait_;
mod type_alias;

use alder_region::{Located, Position, Region};
use alder_source::{
    FnDecl, ImplItem, ImportTail, Item, ItemKind, Modifier, SchemaItem, TraitItem, VariantPayload,
    Visibility,
};
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};
use fn_::constraint_end;

impl<'a> Parser<'a> {
    /// attributes* [pub] item_body. Chomps trailing whitespace.
    pub fn item(&mut self) -> Result<&'a Located<Item<'a>>, error::Item<'a>> {
        let start = self.get_position();
        let attributes = self.specialize(
            |bump, e, row, col| error::Item::Attribute(bump.alloc(e), row, col),
            |p| p.attributes(),
        )?;
        let visibility = if self.peek_keyword(b"pub") {
            let pub_start = self.get_position();
            self.advance_by(3);
            let region = Region::new(pub_start, self.get_position());
            self.chomp();
            Visibility::Pub(region)
        } else {
            Visibility::Private
        };
        let body_start = self.get_position();
        let kind = self.item_body(visibility)?;
        let end = self.item_end(item_lower_bound(&kind, body_start));
        let item = self.alloc(Located::at(
            Region::new(start, end),
            Item {
                attributes,
                visibility,
                kind,
            },
        ));
        self.chomp();
        Ok(item)
    }

    /// Dispatch on the leading keyword. `visibility` only decides which error
    /// a non-item gets (`AfterPub` vs `Start`) and whether an `import` is a
    /// re-export.
    fn item_body(&mut self, visibility: Visibility) -> Result<ItemKind<'a>, error::Item<'a>> {
        let word = self.peek_word();
        let kind = match word {
            "import" => self.specialize(
                |bump, e, row, col| error::Item::Import(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.import(matches!(visibility, Visibility::Pub(_)))
                        .map(ItemKind::Import)
                },
            )?,
            "fn" => self.specialize(
                |bump, e, row, col| error::Item::Fn(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.chomp();
                    p.fn_decl().map(ItemKind::Fn)
                },
            )?,
            "let" => self.specialize(
                |bump, e, row, col| error::Item::Let(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.let_decl().map(ItemKind::Let)
                },
            )?,
            "type" => self.specialize(
                |bump, e, row, col| error::Item::TypeAlias(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.type_decl()
                },
            )?,
            "enum" => self.specialize(
                |bump, e, row, col| error::Item::Enum(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.chomp();
                    p.enum_decl().map(ItemKind::Enum)
                },
            )?,
            "trait" => self.specialize(
                |bump, e, row, col| error::Item::Trait(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.chomp();
                    p.trait_decl().map(ItemKind::Trait)
                },
            )?,
            "impl" => self.specialize(
                |bump, e, row, col| error::Item::Impl(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.chomp();
                    p.impl_decl().map(ItemKind::Impl)
                },
            )?,
            "error" => self.specialize(
                |bump, e, row, col| error::Item::ErrorDecl(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.chomp();
                    p.error_decl().map(ItemKind::Error)
                },
            )?,
            "component" => self.specialize(
                |bump, e, row, col| error::Item::Component(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.chomp();
                    p.component_decl().map(ItemKind::Component)
                },
            )?,
            "table" => self.specialize(
                |bump, e, row, col| error::Item::Table(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.chomp();
                    p.table_decl().map(ItemKind::Table)
                },
            )?,
            "schema" => self.specialize(
                |bump, e, row, col| error::Item::Schema(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.chomp();
                    p.schema_decl().map(ItemKind::Schema)
                },
            )?,
            "macro" => self.specialize(
                |_, e, row, col| error::Item::Macro(e, row, col),
                |p| {
                    p.advance_by(word.len());
                    p.macro_decl().map(ItemKind::Macro)
                },
            )?,
            "comptime" => self.specialize(
                |bump, e, row, col| error::Item::Comptime(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.comptime_block().map(ItemKind::Comptime)
                },
            )?,
            "test" => self.specialize(
                |bump, e, row, col| error::Item::Test(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.test_decl().map(ItemKind::Test)
                },
            )?,
            "tests" => self.specialize(
                |bump, e, row, col| error::Item::Tests(bump.alloc(e), row, col),
                |p| {
                    p.advance_by(word.len());
                    p.tests_block().map(ItemKind::Tests)
                },
            )?,
            _ => {
                let (row, col) = self.position();
                return Err(match visibility {
                    Visibility::Pub(_) => error::Item::AfterPub(row, col),
                    Visibility::Private if self.peek() == Some(b';') => {
                        error::Item::Semicolon(row, col)
                    }
                    Visibility::Private => error::Item::Start(row, col),
                });
            }
        };
        Ok(kind)
    }

    /// Items until `}` (for `tests { }`); `}` is consumed. Same line-break rule as
    /// `module()` → Tests::SameLine. `item()` itself reports a `;` as Item::Semicolon.
    ///
    /// Precedence mirrors `module()` and `block()`: a byte that cannot start
    /// an item is `Tests::End` even when it sits on the previous item's line
    /// (`tests { let x = 1 42 }`), because `SameLine` describes a *second
    /// item* on the line (§10.38) and `42` is not one.
    pub(crate) fn items_until_close(
        &mut self,
    ) -> Result<&'a [&'a Located<Item<'a>>], error::Tests<'a>> {
        let mut items: BumpVec<'a, &'a Located<Item<'a>>> = BumpVec::new_in(self.bump);
        let mut last_end: Option<Position> = None;
        loop {
            let (row, col) = self.position();
            // `;` is exempt from the same-line rule: `item()` reports it as
            // `Item::Semicolon` (the more specific hint).
            let mut same_line = false;
            match self.peek() {
                Some(b'}') => {
                    self.advance();
                    break;
                }
                None => return Err(error::Tests::End(row, col)),
                Some(b';') => {}
                Some(_) => same_line = last_end.is_some_and(|end| !self.newline_since(end)),
            }
            let item = match self.item() {
                // Not an item start at all (`)`, `42`, …): expected an item or `}`.
                Err(error::Item::Start(r, c)) if (r, c) == (row, col) => {
                    return Err(error::Tests::End(row, col));
                }
                _ if same_line => return Err(error::Tests::SameLine(row, col)),
                Err(e) => return Err(error::Tests::Item(self.alloc(e), row, col)),
                Ok(item) => item,
            };
            last_end = Some(item.region.end);
            items.push(item);
        }
        Ok(items.into_bump_slice())
    }

    /// The end of the item whose body's last located node ends at `lower`:
    /// rescan from `lower` to the cursor, skipping whitespace and `//`
    /// comments like `chomp`, and return the position after the last other
    /// byte (`lower` itself when there is none).
    ///
    /// `lower` is at or after every string / template of the item, so a `//`
    /// met here is always a comment (see the module docs).
    fn item_end(&self, lower: Position) -> Position {
        // Byte offset of `lower`: walk back from the cursor's line start to
        // the start of `lower.line`, then over to its column. Every `\n` is a
        // line break for `advance`, even inside strings, and columns count
        // bytes, so this arithmetic is exact.
        let mut line_start = self.pos - (self.col as usize - 1);
        let mut line = self.row;
        while line > lower.line {
            // `line_start - 1` is the `\n` that ended the previous line.
            let mut p = line_start - 1;
            while p > 0 && self.src[p - 1] != b'\n' {
                p -= 1;
            }
            line_start = p;
            line -= 1;
        }
        let mut pos = line_start + lower.column as usize - 1;
        let mut col = lower.column;
        let mut end = lower;
        while pos < self.pos {
            match self.src[pos] {
                b' ' | b'\t' | b'\r' => {
                    pos += 1;
                    col += 1;
                }
                b'\n' => {
                    pos += 1;
                    line += 1;
                    col = 1;
                }
                b'/' if self.src.get(pos + 1) == Some(&b'/') => {
                    while pos < self.pos && self.src[pos] != b'\n' {
                        pos += 1;
                        col += 1;
                    }
                }
                _ => {
                    pos += 1;
                    col += 1;
                    end = Position::new(line, col);
                }
            }
        }
        end
    }
}

/// The end of the last located node of an item body (see `Parser::item_end`).
/// `body_start` is the fallback for a body with no located node at all
/// (`tests { }`).
fn item_lower_bound(kind: &ItemKind<'_>, body_start: Position) -> Position {
    match kind {
        ItemKind::Import(import) => match import.tail {
            ImportTail::Module => import.path.region.end,
            ImportTail::Alias(name) => name.region.end,
            ImportTail::All(region) => region.end,
            ImportTail::Names(names) => names
                .last()
                .map(|n| n.alias.unwrap_or(n.name).region.end)
                .unwrap_or(import.path.region.end),
        },
        ItemKind::Fn(decl) => fn_lower_bound(decl),
        ItemKind::Let(decl) => decl.value.region.end,
        ItemKind::TypeAlias(alias) => alias.typ.region.end,
        ItemKind::OpaqueType(name) => name.region.end,
        ItemKind::Enum(decl) => match decl.variants.last() {
            Some(variant) => match variant.payload {
                VariantPayload::Unit => variant.name.region.end,
                VariantPayload::Tuple(types) => types
                    .last()
                    .map(|t| t.region.end)
                    .unwrap_or(variant.name.region.end),
                VariantPayload::Record(fields) => fields
                    .last()
                    .map(|f| f.typ.region.end)
                    .unwrap_or(variant.name.region.end),
            },
            None => decl
                .params
                .last()
                .map(|p| p.region.end)
                .unwrap_or(decl.name.region.end),
        },
        ItemKind::Trait(decl) => match decl.items.last() {
            Some(TraitItem::AssocType(name)) => name.region.end,
            Some(TraitItem::Fn(f)) => fn_lower_bound(f),
            None => decl
                .where_clause
                .last()
                .map(constraint_end)
                .or_else(|| decl.params.last().map(|p| p.region.end))
                .unwrap_or(decl.name.region.end),
        },
        ItemKind::Impl(decl) => match decl.items.last() {
            Some(ImplItem::AssocType { typ, .. }) => typ.region.end,
            Some(ImplItem::Fn(f)) => fn_lower_bound(f),
            None => decl
                .where_clause
                .last()
                .map(constraint_end)
                .or_else(|| decl.args.last().map(|t| t.region.end))
                .unwrap_or(decl.trait_.region().end),
        },
        ItemKind::Error(decl) => decl
            .tags
            .last()
            .map(|tag| {
                tag.args
                    .last()
                    .map(|t| t.region.end)
                    .unwrap_or(tag.name.region.end)
            })
            .unwrap_or(decl.name.region.end),
        ItemKind::Component(decl) => decl.body.region.end,
        ItemKind::Table(decl) => decl
            .columns
            .last()
            .map(|column| {
                column
                    .modifiers
                    .last()
                    .map(modifier_end)
                    .unwrap_or(column.builder.region.end)
            })
            .unwrap_or(decl.name.region.end),
        ItemKind::Schema(decl) => match decl.items.last() {
            Some(SchemaItem::Pick(names)) => names
                .last()
                .map(|n| n.region.end)
                .unwrap_or(decl.name.region.end),
            Some(SchemaItem::Field { name, typ, rules }) => rules
                .last()
                .map(modifier_end)
                .or_else(|| typ.map(|t| t.region.end))
                .unwrap_or(name.region.end),
            None => decl
                .from
                .map(|n| n.region.end)
                .unwrap_or(decl.name.region.end),
        },
        ItemKind::Macro(decl) => decl.body.region.end,
        ItemKind::Comptime(block) => block.region.end,
        ItemKind::Test(decl) => decl.body.region.end,
        ItemKind::Tests(items) => items
            .last()
            .map(|item| item.region.end)
            .unwrap_or(body_start),
    }
}

fn fn_lower_bound(decl: &FnDecl<'_>) -> Position {
    decl.body
        .map(|b| b.region.end)
        .or_else(|| decl.where_clause.last().map(constraint_end))
        .or_else(|| decl.ret.map(|t| t.region.end))
        .or_else(|| {
            decl.params.last().map(|p| {
                p.annotation
                    .map(|t| t.region.end)
                    .unwrap_or(p.pattern.region.end)
            })
        })
        .unwrap_or(decl.name.region.end)
}

fn modifier_end(modifier: &Modifier<'_>) -> Position {
    modifier
        .args
        .last()
        .map(|a| a.region.end)
        .unwrap_or(modifier.name.region.end)
}

/// Snapshot test macro for successful item parsing.
#[cfg(test)]
macro_rules! assert_item_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .item()
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

/// Snapshot test macro for item parse errors.
#[cfg(test)]
macro_rules! assert_item_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .item()
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
pub(crate) use assert_item_error_snapshot;
#[cfg(test)]
pub(crate) use assert_item_snapshot;

#[cfg(test)]
mod tests {
    #[test]
    fn pub_fn() {
        assert_item_snapshot!("pub fn add(a, b) { a + b }");
    }

    #[test]
    fn pub_enum() {
        assert_item_snapshot!("pub enum Color { Red, Green }");
    }

    #[test]
    fn pub_let() {
        assert_item_snapshot!("pub let answer = 42");
    }

    #[test]
    fn attr_then_pub() {
        assert_item_snapshot!(
            r#"
            #[derive(Show, Eq)]
            pub type Point = { x: Number, y: Number }
            "#
        );
    }

    #[test]
    fn multiple_attrs() {
        assert_item_snapshot!(
            r#"
            #[extern]
            #[durable_object]
            type Counter
            "#
        );
    }

    #[test]
    fn trailing_comment_excluded_from_region() {
        assert_item_snapshot!(
            r#"
            import ~/db/users.{ find } // the finder
            "#
        );
    }

    #[test]
    fn error_pub_alone() {
        assert_item_error_snapshot!("pub");
    }

    #[test]
    fn error_pub_then_non_item() {
        assert_item_error_snapshot!("pub 42");
    }

    #[test]
    fn error_unknown_start() {
        assert_item_error_snapshot!("42");
    }

    #[test]
    fn error_semicolon() {
        assert_item_error_snapshot!(";");
    }

    #[test]
    fn error_attr_then_non_item() {
        assert_item_error_snapshot!("#[derive(Show)] 42");
    }
}
