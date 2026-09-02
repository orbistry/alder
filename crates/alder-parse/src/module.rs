//! Module parsing: a flat, line-break separated item list.
//!
//! Grammar (SPEC.md): `module = { item } ;` — no header, no `exposing`.
//! `Module` is a flat ordered list (§10.30); `Module::imports()` serves the
//! driver. Items follow the statement separation rule (§2.1 rule 3,
//! §10.38): after an item the next one must be EOF or start on a later
//! line, otherwise `Module::SameLine`; a `;` is never a separator
//! (`item()` reports it as `Item::Semicolon`, wrapped in `Module::Item`).
//! A byte that cannot start an item where one is expected (`}`, `42`, …)
//! is `Module::BadEnd`.
//!
//! See docs/parser-internals.md §5.10.
// OWNER: module.rs (Wave 4)

use alder_region::{Located, Position};
use alder_source::{Item, Module};
use bumpalo::collections::Vec as BumpVec;

use crate::{Parser, error};

impl<'a> Parser<'a> {
    /// chomp; items until EOF; a non-item → Module::BadEnd. After each item the
    /// next one must start on a later line (`newline_since(item.region.end)`),
    /// otherwise Module::SameLine (§2.1 rule 3).
    pub fn module(&mut self) -> Result<Module<'a>, error::Module<'a>> {
        self.chomp();
        let mut items: BumpVec<'a, &'a Located<Item<'a>>> = BumpVec::new_in(self.bump);
        let mut last_end: Option<Position> = None;
        while !self.is_eof() {
            let (row, col) = self.position();
            // `;` is exempt from the same-line rule: `item()` reports it as
            // `Item::Semicolon` (the more specific hint).
            let same_line =
                self.peek() != Some(b';') && last_end.is_some_and(|end| !self.newline_since(end));
            let item = match self.item() {
                // Not an item start at all: expected an item or the end of the file.
                Err(error::Item::Start(r, c)) if (r, c) == (row, col) => {
                    return Err(error::Module::BadEnd(row, col));
                }
                _ if same_line => return Err(error::Module::SameLine(row, col)),
                Err(e) => return Err(error::Module::Item(self.alloc(e), row, col)),
                Ok(item) => item,
            };
            last_end = Some(item.region.end);
            items.push(item);
        }
        Ok(Module {
            items: items.into_bump_slice(),
        })
    }
}

/// Snapshot test macro for successful module parsing.
#[cfg(test)]
macro_rules! assert_module_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let result = parser
            .module()
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

/// Snapshot test macro for module parse errors.
#[cfg(test)]
macro_rules! assert_module_error_snapshot {
    ($code:expr) => {{
        let bump = bumpalo::Bump::new();
        let code = indoc::indoc!($code);
        let src = bump.alloc_str(code);
        let mut parser = $crate::Parser::new(&bump, src.as_bytes());
        let err = parser
            .module()
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

// `module.rs` has no submodules; the re-export follows the §7.1 convention
// so a later test file can import the pair like every other module's.
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_module_error_snapshot;
#[cfg(test)]
#[allow(unused)]
pub(crate) use assert_module_snapshot;

#[cfg(test)]
mod tests {
    #[test]
    fn empty_module() {
        assert_module_snapshot!("");
    }

    #[test]
    fn whitespace_only_module() {
        assert_module_snapshot!("\n\n   \n");
    }

    #[test]
    #[ignore = "waits for item/fn_.rs"]
    fn single_fn() {
        assert_module_snapshot!(
            r#"
            fn add(a, b) {
                a + b
            }
            "#
        );
    }

    #[test]
    fn single_let() {
        assert_module_snapshot!("let answer = 42");
    }

    #[test]
    fn imports_then_items() {
        assert_module_snapshot!(
            r#"
            import @alder/http.{ get, Request }
            import ~/db/users

            type Id = Number

            let base = "https://example.com"
            "#
        );
    }

    #[test]
    fn leading_comments() {
        assert_module_snapshot!(
            r#"
            //! Module docs are skipped in M1.
            // A plain comment.

            /// Item docs too.
            type Id = Number
            "#
        );
    }

    #[test]
    fn trailing_comment() {
        assert_module_snapshot!(
            r#"
            type Id = Number
            // the end
            "#
        );
    }

    #[test]
    fn imports_are_filtered() {
        let bump = bumpalo::Bump::new();
        let src = bump.alloc_str(indoc::indoc!(
            r#"
            import @alder/http
            type Id = Number
            pub import ~/leaf.*
            "#
        ));
        let module = crate::parse_module(&bump, src).unwrap_or_else(|e| panic!("{e:#?}"));
        let imports: Vec<_> = module.imports().collect();
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].path.value.segments.len(), 0);
        assert_eq!(imports[1].path.value.segments.len(), 1);
        assert_eq!(imports[1].path.value.segments[0].value, "leaf");
    }

    // §7.2 docs-example tests. Each input is the docs sample with the §10.35
    // typo fixes applied (noted per test); Wave 4 un-ignores them as the
    // item parsers land (§8).

    #[test]
    #[ignore = "waits for item/component.rs"]
    fn docs_counter_component() {
        assert_module_snapshot!(
            r#"
            pub component Counter(props: { start?: Number, label: String }) {
                let mut count = state(props.start ?? 0)
                let double = count * 2                     // memoized automatically

                <button onClick={fn() count += 1}>
                    {props.label}: {count} ({double})
                </button>
            }
            "#
        );
    }

    #[test]
    #[ignore = "waits for item/fn_.rs"]
    fn docs_classify_fn() {
        assert_module_snapshot!(
            r#"
            fn classify(n: Number) -> String {
                if n < 0 {
                    "negative"
                } else if n == 0 {
                    "zero"
                } else {
                    "positive"
                }
            }
            "#
        );
    }

    #[test]
    #[ignore = "waits for item/fn_.rs"]
    fn docs_find_result() {
        assert_module_snapshot!(
            r#"
            fn find(id: Id) -> Result[User] {              // error inferred: [:not_found(Id) | r]
                match db.get(id) {
                    Some(u) => Ok(u),
                    None => Err(:not_found(id)),
                }
            }
            "#
        );
    }

    #[test]
    #[ignore = "waits for item/fn_.rs"]
    fn docs_load_await() {
        assert_module_snapshot!(
            r#"
            fn load(id: Id) -> Result[Profile] {           // inferred: [:not_found(Id) | :timeout | r]
                let user = find(id)?          // rows merge through ?
                let prefs = fetchPrefs(user).await?
                Ok({ user, prefs })
            }
            "#
        );
    }

    // language.md "Traits" with the missing `->` in `map` added and the
    // one-line `trait Iterator` written one item per line (§10.35, §10.38).
    #[test]
    #[ignore = "waits for item/trait_.rs"]
    fn docs_traits_functor() {
        assert_module_snapshot!(
            r#"
            pub trait Show[a] {
                fn show(value: a) -> String
            }

            impl Show[User] {
                fn show(user: User) -> String { user.name }
            }

            pub trait Functor[f] {
                fn map(fa: f[a], g: fn(a) -> b) -> f[b]
            }

            impl Functor[Option] {
                fn map(fa: Option[a], g: fn(a) -> b) -> Option[b] {
                    match fa {
                        Some(x) => Some(g(x)),
                        None => None,
                    }
                }
            }

            fn describe(xs: Array[a]) -> String where a: Show {
                xs |> Array.map(show) |> String.join(", ")
            }

            trait Iterator[i] {
                type Item
                fn next(it: i) -> Option[Item]
            }
            "#
        );
    }

    // language.md "Tests" with the path-first `import @alder/test.{ fakeDb }`
    // (§10.34); `test` is reserved but a legal path segment (§2.4).
    #[test]
    fn docs_tests_block() {
        assert_module_snapshot!(
            r#"
            tests {
                import @alder/test.{ fakeDb }

                test "adds numbers" {
                    assert add(1, 2) == 3
                }

                test "finds a user" {
                    provide Db = fakeDb() {
                        assert find(1).await == Ok(ada)
                    }
                }
            }
            "#
        );
    }

    // web.md "Loading data" with `event.params.id` pinned (§10.35).
    #[test]
    #[ignore = "waits for item/fn_.rs"]
    fn docs_web_load_page() {
        assert_module_snapshot!(
            r#"
            // users/[id]/+page.server.ald
            pub fn load(event: LoadEvent) -> Result[{ user: User, posts: Array[Post] }] {
                use Db
                let user = db.run(query { select * from users where users.id == ^event.params.id }).await?
                let posts = loadPosts(user.id).await?
                Ok({ user, posts })
            }

            // users/[id]/+page.ald
            pub component page(props: { data: PageData }) {
                <h1>{props.data.user.name}</h1>
            }
            "#
        );
    }

    // web.md "Forms and validation": the `{ ... }` placeholder body is
    // replaced by a real one and the top-level markup is wrapped in a
    // component (markup is an expression, not an item).
    #[test]
    #[ignore = "waits for item/schema.rs"]
    fn docs_web_login_form() {
        assert_module_snapshot!(
            r#"
            schema SignUp from users {
                pick email, name
                name: min(3)
                password: String, min(12)
                confirm: String, equals(password)
            }

            // lib/auth.remote.ald
            pub fn signUp(input: SignUp) -> Result[User] {
                Ok(createUser(input))
            }

            component LoginForm() {
                <Form action={signUp}>
                    <Field name="email" />
                    <Field name="password" type="password" />
                </Form>
            }
            "#
        );
    }

    #[test]
    #[ignore = "waits for item/component.rs"]
    fn docs_web_tui_app() {
        assert_module_snapshot!(
            r#"
            component App() {
                let mut selected = state(0)
                <box direction="column" border="round">
                    <text bold>Tasks</text>
                    @for (task, i) in tasks; key task {
                        <text inverse={i == selected}>{task}</text>
                    }
                </box>
            }
            "#
        );
    }

    // data.md "Tables" with the path-first import (§10.34).
    #[test]
    #[ignore = "waits for item/table.rs"]
    fn docs_data_tables() {
        assert_module_snapshot!(
            r#"
            import @alder/sqlite.{ text, integer, timestamp, primaryKey }

            pub table users {
                id: integer() primaryKey autoIncrement
                email: text() notNull unique
                name: text() notNull
                created: timestamp() notNull default(now)
            }

            pub table posts {
                id: integer() primaryKey autoIncrement
                author: integer() notNull references(users.id)
                title: text() notNull
                body: text()
            }
            "#
        );
    }

    // data.md "Queries": the bare `db.run(...)` statements are wrapped in a
    // function (statements are not items).
    #[test]
    #[ignore = "waits for query.rs"]
    fn docs_data_queries() {
        assert_module_snapshot!(
            r#"
            let recent = query {
                select { u.name, p.title, p.created }
                from users as u
                join posts as p on p.author == u.id
                where u.active && p.created > ^since && u.id in ^ids
                orderBy p.created desc
                limit ^pageSize
            }

            fn run(db: Db) -> Result[()] {
                let rows = db.run(recent).await?      // Array[{ name: String, title: String, created: Timestamp }]
                db.run(query { insert into users values ^{ email, name } }).await?
                db.run(query { update users set { name: ^newName } where users.id == ^user.id }).await?
                db.run(query { delete from posts where posts.author == ^user.id }).await?
                Ok(())
            }
            "#
        );
    }

    #[test]
    fn error_bad_end() {
        assert_module_error_snapshot!(
            r#"
            type Id = Number
            }
            "#
        );
    }

    #[test]
    fn error_item() {
        assert_module_error_snapshot!("import http");
    }

    #[test]
    #[ignore = "waits for item/fn_.rs"]
    fn error_same_line_items() {
        assert_module_error_snapshot!("fn a() {} fn b() {}");
    }

    #[test]
    fn error_same_line_let_items() {
        assert_module_error_snapshot!("let a = 1 let b = 2");
    }

    #[test]
    fn error_same_line_import_then_type() {
        assert_module_error_snapshot!("import ~/db type Id = Number");
    }

    #[test]
    fn error_semicolon_after_item() {
        assert_module_error_snapshot!("let a = 1;");
    }

    #[test]
    fn error_pub_alone() {
        assert_module_error_snapshot!(
            r#"
            type Id = Number
            pub
            "#
        );
    }
}
