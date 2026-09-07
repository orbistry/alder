//! Source-aware rendering of the active Alder parser error hierarchy.

use alder_region::{Position, Region};
use alder_report::{Diagnostic, Source};

pub fn parse(source: Source, error: &alder_parse::error::Module<'_>) -> Diagnostic {
    let mut problem = SyntaxReporter { source: &source }.module_problem(error);
    let mut primary = syntax_region(&source, problem.row, problem.column);
    if let Some(width) = problem.width {
        primary.end.column = problem.column.saturating_add(width);
    }
    let detection = problem
        .detected
        .map_or(primary, |pos| Region::new(pos, pos));
    if source.span(detection).offset() == source.text().len() {
        problem.message.push_str(" at the end of the file");
    }
    let context = problem
        .context
        .map(|(row, column, label)| (syntax_region(&source, row, column), label));
    let mut diagnostic = Diagnostic::error(source, problem.message)
        .with_code("alder::syntax")
        .with_primary_label(primary, problem.label);
    if let Some(help) = problem.help {
        diagnostic = diagnostic.with_help(help);
    }
    if let Some((region, label)) = context {
        diagnostic = diagnostic.with_secondary_label(region, label);
    }
    diagnostic
}

/// Parser columns count bytes. Label a whole scalar, or a zero-width insertion
/// point at EOF, without changing the retained source or editor coordinates.
fn syntax_region(source: &Source, row: u32, column: u32) -> Region {
    let start = Position::new(row, column);
    let offset = source.span(Region::new(start, start)).offset();
    let width = source
        .text()
        .get(offset..)
        .and_then(|tail| tail.chars().next())
        .filter(|character| *character != '\n' && *character != '\r')
        .map_or(0, char::len_utf8) as u32;
    Region::new(start, Position::new(row, column.saturating_add(width)))
}

struct SyntaxProblem {
    detected: Option<Position>,
    width: Option<u32>,
    context: Option<(u32, u32, &'static str)>,
    message: String,
    row: u32,
    column: u32,
    label: &'static str,
    help: Option<String>,
}

impl SyntaxProblem {
    fn covering(mut self, width: u32) -> Self {
        self.width = Some(width);
        self
    }

    /// Keep the innermost useful context, not every wrapper on the error path.
    fn in_context(mut self, row: u32, column: u32, label: &'static str) -> Self {
        if self.context.is_none() && (self.row, self.column) != (row, column) {
            self.context = Some((row, column, label));
        }
        self
    }
}

fn expected_problem(
    message: impl Into<String>,
    row: u32,
    column: u32,
    label: &'static str,
    help: Option<String>,
) -> SyntaxProblem {
    SyntaxProblem {
        width: None,
        detected: None,
        context: None,
        message: message.into(),
        row,
        column,
        label,
        help,
    }
}

struct SyntaxReporter<'s> {
    source: &'s Source,
}

impl SyntaxReporter<'_> {
    fn end_problem(
        &self,
        end: &alder_parse::error::ExpectedEnd,
        message: impl Into<String>,
        label: &'static str,
        help: Option<String>,
    ) -> SyntaxProblem {
        self.locate_end(
            end,
            expected_problem(
                message,
                end.unexpected.line,
                end.unexpected.column,
                label,
                help,
            ),
        )
    }

    fn locate_end(
        &self,
        end: &alder_parse::error::ExpectedEnd,
        mut problem: SyntaxProblem,
    ) -> SyntaxProblem {
        let label = match self
            .at(end.opening.line, end.opening.column)
            .as_bytes()
            .first()
        {
            Some(b'(') => "this `(` opens here",
            Some(b'[') => "this `[` opens here",
            Some(b'{') => "this `{` opens here",
            Some(b'<') => "this `<` opens here",
            _ => "this delimiter opens here",
        };
        problem.context = Some((end.opening.line, end.opening.column, label));
        // A real wrong closer is useful evidence at the cursor. Otherwise a
        // crossed newline must not blame the next declaration for punctuation
        // missing after the preceding token. No indentation/keyword heuristics.
        if end.boundary.line < end.unexpected.line
            && !self
                .at(end.unexpected.line, end.unexpected.column)
                .starts_with("</")
            && !self
                .at(end.unexpected.line, end.unexpected.column)
                .starts_with([')', ']', '}', '>'])
        {
            problem.row = end.boundary.line;
            problem.column = end.boundary.column;
            problem.width = Some(0);
            problem.detected = Some(end.unexpected);
        }
        problem
    }

    fn at(&self, row: u32, column: u32) -> &str {
        let position = Position::new(row, column);
        let offset = self.source.span(Region::new(position, position)).offset();
        self.source.text().get(offset..).unwrap_or("")
    }

    fn end_position(&self) -> Position {
        let text = self.source.text();
        let row = text.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
        let column = text.rsplit('\n').next().unwrap_or("").len() as u32 + 1;
        Position::new(row, column)
    }

    fn module_problem(&self, error: &alder_parse::error::Module<'_>) -> SyntaxProblem {
        use alder_parse::error::Module;
        match error {
            Module::Item(error, ..) => self.item_problem(error),
            Module::SameLine(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "module items must be separated by a line break".to_owned(),
                row: *row,
                column: *column,
                label: "this item starts on the previous item's line",
                help: Some("start this item on a new line".to_owned()),
            },
            Module::BadEnd(row, column) => expected_problem(
                "this input cannot start a module item",
                *row,
                *column,
                "expected a declaration or import",
                Some(
                    "module items start with keywords such as `fn`, `let`, `type`, or `import`"
                        .to_owned(),
                ),
            ),
        }
    }

    fn item_problem(&self, error: &alder_parse::error::Item<'_>) -> SyntaxProblem {
        use alder_parse::error::Item;
        match error {
            Item::Start(row, column) => expected_problem(
                "I was expecting a module item such as `fn`, `let`, `enum`, or `trait`",
                *row,
                *column,
                "expected a declaration or import",
                None,
            ),
            Item::AfterPub(row, column) => expected_problem(
                "I was expecting an item after `pub`",
                *row,
                *column,
                "expected a declaration or import",
                Some(
                    "`pub` marks the following item as public, as in `pub fn main() { 0 }`"
                        .to_owned(),
                ),
            ),
            Item::Semicolon(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "module items are separated by line breaks, not semicolons".to_owned(),
                row: *row,
                column: *column,
                label: "remove this semicolon",
                help: None,
            },
            Item::Trait(error, row, column) => self.trait_problem(error).in_context(
                *row,
                *column,
                "this trait declaration starts here",
            ),
            Item::Impl(error, row, column) => self.impl_problem(error).in_context(
                *row,
                *column,
                "this impl declaration starts here",
            ),
            Item::Attribute(error, row, column) => self.attribute_problem(error).in_context(
                *row,
                *column,
                "this attribute sequence starts here",
            ),
            Item::Import(error, row, column) => {
                self.import_problem(error)
                    .in_context(*row, *column, "this import starts here")
            }
            Item::Fn(error, row, column) => self.function_problem(error, "function").in_context(
                *row,
                *column,
                "this function declaration starts here",
            ),
            Item::Let(error, row, column) => {
                self.let_problem(error)
                    .in_context(*row, *column, "this binding starts here")
            }
            Item::TypeAlias(error, row, column) => self.type_alias_problem(error).in_context(
                *row,
                *column,
                "this type alias starts here",
            ),
            Item::Enum(error, row, column) => self.enum_problem(error).in_context(
                *row,
                *column,
                "this enum declaration starts here",
            ),
            Item::ErrorDecl(error, row, column) => self.error_decl_problem(error).in_context(
                *row,
                *column,
                "this error declaration starts here",
            ),
            Item::Component(error, row, column) => self.component_problem(error).in_context(
                *row,
                *column,
                "this component declaration starts here",
            ),
            Item::Table(error, row, column) => self.table_problem(error).in_context(
                *row,
                *column,
                "this table declaration starts here",
            ),
            Item::Schema(error, row, column) => self.schema_problem(error).in_context(
                *row,
                *column,
                "this schema declaration starts here",
            ),
            Item::Macro(error, row, column) => self.macro_problem(error).in_context(
                *row,
                *column,
                "this macro declaration starts here",
            ),
            Item::Comptime(error, row, column) => self.block_problem(error).in_context(
                *row,
                *column,
                "this `comptime` block starts here",
            ),
            Item::Test(error, row, column) => self.test_problem(error).in_context(
                *row,
                *column,
                "this test declaration starts here",
            ),
            Item::Tests(error, row, column) => {
                self.tests_problem(error)
                    .in_context(*row, *column, "this tests block starts here")
            }
        }
    }

    fn trait_problem(&self, error: &alder_parse::error::Trait<'_>) -> SyntaxProblem {
        use alder_parse::error::Trait;
        match error {
            Trait::Name(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting a trait name after `trait`".to_owned(),
                row: *row,
                column: *column,
                label: "expected an upper-case name",
                help: Some("trait declarations start like `trait Show[a]`".to_owned()),
            },
            Trait::Params(error, row, column) => self
                .type_params_problem(error, "trait")
                .in_context(*row, *column, "this type parameter list starts here"),
            Trait::Where(error, ..) => self.where_problem(error),
            Trait::Open(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting `{` to start this trait body".to_owned(),
                row: *row,
                column: *column,
                label: "expected `{` here",
                help: None,
            },
            Trait::Item(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting a trait method, associated type, or `}`".to_owned(),
                row: *row,
                column: *column,
                label: "expected `fn`, `type`, or `}`",
                help: None,
            },
            Trait::SameLine(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "trait items must be separated by a line break".to_owned(),
                row: *row,
                column: *column,
                label: "this item needs its own line",
                help: None,
            },
            Trait::Semicolon(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "trait items are separated by line breaks, not semicolons".to_owned(),
                row: *row,
                column: *column,
                label: "remove this semicolon",
                help: None,
            },
            Trait::AssocType(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting an associated type name after `type`".to_owned(),
                row: *row,
                column: *column,
                label: "expected an upper-case name",
                help: Some("declare an associated type like `type Item`".to_owned()),
            },
            Trait::AssocTypeHasBody(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "associated types are declared without a value in traits".to_owned(),
                row: *row,
                column: *column,
                label: "remove this definition",
                help: Some("provide the associated type value in each `impl`".to_owned()),
            },
            Trait::Fn(error, row, column) => self
                .function_problem(error, "trait method")
                .in_context(*row, *column, "this trait method starts here"),
        }
    }

    fn impl_problem(&self, error: &alder_parse::error::Impl<'_>) -> SyntaxProblem {
        use alder_parse::error::Impl;
        match error {
            Impl::Trait(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting a trait name after `impl`".to_owned(),
                row: *row,
                column: *column,
                label: "expected an upper-case path",
                help: Some("implement a trait like `impl Show[User] { ... }`".to_owned()),
            },
            Impl::PathMember(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting a trait name after `::`".to_owned(),
                row: *row,
                column: *column,
                label: "expected a name here",
                help: None,
            },
            Impl::Open(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting `[` after the trait name".to_owned(),
                row: *row,
                column: *column,
                label: "expected `[` here",
                help: Some("the subject type goes in brackets, as in `impl Show[User]`".to_owned()),
            },
            Impl::Arg(error, ..) => self.type_problem(error),
            Impl::ArgEnd(end) => self.locate_end(
                end,
                SyntaxProblem {
                    width: None,
                    detected: None,
                    context: None,
                    message: "I was expecting `,` or `]` after this trait argument".to_owned(),
                    row: end.unexpected.line,
                    column: end.unexpected.column,
                    label: "expected `,` or `]`",
                    help: None,
                },
            ),
            Impl::Where(error, ..) => self.where_problem(error),
            Impl::BodyOpen(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting `{` to start this impl body".to_owned(),
                row: *row,
                column: *column,
                label: "expected `{` here",
                help: None,
            },
            Impl::Item(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting an impl method, associated type, or `}`".to_owned(),
                row: *row,
                column: *column,
                label: "expected `fn`, `type`, or `}`",
                help: None,
            },
            Impl::SameLine(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "impl items must be separated by a line break".to_owned(),
                row: *row,
                column: *column,
                label: "this item needs its own line",
                help: None,
            },
            Impl::Semicolon(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "impl items are separated by line breaks, not semicolons".to_owned(),
                row: *row,
                column: *column,
                label: "remove this semicolon",
                help: None,
            },
            Impl::AssocType(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting an associated type name after `type`".to_owned(),
                row: *row,
                column: *column,
                label: "expected an upper-case name",
                help: None,
            },
            Impl::AssocEquals(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting `=` after this associated type name".to_owned(),
                row: *row,
                column: *column,
                label: "expected `=` here",
                help: Some("define it like `type Item = Value`".to_owned()),
            },
            Impl::AssocBody(error, ..) => self.type_problem(error),
            Impl::Fn(error, row, column) => self
                .function_problem(error, "implementation method")
                .in_context(*row, *column, "this impl method starts here"),
        }
    }

    fn type_params_problem(
        &self,
        error: &alder_parse::error::TypeParams,
        owner: &str,
    ) -> SyntaxProblem {
        use alder_parse::error::TypeParams;
        match error {
            TypeParams::Open(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: format!("I was expecting `[` after this {owner} name"),
                row: *row,
                column: *column,
                label: "expected `[` here",
                help: Some(format!("{owner} parameters look like `[a]` or `[f, a]`")),
            },
            TypeParams::Var(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: format!("I was expecting a type parameter in this {owner} declaration"),
                row: *row,
                column: *column,
                label: "expected a lower-case name",
                help: None,
            },
            TypeParams::End(end) => self.locate_end(
                end,
                SyntaxProblem {
                    width: None,
                    detected: None,
                    context: None,
                    message: "I was expecting `,` or `]` after this type parameter".to_owned(),
                    row: end.unexpected.line,
                    column: end.unexpected.column,
                    label: "expected `,` or `]`",
                    help: None,
                },
            ),
            TypeParams::Empty(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "type parameter lists cannot be empty".to_owned(),
                row: *row,
                column: *column,
                label: "empty parameter list",
                help: None,
            },
        }
    }

    fn where_problem(&self, error: &alder_parse::error::Where<'_>) -> SyntaxProblem {
        use alder_parse::error::Where;
        match error {
            Where::Var(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting a type variable after `where`".to_owned(),
                row: *row,
                column: *column,
                label: "expected a lower-case type variable",
                help: Some("a bound looks like `where a: Show`".to_owned()),
            },
            Where::Colon(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting a trait bound or associated type equality".to_owned(),
                row: *row,
                column: *column,
                label: "expected `:` or `.Assoc ==`",
                help: Some("write `a: Show` or `i.Item == Number`".to_owned()),
            },
            Where::Bound(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting a trait name in this bound".to_owned(),
                row: *row,
                column: *column,
                label: "expected an upper-case trait path",
                help: None,
            },
            Where::AssocName(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting an associated type name after `.`".to_owned(),
                row: *row,
                column: *column,
                label: "expected an upper-case name",
                help: None,
            },
            Where::AssocEq(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "I was expecting `==` after this associated type".to_owned(),
                row: *row,
                column: *column,
                label: "expected `==` here",
                help: None,
            },
            Where::Type(error, ..) => self.type_problem(error),
        }
    }

    fn type_problem(&self, error: &alder_parse::error::Type<'_>) -> SyntaxProblem {
        use alder_parse::error::Type;
        match error {
            Type::Start(row, column) => expected_problem("I was expecting a type", *row, *column,
                "expected a type here", Some("types include names like `Number`, applications like `Array[Number]`, and type variables like `a`".to_owned())),
            Type::Reserved(keyword, row, column) => SyntaxProblem {
                width: Some(keyword.as_str().len() as u32),
                detected: None,
                context: None,
                message: format!("`{}` is reserved and cannot be used as a type", keyword.as_str()),
                row: *row,
                column: *column,
                label: "this is a reserved word",
                help: None,
            },
            Type::PathMember(row, column) => {
                expected_problem("I was expecting a type name after `::`", *row, *column,
                    "expected an uppercase type name",
                    Some("type path members start with an uppercase letter, as in `Outer::Inner`".to_owned()))
            }
            Type::Args(error, row, column) => self.type_args_problem(error).in_context(*row, *column, "this type argument list starts here"),
            Type::Fn(error, row, column) => self.function_type_problem(error).in_context(*row, *column, "this function type starts here"),
            Type::Tuple(error, row, column) => self.tuple_type_problem(error).in_context(*row, *column, "this tuple type starts here"),
            Type::Record(error, row, column) => self.record_type_problem(error).in_context(*row, *column, "this record type starts here"),
            Type::ErrorRow(error, row, column) => self.error_row_problem(error).in_context(*row, *column, "this error row starts here"),
            Type::TooDeep(row, column) => SyntaxProblem {
                width: None,
                detected: None,
                context: None,
                message: "this type is nested too deeply".to_owned(),
                row: *row,
                column: *column,
                label: "nesting limit reached here",
                help: Some("split this type into named aliases".to_owned()),
            },
        }
    }

    fn attribute_problem(&self, error: &alder_parse::error::Attribute<'_>) -> SyntaxProblem {
        use alder_parse::error::Attribute;
        match error {
            Attribute::Open(row, column) => expected_problem(
                "I found `#`, but an attribute starts with `#[`",
                *row,
                *column,
                "expected `[` after `#`",
                Some("write an attribute like `#[derive(Eq)]`".to_owned()),
            ),
            Attribute::Name(row, column) => expected_problem(
                "I was expecting an attribute name",
                *row,
                *column,
                "expected a lower-case name",
                None,
            ),
            Attribute::Arg(error, ..) => self.expression_problem(error),
            Attribute::ArgEnd(end) => self.end_problem(
                end,
                "I was expecting `,` or `)` after this attribute argument",
                "expected `,` or `)`",
                None,
            ),
            Attribute::End(end) => self.end_problem(
                end,
                "this attribute is missing its closing bracket",
                "expected `]`",
                None,
            ),
            Attribute::Dangling(row, column) => expected_problem(
                "an attribute must be followed by a declaration",
                *row,
                *column,
                "expected a declaration here",
                None,
            ),
        }
    }

    fn import_problem(&self, error: &alder_parse::error::Import<'_>) -> SyntaxProblem {
        use alder_parse::error::Import;
        match error {
            Import::Path(error, ..) => self.module_path_problem(error),
            Import::Tail(row, column) => expected_problem(
                "I was expecting imported names after this dot",
                *row,
                *column,
                "expected `{` or `*`",
                None,
            ),
            Import::Name(row, column) => expected_problem(
                "I was expecting a name in this import list",
                *row,
                *column,
                "expected an imported name",
                None,
            ),
            Import::NameAlias(row, column) => expected_problem(
                "I was expecting an alias for this imported name after `as`",
                *row,
                *column,
                "expected an identifier",
                Some(
                    "rename an imported name inside the list, as in `.{ Request as HttpRequest }`"
                        .to_owned(),
                ),
            ),
            Import::Alias(row, column) => expected_problem(
                "I was expecting a lower-case alias after `as`",
                *row,
                *column,
                "expected an alias",
                None,
            ),
            Import::NamesEnd(end) => self.end_problem(
                end,
                "I was expecting `,` or `}` after this imported name",
                "expected `,` or `}`",
                None,
            ),
            Import::GroupEnd(end) => self.end_problem(
                end,
                "I was expecting `,` or `)` after this imported module",
                "expected `,` or `)`",
                None,
            ),
            Import::ReservedBinding(keyword, row, column) => expected_problem(
                format!(
                    "`{}` is reserved and cannot be the local module name",
                    keyword.as_str()
                ),
                *row,
                *column,
                "reserved word used as a binding",
                Some("add `as name` or import selected names with `.{ ... }`".to_owned()),
            )
            .covering(keyword.as_str().len() as u32),
            Import::RootOnly(row, column) => expected_problem(
                "the local import root does not provide a module name",
                *row,
                *column,
                "expected a module path",
                Some("add a path, an alias, or a selected-name import".to_owned()),
            ),
        }
    }

    fn module_path_problem(&self, error: &alder_parse::error::ModulePath) -> SyntaxProblem {
        use alder_parse::error::ModulePath;
        match error {
            ModulePath::Start(row, column) => expected_problem(
                "I was expecting an import path",
                *row,
                *column,
                "expected a bundled module name, `@`, or `~`",
                None,
            ),
            ModulePath::Author(row, column) => expected_problem(
                "I was expecting a package author after `@`",
                *row,
                *column,
                "expected a lower-case author name",
                None,
            ),
            ModulePath::Slash(row, column) => expected_problem(
                "I was expecting `/` between the package author and name",
                *row,
                *column,
                "expected `/`",
                None,
            ),
            ModulePath::Package(row, column) => expected_problem(
                "I was expecting a package name after `/`",
                *row,
                *column,
                "expected a lower-case package name",
                None,
            ),
            ModulePath::Segment(row, column) => expected_problem(
                "I was expecting a module path segment after `/`",
                *row,
                *column,
                "expected a lower-case name",
                None,
            ),
        }
    }

    fn function_problem(&self, error: &alder_parse::error::Fn<'_>, owner: &str) -> SyntaxProblem {
        use alder_parse::error::Fn;
        match error {
            Fn::Keyword(row, column) => expected_problem(
                "I was expecting `fn` after `async` in this declaration",
                *row,
                *column,
                "expected `fn`",
                None,
            ),
            Fn::Name(row, column) => expected_problem(
                format!("I was expecting a name for this {owner}"),
                *row,
                *column,
                "expected a lower-case name",
                None,
            ),
            Fn::Params(error, row, column) => self.params_problem(error).in_context(
                *row,
                *column,
                "this parameter list starts here",
            ),
            Fn::Ret(error, ..) => self.type_problem(error),
            Fn::Where(error, ..) => self.where_problem(error),
            Fn::Body(error, row, column) => self.block_problem(error).in_context(
                *row,
                *column,
                "this function body starts here",
            ),
        }
    }

    fn params_problem(&self, error: &alder_parse::error::Params<'_>) -> SyntaxProblem {
        use alder_parse::error::Params;
        match error {
            Params::Open(row, column) => expected_problem(
                "I was expecting `(` to start the parameter list",
                *row,
                *column,
                "expected `(`",
                None,
            ),
            Params::Pattern(error, ..) => self.pattern_problem(error),
            Params::Type(error, ..) => self.type_problem(error),
            Params::OptionalAnnotation(row, column) => expected_problem(
                "I was expecting a type annotation for this optional parameter",
                *row,
                *column,
                "expected `: Type`",
                None,
            ),
            Params::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `)` after this parameter",
                "expected `,` or `)`",
                None,
            ),
        }
    }

    fn let_problem(&self, error: &alder_parse::error::Let<'_>) -> SyntaxProblem {
        use alder_parse::error::Let;
        match error {
            Let::Pattern(error, ..) => self.pattern_problem(error),
            Let::Type(error, ..) => self.type_problem(error),
            Let::Equals(row, column) => expected_problem(
                "I was expecting `=` after this binding",
                *row,
                *column,
                "expected `=`",
                None,
            ),
            Let::Body(error, ..) => self.expression_in(error, "a value after `=` in this binding"),
        }
    }

    fn type_alias_problem(&self, error: &alder_parse::error::TypeAlias<'_>) -> SyntaxProblem {
        use alder_parse::error::TypeAlias;
        match error {
            TypeAlias::Name(row, column) => expected_problem(
                "I was expecting a name after `type`",
                *row,
                *column,
                "expected an upper-case name",
                Some("a type alias starts like `type UserId = Number`".to_owned()),
            ),
            TypeAlias::Params(error, row, column) => self
                .type_params_problem(error, "type alias")
                .in_context(*row, *column, "this type parameter list starts here"),
            TypeAlias::Body(error, ..) => self.type_problem(error),
        }
    }

    fn enum_problem(&self, error: &alder_parse::error::Enum<'_>) -> SyntaxProblem {
        use alder_parse::error::Enum;
        match error {
            Enum::Name(row, column) => expected_problem(
                "I was expecting an enum name after `enum`",
                *row,
                *column,
                "expected an upper-case name",
                None,
            ),
            Enum::Params(error, row, column) => self.type_params_problem(error, "enum").in_context(
                *row,
                *column,
                "this type parameter list starts here",
            ),
            Enum::Open(row, column) => expected_problem(
                "I was expecting `{` to start this enum",
                *row,
                *column,
                "expected `{`",
                None,
            ),
            Enum::Variant(row, column) => expected_problem(
                "I was expecting an enum variant",
                *row,
                *column,
                "expected an upper-case variant name",
                None,
            ),
            Enum::VariantArg(error, ..) => self.type_problem(error),
            Enum::VariantArgEnd(end) => self.end_problem(
                end,
                "I was expecting `,` or `)` after this variant payload type",
                "expected `,` or `)`",
                None,
            ),
            Enum::VariantRecord(error, row, column) => self.record_type_problem(error).in_context(
                *row,
                *column,
                "this variant record starts here",
            ),
            Enum::VariantRecordExt(row, column) => expected_problem(
                "enum record payloads cannot extend another record",
                *row,
                *column,
                "remove this record extension",
                None,
            ),
            Enum::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `}` after this enum variant",
                "expected `,` or `}`",
                None,
            ),
        }
    }

    fn error_decl_problem(&self, error: &alder_parse::error::ErrorDecl<'_>) -> SyntaxProblem {
        use alder_parse::error::ErrorDecl;
        match error {
            ErrorDecl::Name(row, column) => expected_problem(
                "I was expecting a name after `error`",
                *row,
                *column,
                "expected an upper-case name",
                None,
            ),
            ErrorDecl::Open(row, column) => expected_problem(
                "I was expecting `{` to start this error group",
                *row,
                *column,
                "expected `{`",
                None,
            ),
            ErrorDecl::Tag(error, ..) => self.tag_variant_problem(error),
            ErrorDecl::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `}` after this error tag",
                "expected `,` or `}`",
                None,
            ),
        }
    }

    fn tag_variant_problem(&self, error: &alder_parse::error::TagVariant<'_>) -> SyntaxProblem {
        use alder_parse::error::TagVariant;
        match error {
            TagVariant::Name(row, column) => expected_problem(
                "I was expecting an error tag such as `:failed`",
                *row,
                *column,
                "expected `:` followed by a lower-case tag name",
                None,
            ),
            TagVariant::Arg(error, ..) => self.type_problem(error),
            TagVariant::ArgEnd(end) => self.end_problem(
                end,
                "I was expecting `,` or `)` after this tag payload type",
                "expected `,` or `)`",
                None,
            ),
        }
    }

    fn component_problem(&self, error: &alder_parse::error::Component<'_>) -> SyntaxProblem {
        use alder_parse::error::Component;
        match error {
            Component::Name(row, column) => expected_problem(
                "I was expecting a component name",
                *row,
                *column,
                "expected a non-reserved identifier",
                None,
            ),
            Component::Params(error, row, column) => self.params_problem(error).in_context(
                *row,
                *column,
                "this parameter list starts here",
            ),
            Component::Body(error, row, column) => self.block_problem(error).in_context(
                *row,
                *column,
                "this component body starts here",
            ),
        }
    }

    fn type_args_problem(&self, error: &alder_parse::error::TArgs<'_>) -> SyntaxProblem {
        use alder_parse::error::TArgs;
        match error {
            TArgs::Type(error, ..) => self.type_problem(error),
            TArgs::Empty(row, column) => expected_problem(
                "type argument lists cannot be empty",
                *row,
                *column,
                "add a type argument or remove `[]`",
                None,
            ),
            TArgs::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `]` after this type argument",
                "expected `,` or `]`",
                Some("separate type arguments with commas and close the list with `]`".to_owned()),
            ),
        }
    }

    fn function_type_problem(&self, error: &alder_parse::error::TFn<'_>) -> SyntaxProblem {
        use alder_parse::error::TFn;
        match error {
            TFn::Open(row, column) => expected_problem(
                "I was expecting `(` after `fn` in this function type",
                *row,
                *column,
                "expected `(`",
                None,
            ),
            TFn::Param(error, ..) | TFn::Ret(error, ..) => self.type_problem(error),
            TFn::ParamEnd(end) => self.end_problem(
                end,
                "I was expecting `,` or `)` after this parameter type",
                "expected `,` or `)`",
                Some("separate parameter types with commas and close the list with `)`".to_owned()),
            ),
        }
    }

    fn tuple_type_problem(&self, error: &alder_parse::error::TTuple<'_>) -> SyntaxProblem {
        use alder_parse::error::TTuple;
        match error {
            TTuple::Type(error, ..) => self.type_problem(error),
            TTuple::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `)` after this tuple entry",
                "expected `,` or `)`",
                Some("separate tuple entries with commas and close the type with `)`".to_owned()),
            ),
        }
    }

    fn record_type_problem(&self, error: &alder_parse::error::TRecord<'_>) -> SyntaxProblem {
        use alder_parse::error::TRecord;
        match error {
            TRecord::Field(row, column) => expected_problem(
                "I was expecting a field name in this record type",
                *row,
                *column,
                "expected a lower-case field name",
                None,
            ),
            TRecord::Colon(row, column) => expected_problem(
                "I was expecting a colon after this record field name",
                *row,
                *column,
                "expected `:` or `?:`",
                None,
            ),
            TRecord::Type(error, ..) => self.type_problem(error),
            TRecord::ExtField(row, column) => expected_problem(
                "an extended record type needs at least one field",
                *row,
                *column,
                "expected a field after `|`",
                None,
            ),
            TRecord::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `}` after this record field",
                "expected `,` or `}`",
                Some("separate record fields with commas and close the type with `}`".to_owned()),
            ),
        }
    }

    fn error_row_problem(&self, error: &alder_parse::error::TErrorRow<'_>) -> SyntaxProblem {
        use alder_parse::error::TErrorRow;
        match error {
            TErrorRow::Start(row, column) => expected_problem(
                "I was expecting an error tag, row variable, or `]`",
                *row,
                *column,
                "expected `:tag`, a row variable, or `]`",
                None,
            ),
            TErrorRow::Tag(error, ..) => self.tag_variant_problem(error),
            TErrorRow::Ext(row, column) => expected_problem(
                "I was expecting an error tag or row variable after `|`",
                *row,
                *column,
                "expected `:tag` or a row variable",
                None,
            ),
            TErrorRow::End(end) => self.end_problem(
                end,
                "I was expecting `|` or `]` after this error tag",
                "expected `|` or `]`",
                Some("separate error tags with `|` and close the row with `]`".to_owned()),
            ),
            TErrorRow::ExtEnd(end) => self.end_problem(
                end,
                "I was expecting `]` after this error row extension",
                "expected `]`",
                Some("the extension variable must be the last entry in an error row".to_owned()),
            ),
        }
    }

    fn block_problem(&self, error: &alder_parse::error::Block<'_>) -> SyntaxProblem {
        use alder_parse::error::Block;
        match error {
            Block::Open(row, column) => expected_problem(
                "I was expecting `{` to start this block",
                *row,
                *column,
                "expected `{`",
                None,
            ),
            Block::Stmt(error, ..) => self.statement_problem(error),
            Block::SameLine(row, column) => expected_problem(
                "statements must be separated by a line break",
                *row,
                *column,
                "this statement starts on the previous statement's line",
                Some("start this statement on a new line".to_owned()),
            ),
            Block::LooksLikeRecord(row, column) => expected_problem(
                "this looks like a record where a block was expected",
                *row,
                *column,
                "record syntax starts here",
                Some("wrap the record in parentheses to use it as the block value".to_owned()),
            ),
            Block::End(end) => self.end_problem(
                end,
                "I was expecting another statement or the end of this block",
                "expected a statement or `}`",
                None,
            ),
            Block::TooDeep(row, column) => expected_problem(
                "this block is nested too deeply",
                *row,
                *column,
                "nesting limit reached here",
                Some("move part of this expression into a named function".to_owned()),
            ),
        }
    }

    fn statement_problem(&self, error: &alder_parse::error::Stmt<'_>) -> SyntaxProblem {
        use alder_parse::error::Stmt;
        match error {
            Stmt::Let(error, row, column) => {
                self.let_problem(error)
                    .in_context(*row, *column, "this binding starts here")
            }
            Stmt::Use(row, column) => expected_problem(
                "I was expecting a provider name after `use`",
                *row,
                *column,
                "expected a provider path",
                None,
            ),
            Stmt::UseMember(row, column) => expected_problem(
                "`use` names a provider, not one of its members",
                *row,
                *column,
                "remove this member access",
                None,
            ),
            Stmt::For(error, row, column) => {
                self.for_problem(error)
                    .in_context(*row, *column, "this `for` loop starts here")
            }
            Stmt::While(error, row, column) => {
                self.while_problem(error)
                    .in_context(*row, *column, "this `while` loop starts here")
            }
            Stmt::Return(error, ..) => self.expression_in(error, "the value to return"),
            Stmt::Break(error, ..) => self.expression_in(error, "the value to break with"),
            Stmt::Assert(error, ..) => self.expression_in(error, "a condition after `assert`"),
            Stmt::Expr(error, ..) => self.expression_problem(error),
            Stmt::AssignValue(error, ..) => {
                self.expression_in(error, "a value on the right side of this assignment")
            }
            Stmt::AssignTarget(operator, row, column) => expected_problem(
                "the left side of this assignment is not assignable",
                *row,
                *column,
                "expected a variable, field, or index",
                matches!(operator, alder_source::AssignOp::Div)
                    .then(|| "if you meant inequality, use `!=` instead of `/=`".to_owned()),
            ),
            Stmt::Semicolon(row, column) => expected_problem(
                "statements are separated by line breaks, not semicolons",
                *row,
                *column,
                "remove this semicolon",
                None,
            ),
        }
    }

    fn pattern_problem(&self, error: &alder_parse::error::Pattern<'_>) -> SyntaxProblem {
        use alder_parse::error::Pattern;
        match error {
            Pattern::Start(row, column) => expected_problem(
                "I was expecting a pattern",
                *row,
                *column,
                "expected a pattern here",
                Some(
                    "a pattern can bind a name, destructure a value, or use `_` to ignore it"
                        .to_owned(),
                ),
            ),
            Pattern::Reserved(keyword, row, column) => expected_problem(
                format!(
                    "`{}` is reserved and cannot be used as a pattern",
                    keyword.as_str()
                ),
                *row,
                *column,
                "reserved word used here",
                None,
            )
            .covering(keyword.as_str().len() as u32),
            Pattern::SqlKeyword(keyword, row, column) => expected_problem(
                format!("`{}` is a query keyword, not a pattern", keyword.as_str()),
                *row,
                *column,
                "query keyword used here",
                None,
            )
            .covering(keyword.as_str().len() as u32),
            Pattern::Number(error, row, column) => self.number_problem(error, *row, *column),
            Pattern::String(error, row, column) => self.string_problem(error, *row, *column),
            Pattern::Pin(error, ..) => self.expression_problem(error),
            Pattern::PathMember(row, column) => expected_problem(
                "I was expecting a constructor name after `::`",
                *row,
                *column,
                "expected a name",
                None,
            ),
            Pattern::PathVar(row, column) => expected_problem(
                "a value path must be pinned when used as a pattern",
                *row,
                *column,
                "this is a value path",
                Some("prefix the path with `^` to compare against its value".to_owned()),
            ),
            Pattern::Ctor(error, row, column) | Pattern::Tag(error, row, column) => self
                .constructor_pattern_problem(error)
                .in_context(*row, *column, "this constructor pattern starts here"),
            Pattern::TagName(row, column) => expected_problem(
                "I was expecting a lower-case tag name after `:`",
                *row,
                *column,
                "expected a tag name",
                None,
            ),
            Pattern::Tuple(error, row, column) => self.tuple_pattern_problem(error).in_context(
                *row,
                *column,
                "these pattern parentheses start here",
            ),
            Pattern::Array(error, row, column) => self.array_pattern_problem(error).in_context(
                *row,
                *column,
                "this array pattern starts here",
            ),
            Pattern::Record(error, row, column) => self.record_pattern_problem(error).in_context(
                *row,
                *column,
                "this record pattern starts here",
            ),
            Pattern::Alias(row, column) => expected_problem(
                "I was expecting a binding name after `as`",
                *row,
                *column,
                "expected a lower-case name",
                None,
            ),
            Pattern::WildcardNotVar(name, width, row, column) => expected_problem(
                format!("`{name}` looks like a wildcard, not a binding"),
                *row,
                *column,
                "only `_` is a wildcard",
                Some(
                    "use `_` to ignore the value, or choose a binding name starting with a letter"
                        .to_owned(),
                ),
            )
            .covering(*width as u32),
            Pattern::TooDeep(row, column) => expected_problem(
                "this pattern is nested too deeply",
                *row,
                *column,
                "nesting limit reached here",
                Some("match the outer structure first, then inspect nested values in a separate match".to_owned()),
            ),
        }
    }

    fn expression_problem(&self, error: &alder_parse::error::Expr<'_>) -> SyntaxProblem {
        use alder_parse::error::Expr;
        match error {
            Expr::Start(row, column) => expected_problem(
                "I was expecting an expression",
                *row,
                *column,
                "expected an expression here",
                None,
            ),
            Expr::Reserved(keyword, row, column) => expected_problem(
                format!("`{}` cannot start an expression here", keyword.as_str()),
                *row,
                *column,
                "reserved word used here",
                None,
            )
            .covering(keyword.as_str().len() as u32),
            Expr::SqlKeyword(keyword, row, column) => expected_problem(
                format!("`{}` is a query keyword, not a value", keyword.as_str()),
                *row,
                *column,
                "query keyword used as a value",
                None,
            )
            .covering(keyword.as_str().len() as u32),
            Expr::Number(error, row, column) => self.number_problem(error, *row, *column),
            Expr::String(error, row, column) => self.string_problem(error, *row, *column),
            Expr::Template(error, row, column) | Expr::TaggedTemplate(error, row, column) => self
                .template_problem(error)
                .in_context(*row, *column, "this template starts here"),
            Expr::Array(error, row, column) => {
                self.array_problem(error)
                    .in_context(*row, *column, "this array starts here")
            }
            Expr::Tuple(error, row, column) => {
                self.tuple_problem(error)
                    .in_context(*row, *column, "these parentheses start here")
            }
            Expr::Record(error, row, column) | Expr::RecordCtor(error, row, column) => self
                .record_problem(error)
                .in_context(*row, *column, "this record starts here"),
            Expr::Block(error, row, column) => {
                self.block_problem(error)
                    .in_context(*row, *column, "this block starts here")
            }
            Expr::Loop(error, row, column) => {
                self.block_problem(error)
                    .in_context(*row, *column, "this loop starts here")
            }
            Expr::Lambda(error, row, column) => self.lambda_problem(error).in_context(
                *row,
                *column,
                "this anonymous function starts here",
            ),
            Expr::If(error, row, column) => {
                self.if_problem(error)
                    .in_context(*row, *column, "this `if` starts here")
            }
            Expr::Match(error, row, column) => {
                self.match_problem(error)
                    .in_context(*row, *column, "this `match` starts here")
            }
            Expr::Provide(error, row, column) => {
                self.provide_problem(error)
                    .in_context(*row, *column, "this `provide` starts here")
            }
            Expr::Call(error, row, column) => {
                self.call_problem(error)
                    .in_context(*row, *column, "this argument list starts here")
            }
            Expr::Index(error, row, column) => {
                self.index_problem(error)
                    .in_context(*row, *column, "this index starts here")
            }
            Expr::Tag(error, row, column) => {
                self.tag_problem(error)
                    .in_context(*row, *column, "this tagged value starts here")
            }
            Expr::State(error, row, column) => {
                self.state_problem(error)
                    .in_context(*row, *column, "this `state` starts here")
            }
            Expr::Style(error, row, column) => {
                self.style_problem(error)
                    .in_context(*row, *column, "this style block starts here")
            }
            Expr::Query(error, row, column) => {
                self.query_problem(error)
                    .in_context(*row, *column, "this query starts here")
            }
            Expr::Markup(error, row, column) => {
                self.markup_problem(error)
                    .in_context(*row, *column, "this markup starts here")
            }
            Expr::MacroCall(error, row, column) => {
                self.raw_tokens_problem(error, *row, *column, false)
            }
            Expr::PathMember(row, column) => expected_problem(
                "I was expecting a name after `::`",
                *row,
                *column,
                "expected a path member",
                None,
            ),
            Expr::Access(row, column) => expected_problem(
                "I was expecting a field name, tuple index, or `await` after `.`",
                *row,
                *column,
                "expected an accessor",
                None,
            ),
            Expr::TupleIndexOverflow(row, column) => expected_problem(
                "this tuple index is too large",
                *row,
                *column,
                "tuple index exceeds the supported integer range",
                Some(
                    "tuple indices must fit in an unsigned 32-bit integer (0 through 4294967295)"
                        .to_owned(),
                ),
            ),
            Expr::Unary(row, column) => expected_problem(
                "this unary operator is missing its operand",
                *row,
                *column,
                "expected an expression",
                None,
            ),
            Expr::PinOutsideQuery(row, column) => expected_problem(
                "expression pins are only allowed inside queries",
                *row,
                *column,
                "`^` is not valid here",
                None,
            ),
            Expr::Placeholder(row, column) => expected_problem(
                "`_` is only allowed as a complete call argument",
                *row,
                *column,
                "placeholder is not valid here",
                None,
            ),
            Expr::OperatorReserved(operator, row, column) => {
                self.bad_operator_problem(operator, *row, *column)
            }
            Expr::OperatorRight(operator, row, column) => expected_problem(
                format!(
                    "operator `{}` is missing its right operand",
                    operator.as_str()
                ),
                *row,
                *column,
                "expected an expression",
                None,
            ),
            Expr::UnexpectedClose(row, column) => expected_problem(
                "I found a closing markup tag where an expression was expected",
                *row,
                *column,
                "unexpected closing tag",
                None,
            ),
            Expr::TooDeep(row, column) => expected_problem(
                "this expression is nested too deeply",
                *row,
                *column,
                "nesting limit reached here",
                Some("move part of this expression into a named binding".to_owned()),
            ),
        }
    }

    fn bad_operator_problem(
        &self,
        operator: &alder_parse::error::BadOperator,
        row: u32,
        column: u32,
    ) -> SyntaxProblem {
        use alder_parse::error::BadOperator;
        let (symbol, help) = match operator {
            BadOperator::Arrow => (
                "->",
                "`->` introduces an anonymous function body; match arms use `=>`",
            ),
            BadOperator::Bar => (
                "|",
                "use `||` for boolean OR; a single `|` separates alternatives in a match pattern",
            ),
            BadOperator::PlusPlus => (
                "++",
                "Alder has no `++` concatenation operator; use an array operation or a string template for the values you intend to combine",
            ),
            BadOperator::DoubleColon => (
                "::",
                "`::` separates path members, as in `Option::Some`; it is not a list constructor",
            ),
            BadOperator::DotDot => (
                "..",
                "`..` is used for record spreads and rest patterns, not as a range operator between expressions",
            ),
            BadOperator::PipeLeft => (
                "<|",
                "write a function call such as `f(value)`; Alder does not have Elm's reverse application operator",
            ),
            BadOperator::ComposeRight => (
                ">>",
                "write an explicit function such as `(x) -> g(f(x))` for composition; `>>` is not an Alder operator",
            ),
            BadOperator::ComposeLeft => (
                "<<",
                "write an explicit function such as `(x) -> f(g(x))` for composition; `<<` is not an Alder operator",
            ),
            BadOperator::Caret => (
                "^",
                "`^` introduces pinned values in patterns and queries; it is not a power operator",
            ),
        };
        expected_problem(
            format!("`{symbol}` is not a binary operator in Alder"),
            row,
            column,
            "unsupported operator",
            Some(help.to_owned()),
        )
        .covering(symbol.len() as u32)
    }

    fn for_problem(&self, error: &alder_parse::error::For<'_>) -> SyntaxProblem {
        use alder_parse::error::For;
        match error {
            For::Pattern(error, ..) => self.pattern_problem(error),
            For::In(row, column) => expected_problem(
                "I was expecting `in` after this loop's pattern",
                *row,
                *column,
                "expected `in`",
                Some("write `for pattern in values { ... }`".to_owned()),
            ),
            For::Iter(error, ..) => self.expression_problem(error),
            For::Body(error, ..) => self.block_problem(error),
        }
    }

    fn while_problem(&self, error: &alder_parse::error::While<'_>) -> SyntaxProblem {
        use alder_parse::error::While;
        match error {
            While::Condition(error, ..) => self.expression_problem(error),
            While::Body(error, ..) => self.block_problem(error),
        }
    }

    fn constructor_pattern_problem(&self, error: &alder_parse::error::PCtor<'_>) -> SyntaxProblem {
        use alder_parse::error::PCtor;
        match error {
            PCtor::Arg(error, ..) => self.pattern_problem(error),
            PCtor::Record(error, row, column) => self.record_pattern_problem(error).in_context(
                *row,
                *column,
                "this record pattern starts here",
            ),
            PCtor::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `)` after this constructor argument",
                "expected `,` or `)`",
                Some(
                    "separate constructor arguments with commas and close them with `)`".to_owned(),
                ),
            ),
        }
    }

    fn tuple_pattern_problem(&self, error: &alder_parse::error::PTuple<'_>) -> SyntaxProblem {
        use alder_parse::error::PTuple;
        match error {
            PTuple::Pattern(error, ..) => self.pattern_problem(error),
            PTuple::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `)` after this pattern",
                "expected `,` or `)`",
                Some(
                    "separate tuple patterns with commas and close the parentheses with `)`"
                        .to_owned(),
                ),
            ),
        }
    }

    fn array_pattern_problem(&self, error: &alder_parse::error::PArray<'_>) -> SyntaxProblem {
        use alder_parse::error::PArray;
        match error {
            PArray::Pattern(error, ..) => self.pattern_problem(error),
            PArray::RestNotLast(row, column) => expected_problem(
                "the rest pattern must come last in an array pattern",
                *row,
                *column,
                "nothing may follow the rest pattern",
                Some("write the fixed entries first, as in `[first, ..rest]`".to_owned()),
            ),
            PArray::RestName(row, column) => expected_problem(
                "I was expecting a binding name or the end of this rest pattern",
                *row,
                *column,
                "expected a non-reserved lower-case name, `,`, or `]`",
                Some(
                    "write `..rest` to bind the remaining entries, or `..` to ignore them"
                        .to_owned(),
                ),
            ),
            PArray::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `]` after this array pattern entry",
                "expected `,` or `]`",
                Some(
                    "separate array patterns with commas, including before a `..rest` pattern"
                        .to_owned(),
                ),
            ),
        }
    }

    fn record_pattern_problem(&self, error: &alder_parse::error::PRecord<'_>) -> SyntaxProblem {
        use alder_parse::error::PRecord;
        match error {
            PRecord::Field(row, column) => expected_problem(
                "I was expecting a field name or `..` in this record pattern",
                *row,
                *column,
                "expected a lower-case field name or `..`",
                Some(
                    "write `{ name }` to bind a field, or `{ name: pattern }` to match it"
                        .to_owned(),
                ),
            ),
            PRecord::Pattern(error, ..) => self.pattern_problem(error),
            PRecord::RestNotLast(row, column) => expected_problem(
                "`..` must be last and unnamed in a record pattern",
                *row,
                *column,
                "expected the end of the record pattern",
                Some("write `{ name, .. }` to match named fields and ignore the others".to_owned()),
            ),
            PRecord::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `}` after this record pattern field",
                "expected `,` or `}`",
                Some("separate record patterns with commas, including before `..`".to_owned()),
            ),
        }
    }

    fn lambda_problem(&self, error: &alder_parse::error::Lambda<'_>) -> SyntaxProblem {
        use alder_parse::error::Lambda;
        match error {
            Lambda::Params(error, ..) => self.params_problem(error),
            Lambda::Ret(error, ..) => self.type_problem(error),
            Lambda::Body(error, ..) | Lambda::AssignValue(error, ..) => {
                self.expression_problem(error)
            }
            Lambda::Block(error, ..) => self.block_problem(error),
            Lambda::AssignTarget(operator, row, column) => expected_problem(
                "the left side of this assignment is not assignable",
                *row,
                *column,
                "expected a variable, field, or index",
                matches!(operator, alder_source::AssignOp::Div)
                    .then(|| "if you meant inequality, use `!=` instead of `/=`".to_owned()),
            ),
        }
    }

    fn match_problem(&self, error: &alder_parse::error::Match<'_>) -> SyntaxProblem {
        use alder_parse::error::Match;
        match error {
            Match::Scrutinee(error, ..) => self.expression_problem(error),
            Match::Open(row, column) => expected_problem(
                "I was expecting `{` to start this match's arms",
                *row,
                *column,
                "expected `{`",
                Some(
                    "write `match value { pattern => expression }`; Alder does not use `of` here"
                        .to_owned(),
                ),
            ),
            Match::Arm(error, row, column) => {
                self.arm_problem(error)
                    .in_context(*row, *column, "this match arm starts here")
            }
            Match::End(end) => self.end_problem(
                end,
                "I was expecting another match arm or the end of this match",
                "expected `,`, a pattern, or `}`",
                Some(
                    "write each arm as `pattern => expression` and close the match with `}`"
                        .to_owned(),
                ),
            ),
        }
    }

    fn arm_problem(&self, error: &alder_parse::error::Arm<'_>) -> SyntaxProblem {
        use alder_parse::error::Arm;
        match error {
            Arm::Pattern(error, ..) => self.pattern_problem(error),
            Arm::Guard(error, ..) | Arm::Body(error, ..) => self.expression_problem(error),
            Arm::Block(error, ..) => self.block_problem(error),
            Arm::Arrow(row, column) => expected_problem(
                "I was expecting `=>` between this match pattern and its body",
                *row,
                *column,
                "expected `=>`",
                Some(
                    "match arms use `pattern => expression`; `->` is for anonymous functions"
                        .to_owned(),
                ),
            ),
        }
    }

    fn provide_problem(&self, error: &alder_parse::error::Provide<'_>) -> SyntaxProblem {
        use alder_parse::error::Provide;
        match error {
            Provide::Name(row, column) => expected_problem(
                "I was expecting a provider name after `provide`",
                *row,
                *column,
                "expected an upper-case provider path",
                Some("write `provide Db = connection { ... }`".to_owned()),
            ),
            Provide::Equals(row, column) => expected_problem(
                "I was expecting `=` after this provider name",
                *row,
                *column,
                "expected `=`",
                Some("write `provide Db = connection { ... }`".to_owned()),
            ),
            Provide::Value(error, ..) => self.expression_problem(error),
            Provide::Body(error, ..) => self.block_problem(error),
        }
    }

    fn index_problem(&self, error: &alder_parse::error::Index<'_>) -> SyntaxProblem {
        use alder_parse::error::Index;
        match error {
            Index::Expr(error, ..) => self.expression_problem(error),
            Index::End(end) => self.end_problem(
                end,
                "I was expecting `]` after this index",
                "expected `]`",
                Some("write an index like `values[index]`".to_owned()),
            ),
        }
    }

    fn tag_problem(&self, error: &alder_parse::error::Tag<'_>) -> SyntaxProblem {
        use alder_parse::error::Tag;
        match error {
            Tag::Name(row, column) => expected_problem(
                "I was expecting a lower-case tag name after `:`",
                *row,
                *column,
                "expected a tag name",
                Some("write a tagged value like `:missing` or `:failed(reason)`".to_owned()),
            ),
            Tag::Arg(error, ..) => self.expression_problem(error),
            Tag::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `)` after this tag argument",
                "expected `,` or `)`",
                Some("separate tag arguments with commas and close them with `)`".to_owned()),
            ),
        }
    }

    fn state_problem(&self, error: &alder_parse::error::State<'_>) -> SyntaxProblem {
        use alder_parse::error::State;
        match error {
            State::Open(row, column) => expected_problem(
                "I was expecting `(` after `state`",
                *row,
                *column,
                "expected `(`",
                Some("write `state(initialValue)`".to_owned()),
            ),
            State::Expr(error, ..) => self.expression_problem(error),
            State::End(end) => self.end_problem(
                end,
                "I was expecting `)` after this state's initial value",
                "expected `)`",
                Some("`state` takes one initial value, as in `state(0)`".to_owned()),
            ),
        }
    }

    fn array_problem(&self, error: &alder_parse::error::Array<'_>) -> SyntaxProblem {
        use alder_parse::error::Array;
        match error {
            Array::Expr(error, ..) => self.expression_problem(error),
            Array::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `]` after this array entry",
                "expected `,` or `]`",
                Some("separate array entries with commas and close the array with `]`".to_owned()),
            ),
        }
    }

    fn tuple_problem(&self, error: &alder_parse::error::Tuple<'_>) -> SyntaxProblem {
        use alder_parse::error::Tuple;
        match error {
            Tuple::Expr(error, ..) => self.expression_problem(error),
            Tuple::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `)` after this parenthesized expression",
                "expected `,` or `)`",
                Some(
                    "separate tuple entries with commas and close the parentheses with `)`"
                        .to_owned(),
                ),
            ),
        }
    }

    fn record_problem(&self, error: &alder_parse::error::Record<'_>) -> SyntaxProblem {
        use alder_parse::error::Record;
        match error {
            Record::Field(row, column) => expected_problem(
                "I was expecting a field name or a spread in this record",
                *row,
                *column,
                "expected a lower-case field name or `..`",
                Some("write a field like `name: value` or a spread like `..other`".to_owned()),
            ),
            Record::Spread(error, ..) | Record::Expr(error, ..) => self.expression_problem(error),
            Record::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `}` after this record field",
                "expected `,` or `}`",
                Some("separate record fields with commas and close the record with `}`".to_owned()),
            ),
            Record::EqualsNotColon(row, column) => expected_problem(
                "record fields use `:`, not `=`",
                *row,
                *column,
                "replace this `=` with `:`",
                Some("write a field like `{ name: value }`".to_owned()),
            ),
        }
    }

    fn if_problem(&self, error: &alder_parse::error::If<'_>) -> SyntaxProblem {
        use alder_parse::error::If;
        match error {
            If::Condition(error, ..) => self.expression_problem(error),
            If::Then(error, ..) | If::Else(error, ..) => self.block_problem(error),
            If::ThenKeyword(row, column) => expected_problem(
                "Alder's `if` branches use braces, not `then`",
                *row,
                *column,
                "remove `then` and put the branch inside `{ ... }`",
                Some("write `if condition { value } else { other }`".to_owned()),
            ),
            If::ElseBranchStart(row, column) => expected_problem(
                "I was expecting a block or another `if` after `else`",
                *row,
                *column,
                "expected `{` or `if`",
                Some("write `else { value }` or `else if condition { value }`".to_owned()),
            ),
        }
    }

    fn call_problem(&self, error: &alder_parse::error::Call<'_>) -> SyntaxProblem {
        use alder_parse::error::Call;
        match error {
            Call::Arg(error, ..) => self.expression_problem(error),
            Call::End(end) => self.end_problem(
                end,
                "I was expecting `,` or `)` after this call argument",
                "expected `,` or `)`",
                Some("separate arguments with commas and close the call with `)`".to_owned()),
            ),
        }
    }

    fn number_problem(
        &self,
        error: &alder_parse::error::Number,
        row: u32,
        column: u32,
    ) -> SyntaxProblem {
        use alder_parse::error::Number;
        let (message, label, help) = match error {
            Number::End => (
                "a number cannot run directly into a name",
                "separate the number and name",
                None,
            ),
            Number::Dot => (
                "this decimal point needs digits after it",
                "expected a decimal digit",
                None,
            ),
            Number::Exponent => (
                "this exponent needs at least one digit",
                "expected an exponent digit",
                None,
            ),
            Number::HexDigit => (
                "this hexadecimal number needs a hexadecimal digit",
                "expected `0`-`9` or `a`-`f`",
                None,
            ),
            Number::NoLeadingZero => (
                "decimal numbers cannot have leading zeros",
                "remove the leading zero",
                None,
            ),
            Number::BigIntFraction => (
                "BigInt literals cannot contain a fractional part",
                "fractional BigInt",
                Some("remove the decimal part or the `n` suffix".to_owned()),
            ),
        };
        expected_problem(message, row, column, label, help)
    }

    /// A missing expression needs its syntactic role. More specific nested
    /// failures keep their own explanation and innermost context.
    fn expression_in(&self, error: &alder_parse::error::Expr<'_>, purpose: &str) -> SyntaxProblem {
        match error {
            alder_parse::error::Expr::Start(row, column) => expected_problem(
                format!("I was expecting {purpose}"),
                *row,
                *column,
                "expected an expression",
                None,
            ),
            _ => self.expression_problem(error),
        }
    }

    fn markup_problem(&self, error: &alder_parse::error::Markup<'_>) -> SyntaxProblem {
        use alder_parse::error::Markup;
        match error {
            Markup::Name(row, column) => expected_problem(
                "I was expecting an element name after `<`",
                *row,
                *column,
                "expected an element name",
                Some("write an element like `<div>` or a fragment like `<>`".to_owned()),
            ),
            Markup::Attr(error, row, column) => self.markup_attr_problem(error).in_context(
                *row,
                *column,
                "this attribute starts here",
            ),
            Markup::TagEnd(end) => self.end_problem(
                end,
                "I was expecting an attribute or the end of this opening tag",
                "expected an attribute, `>`, or `/>`",
                None,
            ),
            Markup::Child(error, ..) => self.markup_child_problem(error),
            Markup::CloseName(row, column)
                if self
                    .at(*row, *column)
                    .starts_with(|ch: char| ch.is_ascii_alphabetic()) =>
            {
                expected_problem(
                    "this closing tag appears before the markup child block is complete",
                    *row,
                    *column,
                    "unexpected closing tag",
                    Some(
                        "close the child block with `}` before closing its parent element"
                            .to_owned(),
                    ),
                )
            }
            Markup::CloseName(row, column) => expected_problem(
                "I was expecting an element name in this closing tag",
                *row,
                *column,
                "expected a name after `</`",
                None,
            ),
            Markup::CloseMismatch {
                expected,
                found,
                row,
                col,
            } => {
                let closing = if expected.is_empty() {
                    "</>".to_owned()
                } else {
                    format!("</{expected}>")
                };
                expected_problem(
                    format!(
                        "this closing tag does not match: expected `{closing}`, found `</{found}>`"
                    ),
                    *row,
                    *col,
                    "mismatched closing tag",
                    Some(
                        "check the tag spelling and nesting; close the innermost element first"
                            .to_owned(),
                    ),
                )
                .covering(found.len() as u32)
            }
            Markup::CloseEnd(end) => self.end_problem(
                end,
                "I was expecting `>` after this closing tag's name",
                "expected `>`",
                None,
            ),
            Markup::Unclosed { name, row, col } => {
                let end = self.end_position();
                expected_problem(
                    format!("I was expecting `</{name}>` to close this element"),
                    end.line,
                    end.column,
                    "expected a closing tag",
                    None,
                )
                .in_context(*row, *col, "this element starts here")
            }
            Markup::FragmentUnclosed(row, column) => {
                let end = self.end_position();
                expected_problem(
                    "I was expecting `</>` to close this fragment",
                    end.line,
                    end.column,
                    "expected `</>`",
                    None,
                )
                .in_context(*row, *column, "this fragment starts here")
            }
        }
    }

    fn markup_attr_problem(&self, error: &alder_parse::error::Attr<'_>) -> SyntaxProblem {
        use alder_parse::error::Attr;
        match error {
            Attr::Value(row, column) => expected_problem(
                "I was expecting a value for this markup attribute",
                *row,
                *column,
                "expected a quoted string or `{expression}`",
                Some("write `class=\"card\"` or `class={name}`".to_owned()),
            ),
            Attr::String(error, row, column) => self.string_problem(error, *row, *column),
            Attr::Expr(error, ..) => self.expression_in(error, "a value for this markup attribute"),
            Attr::ExprEnd(end) => self.end_problem(
                end,
                "I was expecting `}` after this attribute expression",
                "expected `}`",
                None,
            ),
        }
    }

    fn markup_child_problem(&self, error: &alder_parse::error::Child<'_>) -> SyntaxProblem {
        use alder_parse::error::Child;
        match error {
            Child::HoleEmpty(row, column) => expected_problem("this markup interpolation is empty", *row, *column,
                "expected an expression before `}`", Some("put an expression inside `{ ... }`, or remove the empty interpolation".to_owned())),
            Child::Hole(error, ..) => self.expression_in(error, "an expression inside this markup interpolation"),
            Child::HoleEnd(end) => self.end_problem(end, "I was expecting `}` after this markup expression",  "expected `}`", None),
            Child::StrayBrace(row, column) => expected_problem("this `}` is not closing a markup expression", *row, *column,
                "unexpected `}`", Some("write `{\"}\"}` to display a literal closing brace".to_owned())),
            Child::Element(error, row, column) => {
                let label = if self.at(*row, *column).starts_with("</") {
                    "this closing tag starts here"
                } else {
                    "this element starts here"
                };
                self.markup_problem(error).in_context(*row, *column, label)
            }
            Child::If(error, row, column) => self.markup_if_problem(error).in_context(*row, *column, "this `@if` starts here"),
            Child::For(error, row, column) => self.markup_for_problem(error).in_context(*row, *column, "this `@for` starts here"),
            Child::Match(error, row, column) => self.markup_match_problem(error).in_context(*row, *column, "this `@match` starts here"),
            Child::UnknownDirective(row, column) => expected_problem("I do not recognize this markup directive", *row, *column,
                "unknown directive", Some("use `@if`, `@for`, or `@match`; `@else` follows `@if`, and `@empty` follows `@for`".to_owned())),
            Child::StrayElse(row, column) => expected_problem("this `@else` has no preceding `@if`", *row, *column,
                "unexpected `@else`", Some("place `@else { ... }` directly after an `@if` branch".to_owned())),
            Child::StrayEmpty(row, column) => expected_problem("this `@empty` has no preceding `@for`", *row, *column,
                "unexpected `@empty`", Some("place `@empty { ... }` directly after an `@for` body".to_owned())),
            Child::Stmt(error, ..) => self.statement_problem(error),
            Child::TooDeep(row, column) => expected_problem("this markup is nested too deeply", *row, *column,
                "nesting limit reached here", Some("split deeply nested markup into smaller components".to_owned())),
        }
    }

    fn markup_if_problem(&self, error: &alder_parse::error::DirIf<'_>) -> SyntaxProblem {
        use alder_parse::error::DirIf;
        match error {
            DirIf::Condition(error, ..) => self.expression_in(error, "a condition after `@if`"),
            DirIf::Body(error, ..) | DirIf::Else(error, ..) => self.markup_block_problem(error),
            DirIf::ElseBranchStart(row, column) => expected_problem(
                "I was expecting `if` or `{` after `@else`",
                *row,
                *column,
                "expected `if` or `{`",
                Some("write `@else { ... }` or `@else if condition { ... }`".to_owned()),
            ),
        }
    }

    fn markup_for_problem(&self, error: &alder_parse::error::DirFor<'_>) -> SyntaxProblem {
        use alder_parse::error::DirFor;
        match error {
            DirFor::Pattern(error, ..) => self.pattern_problem(error),
            DirFor::In(row, column) => expected_problem(
                "I was expecting `in` after this markup loop pattern",
                *row,
                *column,
                "expected `in`",
                Some("write `@for item in items { ... }`".to_owned()),
            ),
            DirFor::Iter(error, ..) => self.expression_in(error, "an iterable after `in`"),
            DirFor::Key(row, column) => expected_problem(
                "I was expecting `key` after `;` in this markup loop",
                *row,
                *column,
                "expected `key`",
                Some("write `@for item in items; key item.id { ... }`".to_owned()),
            ),
            DirFor::KeyExpr(error, ..) => self.expression_in(error, "a key expression after `key`"),
            DirFor::Body(error, ..) | DirFor::Empty(error, ..) => self.markup_block_problem(error),
        }
    }

    fn markup_match_problem(&self, error: &alder_parse::error::DirMatch<'_>) -> SyntaxProblem {
        use alder_parse::error::DirMatch;
        match error {
            DirMatch::Scrutinee(error, ..) => self.expression_in(error, "a value after `@match`"),
            DirMatch::Open(row, column) => expected_problem(
                "I was expecting `{` to start the markup match arms",
                *row,
                *column,
                "expected `{`",
                None,
            ),
            DirMatch::Pattern(error, ..) => self.pattern_problem(error),
            DirMatch::Guard(error, ..) => {
                self.expression_in(error, "a condition for this markup match guard")
            }
            DirMatch::Arrow(row, column) => expected_problem(
                "I was expecting `=>` after this markup match pattern or guard",
                *row,
                *column,
                "expected `=>`",
                None,
            ),
            DirMatch::Body(_, row, column) if self.at(*row, *column).starts_with("</") => {
                expected_problem(
                    "I was expecting a markup body after `=>`, not a closing tag",
                    *row,
                    *column,
                    "expected a markup body",
                    Some(
                        "give this arm an element, fragment, directive, or `{ ... }` child block"
                            .to_owned(),
                    ),
                )
            }
            DirMatch::Body(error, ..) => self.markup_child_problem(error),
            DirMatch::BareText(row, column) => expected_problem(
                "I was expecting a markup body after `=>`",
                *row,
                *column,
                "expected an element, fragment, directive, or child block",
                Some("wrap text in a child block, like `Pattern => { text }`".to_owned()),
            ),
            DirMatch::Block(error, ..) => self.markup_block_problem(error),
            DirMatch::End(end) => self.end_problem(
                end,
                "I was expecting another markup match arm or `}`",
                "expected `,`, a pattern, or `}`",
                None,
            ),
        }
    }

    fn markup_block_problem(&self, error: &alder_parse::error::ChildBlock<'_>) -> SyntaxProblem {
        use alder_parse::error::ChildBlock;
        match error {
            ChildBlock::Open(row, column) => expected_problem(
                "I was expecting `{` to start this markup child block",
                *row,
                *column,
                "expected `{`",
                None,
            ),
            ChildBlock::Item(error, ..) => self.markup_child_problem(error),
            ChildBlock::End(end) => self.end_problem(
                end,
                "I was expecting `}` to close this markup child block",
                "expected `}`",
                None,
            ),
        }
    }

    fn style_problem(&self, error: &alder_parse::error::Style<'_>) -> SyntaxProblem {
        use alder_parse::error::{Number, Style};
        match error {
            Style::Open(row, column) => expected_problem(
                "I was expecting `{` to start this style block",
                *row, *column, "expected `{`",
                Some("write `style { padding: 16px }`".to_owned()),
            ),
            Style::Key(row, column) => expected_problem(
                "I was expecting a style property, a quoted selector, or `}`",
                *row, *column, "expected a lower-case name, a string, or `}`",
                Some("write a property like `padding: 16px`, or a nested selector like `\":hover\": { ... }`".to_owned()),
            ),
            Style::KeyString(error, row, column) => self.string_problem(error, *row, *column),
            Style::Colon(row, column) => expected_problem(
                "I was expecting `:` after this style property or selector",
                *row, *column, "expected `:`", None,
            ),
            Style::Value(error, ..) => self.expression_in(error, "a value for this style property"),
            Style::Dimension(Number::End, row, column) if self.at(*row, *column).starts_with([' ', '\t']) => expected_problem(
                "a style unit must be adjacent to its number",
                *row, *column, "remove the space before the unit",
                Some("write `16px`, not `16 px`".to_owned()),
            ),
            Style::Dimension(error, row, column) => self.number_problem(error, *row, *column),
            Style::Nested(error, row, column) => self.style_problem(error)
                .in_context(*row, *column, "this nested style block starts here"),
            Style::End(end) => self.end_problem(end,
                "I was expecting another style property or the end of this style block",
                 "expected `,`, a property, a quoted selector, or `}`",
                Some("write each entry as `property: value`; use commas or new lines, not CSS semicolons".to_owned()),
            ),
            Style::TooDeep(row, column) => expected_problem(
                "this style block is nested too deeply",
                *row, *column, "nesting limit reached here",
                Some("split deeply nested styles into smaller blocks".to_owned()),
            ),
        }
    }

    fn query_problem(&self, error: &alder_parse::error::Query<'_>) -> SyntaxProblem {
        use alder_parse::error::Query;
        match error {
            Query::Open(row, column) => expected_problem(
                "I was expecting `{` after `query`",
                *row, *column, "expected `{`",
                Some("write `query { select * from users }`".to_owned()),
            ),
            Query::Verb(row, column) => expected_problem(
                "I was expecting a query operation",
                *row, *column, "expected `select`, `insert`, `update`, or `delete`",
                Some("a query contains one operation, such as `query { select * from users }`".to_owned()),
            ),
            Query::Select(error, row, column) => self.select_problem(error)
                .in_context(*row, *column, "this `select` starts here"),
            Query::Insert(error, row, column) => self.insert_problem(error)
                .in_context(*row, *column, "this `insert` starts here"),
            Query::Update(error, row, column) => self.update_problem(error)
                .in_context(*row, *column, "this `update` starts here"),
            Query::Delete(error, row, column) => self.delete_problem(error)
                .in_context(*row, *column, "this `delete` starts here"),
            Query::ClauseOrder(clause, row, column) => expected_problem(
                format!("the `{}` clause cannot appear here", clause.as_str()),
                *row, *column, "this clause is repeated, out of order, or not allowed for this operation",
                Some("select clauses follow `from`, joins, `where`, `groupBy`, `orderBy`, `limit`, `offset`; only joins may repeat. Update/delete allow one `where`; insert has no trailing clauses".to_owned()),
            ),
            Query::End(end) => self.end_problem(end,
                "I was expecting an allowed query clause or `}`",
                 "expected an allowed query clause or `}`",
                Some("close the query with `}` after its operation and any permitted clauses".to_owned()),
            ),
            Query::OperationEnd(end) => self.end_problem(end,
                "I was expecting `}` after this query operation",
                "expected `}`", None,
            ),
        }
    }

    fn select_problem(&self, error: &alder_parse::error::Select<'_>) -> SyntaxProblem {
        use alder_parse::error::Select;
        match error {
            Select::Projection(row, column) => expected_problem(
                "I was expecting selected expressions or `*` after `select`",
                *row,
                *column,
                "expected `{` or `*`",
                Some(
                    "write `select { name, email } from users` or `select * from users`".to_owned(),
                ),
            ),
            Select::ProjectionExpr(error, ..) => {
                self.expression_in(error, "an expression in this select projection")
            }
            Select::ProjectionEnd(end) => self.end_problem(
                end,
                "I was expecting `,` or `}` after this selected expression",
                "expected `,` or `}`",
                Some("separate selected expressions with commas inside `{ ... }`".to_owned()),
            ),
            Select::From(row, column) => expected_problem(
                "I was expecting `from` after the select projection",
                *row,
                *column,
                "expected `from`",
                Some("write `select * from users`".to_owned()),
            ),
            Select::Table(error, ..) => self.table_ref_problem(error),
            Select::Join(error, row, column) => {
                self.join_problem(error)
                    .in_context(*row, *column, "this join starts here")
            }
            Select::Where(error, ..) => self.expression_in(error, "a condition after `where`"),
            Select::GroupBy(error, ..) => {
                self.expression_in(error, "an expression in this `groupBy` clause")
            }
            Select::OrderBy(error, ..) => {
                self.expression_in(error, "an expression in this `orderBy` clause")
            }
            Select::Limit(error, ..) => self.expression_in(error, "a value after `limit`"),
            Select::Offset(error, ..) => self.expression_in(error, "a value after `offset`"),
        }
    }

    fn table_ref_problem(&self, error: &alder_parse::error::TableRef) -> SyntaxProblem {
        use alder_parse::error::TableRef;
        match error {
            TableRef::Name(row, column) => expected_problem(
                "I was expecting a table name in this query",
                *row,
                *column,
                "expected a non-reserved lower-case table name",
                None,
            ),
            TableRef::Alias(row, column) => expected_problem(
                "I was expecting a table alias after `as`",
                *row,
                *column,
                "expected a non-reserved lower-case alias",
                Some("write a table reference like `users as u`".to_owned()),
            ),
        }
    }

    fn join_problem(&self, error: &alder_parse::error::Join<'_>) -> SyntaxProblem {
        use alder_parse::error::Join;
        match error {
            Join::Keyword(row, column) => expected_problem(
                "I was expecting `join` after this join qualifier",
                *row,
                *column,
                "expected `join`",
                Some(
                    "write `left join posts on condition` or `inner join posts on condition`"
                        .to_owned(),
                ),
            ),
            Join::Table(error, ..) => self.table_ref_problem(error),
            Join::On(row, column) => expected_problem(
                "I was expecting `on` after this joined table",
                *row,
                *column,
                "expected `on`",
                Some(
                    "each join needs a condition, as in `join posts on users.id == posts.userId`"
                        .to_owned(),
                ),
            ),
            Join::Condition(error, ..) => self.expression_in(error, "a join condition after `on`"),
        }
    }

    fn insert_problem(&self, error: &alder_parse::error::Insert<'_>) -> SyntaxProblem {
        use alder_parse::error::Insert;
        match error {
            Insert::Into(row, column) => expected_problem(
                "I was expecting `into` after `insert`",
                *row,
                *column,
                "expected `into`",
                Some("write `insert into users values ^rows`".to_owned()),
            ),
            Insert::Table(row, column) => expected_problem(
                "I was expecting a table name after `into`",
                *row,
                *column,
                "expected a non-reserved lower-case table name",
                None,
            ),
            Insert::Values(row, column) => expected_problem(
                "I was expecting `values` after this insert's table name",
                *row,
                *column,
                "expected `values`",
                Some("write `insert into users values ^rows`".to_owned()),
            ),
            Insert::Pin(row, column) => expected_problem(
                "insert values must be a pinned Alder value",
                *row,
                *column,
                "expected `^` followed immediately by a value",
                Some("write `values ^rows` or `values ^{ name, email }`".to_owned()),
            ),
            Insert::Value(error, ..) => {
                self.expression_in(error, "the pinned value for this insert")
            }
        }
    }

    fn update_problem(&self, error: &alder_parse::error::Update<'_>) -> SyntaxProblem {
        use alder_parse::error::Update;
        match error {
            Update::Table(row, column) => expected_problem(
                "I was expecting a table name after `update`",
                *row,
                *column,
                "expected a non-reserved lower-case table name",
                None,
            ),
            Update::Set(row, column) => expected_problem(
                "I was expecting `set` after this update's table name",
                *row,
                *column,
                "expected `set`",
                Some("write `update users set { name: ^name }`".to_owned()),
            ),
            Update::RecordOpen(row, column) => expected_problem(
                "I was expecting a record after `set`",
                *row,
                *column,
                "expected `{`",
                Some(
                    "put the updated fields inside braces, as in `set { name: ^name }`".to_owned(),
                ),
            ),
            Update::Record(error, row, column) => self.record_problem(error).in_context(
                *row,
                *column,
                "this update record starts here",
            ),
            Update::Where(error, ..) => {
                self.expression_in(error, "a condition after this update's `where`")
            }
        }
    }

    fn delete_problem(&self, error: &alder_parse::error::Delete<'_>) -> SyntaxProblem {
        use alder_parse::error::Delete;
        match error {
            Delete::From(row, column) => expected_problem(
                "I was expecting `from` after `delete`",
                *row,
                *column,
                "expected `from`",
                Some("write `delete from users where condition`".to_owned()),
            ),
            Delete::Table(row, column) => expected_problem(
                "I was expecting a table name after `delete from`",
                *row,
                *column,
                "expected a non-reserved lower-case table name",
                None,
            ),
            Delete::Where(error, ..) => {
                self.expression_in(error, "a condition after this delete's `where`")
            }
        }
    }

    fn table_problem(&self, error: &alder_parse::error::Table<'_>) -> SyntaxProblem {
        use alder_parse::error::Table;
        match error {
            Table::Name(row, column) => expected_problem(
                "I was expecting a lower-case table name",
                *row, *column, "expected a name after `table`",
                Some("write a declaration like `table users { id: int() }`".to_owned()),
            ),
            Table::Open(row, column) => expected_problem(
                "I was expecting `{` to start this table's columns",
                *row, *column, "expected `{`", None,
            ),
            Table::Column(row, column) => expected_problem(
                "I was expecting a column name or the end of this table",
                *row, *column, "expected a lower-case column name or `}`",
                Some("write each column as `name: builder`, for example `id: int()`".to_owned()),
            ),
            Table::Colon(row, column) => expected_problem(
                "I was expecting `:` after this column name",
                *row, *column, "expected `:`",
                Some("write each column as `name: builder`, for example `id: int()`".to_owned()),
            ),
            Table::Builder(error, ..) | Table::ModifierArg(error, ..) => self.expression_problem(error),
            Table::ModifierArgEnd(end) => self.end_problem(end,
                "I was expecting `,` or `)` after this modifier argument",
                 "expected `,` or `)`",
                Some("separate modifier arguments with commas and close them with `)`".to_owned()),
            ),
            Table::End(end) => self.end_problem(end,
                "I was expecting another column or the end of this table",
                 "expected a lower-case column name or `}`",
                Some("write each column as `name: builder` and close the table with `}`; columns do not use commas or semicolons".to_owned()),
            ),
        }
    }

    fn schema_problem(&self, error: &alder_parse::error::Schema<'_>) -> SyntaxProblem {
        use alder_parse::error::Schema;
        match error {
            Schema::Name(row, column) => expected_problem(
                "I was expecting an upper-case schema name",
                *row, *column, "expected a name after `schema`",
                Some("write a declaration like `schema User { name: String }`".to_owned()),
            ),
            Schema::From(row, column) => expected_problem(
                "I was expecting a table name after `from`",
                *row, *column, "expected a lower-case table name",
                Some("write `schema User from users { ... }`".to_owned()),
            ),
            Schema::Open(row, column) => expected_problem(
                "I was expecting `{` to start this schema body",
                *row, *column, "expected `{`", None,
            ),
            Schema::Item(row, column) => expected_problem(
                "I was expecting a field, `pick`, or the end of this schema",
                *row, *column, "expected a lower-case field name, `pick`, or `}`",
                Some("write a field like `name: String`, or select fields with `pick name, email`".to_owned()),
            ),
            Schema::PickName(row, column) => expected_problem(
                "I was expecting a field name in this `pick` list",
                *row, *column, "expected a lower-case field name",
                Some("write `pick name, email` with commas between field names".to_owned()),
            ),
            Schema::Colon(row, column) => expected_problem(
                "I was expecting `:` after this schema field name",
                *row, *column, "expected `:`",
                Some("write a field like `name: String` or `name: min(1)`".to_owned()),
            ),
            Schema::Type(error, ..) => self.type_problem(error),
            Schema::Rule(row, column) => expected_problem(
                "I was expecting a schema rule name",
                *row, *column, "expected a lower-case rule name",
                Some("write a rule like `min(1)` and separate multiple rules with commas".to_owned()),
            ),
            Schema::RuleArg(error, ..) => self.expression_problem(error),
            Schema::RuleArgEnd(end) => self.end_problem(end,
                "I was expecting `,` or `)` after this rule argument",
                 "expected `,` or `)`",
                Some("separate rule arguments with commas and close them with `)`".to_owned()),
            ),
            Schema::End(end) => self.end_problem(end,
                "I was expecting a comma, another schema item, or `}`",
                 "expected `,`, a field, `pick`, or `}`",
                Some("rules and `pick` names need commas between them; each new field starts with `name:`".to_owned()),
            ),
        }
    }

    fn test_problem(&self, error: &alder_parse::error::Test<'_>) -> SyntaxProblem {
        use alder_parse::error::Test;
        match error {
            Test::Name(row, column) => expected_problem(
                "I was expecting a quoted test name after `test`",
                *row,
                *column,
                "expected a string",
                Some("write `test \"description\" { ... }`".to_owned()),
            ),
            Test::NameString(error, row, column) => self.string_problem(error, *row, *column),
            Test::Body(error, ..) => self.block_problem(error),
        }
    }

    fn tests_problem(&self, error: &alder_parse::error::Tests<'_>) -> SyntaxProblem {
        use alder_parse::error::Tests;
        match error {
            Tests::Open(row, column) => expected_problem(
                "I was expecting `{` after `tests`",
                *row, *column, "expected `{`",
                Some("write `tests { ... }` with a declaration on each line".to_owned()),
            ),
            Tests::Item(error, ..) => self.item_problem(error),
            Tests::SameLine(row, column) => expected_problem(
                "declarations inside a tests block must be separated by line breaks",
                *row, *column, "start this declaration on a new line", None,
            ),
            Tests::End(end) => self.end_problem(end,
                "I was expecting another declaration or the end of this tests block",
                 "expected a declaration or `}`",
                Some("put declarations such as `test`, `fn`, or `let` on separate lines and close the block with `}`".to_owned()),
            ),
        }
    }

    fn template_problem(&self, error: &alder_parse::error::Template<'_>) -> SyntaxProblem {
        use alder_parse::error::Template;
        match error {
            Template::Endless(row, column) => {
                let end = self.end_position();
                expected_problem(
                    "this template is missing its closing backtick",
                    end.line,
                    end.column,
                    "expected a closing backtick",
                    Some(
                        "close the template with a backtick; put a backslash before a literal backtick in its text"
                            .to_owned(),
                    ),
                )
                .in_context(*row, *column, "this template starts here")
            }
            Template::Escape(error, row, column) => self.escape_problem(error, *row, *column, true),
            Template::HoleEmpty(row, column) => expected_problem(
                "this template interpolation needs an expression",
                *row,
                *column,
                "expected an expression before `}`",
                Some(
                    "write `${value}` to insert a value, or remove `${}` if no value belongs here"
                        .to_owned(),
                ),
            ),
            Template::HoleExpr(error, ..) => self.expression_problem(error),
            Template::HoleEnd(end) => self.end_problem(
                end,
                "I was expecting `}` to close this template interpolation",
                "expected `}`",
                Some(
                    "each `${` interpolation must end with `}` before the template text continues"
                        .to_owned(),
                ),
            ),
        }
    }

    fn macro_problem(&self, error: &alder_parse::error::Macro) -> SyntaxProblem {
        use alder_parse::error::Macro;
        match error {
            Macro::Name(row, column) => expected_problem(
                "I was expecting a lower-case name after `macro`",
                *row, *column, "expected a macro name",
                Some("write a declaration like `macro identity(value) { value }`".to_owned()),
            ),
            Macro::ParamsOpen(row, column) => expected_problem(
                "I was expecting `(` after this macro's name",
                *row, *column, "expected `(`",
                Some("a macro needs a parameter list, even when it is empty: `macro now() { ... }`".to_owned()),
            ),
            Macro::Param(row, column) => expected_problem(
                "I was expecting a macro parameter name",
                *row, *column, "expected a lower-case name",
                Some("macro parameters are names separated by commas, as in `macro pair(left, right) { ... }`".to_owned()),
            ),
            Macro::ParamEnd(end) => self.end_problem(end,
                "I was expecting `,` or `)` after this macro parameter",
                 "expected `,` or `)`", None,
            ),
            Macro::Body(error, row, column) => self.raw_tokens_problem(error, *row, *column, true),
        }
    }

    fn raw_tokens_problem(
        &self,
        error: &alder_parse::error::RawTokens,
        row: u32,
        column: u32,
        body: bool,
    ) -> SyntaxProblem {
        use alder_parse::error::{RawTokens, StringError, Template};
        match error {
            RawTokens::Unbalanced {
                found,
                expected,
                opening,
            } => expected_problem(
                format!(
                    "I found `{}` where this macro's open delimiter needs `{}`",
                    char::from(*found),
                    char::from(*expected)
                ),
                row,
                column,
                "this closing delimiter does not match",
                Some(format!(
                    "close the highlighted opening delimiter with `{}`",
                    char::from(*expected)
                )),
            )
            .in_context(opening.line, opening.column, "this delimiter is still open"),
            RawTokens::Endless { expected, opening } => {
                let end = self.end_position();
                expected_problem(
                    format!(
                        "I was expecting `{}` to close this macro's open delimiter",
                        char::from(*expected)
                    ),
                    end.line,
                    end.column,
                    "missing closing delimiter",
                    Some(
                        "macro token text must balance parentheses, brackets, and braces"
                            .to_owned(),
                    ),
                )
                .in_context(
                    opening.line,
                    opening.column,
                    "this delimiter is still open",
                )
            }
            RawTokens::String(StringError::Endless) if self.at(row, column).starts_with('`') => {
                self.template_problem(&Template::Endless(row, column))
            }
            // RawTokens does not distinguish the mode of an escape failure.
            // Standard escape examples apply to both, but a preceding quote
            // may be ordinary template text rather than a string opener.
            RawTokens::String(StringError::Escape(error)) => {
                self.escape_problem(error, row, column, false)
            }
            RawTokens::String(error) => self.string_problem(error, row, column),
            RawTokens::Open => expected_problem(
                if body {
                    "I was expecting `{` to start this macro body"
                } else {
                    "I was expecting `(` to start this macro invocation"
                },
                row,
                column,
                if body { "expected `{`" } else { "expected `(`" },
                Some(
                    if body {
                        "write `macro name(args) { ... }`"
                    } else {
                        "write `name!(...)` with `!(` adjacent to the name"
                    }
                    .to_owned(),
                ),
            ),
        }
    }

    fn escape_problem(
        &self,
        error: &alder_parse::error::Escape,
        row: u32,
        column: u32,
        template: bool,
    ) -> SyntaxProblem {
        use alder_parse::error::Escape;
        match error {
            Escape::Unknown => {
                let width = self.at(row, column).chars().take(2).map(char::len_utf8).sum::<usize>() as u32;
                expected_problem(
                    "I do not recognize this escape sequence",
                    row, column, "unknown escape",
                    Some(if template {
                        "use an escape such as `\\n`, `\\t`, `\\\\`, or `\\u{0041}`; templates also allow a backslash before a backtick or `$`"
                    } else {
                        "use an escape such as `\\n`, `\\t`, `\\\"`, `\\\\`, or `\\u{0041}`; a literal backslash is written `\\\\`"
                    }.to_owned()),
                ).covering(width)
            }
            Escape::BadUnicodeFormat(width) => expected_problem(
                "this Unicode escape needs hexadecimal digits inside braces",
                row, column, "malformed Unicode escape",
                Some("write `\\u{0041}` or `\\u{1F600}`: braces are required, with four to six hexadecimal digits inside".to_owned()),
            ).covering(u32::from(*width)),
            Escape::BadUnicodeCode(width) => expected_problem(
                "this Unicode escape does not name a valid Unicode scalar value",
                row, column, "invalid Unicode scalar value",
                Some("use a value from `0000` to `10FFFF`, excluding surrogate values `D800` through `DFFF`".to_owned()),
            ).covering(u32::from(*width)),
            Escape::BadUnicodeLength { width, expected, actual } => expected_problem(
                format!("this Unicode escape has {actual} hexadecimal digits, but only four to six are allowed"),
                row, column, "expected four to six hexadecimal digits",
                Some(if actual < expected {
                    "pad the code with leading zeros, as in `\\u{0041}`"
                } else {
                    "remove excess leading zeros so the code has four to six digits"
                }.to_owned()),
            ).covering(u32::from(*width)),
        }
    }

    fn string_problem(
        &self,
        error: &alder_parse::error::StringError,
        row: u32,
        column: u32,
    ) -> SyntaxProblem {
        use alder_parse::error::StringError;
        let mut problem = match error {
            StringError::Endless => expected_problem(
                "this string is missing its closing quote",
                row,
                column,
                "expected a closing quote here",
                Some("add a closing `\"` before the end of the line".to_owned()),
            ),
            StringError::Newline => expected_problem(
                "ordinary strings cannot contain a line break",
                row,
                column,
                "line break occurs here",
                Some("use a template literal for multiline text".to_owned()),
            ),
            StringError::Escape(error) => self.escape_problem(error, row, column, false),
        };
        // A committed ordinary-string failure cannot have crossed an
        // unescaped closing quote. Its nearest preceding unescaped quote on
        // this line is therefore the opener, even after earlier valid strings.
        let line_start = self
            .source
            .span(Region::new(Position::new(row, 1), Position::new(row, 1)))
            .offset();
        let offset = self
            .source
            .span(Region::new(
                Position::new(row, column),
                Position::new(row, column),
            ))
            .offset();
        if let Some(prefix) = self.source.text().get(line_start..offset)
            && let Some((opening, _)) = prefix.rmatch_indices('"').find(|(position, _)| {
                prefix[..*position]
                    .bytes()
                    .rev()
                    .take_while(|byte| *byte == b'\\')
                    .count()
                    % 2
                    == 0
            })
        {
            problem = problem.in_context(row, opening as u32 + 1, "this string starts here");
        }
        problem
    }
}
