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
            comments: self.bump.alloc_slice_copy(self.comments.as_slice()),
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

    // §7.2 docs-example tests: one per docs sample (language.md, web.md,
    // data.md) that is a full module or a run of items, with the §10.35
    // typo fixes applied (noted per test). Samples that are a statement,
    // an expression or bare markup are covered by the statement /
    // expression / markup tests instead. A `{ ... }` placeholder body in
    // the docs is replaced by a one-line real body.

    #[test]
    fn docs_counter_component() {
        assert_module_snapshot!(
            r#"
            pub component Counter(props: { start?: Number, label: String }) {
                let mut count = state(props.start ?? 0)
                let double = count * 2                     // memoized automatically

                <button onClick={() -> count += 1}>
                    {props.label}: {count} ({double})
                </button>
            }
            "#
        );
    }

    #[test]
    fn docs_classify_fn() {
        assert_module_snapshot!(
            r#"
            fn classify(n: Number) String {
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
    fn docs_find_result() {
        assert_module_snapshot!(
            r#"
            fn find(id: Id) Result[User] {              // error inferred: [:not_found(Id) | r]
                match db.get(id) {
                    Some(u) => Ok(u),
                    None => Err(:not_found(id)),
                }
            }
            "#
        );
    }

    #[test]
    fn docs_load_await() {
        assert_module_snapshot!(
            r#"
            fn load(id: Id) Result[Profile] {           // inferred: [:not_found(Id) | :timeout | r]
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
    fn docs_traits_functor() {
        assert_module_snapshot!(
            r#"
            pub trait Show[a] {
                fn show(value: a) String
            }

            impl Show[User] {
                fn show(user: User) String { user.name }
            }

            pub trait Functor[f] {
                fn map(fa: f[a], g: fn(a) b) f[b]
            }

            impl Functor[Option] {
                fn map(fa: Option[a], g: fn(a) b) Option[b] {
                    match fa {
                        Some(x) => Some(g(x)),
                        None => None,
                    }
                }
            }

            fn describe(xs: Array[a]) String where a: Show {
                xs |> Array.map(show) |> String.join(", ")
            }

            trait Iterator[i] {
                type Item
                fn next(it: i) Option[Item]
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
    fn docs_web_load_page() {
        assert_module_snapshot!(
            r#"
            // users/[id]/+page.server.ald
            pub fn load(event: LoadEvent) Result[{ user: User, posts: Array[Post] }] {
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
            pub fn signUp(input: SignUp) Result[User] {
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

            fn run(db: Db) Result[()] {
                let rows = db.run(recent).await?      // Array[{ name: String, title: String, created: Timestamp }]
                db.run(query { insert into users values ^{ email, name } }).await?
                db.run(query { update users set { name: ^newName } where users.id == ^user.id }).await?
                db.run(query { delete from posts where posts.author == ^user.id }).await?
                Ok(())
            }
            "#
        );
    }

    // language.md "Modules": the import forms and the re-exports (the bare
    // `http.get(url)` calls after them are statements, not items).
    #[test]
    fn docs_imports() {
        assert_module_snapshot!(
            r#"
            import @alder/http                    // binds `http` (last segment, lowercase)
            import @alder/http as h               // binds `h`
            import @alder/http.{ get, Request }   // names into scope
            import @alder/http.*                  // every pub name into scope
            import ~/db/users                     // this package: binds `users`
            import ~/db/users.{ find }

            pub import ~/leaf.{ someFunc }
            pub import ~/leaf.*                   // typical for mod.ald
            "#
        );
    }

    // language.md "Functions"; the trailing pipe chain is bound with `let`
    // so it is an item.
    #[test]
    fn docs_functions() {
        assert_module_snapshot!(
            r#"
            pub fn add(a: Number, b: Number) Number {
                a + b
            }

            fn greet(name: String) String {
                `Hello ${name}`
            }

            let inc = x -> x + 1
            let block = (x) -> {
                let y = x * 2
                y + 1
            }

            let big = [1, 2, 3]
                |> Array.map(x -> x * 2)
                |> Array.filter(x -> x > 2)
            "#
        );
    }

    // language.md "Type application and variables": bodiless signatures
    // with `where` clauses, including a trailing comma before EOF.
    #[test]
    fn docs_type_variables() {
        assert_module_snapshot!(
            r#"
            fn zip(xs: Array[a], ys: Array[b]) Array[(a, b)]

            fn lookup(cache: Cache[k, v], key: k) Option[v]
                where k: Eq + Hash

            fn traverse(xs: t[f[a]], g: fn(a) f[b]) f[t[b]]
                where
                    t: Traversable,
                    f: Applicative,
            "#
        );
    }

    // language.md "Enums".
    #[test]
    fn docs_enums() {
        assert_module_snapshot!(
            r#"
            pub enum Option[a] {
                Some(a),
                None,
            }

            pub enum Shape {
                Circle(Number),
                Rect { width: Number, height: Number },
            }

            let s = Shape::Rect { width: 1, height: 2 }
            let o = Option::Some(3)
            "#
        );
    }

    // language.md "Records and rows"; the trailing `match u.nickname` is an
    // expression, not an item.
    #[test]
    fn docs_records() {
        assert_module_snapshot!(
            r#"
            type User = {
                id: Id,
                name: String,
                nickname?: String,        // read as Option[String]
            }

            fn rename(user: { r | name: String }, name: String) ({ r | name: String }) {
                { ..user, name }
            }

            let u: User = { id, name: "Ada" }          // nickname omitted
            "#
        );
    }

    // language.md "Errors": the closed-row function, the `error` group and
    // the bodiless signature using it.
    #[test]
    fn docs_error_group() {
        assert_module_snapshot!(
            r#"
            fn loadStrict(id: Id) Result[Profile, [:not_found(Id) | :timeout]] {
                load(id)                       // explicit, closed row
            }

            pub error AuthError {
                :invalid_token,
                :expired(Timestamp),
            }

            fn check(token: String) Result[Session, AuthError]
            "#
        );
    }

    // language.md "Async and fibers".
    #[test]
    fn docs_async_fibers() {
        assert_module_snapshot!(
            r#"
            fn profile(id: Id) Result[Profile] {
                let user = Http.get(`/users/${id}`).await?
                let posts = Http.get(`/users/${id}/posts`).await?
                Ok({ user, posts })
            }

            let (a, b) = Fiber.all(profile(1), profile(2)).await
            "#
        );
    }

    // language.md "Context (dependency injection)".
    #[test]
    fn docs_context() {
        assert_module_snapshot!(
            r#"
            fn saveUser(user: User) Result[()] {
                use Db
                Db.insert(users, user).await
            }

            fn main() {
                provide Db = Sqlite.open("app.db") {
                    saveUser(u).await
                }
            }
            "#
        );
    }

    // language.md "Attributes and macros"; the `...` inside `comptime` is
    // replaced by a real statement. Macro bodies are raw text in M1
    // (§10.29), so `quote` / `unquote` / `stringify` never reach the parser.
    #[test]
    fn docs_macros() {
        assert_module_snapshot!(
            r#"
            #[derive(Show, Eq, Json)]
            type Point = { x: Number, y: Number }

            macro assert_eq(left, right) {
                quote {
                    let l = unquote(left)
                    let r = unquote(right)
                    if l != r { Test.fail(unquote(stringify(left)), l, r) }
                }
            }

            comptime {
                let routes = Fs.readDir("routes")
                Routes.generate(routes)
            }
            "#
        );
    }

    // language.md "FFI", plus the `#[extern] type Response` opaque type the
    // prose mentions.
    #[test]
    fn docs_ffi() {
        assert_module_snapshot!(
            r#"
            #[extern("node:crypto", "randomUUID")]
            fn randomUUID() String

            #[extern("node:fs/promises", "readFile")]
            fn readFile(path: String, encoding: String) Task[Result[String, [:io(String)]]]

            #[extern("globalThis", "JSON.parse")]
            fn parseJson(s: String) Result[Json, [:syntax(String)]]

            #[extern] type Response
            "#
        );
    }

    // web.md "Server hooks" with `handleError`'s parameter renamed (`error`
    // is reserved, §10.35). `handle` ends in a `provide` expression, so that
    // expression is the block tail (§10.40).
    #[test]
    fn docs_web_hooks() {
        assert_module_snapshot!(
            r#"
            // src/hooks.server.ald
            pub fn handle(event: RequestEvent, resolve: fn(RequestEvent) Task[Response]) Task[Response] {
                let session = Auth.fromCookie(event.cookies).await
                provide Session = session {
                    resolve(event).await
                }
            }

            pub fn handleError(err: Error, event: RequestEvent) ErrorResponse { report(err) }
            pub fn handleFetch(event: RequestEvent, request: Request, fetch: Fetch) Task[Response] { fetch(request) }
            "#
        );
    }

    // web.md "Page options".
    #[test]
    fn docs_web_page_options() {
        assert_module_snapshot!(
            r#"
            pub let prerender = true      // build-time render (SSG); default false
            pub let ssr = false           // skip server render for this subtree; default true
            pub let csr = false           // ship no JS for this subtree; default true
            pub let trailingSlash = Never // Never | Always | Ignore
            "#
        );
    }

    // web.md "Remote functions".
    #[test]
    fn docs_web_remote_functions() {
        assert_module_snapshot!(
            r#"
            // lib/users.remote.ald
            pub fn getUser(id: Id) Result[User] { db.get(id) }              // query
            pub fn deleteUser(id: Id) Result[()] { db.delete(id) }           // command
            pub fn signUp(input: SignUp) Result[User] { db.insert(input) }   // form action, typed by schema

            // any component
            component UserCard(props: { id: Id }) {
                let user = resource(() -> getUser(props.id))
                <button onClick={() -> deleteUser(props.id)}>Delete</button>
            }
            "#
        );
    }

    // web.md "Stores".
    #[test]
    fn docs_web_stores() {
        assert_module_snapshot!(
            r#"
            // src/stores/cart.ald
            pub let mut items = state([])
            pub fn add(item: Item) { items.push(item) }
            "#
        );
    }

    // web.md "Styles"; the `<div class={card}>` line is markup, not an item.
    #[test]
    fn docs_web_styles() {
        assert_module_snapshot!(
            r#"
            let card = style {
                padding: 16px,
                color: theme.text,
                ":hover": { color: theme.accent },
                "@media (max-width: 600px)": { padding: 8px },
            }
            "#
        );
    }

    /// docs/runtime.md "Cloudflare" with the `{ ... }` placeholder body
    /// replaced by a real one.
    #[test]
    fn docs_runtime_cloudflare() {
        assert_module_snapshot!(
            r#"
            #[durable_object]
            type Counter = { count: Number }

            impl DurableObject[Counter] {
                fn fetch(obj: Counter, req: Request) Response { obj.count }
            }

            fn handler(req: Request) Response {
                use Kv                      // bound to the worker's KV namespace via wrangler config
                Kv.get(cache, "key").await
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

    /// Positions are `u32`: a line longer than `u16::MAX` bytes keeps exact
    /// columns (the old `u16` counters overflowed in debug and wrapped in
    /// release, misplacing every later region and error).
    #[test]
    fn long_line_keeps_exact_columns() {
        let bump = bumpalo::Bump::new();
        let text = "a".repeat(70_000);
        let src = bump.alloc_str(&format!("let x = \"{text}\"\n"));
        let module = crate::parse_module(&bump, src).unwrap_or_else(|e| panic!("{e:#?}"));
        let [item] = module.items else {
            panic!("expected one item")
        };
        assert_eq!(item.region.start, alder_region::Position::new(1, 1));
        // `let x = "` is 9 bytes, the text 70_000, the closing quote 1.
        assert_eq!(item.region.end, alder_region::Position::new(1, 70_011));
    }

    /// Positions are `u32`: a file with more than `u16::MAX` lines keeps
    /// exact rows.
    #[test]
    fn many_lines_keep_exact_rows() {
        let bump = bumpalo::Bump::new();
        let src = bump.alloc_str(&"let x = 1\n".repeat(70_000));
        let module = crate::parse_module(&bump, src).unwrap_or_else(|e| panic!("{e:#?}"));
        assert_eq!(module.items.len(), 70_000);
        let last = module.items.last().unwrap();
        assert_eq!(last.region.start, alder_region::Position::new(70_000, 1));
        assert_eq!(last.region.end, alder_region::Position::new(70_000, 10));
    }
}
