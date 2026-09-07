use alder_report::{Diagnostic, Source};
use bumpalo::Bump;
use miette::Diagnostic as _;

fn parse_error(text: &str) -> Diagnostic {
    parse_case(text).0
}

fn parse_case(text: &str) -> (Diagnostic, String) {
    let bump = Bump::new();
    let source = bump.alloc_str(text);
    let mut parser = alder_parse::Parser::new(&bump, source.as_bytes());
    let error = parser
        .module()
        .expect_err("fixture must fail during parsing");
    (
        super::parse(Source::new("/project/src/main.ald", text), &error),
        format!("{error:?}"),
    )
}

#[test]
fn delimiter_boundaries_do_not_blame_following_lines() {
    for prefix in [
        "import ~/helper.{ delayed",
        "import ~/helper.{ delayed as later",
        "let x = [1",
        "let x = (1, 2",
        "let x = { a: 1",
        "let x = f(1",
        "let x = xs[1",
        "let x = :some(1",
        "fn f(x: Int",
        "type X[a",
        "type X = Array[Int",
        "type X = (Int, String",
        "type X = { a: Int",
        "let [x",
        "let (x, y",
        "let { x",
        "let Some(x",
        "let x = state(0",
        "#[derive(Show",
        "#[derive(Show)",
        "enum X { One(Int",
        "enum X { One",
        "error E { :one(Int",
        "error E { :one",
        "impl Show[Int",
        "type X = fn(Int",
        "type X = [:one",
        "type X = [:one(Int",
        "let x = `hello ${ 1",
        "let x = <div class={1",
        "let x = <div>{1",
        "let x = <div></div",
        "let x = query { select { a",
        "let x = query { select * from users",
        "schema X { x: min(1",
        "macro x(a",
    ] {
        let source = format!("{prefix} // 😀 boundary\r\n\r\ntype Next = Int");
        let diagnostic = parse_error(&source);
        let labels: Vec<_> = diagnostic.labels().unwrap().collect();
        let primary = labels.iter().find(|label| label.primary()).unwrap();
        assert_eq!(primary.offset(), prefix.len(), "{source}: {diagnostic}");
        assert_eq!(primary.len(), 0, "{source}");
        assert_eq!(
            labels.len(),
            2,
            "only the insertion and opener should be labeled: {source}"
        );
        let opener = labels.iter().find(|label| !label.primary()).unwrap();
        assert_eq!(opener.len(), 1, "{source}");
        assert!(
            matches!(
                source.as_bytes()[opener.offset()],
                b'(' | b'[' | b'{' | b'<'
            ),
            "opener must be punctuation, not a keyword: {source}"
        );
        assert!(
            labels.iter().all(|label| label.offset() <= prefix.len()),
            "must not implicate the following declaration: {source}"
        );
    }
}

#[test]
fn wrong_closers_stay_at_the_actual_token() {
    for (prefix, closer) in [
        ("import ~/helper.{ delayed", "]"),
        ("let x = [1", ")"),
        ("let x = f(1", "}"),
        ("type X = Array[Int", ")"),
    ] {
        let source = format!("{prefix}\n{closer}");
        let diagnostic = parse_error(&source);
        let primary = diagnostic
            .labels()
            .unwrap()
            .find(|label| label.primary())
            .unwrap();
        assert_eq!(primary.offset(), prefix.len() + 1, "{source}");
        assert_eq!(primary.len(), 1, "{source}");
    }
}

#[test]
fn every_delimiter_end_labels_its_exact_opening_punctuation() {
    // The marker identifies the owning opener, not simply the last bracket in
    // the source. These 46 cases cover each shared delimiter-end variant.
    for marked in [
        "#[derive§(Show Eq)] type X",                      // Attribute::ArgEnd
        "#§[derive(Show) type X",                          // Attribute::End
        "import ~/x.§{ one as two\n\ntype X = Int",        // Import::NamesEnd
        "fn f§(x: Array[Int] y: Int) {}",                  // Params::End
        "type X§[a b] = a",                                // TypeParams::End
        "enum X { A§(Array[Int] Int) }",                   // Enum::VariantArgEnd
        "enum X §{ A(Int) B }",                            // Enum::End
        "impl Show§[Array[Int] Int] {}",                   // Impl::ArgEnd
        "error E §{ :a(Int) :b }",                         // ErrorDecl::End
        "error E { :a§(Array[Int] Int) }",                 // TagVariant::ArgEnd
        "table x { id: int() default§(f(1) 2) }",          // Table::ModifierArgEnd
        "table x §{ id: int() ; }",                        // Table::End
        "schema X { name: min§(f(1) 2) }",                 // Schema::RuleArgEnd
        "schema X §{ pick email name }",                   // Schema::End
        "macro x§(a b) {}",                                // Macro::ParamEnd
        "tests §{\nfn f() { 0 }\n42",                      // Tests::End
        "fn f() §{\nlet x = f(1)\n",                       // Block::End
        "let x = `hello $§{ f(1) x`",                      // Template::HoleEnd
        "let x = §[f(1), f(2)\ntype X = Int",              // Array::End
        "let x = §(f(1), f(2)\ntype X = Int",              // Tuple::End
        "let x = Thing §{ a: f(1)\ntype X = Int",          // Record::End
        "let x = match x §{ A => f(1) )",                  // Match::End
        "let x = f§(g(1)\ntype X = Int",                   // Call::End
        "let x = xs§[f(1)\ntype X = Int",                  // Index::End
        "let x = :a§(f(1)\ntype X = Int",                  // Tag::End
        "let x = state§(f(1)\ntype X = Int",               // State::End
        "let x = style §{ padding: 16px ; }",              // Style::End
        "let x = query §{ select * from users on x }",     // Query::End
        "let x = query { select §{ f(1) x } from users }", // Select::ProjectionEnd
        "let x = §<div class={f(1)} =>",                   // Markup::TagEnd
        "let x = <div>text§</div x>",                      // Markup::CloseEnd
        "let x = <div class=§{f(1) x}>",                   // Attr::ExprEnd
        "let x = <div>§{f(1) x}</div>",                    // Child::HoleEnd
        "let x = <div>@match x §{ A => <b/> ) }</div>",    // DirMatch::End
        "let x = <div>@if x §{<b/>",                       // ChildBlock::End
        "let Some§((x, y) z) = value",                     // PCtor::End
        "let §(Some(x), y z) = value",                     // PTuple::End
        "let §[Some(x) y] = value",                        // PArray::End
        "let Thing §{ x: Some(y) z } = value",             // PRecord::End
        "type X = Array§[Array[Int] Int]",                 // TArgs::End
        "type X = fn§(Array[Int] Int) Int",                // TFn::ParamEnd
        "type X = §(Array[Int], Int Bool)",                // TTuple::End
        "type X = §{ a: Array[Int] b: Int }",              // TRecord::End
        "type X = §[:a(Array[Int]) :b]",                   // TErrorRow::End
        "type X = §[:a | r :b]",                           // TErrorRow::ExtEnd
        "let x = query §{ insert into users values ^rows where x }", // Query::OperationEnd
    ] {
        let opening = marked.find('§').unwrap();
        let source = marked.replace('§', "");
        let (diagnostic, error) = parse_case(&source);
        assert!(
            error.contains("ExpectedEnd {"),
            "wrong failure: {source}: {error}"
        );
        let labels: Vec<_> = diagnostic.labels().unwrap().collect();
        assert_eq!(labels.len(), 2, "{source}");
        let context = labels.iter().find(|label| !label.primary()).unwrap();
        assert_eq!(
            (context.offset(), context.len()),
            (opening, 1),
            "{source}: {error}"
        );
        assert!(context.label().unwrap().contains("opens here"), "{source}");
    }
}

#[test]
fn closing_only_states_do_not_offer_invalid_continuations() {
    let row = parse_error("type X = [:a | r\nnext");
    assert!(
        row.message()
            .contains("expecting `]` after this error row extension")
    );
    assert!(!row.message().contains("`|`"));
    for operation in [
        "insert into users values ^rows",
        "update users set { name: ^name }",
        "delete from users",
    ] {
        let diagnostic = parse_error(&format!("let x = query {{ {operation}\nnext"));
        assert!(
            diagnostic
                .message()
                .contains("expecting `}` after this query operation"),
            "{diagnostic}"
        );
        assert!(!diagnostic.message().contains("clause"));
    }
}

#[test]
fn valid_multiline_delimiters_and_comments_keep_parsing() {
    for source in [
        "import ~/helper.{ delayed // comment\n, other as later\n}\nlet x = 1",
        "let x = [f(1 // argument\n, 2\n) // entry\n, (3, 4)\n]",
        "type X[a // parameter\n, b] = { x: Array[a\n], y: fn(b\n) a\n}",
        "fn f([x, ..xs] // pattern\n, { y }: Y\n) { [x, y]\n}",
        "let x = <div class={f(1\n)}>{1\n}</div>",
    ] {
        let bump = Bump::new();
        assert!(alder_parse::parse_module(&bump, source).is_ok(), "{source}");
    }
}

macro_rules! syntax_case {
    ($name:ident, $source:literal, $variant:literal) => {
        #[test]
        fn $name() {
            let source = indoc::indoc!($source);
            let (diagnostic, error) = parse_case(source);
            assert!(error.contains($variant), "expected {}, got {error}", $variant);
            let mut rendered = String::new();
            miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::unicode_nocolor())
                .with_width(80)
                .render_report(&mut rendered, &diagnostic)
                .unwrap();
            insta::with_settings!({ description => source, omit_expression => true }, {
                insta::assert_snapshot!(rendered);
            });
        }
    };
}

syntax_case!(
    boundary_import,
    "import ~/helper.{ delayed\n\ntype Operation = Int",
    "NamesEnd("
);
syntax_case!(
    boundary_row_extension,
    "type X = [:a | r\n\ntype Next = Int",
    "ExtEnd("
);
syntax_case!(
    boundary_query_operation,
    "let x = query { insert into users values ^rows\n\ntype Next = Int",
    "OperationEnd("
);
syntax_case!(
    boundary_import_alias,
    "import ~/helper.{ delayed as later // keep this comment\n\ntype Operation = Int",
    "NamesEnd("
);
syntax_case!(
    boundary_array,
    "let x = [1\n\nfn next() { 0 }",
    "Array(End("
);
syntax_case!(
    boundary_nested_call,
    "let x = [f(1\n\ntype Next = Int",
    "Call(End("
);
syntax_case!(
    boundary_type,
    "type X = Array[Int\n\nfn next() { 0 }",
    "Args(End("
);
syntax_case!(boundary_pattern, "let [x\n\ntype Next = Int", "Array(End(");
syntax_case!(boundary_separator, "let x = [1\n    2]", "Array(End(");
syntax_case!(boundary_wrong_closer, "let x = [1\n)", "Array(End(");
syntax_case!(
    boundary_eof_comment,
    "let x = [\"😀\" // trailing\r\n",
    "Array(End("
);

macro_rules! nesting_case {
    ($name:ident, $source:expr, $variant:literal) => {
        #[test]
        fn $name() {
            let source = $source;
            let (diagnostic, error) = parse_case(&source);
            assert!(
                error.contains($variant),
                "expected {}, got {error}",
                $variant
            );
            let mut rendered = String::new();
            miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::unicode_nocolor())
                .with_width(80)
                .render_report(&mut rendered, &diagnostic)
                .unwrap();
            insta::assert_snapshot!(rendered);
        }
    };
}

nesting_case!(
    nesting_expression,
    format!(
        "let x = {}1{}",
        "(\n".repeat(alder_parse::MAX_NESTING as usize),
        "\n)".repeat(alder_parse::MAX_NESTING as usize)
    ),
    "Expr(TooDeep("
);
nesting_case!(
    nesting_pattern,
    format!(
        "let {}_{} = value",
        "Some(\n".repeat(alder_parse::MAX_NESTING as usize),
        "\n)".repeat(alder_parse::MAX_NESTING as usize)
    ),
    "Arg(TooDeep("
);
nesting_case!(
    nesting_type,
    format!(
        "type X = {}a{}",
        "Array[\n".repeat(alder_parse::MAX_NESTING as usize),
        "\n]".repeat(alder_parse::MAX_NESTING as usize)
    ),
    "Type(TooDeep("
);
nesting_case!(
    nesting_markup,
    format!(
        "let x = {}text{}",
        "<a>\n".repeat(alder_parse::MAX_NESTING as usize),
        "\n</a>".repeat(alder_parse::MAX_NESTING as usize)
    ),
    "Child(TooDeep("
);
nesting_case!(
    nesting_style,
    format!(
        "let x = style {{\n{}a: 1{}\n}}",
        "a: {\n".repeat(alder_parse::MAX_NESTING as usize),
        "\n}".repeat(alder_parse::MAX_NESTING as usize)
    ),
    "Nested(TooDeep("
);
nesting_case!(
    nesting_block,
    format!(
        "fn f() {{\n{}0{}\n}}",
        "loop {\n".repeat(alder_parse::MAX_NESTING as usize / 2),
        "\n}".repeat(alder_parse::MAX_NESTING as usize / 2)
    ),
    "Loop(TooDeep("
);

syntax_case!(entry_module_value, "42", "BadEnd(");
syntax_case!(statement_use, "fn f() { use 1 }", "Stmt(Use(");
syntax_case!(
    statement_use_member,
    "fn f() { use Db.value }",
    "UseMember("
);
syntax_case!(statement_return, "fn f() { return (1 +) }", "Return(");
syntax_case!(statement_break, "fn f() { loop { break (1 +) } }", "Break(");
syntax_case!(statement_assert, "fn f() { assert }", "Assert(");
syntax_case!(
    statement_assignment_target,
    "fn f() { 1 /= 2 }",
    "AssignTarget("
);
syntax_case!(statement_assignment_value, "fn f() { x = }", "AssignValue(");
syntax_case!(statement_semicolon, "fn f() { let x = 1; }", "Semicolon(");
syntax_case!(
    pattern_query_keyword,
    "let x = query { select * from users where match x { select => 1 } }",
    "Pattern(SqlKeyword("
);
syntax_case!(expression_reserved, "let x = type", "Reserved(Type");
syntax_case!(
    expression_sql_keyword,
    "let x = query { select * from users where select }",
    "SqlKeyword("
);
syntax_case!(expression_path_member, "let x = Option::", "PathMember(");
syntax_case!(expression_access, "let x = value.", "Access(");
syntax_case!(expression_unary_operand, "let x = !", "Unary(");
syntax_case!(expression_binary_operand, "let x = 1 +", "OperatorRight(");
syntax_case!(expression_pin, "let x = ^value", "PinOutsideQuery(");
syntax_case!(expression_placeholder, "let x = _", "Placeholder(");
syntax_case!(expression_closing_tag, "let x = </p>", "UnexpectedClose(");
syntax_case!(pattern_reserved, "let match = x", "Pattern(Reserved(");
syntax_case!(pattern_wildcard_name, "let _foo = x", "WildcardNotVar(");
syntax_case!(pattern_path_member, "let Option:: = x", "PathMember(");
syntax_case!(pattern_value_path, "let Color::red = x", "PathVar(");
syntax_case!(pattern_pin_nested, "let ^(1 +) = x", "Pin(");
syntax_case!(pattern_tag_name, "let :Bad = x", "TagName(");
syntax_case!(pattern_alias, "let x as = value", "Alias(");
syntax_case!(number_suffix, "let x = 123abc", "Number(End");
syntax_case!(number_dot, "let x = 1.", "Number(Dot");
syntax_case!(number_exponent, "let x = 1e+", "Number(Exponent");
syntax_case!(number_hex, "let x = 0xG", "Number(HexDigit");
syntax_case!(number_leading_zero, "let x = 007", "Number(NoLeadingZero");
syntax_case!(
    number_bigint_fraction,
    "let x = 1.5n",
    "Number(BigIntFraction"
);
syntax_case!(
    operator_arrow,
    "let x = a + b -> c",
    "OperatorReserved(Arrow"
);
syntax_case!(operator_bar, "let x = a | b", "OperatorReserved(Bar");
syntax_case!(
    operator_concat,
    "let x = a ++ b",
    "OperatorReserved(PlusPlus"
);
syntax_case!(
    operator_cons,
    "let x = a :: b",
    "OperatorReserved(DoubleColon"
);
syntax_case!(operator_range, "let x = a .. b", "OperatorReserved(DotDot");
syntax_case!(
    operator_pipe_left,
    "let x = a <| b",
    "OperatorReserved(PipeLeft"
);
syntax_case!(
    operator_compose_right,
    "let x = a >> b",
    "OperatorReserved(ComposeRight"
);
syntax_case!(
    operator_compose_left,
    "let x = a << b",
    "OperatorReserved(ComposeLeft"
);
syntax_case!(operator_power, "let x = a ^ b", "OperatorReserved(Caret");
syntax_case!(trait_name, "trait [a] {}", "Trait(Name(");
syntax_case!(trait_params_open, "trait Show {", "Params(Open(");
syntax_case!(trait_open, "trait Show[a] fn", "Trait(Open(");
syntax_case!(trait_item, "trait Show[a] { let x = 1 }", "Trait(Item(");
syntax_case!(
    trait_same_line,
    "trait Iterator[i] { type Item fn next(it: i) Item }",
    "SameLine("
);
syntax_case!(
    trait_semicolon,
    "trait Iterator[i] { type Item; }",
    "Semicolon("
);
syntax_case!(
    trait_assoc_name,
    "trait Iterator[i] { type item }",
    "AssocType("
);
syntax_case!(
    trait_assoc_body,
    "trait Iterator[i] { type Item = Number }",
    "AssocTypeHasBody("
);
syntax_case!(
    trait_method,
    "trait Show[a] { fn (value: a) String }",
    "Fn(Name("
);
syntax_case!(
    trait_where_bound,
    "trait Ord[a] where a: 1 {}",
    "Where(Bound("
);
syntax_case!(where_variable, "fn f() where type: Show {}", "Where(Var(");
syntax_case!(
    where_additional_bound,
    "fn f() where a: Show + 1 {}",
    "Where(Bound("
);
syntax_case!(where_colon, "fn f() where a Show {}", "Where(Colon(");
syntax_case!(
    where_assoc_name,
    "fn f() where i.item == Number {}",
    "AssocName("
);
syntax_case!(
    where_assoc_equals,
    "fn f() where i.Item = Number {}",
    "AssocEq("
);
syntax_case!(
    where_assoc_type,
    "fn f() where i.Item == 1 {}",
    "Where(Type(Start("
);
syntax_case!(impl_trait, "impl show[User] {}", "Impl(Trait(");
syntax_case!(impl_path_member, "impl Show::[User] {}", "PathMember(");
syntax_case!(impl_open, "impl Show {", "Impl(Open(");
syntax_case!(impl_argument, "impl Show[1] {}", "Arg(Start(");
syntax_case!(impl_argument_end, "impl Show[User User] {}", "ArgEnd(");
syntax_case!(impl_where, "impl Show[User] where a: 1 {}", "Where(Bound(");
syntax_case!(impl_body_open, "impl Show[User] fn", "BodyOpen(");
syntax_case!(impl_item, "impl Show[User] { let x = 1 }", "Impl(Item(");
syntax_case!(
    impl_same_line,
    "impl Iterator[Foo] { type Item = Number fn next() {} }",
    "SameLine("
);
syntax_case!(
    impl_semicolon,
    "impl Iterator[Foo] { type Item = Number; }",
    "Semicolon("
);
syntax_case!(
    impl_assoc_name,
    "impl Iterator[Foo] { type item = Number }",
    "AssocType("
);
syntax_case!(
    impl_assoc_equals,
    "impl Iterator[Foo] { type Item }",
    "AssocEquals("
);
syntax_case!(
    impl_assoc_body,
    "impl Iterator[Foo] { type Item = 1 }",
    "AssocBody(Start("
);
syntax_case!(
    impl_method_nested,
    "impl Show[User] { fn show(user: User) String { let } }",
    "Fn(Body(Stmt(Let("
);
syntax_case!(attribute_open, "# derive", "Attribute(Open(");
syntax_case!(attribute_name, "#[Derive] type X", "Attribute(Name(");
syntax_case!(attribute_argument, "#[derive(Show, ])] type X", "Arg(");
syntax_case!(
    attribute_argument_end,
    "#[derive(Show Eq)] type X",
    "ArgEnd("
);
syntax_case!(attribute_end, "#[derive(Show) type X", "Attribute(End(");
syntax_case!(attribute_dangling, "#[extern]", "Dangling(");
syntax_case!(import_path_start, "import /http", "Path(Start(");
syntax_case!(import_path_author, "import @/http", "Author(");
syntax_case!(import_path_slash, "import @alder", "Slash(");
syntax_case!(import_path_package, "import @alder/", "Package(");
syntax_case!(import_path_segment, "import ~/db/Users", "Segment(");
syntax_case!(import_tail, "import @alder/http.get", "Tail(");
syntax_case!(import_name, "import @alder/http.{ 1 }", "Import(Name(");
syntax_case!(
    import_name_alias,
    "import @alder/http.{ Request as }",
    "NameAlias("
);
syntax_case!(
    import_module_alias,
    "import @alder/http as H",
    "Import(Alias("
);
syntax_case!(
    import_names_end,
    "import @alder/http.{ get Request }",
    "NamesEnd("
);
syntax_case!(import_group_end, "pub import (json io)", "GroupEnd(");
syntax_case!(
    import_reserved_binding,
    "import ~/db/type",
    "ReservedBinding("
);
syntax_case!(import_root_only, "import ~", "RootOnly(");
syntax_case!(enum_name, "enum { A }", "Enum(Name(");
syntax_case!(enum_params, "enum Pair[] {}", "Params(Empty(");
syntax_case!(enum_open, "enum Color Red", "Enum(Open(");
syntax_case!(enum_variant, "enum Color { red }", "Variant(");
syntax_case!(
    enum_payload,
    "enum Shape { Circle(1) }",
    "VariantArg(Start("
);
syntax_case!(
    enum_payload_end,
    "enum Shape { Circle(Number Number) }",
    "VariantArgEnd("
);
syntax_case!(
    enum_record,
    "enum Shape { Rect { width } }",
    "VariantRecord(Colon("
);
syntax_case!(
    enum_record_extension,
    "enum Shape { Rect { r | width: Number } }",
    "VariantRecordExt("
);
syntax_case!(enum_end, "enum Color { Red Green }", "Enum(End(");
syntax_case!(error_group_name, "error { :a }", "ErrorDecl(Name(");
syntax_case!(error_group_open, "error E :a", "ErrorDecl(Open(");
syntax_case!(error_group_tag, "error E { expired }", "Tag(Name(");
syntax_case!(error_group_payload, "error E { :a(1) }", "Tag(Arg(Start(");
syntax_case!(
    error_group_payload_end,
    "error E { :a(Number Number) }",
    "ArgEnd("
);
syntax_case!(error_group_end, "error E { :a :b }", "ErrorDecl(End(");
syntax_case!(component_name, "component for() {}", "Component(Name(");
syntax_case!(component_params, "component App(a b) {}", "Params(End(");
syntax_case!(component_body_open, "component App()", "Body(Open(");
syntax_case!(
    component_body_nested,
    "component App() { let }",
    "Body(Stmt(Let("
);
syntax_case!(alias_name, "type id = Number", "TypeAlias(Name(");
syntax_case!(alias_params_end, "type Pair[a, b = (a, b)", "Params(End(");
syntax_case!(alias_params_empty, "type Pair[] = Number", "Params(Empty(");
syntax_case!(alias_params_var, "type Pair[A] = A", "Params(Var(");
syntax_case!(entry_module_close, "let x = 1\n}", "BadEnd(");
syntax_case!(entry_after_pub, "pub", "AfterPub(");
syntax_case!(entry_type, "let x: = 1", "Type(Start(");
syntax_case!(entry_type_reserved, "let x: while = 1", "Reserved(While");
syntax_case!(entry_type_path, "let x: Outer::inner = 1", "PathMember(");
syntax_case!(entry_pattern, "let + = 1", "Pattern(Start(");
syntax_case!(entry_expression, "let x =", "Body(Start(");
syntax_case!(declaration_function_name, "fn Upper() {}", "Fn(Name(");
syntax_case!(declaration_async_keyword, "async work() {}", "Fn(Keyword(");
syntax_case!(declaration_function_params, "fn work", "Params(Open(");
syntax_case!(
    declaration_parameter_type,
    "fn work(x: ) {}",
    "Params(Type(Start("
);
syntax_case!(
    declaration_parameter_pattern,
    "fn work(+x) {}",
    "Params(Pattern(Start("
);
syntax_case!(
    declaration_parameter_separator,
    "fn work(x y) {}",
    "Params(End("
);
syntax_case!(
    declaration_parameter_optional,
    "fn work(x?) {}",
    "OptionalAnnotation("
);
syntax_case!(
    declaration_function_return,
    "fn work() Array[] {}",
    "Ret(Args(Empty("
);
syntax_case!(declaration_function_body, "fn work() {", "Body(End(");
syntax_case!(declaration_alias_type, "type User =", "Body(Start(");
syntax_case!(
    declaration_local_initializer,
    "fn work() {\n    let x =",
    "Let(Body(Start("
);
syntax_case!(declaration_binding_equals, "let x 1", "Let(Equals(");
syntax_case!(type_arguments_end, "type X = Array[Number", "Args(End(");
syntax_case!(
    type_arguments_nested,
    "type X = Array[1]",
    "Args(Type(Start("
);
syntax_case!(type_function_open, "type X = fn -> a", "Fn(Open(");
syntax_case!(type_function_parameter, "type X = fn(1) a", "Param(Start(");
syntax_case!(type_function_separator, "type X = fn(a b) c", "ParamEnd(");
syntax_case!(type_function_return, "type X = fn(a)", "Ret(Start(");
syntax_case!(type_tuple_end, "type X = (a, b", "Tuple(End(");
syntax_case!(type_tuple_nested, "type X = (a, 1)", "Tuple(Type(Start(");
syntax_case!(
    type_record_field,
    "type X = { Name: String }",
    "Record(Field("
);
syntax_case!(
    type_record_colon,
    "type X = { name String }",
    "Record(Colon("
);
syntax_case!(
    type_record_nested,
    "type X = { name: Array[] }",
    "Record(Type(Args(Empty("
);
syntax_case!(type_record_extension, "type X = { r | }", "ExtField(");
syntax_case!(type_record_end, "type X = { name: String", "Record(End(");
syntax_case!(type_error_row_start, "type X = [1]", "ErrorRow(Start(");
syntax_case!(type_error_row_tag, "type X = [:1]", "Tag(Name(");
syntax_case!(
    type_error_row_extension,
    "type X = [:timeout | 1]",
    "ErrorRow(Ext("
);
syntax_case!(type_error_row_end, "type X = [:timeout", "ErrorRow(End(");
syntax_case!(
    type_error_tag_payload,
    "type X = [:failed(1)]",
    "Tag(Arg(Start("
);
syntax_case!(
    type_error_tag_separator,
    "type X = [:failed(Number]",
    "ArgEnd("
);
syntax_case!(table_name, "table Upper {}", "Table(Name(");
syntax_case!(
    markup_match_scrutinee,
    "let value = <p>@match ) {}</p>",
    "Scrutinee("
);
syntax_case!(
    markup_match_pattern,
    "let value = <p>@match x { + => <b/> }</p>",
    "Pattern("
);
syntax_case!(
    markup_match_guard,
    "let value = <p>@match x { A if ) => <b/> }</p>",
    "Guard("
);
syntax_case!(
    markup_match_block,
    "let value = <p>@match x { A => { {)} } }</p>",
    "Block(Item(Hole("
);
syntax_case!(
    markup_else_body,
    "let value = <p>@if x {} @else { {)} }</p>",
    "Else(Item(Hole("
);
syntax_case!(
    markup_if_condition,
    "let value = <p>@if ) {<b>x</b>}</p>",
    "Condition("
);
syntax_case!(markup_if_body, "let value = <p>@if x }</p>", "Body(Open(");
syntax_case!(
    markup_else_start,
    "let value = <p>@if x{} @else <b>x</b></p>",
    "ElseBranchStart("
);
syntax_case!(
    markup_for_in,
    "let value = <ul>@for x items {<li>{x}</li>}</ul>",
    "In("
);
syntax_case!(
    markup_for_key,
    "let value = <ul>@for x in xs; foo x {}</ul>",
    "Key("
);
syntax_case!(
    markup_for_key_expr,
    "let value = <ul>@for x in xs; key ) {}</ul>",
    "KeyExpr("
);
syntax_case!(
    markup_for_pattern,
    "let value = <ul>@for +x in xs {}</ul>",
    "Pattern("
);
syntax_case!(
    markup_for_iter,
    "let value = <ul>@for x in ) {}</ul>",
    "Iter("
);
syntax_case!(
    markup_for_body,
    "let value = <ul>@for x in xs ]</ul>",
    "Body(Open("
);
syntax_case!(
    markup_for_empty,
    "let value = <ul>@for x in xs {} @empty ]</ul>",
    "Empty(Open("
);
syntax_case!(
    markup_match_text,
    "let value = <p>@match x { A => hi }</p>",
    "BareText("
);
syntax_case!(
    markup_match_closing_tag,
    "let value = <p>@match x { A => </p>",
    "CloseName("
);
syntax_case!(
    markup_match_open,
    "let value = <p>@match x A => <b>a</b> }</p>",
    "Open("
);
syntax_case!(
    markup_match_arrow,
    "let value = <p>@match x { A -> <b>a</b> }</p>",
    "Arrow("
);
syntax_case!(
    markup_match_end,
    "let value = <p>@match x { A => <b>a</b> ) }</p>",
    "End("
);
syntax_case!(markup_block_end, "let value = <p>@if x {<b>y</b>", "End(");
syntax_case!(
    markup_block_closing_tag,
    "let value = <p>@if x { </p> }",
    "CloseName("
);
syntax_case!(
    markup_block_let,
    "let value = <p>@if x { let y }</p>",
    "Stmt(Let("
);
syntax_case!(
    markup_block_use,
    "let value = <p>@if x { use Db.run }</p>",
    "Stmt(UseMember("
);
syntax_case!(
    markup_unknown_directive,
    "let value = <p>@when x</p>",
    "UnknownDirective("
);
syntax_case!(
    markup_stray_else,
    "let value = <p>@else {}</p>",
    "StrayElse("
);
syntax_case!(
    markup_stray_empty,
    "let value = <p>@empty {}</p>",
    "StrayEmpty("
);
syntax_case!(markup_stray_brace, "let value = <p>}</p>", "StrayBrace(");
syntax_case!(markup_name, "let value = <div>< </div>", "Name(");
syntax_case!(markup_tag_end, "let value = <div =>", "TagEnd(");
syntax_case!(markup_mismatch, "let value = <div></span>", "CloseMismatch");
syntax_case!(markup_close_name, "let value = <div></>", "CloseName(");
syntax_case!(markup_close_end, "let value = <div></div x>", "CloseEnd(");
syntax_case!(markup_unclosed, "let value = <div><p>x", "Unclosed {");
syntax_case!(
    markup_fragment_unclosed,
    "let value = <>hi",
    "FragmentUnclosed("
);
syntax_case!(
    markup_fragment_mismatch,
    "let value = <>hi</div>",
    "CloseMismatch"
);
syntax_case!(markup_attr_value, "let value = <div class=>", "Value(");
syntax_case!(
    markup_attr_string,
    "let value = <div class=\"x>",
    "String(Endless"
);
syntax_case!(
    markup_attr_expr,
    "let value = <div class={)}>",
    "Expr(Start("
);
syntax_case!(
    markup_attr_end,
    "let value = <div class={x id=\"y\">",
    "ExprEnd("
);
syntax_case!(markup_hole_empty, "let value = <p>{}</p>", "HoleEmpty(");
syntax_case!(markup_hole_end, "let value = <p>{x y}</p>", "HoleEnd(");
syntax_case!(
    markup_hole_reserved,
    "let value = <p>{else}</p>",
    "Hole(Reserved("
);
syntax_case!(style_open, "let value = style [", "Style(Open(");
syntax_case!(style_key, "let value = style { 42: 1 }", "Key(");
syntax_case!(
    style_key_string,
    "let value = style { \"\\q\": 1 }",
    "KeyString("
);
syntax_case!(style_colon, "let value = style { color red }", "Colon(");
syntax_case!(style_value, "let value = style { color: }", "Value(");
syntax_case!(
    style_dimension_space,
    "let value = style { padding: 16 px }",
    "Dimension(End"
);
syntax_case!(
    style_dimension_number,
    "let value = style { padding: 007px }",
    "Dimension(NoLeadingZero"
);
syntax_case!(
    style_nested,
    "let value = style { \":hover\": { color red } }",
    "Nested("
);
syntax_case!(style_end, "let value = style { padding: 16px 8px }", "End(");
syntax_case!(
    query_open,
    "let value = query select * from users",
    "Query(Open("
);
syntax_case!(
    query_verb,
    "let value = query { fetch * from users }",
    "Verb("
);
syntax_case!(
    query_projection,
    "let value = query { select from users }",
    "Projection("
);
syntax_case!(
    query_projection_value,
    "let value = query { select { } from users }",
    "ProjectionExpr("
);
syntax_case!(
    query_projection_end,
    "let value = query { select { a b } from users }",
    "ProjectionEnd("
);
syntax_case!(query_from, "let value = query { select * users }", "From(");
syntax_case!(
    query_table_name,
    "let value = query { select * from Users }",
    "Table(Name("
);
syntax_case!(
    query_table_alias,
    "let value = query { select * from users as }",
    "Alias("
);
syntax_case!(
    query_join_keyword,
    "let value = query { select * from users left posts on x }",
    "Keyword("
);
syntax_case!(
    query_join_table,
    "let value = query { select * from users join on x }",
    "Join(Table("
);
syntax_case!(
    query_join_on,
    "let value = query { select * from users join posts }",
    "On("
);
syntax_case!(
    query_join_condition,
    "let value = query { select * from users join posts on }",
    "Condition("
);
syntax_case!(
    query_where,
    "let value = query { select * from users where }",
    "Where("
);
syntax_case!(
    query_group_by,
    "let value = query { select * from users groupBy }",
    "GroupBy("
);
syntax_case!(
    query_order_by,
    "let value = query { select * from users orderBy }",
    "OrderBy("
);
syntax_case!(
    query_limit,
    "let value = query { select * from users limit }",
    "Limit("
);
syntax_case!(
    query_offset,
    "let value = query { select * from users offset }",
    "Offset("
);
syntax_case!(
    query_clause_order,
    "let value = query { select * from users orderBy name where active }",
    "ClauseOrder(Where"
);
syntax_case!(
    query_clause_repeated,
    "let value = query { select * from users limit 1 limit 2 }",
    "ClauseOrder(Limit"
);
syntax_case!(
    query_end,
    "let value = query { select * from users on x }",
    "Query(End("
);
syntax_case!(
    query_insert_into,
    "let value = query { insert users values ^rows }",
    "Into("
);
syntax_case!(
    query_insert_table,
    "let value = query { insert into Users values ^rows }",
    "Insert(Table("
);
syntax_case!(
    query_insert_values,
    "let value = query { insert into users ^rows }",
    "Values("
);
syntax_case!(
    query_insert_pin,
    "let value = query { insert into users values rows }",
    "Pin("
);
syntax_case!(
    query_insert_value,
    "let value = query { insert into users values ^ }",
    "Value("
);
syntax_case!(
    query_update_table,
    "let value = query { update Users set {} }",
    "Update(Table("
);
syntax_case!(
    query_update_set,
    "let value = query { update users { name: ^n } }",
    "Set("
);
syntax_case!(
    query_update_record_open,
    "let value = query { update users set name: ^n }",
    "RecordOpen("
);
syntax_case!(
    query_update_record,
    "let value = query { update users set { name = ^n } }",
    "Record(EqualsNotColon("
);
syntax_case!(
    query_update_where,
    "let value = query { update users set {} where }",
    "Where("
);
syntax_case!(
    query_delete_from,
    "let value = query { delete posts }",
    "Delete(From("
);
syntax_case!(
    query_delete_table,
    "let value = query { delete from }",
    "Delete(Table("
);
syntax_case!(
    query_delete_where,
    "let value = query { delete from posts where }",
    "Where("
);
syntax_case!(table_open, "table users [", "Table(Open(");
syntax_case!(table_column, "table users { Upper: int() }", "Column(");
syntax_case!(table_colon, "table users { id int() }", "Colon(");
syntax_case!(table_builder, "table users { id: }", "Builder(");
syntax_case!(
    table_modifier_argument,
    "table users { id: int() default(,) }",
    "ModifierArg("
);
syntax_case!(
    table_modifier_separator,
    "table users { id: int() default(1 2) }",
    "ModifierArgEnd("
);
syntax_case!(table_end, "table users { id: int() ; }", "End(");
syntax_case!(schema_name, "schema lower {}", "Schema(Name(");
syntax_case!(schema_from, "schema User from Upper {}", "From(");
syntax_case!(schema_open, "schema User [", "Schema(Open(");
syntax_case!(schema_item, "schema User { 42 }", "Schema(Item(");
syntax_case!(schema_pick_name, "schema User { pick }", "PickName(");
syntax_case!(schema_colon, "schema User { name String }", "Colon(");
syntax_case!(schema_type, "schema User { name: Array[] }", "Type(");
syntax_case!(schema_rule, "schema User { name: min(1), 2 }", "Rule(");
syntax_case!(
    schema_rule_argument,
    "schema User { name: min(,) }",
    "RuleArg("
);
syntax_case!(
    schema_rule_separator,
    "schema User { name: min(1 2) }",
    "RuleArgEnd("
);
syntax_case!(schema_end, "schema User { pick email name }", "End(");
syntax_case!(test_name, "test 42 {}", "Test(Name(");
syntax_case!(test_name_string, "test \"\\q\" {}", "NameString(");
syntax_case!(test_body_open, "test \"example\" 1", "Body(Open(");
syntax_case!(test_body_nested, "test \"example\" { read(1 2) }", "Body(");
syntax_case!(tests_open, "tests [", "Tests(Open(");
syntax_case!(tests_nested_item, "tests { test 42 {} }", "Tests(Item(");
syntax_case!(
    tests_same_line,
    "tests { test \"a\" {} test \"b\" {} }",
    "SameLine("
);
syntax_case!(tests_end, "tests { let value = 1 42 }", "Tests(End(");
syntax_case!(tests_unclosed, "tests {", "Tests(End(");

syntax_case!(template_unclosed, "let value = `hello", "Endless(");
syntax_case!(
    template_empty_hole,
    "let value = `hello ${ }`",
    "HoleEmpty("
);
syntax_case!(
    template_hole_end,
    "let value = `hello ${ value )`",
    "HoleEnd("
);
syntax_case!(
    template_nested_expression,
    "let value = `hello ${ read(1 2) }`",
    "HoleExpr("
);
syntax_case!(
    template_unknown_escape,
    "let value = `hello \\q`",
    "Escape(Unknown"
);
syntax_case!(
    tagged_template_hole_end,
    "let value = html`hello ${ value )`",
    "TaggedTemplate("
);
syntax_case!(string_unclosed, "let value = \"hello", "String(Endless");
syntax_case!(
    string_line_break,
    "let value = \"hello\nthere\"",
    "String(Newline"
);
syntax_case!(
    string_unknown_escape,
    "let value = \"\\q\"",
    "Escape(Unknown"
);
syntax_case!(
    string_unicode_format,
    "let value = \"\\u1234\"",
    "BadUnicodeFormat("
);
syntax_case!(
    string_unicode_missing_brace,
    "let value = \"\\u{1234\"",
    "BadUnicodeFormat("
);
syntax_case!(
    string_unicode_surrogate,
    "let value = \"\\u{D800}\"",
    "BadUnicodeCode("
);
syntax_case!(
    string_unicode_out_of_range,
    "let value = \"\\u{110000}\"",
    "BadUnicodeCode("
);
syntax_case!(
    string_unicode_too_short,
    "let value = \"\\u{41}\"",
    "BadUnicodeLength"
);
syntax_case!(
    string_unicode_too_long,
    "let value = \"\\u{0000041}\"",
    "BadUnicodeLength"
);
syntax_case!(
    template_unicode_too_short,
    "let value = `\\u{41}`",
    "BadUnicodeLength"
);
syntax_case!(macro_name, "macro Upper() {}", "Macro(Name(");
syntax_case!(macro_params_open, "macro value {}", "ParamsOpen(");
syntax_case!(macro_param, "macro value(Upper) {}", "Param(");
syntax_case!(
    macro_param_separator,
    "macro value(one two) {}",
    "ParamEnd("
);
syntax_case!(macro_body_open, "macro value() 1", "Body(Open");
syntax_case!(macro_body_unclosed, "macro value() { (", "Body(Endless");
syntax_case!(macro_body_mismatch, "macro value() { (] }", "Unbalanced");
syntax_case!(
    macro_call_unclosed,
    "let value = quote!( (",
    "MacroCall(Endless"
);
syntax_case!(
    macro_call_mismatch,
    "let value = quote!( [) )",
    "MacroCall(Unbalanced"
);
syntax_case!(
    macro_call_string_escape,
    "let value = quote!( \"\\q\" )",
    "String(Escape("
);
syntax_case!(
    macro_call_string_unclosed,
    "let value = quote!( \"hello",
    "String(Endless"
);
syntax_case!(
    macro_call_template_unclosed,
    "let value = quote!( `hello",
    "String(Endless"
);
syntax_case!(
    macro_call_inner_unclosed,
    "let value = quote!( [",
    "MacroCall(Endless"
);
syntax_case!(comptime_open, "comptime 1", "Comptime(Open(");
syntax_case!(comptime_nested_call, "comptime { read(1 2) }", "Comptime(");

#[test]
fn nested_failure_keeps_the_innermost_opener() {
    let source = "let value = if true { [read(1 2)] }";
    let diagnostic = parse_error(source);
    let labels: Vec<_> = diagnostic.labels().unwrap().collect();
    assert_eq!(labels.len(), 2);
    assert!(labels[0].primary());
    assert_eq!(labels[0].offset(), source.find('2').unwrap());
    assert_eq!(labels[1].offset(), source.find('(').unwrap());
    assert_eq!(diagnostic.source().text(), source);
    assert_eq!(diagnostic.source().name(), "/project/src/main.ald");
}

#[test]
fn raw_macro_failure_keeps_the_innermost_delimiter_and_expected_closer() {
    let source = "let value = quote!( { [) } )";
    let diagnostic = parse_error(source);
    let labels: Vec<_> = diagnostic.labels().unwrap().collect();
    assert!(diagnostic.message().contains("needs `]`"));
    assert_eq!(labels[0].offset(), source.find(')').unwrap());
    assert_eq!(labels[1].offset(), source.find('[').unwrap());
}

#[test]
fn string_context_ignores_escaped_quotes_and_previous_strings() {
    let source = "let value = [\"done\", \"unterminated \\\"quote";
    let diagnostic = parse_error(source);
    let labels: Vec<_> = diagnostic.labels().unwrap().collect();
    assert_eq!(labels[0].offset(), source.len());
    assert_eq!(labels[1].offset(), source.find("\"unterminated").unwrap());
    assert_eq!(labels[1].label(), Some("this string starts here"));
}

#[test]
fn unknown_escape_with_unicode_labels_the_entire_sequence() {
    let source = "let value = \"\\😀\"";
    let diagnostic = parse_error(source);
    let primary = diagnostic
        .labels()
        .unwrap()
        .find(|label| label.primary())
        .unwrap();
    assert_eq!(primary.offset(), source.find('\\').unwrap());
    assert_eq!(primary.len(), 1 + '😀'.len_utf8());
}

#[test]
fn raw_template_escape_does_not_label_a_quote_in_template_text_as_a_string() {
    let source = "let value = quote!(`say \"hello\" \\q`)";
    let diagnostic = parse_error(source);
    let labels: Vec<_> = diagnostic.labels().unwrap().collect();
    assert!(
        labels
            .iter()
            .all(|label| label.label() != Some("this string starts here"))
    );
}

#[test]
fn unicode_failure_labels_a_whole_scalar() {
    let source = "let value = [1, 😀]";
    let diagnostic = parse_error(source);
    let primary = diagnostic
        .labels()
        .unwrap()
        .find(|label| label.primary())
        .unwrap();
    assert_eq!(primary.offset(), source.find('😀').unwrap());
    assert_eq!(primary.len(), '😀'.len_utf8());
}

#[test]
fn unicode_before_failure_preserves_byte_offsets() {
    let source = "let value = [\"😀\", read(1 2)]";
    let diagnostic = parse_error(source);
    let primary = diagnostic
        .labels()
        .unwrap()
        .find(|label| label.primary())
        .unwrap();
    assert_eq!(primary.offset(), source.find('2').unwrap());
    assert_eq!(primary.len(), 1);
}

#[test]
fn eof_is_an_insertion_point_not_a_fabricated_source_character() {
    let source = "let value = [1";
    let diagnostic = parse_error(source);
    let primary = diagnostic
        .labels()
        .unwrap()
        .find(|label| label.primary())
        .unwrap();
    assert_eq!(primary.offset(), source.len());
    assert_eq!(primary.len(), 0);
    assert!(diagnostic.message().contains("end of the file"));
    assert_eq!(diagnostic.source().text(), source);
}

#[test]
fn crlf_eof_keeps_the_original_source_and_offset() {
    let source = "let value = [\r\n    1\r\n";
    let diagnostic = parse_error(source);
    let primary = diagnostic
        .labels()
        .unwrap()
        .find(|label| label.primary())
        .unwrap();
    assert_eq!(primary.offset(), source.find('1').unwrap() + 1);
    assert_eq!(primary.len(), 0);
    assert!(
        diagnostic
            .labels()
            .unwrap()
            .all(|label| label.offset() < source.len())
    );
    assert!(diagnostic.message().contains("end of the file"));
    assert_eq!(diagnostic.source().text(), source);
}
