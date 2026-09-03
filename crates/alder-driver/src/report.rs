use alder_ast::{ImplId, ImplOrigin, ItemKind, Module, ModuleId, PackageId};
use alder_can::{
    AttributeError, ErrorKind as CanErrorKind, ExprError, ImportError, ItemError, NameError,
    PatternError, StmtError, TypeError, WarningKind,
};
use alder_region::{Position, Region};
use alder_report::{Diagnostic, Source};
use alder_solve::{CoherenceError, SolveError, SolveTraitError};

pub fn source_failure(source: Source, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(source, message)
        .with_code("alder::driver::source")
        .with_primary_label(Region::one(), "module source")
}

pub fn parse(source: Source, error: &alder_parse::error::Module<'_>) -> Diagnostic {
    let problem = module_problem(error);
    let mut diagnostic = Diagnostic::error(source, problem.message)
        .with_code("alder::syntax")
        .with_primary_label(point(problem.row, problem.column), problem.label);
    if let Some(help) = problem.help {
        diagnostic = diagnostic.with_help(help);
    }
    diagnostic
}

struct SyntaxProblem {
    message: String,
    row: u32,
    column: u32,
    label: &'static str,
    help: Option<String>,
}

fn syntax_problem(message: impl Into<String>, row: u32, column: u32) -> SyntaxProblem {
    SyntaxProblem {
        message: message.into(),
        row,
        column,
        label: "I got stuck here",
        help: None,
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
        message: message.into(),
        row,
        column,
        label,
        help,
    }
}

fn module_problem(error: &alder_parse::error::Module<'_>) -> SyntaxProblem {
    use alder_parse::error::Module;
    match error {
        Module::Item(error, ..) => item_problem(error),
        Module::SameLine(row, column) => SyntaxProblem {
            message: "module items must be separated by a line break".to_owned(),
            row: *row,
            column: *column,
            label: "this item starts on the previous item's line",
            help: Some("start this item on a new line".to_owned()),
        },
        Module::BadEnd(row, column) => syntax_problem(
            "I found unexpected input after the last module item",
            *row,
            *column,
        ),
    }
}

fn item_problem(error: &alder_parse::error::Item<'_>) -> SyntaxProblem {
    use alder_parse::error::Item;
    match error {
        Item::Start(row, column) => syntax_problem(
            "I was expecting a module item such as `fn`, `let`, `enum`, or `trait`",
            *row,
            *column,
        ),
        Item::AfterPub(row, column) => {
            syntax_problem("I was expecting an item after `pub`", *row, *column)
        }
        Item::Semicolon(row, column) => SyntaxProblem {
            message: "module items are separated by line breaks, not semicolons".to_owned(),
            row: *row,
            column: *column,
            label: "remove this semicolon",
            help: None,
        },
        Item::Trait(error, ..) => trait_problem(error),
        Item::Impl(error, ..) => impl_problem(error),
        Item::Attribute(error, ..) => attribute_problem(error),
        Item::Import(error, ..) => import_problem(error),
        Item::Fn(error, ..) => function_problem(error, "function"),
        Item::Let(error, ..) => let_problem(error),
        Item::TypeAlias(error, ..) => type_alias_problem(error),
        Item::Enum(error, ..) => enum_problem(error),
        Item::ErrorDecl(error, ..) => error_decl_problem(error),
        Item::Component(error, ..) => component_problem(error),
        Item::Table(_, row, column) => syntax_problem("invalid table declaration", *row, *column),
        Item::Schema(_, row, column) => syntax_problem("invalid schema declaration", *row, *column),
        Item::Macro(_, row, column) => syntax_problem("invalid macro declaration", *row, *column),
        Item::Comptime(_, row, column) => syntax_problem("invalid comptime block", *row, *column),
        Item::Test(_, row, column) | Item::Tests(_, row, column) => {
            syntax_problem("invalid test declaration", *row, *column)
        }
    }
}

fn trait_problem(error: &alder_parse::error::Trait<'_>) -> SyntaxProblem {
    use alder_parse::error::Trait;
    match error {
        Trait::Name(row, column) => SyntaxProblem {
            message: "I was expecting a trait name after `trait`".to_owned(),
            row: *row,
            column: *column,
            label: "expected an upper-case name",
            help: Some("trait declarations start like `trait Show[a]`".to_owned()),
        },
        Trait::Params(error, ..) => type_params_problem(error, "trait"),
        Trait::Where(error, ..) => where_problem(error),
        Trait::Open(row, column) => SyntaxProblem {
            message: "I was expecting `{` to start this trait body".to_owned(),
            row: *row,
            column: *column,
            label: "expected `{` here",
            help: None,
        },
        Trait::Item(row, column) => SyntaxProblem {
            message: "I was expecting a trait method, associated type, or `}`".to_owned(),
            row: *row,
            column: *column,
            label: "expected `fn`, `type`, or `}`",
            help: None,
        },
        Trait::SameLine(row, column) => SyntaxProblem {
            message: "trait items must be separated by a line break".to_owned(),
            row: *row,
            column: *column,
            label: "this item needs its own line",
            help: None,
        },
        Trait::Semicolon(row, column) => SyntaxProblem {
            message: "trait items are separated by line breaks, not semicolons".to_owned(),
            row: *row,
            column: *column,
            label: "remove this semicolon",
            help: None,
        },
        Trait::AssocType(row, column) => SyntaxProblem {
            message: "I was expecting an associated type name after `type`".to_owned(),
            row: *row,
            column: *column,
            label: "expected an upper-case name",
            help: Some("declare an associated type like `type Item`".to_owned()),
        },
        Trait::AssocTypeHasBody(row, column) => SyntaxProblem {
            message: "associated types are declared without a value in traits".to_owned(),
            row: *row,
            column: *column,
            label: "remove this definition",
            help: Some("provide the associated type value in each `impl`".to_owned()),
        },
        Trait::Fn(error, ..) => function_problem(error, "trait method"),
    }
}

fn impl_problem(error: &alder_parse::error::Impl<'_>) -> SyntaxProblem {
    use alder_parse::error::Impl;
    match error {
        Impl::Trait(row, column) => SyntaxProblem {
            message: "I was expecting a trait name after `impl`".to_owned(),
            row: *row,
            column: *column,
            label: "expected an upper-case path",
            help: Some("implement a trait like `impl Show[User] { ... }`".to_owned()),
        },
        Impl::PathMember(row, column) => SyntaxProblem {
            message: "I was expecting a trait name after `::`".to_owned(),
            row: *row,
            column: *column,
            label: "expected a name here",
            help: None,
        },
        Impl::Open(row, column) => SyntaxProblem {
            message: "I was expecting `[` after the trait name".to_owned(),
            row: *row,
            column: *column,
            label: "expected `[` here",
            help: Some("the subject type goes in brackets, as in `impl Show[User]`".to_owned()),
        },
        Impl::Arg(error, ..) => type_problem(error),
        Impl::ArgEnd(row, column) => SyntaxProblem {
            message: "I was expecting `,` or `]` after this trait argument".to_owned(),
            row: *row,
            column: *column,
            label: "expected `,` or `]`",
            help: None,
        },
        Impl::Where(error, ..) => where_problem(error),
        Impl::BodyOpen(row, column) => SyntaxProblem {
            message: "I was expecting `{` to start this impl body".to_owned(),
            row: *row,
            column: *column,
            label: "expected `{` here",
            help: None,
        },
        Impl::Item(row, column) => SyntaxProblem {
            message: "I was expecting an impl method, associated type, or `}`".to_owned(),
            row: *row,
            column: *column,
            label: "expected `fn`, `type`, or `}`",
            help: None,
        },
        Impl::SameLine(row, column) => SyntaxProblem {
            message: "impl items must be separated by a line break".to_owned(),
            row: *row,
            column: *column,
            label: "this item needs its own line",
            help: None,
        },
        Impl::Semicolon(row, column) => SyntaxProblem {
            message: "impl items are separated by line breaks, not semicolons".to_owned(),
            row: *row,
            column: *column,
            label: "remove this semicolon",
            help: None,
        },
        Impl::AssocType(row, column) => SyntaxProblem {
            message: "I was expecting an associated type name after `type`".to_owned(),
            row: *row,
            column: *column,
            label: "expected an upper-case name",
            help: None,
        },
        Impl::AssocEquals(row, column) => SyntaxProblem {
            message: "I was expecting `=` after this associated type name".to_owned(),
            row: *row,
            column: *column,
            label: "expected `=` here",
            help: Some("define it like `type Item = Value`".to_owned()),
        },
        Impl::AssocBody(error, ..) => type_problem(error),
        Impl::Fn(error, ..) => function_problem(error, "implementation method"),
    }
}

fn type_params_problem(error: &alder_parse::error::TypeParams, owner: &str) -> SyntaxProblem {
    use alder_parse::error::TypeParams;
    match error {
        TypeParams::Open(row, column) => SyntaxProblem {
            message: format!("I was expecting `[` after this {owner} name"),
            row: *row,
            column: *column,
            label: "expected `[` here",
            help: Some(format!("{owner} parameters look like `[a]` or `[f, a]`")),
        },
        TypeParams::Var(row, column) => SyntaxProblem {
            message: format!("I was expecting a type parameter in this {owner} declaration"),
            row: *row,
            column: *column,
            label: "expected a lower-case name",
            help: None,
        },
        TypeParams::End(row, column) => SyntaxProblem {
            message: "I was expecting `,` or `]` after this type parameter".to_owned(),
            row: *row,
            column: *column,
            label: "expected `,` or `]`",
            help: None,
        },
        TypeParams::Empty(row, column) => SyntaxProblem {
            message: format!("this {owner} needs at least one type parameter"),
            row: *row,
            column: *column,
            label: "empty parameter list",
            help: None,
        },
    }
}

fn where_problem(error: &alder_parse::error::Where<'_>) -> SyntaxProblem {
    use alder_parse::error::Where;
    match error {
        Where::Var(row, column) => SyntaxProblem {
            message: "I was expecting a type variable after `where`".to_owned(),
            row: *row,
            column: *column,
            label: "expected a lower-case type variable",
            help: Some("a bound looks like `where a: Show`".to_owned()),
        },
        Where::Colon(row, column) => SyntaxProblem {
            message: "I was expecting a trait bound or associated type equality".to_owned(),
            row: *row,
            column: *column,
            label: "expected `:` or `.Assoc ==`",
            help: Some("write `a: Show` or `i.Item == Number`".to_owned()),
        },
        Where::Bound(row, column) => SyntaxProblem {
            message: "I was expecting a trait name after `:`".to_owned(),
            row: *row,
            column: *column,
            label: "expected an upper-case trait path",
            help: None,
        },
        Where::AssocName(row, column) => SyntaxProblem {
            message: "I was expecting an associated type name after `.`".to_owned(),
            row: *row,
            column: *column,
            label: "expected an upper-case name",
            help: None,
        },
        Where::AssocEq(row, column) => SyntaxProblem {
            message: "I was expecting `==` after this associated type".to_owned(),
            row: *row,
            column: *column,
            label: "expected `==` here",
            help: None,
        },
        Where::Type(error, ..) => type_problem(error),
    }
}

fn type_problem(error: &alder_parse::error::Type<'_>) -> SyntaxProblem {
    use alder_parse::error::Type;
    match error {
        Type::Start(row, column) => syntax_problem("I was expecting a type", *row, *column),
        Type::Reserved(_, row, column) => SyntaxProblem {
            message: "reserved words cannot be used as types".to_owned(),
            row: *row,
            column: *column,
            label: "this is a reserved word",
            help: None,
        },
        Type::PathMember(row, column) => {
            syntax_problem("I was expecting a type name after `::`", *row, *column)
        }
        Type::Args(error, ..) => type_args_problem(error),
        Type::Fn(error, ..) => function_type_problem(error),
        Type::Tuple(error, ..) => tuple_type_problem(error),
        Type::Record(error, ..) => record_type_problem(error),
        Type::ErrorRow(error, ..) => error_row_problem(error),
        Type::TooDeep(row, column) => SyntaxProblem {
            message: "this type is nested too deeply".to_owned(),
            row: *row,
            column: *column,
            label: "nesting limit reached here",
            help: Some("split this type into named aliases".to_owned()),
        },
    }
}

fn attribute_problem(error: &alder_parse::error::Attribute<'_>) -> SyntaxProblem {
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
        Attribute::Arg(error, ..) => expression_problem(error),
        Attribute::ArgEnd(row, column) => expected_problem(
            "I was expecting another attribute argument or the end of the argument list",
            *row,
            *column,
            "expected `,` or `)`",
            None,
        ),
        Attribute::End(row, column) => expected_problem(
            "this attribute is missing its closing bracket",
            *row,
            *column,
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

fn import_problem(error: &alder_parse::error::Import<'_>) -> SyntaxProblem {
    use alder_parse::error::Import;
    match error {
        Import::Path(error, ..) => module_path_problem(error),
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
        Import::NameAlias(row, column) | Import::Alias(row, column) => expected_problem(
            "I was expecting a lower-case alias after `as`",
            *row,
            *column,
            "expected an alias",
            None,
        ),
        Import::NamesEnd(row, column) => expected_problem(
            "I was expecting another imported name or the end of this list",
            *row,
            *column,
            "expected `,` or `}`",
            None,
        ),
        Import::PubNeedsNames(row, column) => expected_problem(
            "a public import must say which names it re-exports",
            *row,
            *column,
            "this imports only the module",
            Some("use `pub import path.{ Name }` or `pub import path.*`".to_owned()),
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
        ),
        Import::RootOnly(row, column) => expected_problem(
            "the local import root does not provide a module name",
            *row,
            *column,
            "expected a module path",
            Some("add a path, an alias, or a selected-name import".to_owned()),
        ),
    }
}

fn module_path_problem(error: &alder_parse::error::ModulePath) -> SyntaxProblem {
    use alder_parse::error::ModulePath;
    match error {
        ModulePath::Start(row, column) => expected_problem(
            "I was expecting an import path",
            *row,
            *column,
            "expected `@` or `~`",
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

fn function_problem(error: &alder_parse::error::Fn<'_>, owner: &str) -> SyntaxProblem {
    use alder_parse::error::Fn;
    match error {
        Fn::Name(row, column) => expected_problem(
            format!("I was expecting a name for this {owner}"),
            *row,
            *column,
            "expected a lower-case name",
            None,
        ),
        Fn::Params(error, ..) => params_problem(error),
        Fn::Ret(error, ..) => type_problem(error),
        Fn::Where(error, ..) => where_problem(error),
        Fn::Body(error, ..) => block_problem(error),
    }
}

fn params_problem(error: &alder_parse::error::Params<'_>) -> SyntaxProblem {
    use alder_parse::error::Params;
    match error {
        Params::Open(row, column) => expected_problem(
            "I was expecting `(` to start the parameter list",
            *row,
            *column,
            "expected `(`",
            None,
        ),
        Params::Pattern(error, ..) => pattern_problem(error),
        Params::Type(error, ..) => type_problem(error),
        Params::End(row, column) => expected_problem(
            "I was expecting another parameter or the end of the parameter list",
            *row,
            *column,
            "expected `,` or `)`",
            None,
        ),
    }
}

fn let_problem(error: &alder_parse::error::Let<'_>) -> SyntaxProblem {
    use alder_parse::error::Let;
    match error {
        Let::Pattern(error, ..) => pattern_problem(error),
        Let::Type(error, ..) => type_problem(error),
        Let::Equals(row, column) => expected_problem(
            "I was expecting `=` after this binding",
            *row,
            *column,
            "expected `=`",
            None,
        ),
        Let::Body(error, ..) => expression_problem(error),
    }
}

fn type_alias_problem(error: &alder_parse::error::TypeAlias<'_>) -> SyntaxProblem {
    use alder_parse::error::TypeAlias;
    match error {
        TypeAlias::Name(row, column) => expected_problem(
            "I was expecting a name after `type`",
            *row,
            *column,
            "expected an upper-case name",
            Some("a type alias starts like `type UserId = Number`".to_owned()),
        ),
        TypeAlias::Params(error, ..) => type_params_problem(error, "type alias"),
        TypeAlias::Body(error, ..) => type_problem(error),
    }
}

fn enum_problem(error: &alder_parse::error::Enum<'_>) -> SyntaxProblem {
    use alder_parse::error::Enum;
    match error {
        Enum::Name(row, column) => expected_problem(
            "I was expecting an enum name after `enum`",
            *row,
            *column,
            "expected an upper-case name",
            None,
        ),
        Enum::Params(error, ..) => type_params_problem(error, "enum"),
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
        Enum::VariantArg(error, ..) => type_problem(error),
        Enum::VariantArgEnd(row, column) => expected_problem(
            "I was expecting another payload type or the end of this variant",
            *row,
            *column,
            "expected `,` or `)`",
            None,
        ),
        Enum::VariantRecord(error, ..) => record_type_problem(error),
        Enum::VariantRecordExt(row, column) => expected_problem(
            "enum record payloads cannot extend another record",
            *row,
            *column,
            "remove this record extension",
            None,
        ),
        Enum::End(row, column) => expected_problem(
            "I was expecting another enum variant or the end of the enum",
            *row,
            *column,
            "expected `,` or `}`",
            None,
        ),
    }
}

fn error_decl_problem(error: &alder_parse::error::ErrorDecl<'_>) -> SyntaxProblem {
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
        ErrorDecl::Tag(error, ..) => tag_variant_problem(error),
        ErrorDecl::End(row, column) => expected_problem(
            "I was expecting another error tag or the end of this group",
            *row,
            *column,
            "expected `,` or `}`",
            None,
        ),
    }
}

fn tag_variant_problem(error: &alder_parse::error::TagVariant<'_>) -> SyntaxProblem {
    use alder_parse::error::TagVariant;
    match error {
        TagVariant::Name(row, column) => expected_problem(
            "I was expecting a lower-case tag name after `:`",
            *row,
            *column,
            "expected a tag name",
            None,
        ),
        TagVariant::Arg(error, ..) => type_problem(error),
        TagVariant::ArgEnd(row, column) => expected_problem(
            "I was expecting another tag payload type or `)`",
            *row,
            *column,
            "expected `,` or `)`",
            None,
        ),
    }
}

fn component_problem(error: &alder_parse::error::Component<'_>) -> SyntaxProblem {
    use alder_parse::error::Component;
    match error {
        Component::Name(row, column) => expected_problem(
            "I was expecting a component name",
            *row,
            *column,
            "expected an upper-case name",
            None,
        ),
        Component::Params(error, ..) => params_problem(error),
        Component::Body(error, ..) => block_problem(error),
    }
}

fn type_args_problem(error: &alder_parse::error::TArgs<'_>) -> SyntaxProblem {
    use alder_parse::error::TArgs;
    match error {
        TArgs::Type(error, ..) => type_problem(error),
        TArgs::Empty(row, column) => expected_problem(
            "type argument lists cannot be empty",
            *row,
            *column,
            "add a type argument or remove `[]`",
            None,
        ),
        TArgs::End(row, column) => expected_problem(
            "I was expecting another type argument or the end of this list",
            *row,
            *column,
            "expected `,` or `]`",
            None,
        ),
    }
}

fn function_type_problem(error: &alder_parse::error::TFn<'_>) -> SyntaxProblem {
    use alder_parse::error::TFn;
    match error {
        TFn::Open(row, column) => expected_problem(
            "I was expecting `(` after `fn` in this function type",
            *row,
            *column,
            "expected `(`",
            None,
        ),
        TFn::Param(error, ..) | TFn::Ret(error, ..) => type_problem(error),
        TFn::ParamEnd(row, column) => expected_problem(
            "I was expecting another parameter type or `)`",
            *row,
            *column,
            "expected `,` or `)`",
            None,
        ),
    }
}

fn tuple_type_problem(error: &alder_parse::error::TTuple<'_>) -> SyntaxProblem {
    use alder_parse::error::TTuple;
    match error {
        TTuple::Type(error, ..) => type_problem(error),
        TTuple::End(row, column) => expected_problem(
            "I was expecting another tuple type or `)`",
            *row,
            *column,
            "expected `,` or `)`",
            None,
        ),
    }
}

fn record_type_problem(error: &alder_parse::error::TRecord<'_>) -> SyntaxProblem {
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
            "I was expecting a type after this record field",
            *row,
            *column,
            "expected `:` or `?:`",
            None,
        ),
        TRecord::Type(error, ..) => type_problem(error),
        TRecord::ExtField(row, column) => expected_problem(
            "an extended record type needs at least one field",
            *row,
            *column,
            "expected a field after `|`",
            None,
        ),
        TRecord::End(row, column) => expected_problem(
            "I was expecting another record field or `}`",
            *row,
            *column,
            "expected `,` or `}`",
            None,
        ),
    }
}

fn error_row_problem(error: &alder_parse::error::TErrorRow<'_>) -> SyntaxProblem {
    use alder_parse::error::TErrorRow;
    match error {
        TErrorRow::Start(row, column) => expected_problem(
            "I was expecting an error tag, row variable, or `]`",
            *row,
            *column,
            "expected `:tag`, a row variable, or `]`",
            None,
        ),
        TErrorRow::Tag(error, ..) => tag_variant_problem(error),
        TErrorRow::Ext(row, column) => expected_problem(
            "I was expecting an error tag or row variable after `|`",
            *row,
            *column,
            "expected `:tag` or a row variable",
            None,
        ),
        TErrorRow::End(row, column) => expected_problem(
            "I was expecting another error tag or the end of this row",
            *row,
            *column,
            "expected `|` or `]`",
            None,
        ),
    }
}

fn block_problem(error: &alder_parse::error::Block<'_>) -> SyntaxProblem {
    use alder_parse::error::Block;
    match error {
        Block::Open(row, column) => expected_problem(
            "I was expecting `{` to start this block",
            *row,
            *column,
            "expected `{`",
            None,
        ),
        Block::Stmt(error, ..) => statement_problem(error),
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
        Block::End(row, column) => expected_problem(
            "I was expecting another statement or the end of this block",
            *row,
            *column,
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

fn statement_problem(error: &alder_parse::error::Stmt<'_>) -> SyntaxProblem {
    use alder_parse::error::Stmt;
    match error {
        Stmt::Let(error, ..) => let_problem(error),
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
        Stmt::For(_, row, column) => {
            syntax_problem("I could not finish parsing this `for` loop", *row, *column)
        }
        Stmt::While(_, row, column) => syntax_problem(
            "I could not finish parsing this `while` loop",
            *row,
            *column,
        ),
        Stmt::Return(error, ..)
        | Stmt::Break(error, ..)
        | Stmt::Assert(error, ..)
        | Stmt::Expr(error, ..)
        | Stmt::AssignValue(error, ..) => expression_problem(error),
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

fn pattern_problem(error: &alder_parse::error::Pattern<'_>) -> SyntaxProblem {
    use alder_parse::error::Pattern;
    match error {
        Pattern::Start(row, column) => syntax_problem("I was expecting a pattern", *row, *column),
        Pattern::Reserved(keyword, row, column) => expected_problem(
            format!(
                "`{}` is reserved and cannot be used as a pattern",
                keyword.as_str()
            ),
            *row,
            *column,
            "reserved word used here",
            None,
        ),
        Pattern::SqlKeyword(keyword, row, column) => expected_problem(
            format!("`{}` is a query keyword, not a pattern", keyword.as_str()),
            *row,
            *column,
            "query keyword used here",
            None,
        ),
        Pattern::Number(error, row, column) => number_problem(error, *row, *column),
        Pattern::String(error, row, column) => string_problem(error, *row, *column),
        Pattern::Pin(error, ..) => expression_problem(error),
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
        Pattern::Ctor(_, row, column) | Pattern::Tag(_, row, column) => {
            syntax_problem("invalid constructor pattern", *row, *column)
        }
        Pattern::TagName(row, column) => expected_problem(
            "I was expecting a lower-case tag name after `:`",
            *row,
            *column,
            "expected a tag name",
            None,
        ),
        Pattern::Tuple(_, row, column) => syntax_problem("invalid tuple pattern", *row, *column),
        Pattern::Array(_, row, column) => syntax_problem("invalid array pattern", *row, *column),
        Pattern::Record(_, row, column) => syntax_problem("invalid record pattern", *row, *column),
        Pattern::Alias(row, column) => expected_problem(
            "I was expecting a binding name after `as`",
            *row,
            *column,
            "expected a lower-case name",
            None,
        ),
        Pattern::WildcardNotVar(name, _, row, column) => expected_problem(
            format!("`{name}` looks like a wildcard, not a binding"),
            *row,
            *column,
            "only `_` is a wildcard",
            Some("remove the leading underscore to bind this name".to_owned()),
        ),
        Pattern::TooDeep(row, column) => expected_problem(
            "this pattern is nested too deeply",
            *row,
            *column,
            "nesting limit reached here",
            None,
        ),
    }
}

fn expression_problem(error: &alder_parse::error::Expr<'_>) -> SyntaxProblem {
    use alder_parse::error::Expr;
    match error {
        Expr::Start(row, column) => syntax_problem("I was expecting an expression", *row, *column),
        Expr::Reserved(keyword, row, column) => expected_problem(
            format!("`{}` cannot start an expression here", keyword.as_str()),
            *row,
            *column,
            "reserved word used here",
            None,
        ),
        Expr::SqlKeyword(keyword, row, column) => expected_problem(
            format!("`{}` is a query keyword, not a value", keyword.as_str()),
            *row,
            *column,
            "query keyword used as a value",
            None,
        ),
        Expr::Number(error, row, column) => number_problem(error, *row, *column),
        Expr::String(error, row, column) => string_problem(error, *row, *column),
        Expr::Template(_, row, column) | Expr::TaggedTemplate(_, row, column) => {
            syntax_problem("invalid template literal", *row, *column)
        }
        Expr::Array(_, row, column) => syntax_problem("invalid array expression", *row, *column),
        Expr::Tuple(_, row, column) => syntax_problem("invalid tuple expression", *row, *column),
        Expr::Record(_, row, column) | Expr::RecordCtor(_, row, column) => {
            syntax_problem("invalid record expression", *row, *column)
        }
        Expr::Block(error, ..) | Expr::Loop(error, ..) => block_problem(error),
        Expr::Lambda(_, row, column) => syntax_problem("invalid anonymous function", *row, *column),
        Expr::If(_, row, column) => syntax_problem("invalid `if` expression", *row, *column),
        Expr::Match(_, row, column) => syntax_problem("invalid `match` expression", *row, *column),
        Expr::Provide(_, row, column) => {
            syntax_problem("invalid `provide` expression", *row, *column)
        }
        Expr::Call(_, row, column) => syntax_problem("invalid function call", *row, *column),
        Expr::Index(_, row, column) => syntax_problem("invalid index expression", *row, *column),
        Expr::Tag(_, row, column) => syntax_problem("invalid tagged value", *row, *column),
        Expr::State(_, row, column) => syntax_problem("invalid state expression", *row, *column),
        Expr::Style(_, row, column) => syntax_problem("invalid style block", *row, *column),
        Expr::Query(_, row, column) => syntax_problem("invalid query", *row, *column),
        Expr::Markup(_, row, column) => syntax_problem("invalid markup", *row, *column),
        Expr::MacroCall(_, row, column) => syntax_problem("invalid macro call", *row, *column),
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
        Expr::OperatorReserved(_, row, column) => expected_problem(
            "this operator is not part of Alder",
            *row,
            *column,
            "unsupported operator",
            None,
        ),
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

fn number_problem(error: &alder_parse::error::Number, row: u32, column: u32) -> SyntaxProblem {
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

fn string_problem(error: &alder_parse::error::StringError, row: u32, column: u32) -> SyntaxProblem {
    use alder_parse::error::StringError;
    match error {
        StringError::Endless => expected_problem(
            "this string is missing its closing quote",
            row,
            column,
            "string starts here",
            Some("add a closing `\"` before the end of the line".to_owned()),
        ),
        StringError::Newline => expected_problem(
            "ordinary strings cannot contain a line break",
            row,
            column,
            "line break occurs here",
            Some("use a template literal for multiline text".to_owned()),
        ),
        StringError::Escape(_) => expected_problem(
            "this string contains an invalid escape sequence",
            row,
            column,
            "invalid escape",
            Some("use a standard escape such as `\\n`, `\\t`, `\\\"`, or `\\u{...}`".to_owned()),
        ),
    }
}

pub fn canonicalize(source: Source, error: &alder_can::Error<'_>) -> Diagnostic {
    let (code, message, help, secondary) = match &error.kind {
        CanErrorKind::Import(error) => import_error(error),
        CanErrorKind::Item(error) => item_error(error),
        CanErrorKind::Type(error) => type_error(error),
        CanErrorKind::Pattern(error) => pattern_error(error),
        CanErrorKind::Expr(error) => expression_error(error),
        CanErrorKind::Stmt(error) => statement_error(error),
        CanErrorKind::Attribute(error) => attribute_error(error),
    };
    let primary_label = if matches!(error.kind, CanErrorKind::Type(TypeError::OrphanImpl(_))) {
        "this package owns neither side of the implementation"
    } else {
        "reported here"
    };
    let mut diagnostic = Diagnostic::error(source, message)
        .with_code(format!("alder::canonicalize::{code}"))
        .with_primary_label(error.region, primary_label);
    if let Some((region, label)) = secondary {
        diagnostic = diagnostic.with_secondary_label(region, label);
    }
    if let Some(help) = help {
        diagnostic = diagnostic.with_help(help);
    }
    diagnostic
}

pub fn warning(source: Source, warning: &alder_can::Warning<'_>) -> Diagnostic {
    let (code, message) = match warning.kind {
        WarningKind::UnusedImport { name } => ("unused_import", format!("unused import `{name}`")),
        WarningKind::UnusedBinding { name } => {
            ("unused_binding", format!("unused binding `{name}`"))
        }
        WarningKind::UnusedTypeParameter { name } => (
            "unused_type_parameter",
            format!("unused type parameter `{name}`"),
        ),
    };
    Diagnostic::warning(source, message)
        .with_code(format!("alder::warning::{code}"))
        .with_primary_label(warning.region, "not used")
}

pub fn solve(source: Source, module: &Module<'_>, error: &SolveError<'_>) -> Diagnostic {
    match error {
        SolveError::Core(error) => constrain(source, error),
        SolveError::Trait(error) => trait_error(source, module, error),
        SolveError::Coherence(error) => coherence(source, module, error),
    }
}

pub fn codegen(source: Source, error: &alder_codegen::Error) -> Diagnostic {
    Diagnostic::error(source, error.message)
        .with_code("alder::codegen")
        .with_primary_label(error.region, "code generation failed here")
}

fn constrain(source: Source, error: &alder_constrain::Error) -> Diagnostic {
    use alder_constrain::ErrorKind;
    match &error.kind {
        ErrorKind::NonExhaustiveErrorMatch { missing, open } => {
            let label = if missing.is_empty() {
                "this open error row may contain more tags".to_owned()
            } else {
                format!("missing {}", missing.join(", "))
            };
            let help = if *open {
                "add `Err(_)` to handle every remaining error tag"
            } else {
                "add an arm for each missing Result case"
            };
            return Diagnostic::error(source, "this match does not cover every Result")
                .with_code("alder::type::non_exhaustive_error_match")
                .with_primary_label(error.region, label)
                .with_help(help);
        }
        ErrorKind::ImpossibleErrorPattern { tag } => {
            return Diagnostic::error(
                source,
                format!("`:{tag}` is not part of this closed error row"),
            )
            .with_code("alder::type::impossible_error_pattern")
            .with_primary_label(error.region, "this pattern can never match")
            .with_help("remove this arm or add the tag to the Result error type");
        }
        ErrorKind::InvalidErrorTagPlacement => {
            return Diagnostic::error(source, "error tags are only values inside `Err`")
                .with_code("alder::type::invalid_error_tag_placement")
                .with_primary_label(error.region, "this tag is used as an ordinary value")
                .with_help("construct a Result error with `Err(:tag(...))`");
        }
        _ => {}
    }
    let (code, message) = match &error.kind {
        ErrorKind::Mismatch { actual, expected } => (
            "type_mismatch",
            format!("type mismatch: expected `{expected}`, found `{actual}`"),
        ),
        ErrorKind::Arity { expected, actual } => (
            "arity",
            format!("wrong number of arguments: expected {expected}, found {actual}"),
        ),
        ErrorKind::MissingField { field } => {
            ("missing_field", format!("record has no field `{field}`"))
        }
        ErrorKind::AssocTypeMismatch {
            assoc,
            expected,
            actual,
        } => (
            "associated_type_mismatch",
            format!(
                "associated type `{assoc}` has conflicting equalities: expected `{expected}`, found `{actual}`"
            ),
        ),
        ErrorKind::InfiniteType => ("infinite_type", "infinite type".to_owned()),
        ErrorKind::UnsupportedHigherKindedUnification => (
            "higher_kinded_unification",
            "these higher-kinded types cannot be unified".to_owned(),
        ),
        ErrorKind::InvalidAwait => ("invalid_await", "`.await` requires a Task value".to_owned()),
        ErrorKind::InvalidTry => ("invalid_try", "`?` requires a Result value".to_owned()),
        ErrorKind::ReturnMismatch => (
            "return_mismatch",
            "return value does not match the function result".to_owned(),
        ),
        ErrorKind::NonExhaustiveErrorMatch { .. }
        | ErrorKind::ImpossibleErrorPattern { .. }
        | ErrorKind::InvalidErrorTagPlacement => unreachable!("handled above"),
    };
    Diagnostic::error(source, message)
        .with_code(format!("alder::type::{code}"))
        .with_primary_label(error.region, "type requirement originates here")
}

fn trait_error(source: Source, module: &Module<'_>, error: &SolveTraitError<'_>) -> Diagnostic {
    match error {
        SolveTraitError::MissingInstance {
            trait_,
            subject,
            origin,
            chain,
        } => Diagnostic::error(
            source,
            format!(
                "no implementation of `{}[{subject}]` was found",
                trait_.0.name
            ),
        )
        .with_code("alder::trait::missing_instance")
        .with_primary_label(*origin, "this use needs trait evidence")
        .with_help(with_obligation_chain(
            format!(
                "define an implementation of `{}[{subject}]`, or use a type that already has one",
                trait_.0.name
            ),
            chain,
        )),
        SolveTraitError::AmbiguousInstance {
            trait_,
            subject,
            origin,
            details,
        } => {
            let candidates = details.candidates;
            let mut diagnostic = Diagnostic::error(
                source,
                format!(
                    "multiple implementations of `{}[{subject}]` match ({} candidates)",
                    trait_.0.name,
                    candidates.len()
                ),
            )
            .with_code("alder::trait::ambiguous_instance")
            .with_primary_label(*origin, "the implementation cannot be selected here");
            for (index, candidate) in candidates.iter().enumerate() {
                if let Some(region) = local_impl_region(module, *candidate) {
                    diagnostic = diagnostic.with_secondary_label(
                        region,
                        format!("candidate implementation {}", index + 1),
                    );
                }
            }
            let candidates = candidates
                .iter()
                .enumerate()
                .map(|(index, candidate)| {
                    let availability = if local_impl_region(module, *candidate).is_none() {
                        " (source unavailable)"
                    } else {
                        ""
                    };
                    format!(
                        "  {}. {}{availability}",
                        index + 1,
                        impl_description(*candidate)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            diagnostic.with_help(with_obligation_chain(
                format!(
                    "add a type annotation that selects one implementation; candidates:\n{candidates}"
                ),
                details.chain,
            ))
        }
        SolveTraitError::UnsatisfiedBound {
            trait_,
            subject,
            origin,
            chain,
        } => Diagnostic::error(
            source,
            format!("the generic type `{subject}` requires `{}`", trait_.0.name),
        )
        .with_code("alder::trait::unsatisfied_bound")
        .with_primary_label(*origin, "this use requires a bound")
        .with_help(with_obligation_chain(
            format!(
                "add a matching bound, such as `where {subject}: {}`",
                trait_.0.name
            ),
            chain,
        )),
        SolveTraitError::AmbiguousTypeVariable {
            trait_,
            subject,
            origin,
            chain,
        } => Diagnostic::error(
            source,
            format!(
                "cannot determine which type must implement `{}[{subject}]`",
                trait_.0.name
            ),
        )
        .with_code("alder::trait::ambiguous_type_variable")
        .with_primary_label(*origin, "this trait use has no determining type")
        .with_help(with_obligation_chain(
            "add a type annotation that fixes the operand type".to_owned(),
            chain,
        )),
        SolveTraitError::InstanceCycle {
            trait_,
            subject,
            origin,
            chain,
        } => Diagnostic::error(
            source,
            format!(
                "resolving `{}[{subject}]` forms an instance cycle",
                trait_.0.name
            ),
        )
        .with_code("alder::trait::instance_cycle")
        .with_primary_label(*origin, "instance resolution returns to this requirement")
        .with_help(with_obligation_chain(
            "make the instance prerequisites structurally decrease".to_owned(),
            chain,
        )),
    }
}

fn with_obligation_chain(help: String, chain: &[alder_solve::ObligationFrame<'_>]) -> String {
    if chain.len() < 2 {
        return help;
    }
    let chain = chain
        .iter()
        .map(|frame| {
            let goal = format!("{}[{}]", frame.trait_.0.name, frame.subject);
            frame.required_by.map_or(goal.clone(), |implementation| {
                format!("{goal}, required by {}", impl_description(implementation))
            })
        })
        .collect::<Vec<_>>()
        .join("\n  -> ");
    format!("{help}\n\nobligation chain:\n  {chain}")
}

fn coherence(source: Source, module: &Module<'_>, error: &CoherenceError<'_>) -> Diagnostic {
    let (code, message, primary, primary_label, secondary, help) = match error {
        CoherenceError::SuperclassCycle { traits } => (
            "superclass_cycle",
            format!(
                "trait superclass cycle: {}",
                traits
                    .iter()
                    .map(|trait_| trait_.0.name)
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ),
            traits
                .last()
                .and_then(|trait_| local_trait_region(module, *trait_))
                .unwrap_or_else(Region::one),
            "this superclass closes the cycle",
            None,
            Some("remove one of the superclass constraints in this cycle".to_owned()),
        ),
        CoherenceError::OrphanImpl {
            implementation,
            trait_,
            subject,
            trait_package,
            type_package,
        } => (
            "orphan_impl",
            format!(
                "orphan implementation `{}[{subject}]`: this package defines neither the trait ({}) nor the subject type ({})",
                trait_.0.name,
                package_name(*trait_package),
                type_package
                    .map(package_name)
                    .unwrap_or("no owning package")
            ),
            impl_region(module, *implementation),
            "this package owns neither side of the implementation",
            None,
            Some(
                "define either the trait or subject type in this package, or move the impl to a package that does"
                    .to_owned(),
            ),
        ),
        CoherenceError::OverlappingImpl {
            first,
            second,
            trait_,
        } => (
            "overlapping_impl",
            format!(
                "overlapping implementations of `{}` are not allowed",
                trait_.0.name
            ),
            impl_region(module, *second),
            "this implementation overlaps the first",
            Some((impl_region(module, *first), "first implementation is here")),
            Some("remove one impl or introduce a distinct wrapper type; Alder has no specialization".to_owned()),
        ),
        CoherenceError::InvalidTermination {
            implementation,
            prerequisite,
        } => (
            "invalid_termination",
            format!(
                "instance prerequisite `{}` does not structurally decrease",
                prerequisite.0.name
            ),
            impl_region(module, *implementation),
            "this prerequisite does not get structurally smaller",
            None,
            Some("make every recursive instance prerequisite structurally smaller than its impl head".to_owned()),
        ),
        CoherenceError::KindMismatch {
            implementation,
            parameter,
            expected_arity,
            actual_arity,
        } => (
            "kind_mismatch",
            format!(
                "trait argument {} must be {}, but this is {}",
                parameter + 1,
                kind_description(*expected_arity),
                kind_description(*actual_arity)
            ),
            impl_region(module, *implementation),
            "this trait argument has the wrong kind",
            None,
            Some("use a type constructor with the arity required by the trait".to_owned()),
        ),
        CoherenceError::ProjectionCycle {
            implementation,
            chain,
        } => (
            "projection_cycle",
            format!(
                "associated type cycle: {} -> {}",
                chain
                    .iter()
                    .map(|assoc| assoc.name)
                    .collect::<Vec<_>>()
                    .join(" -> "),
                chain.first().map_or("associated type", |assoc| assoc.name)
            ),
            impl_region(module, *implementation),
            "these associated type definitions form a cycle",
            None,
            Some("make at least one associated type resolve to a non-cyclic type".to_owned()),
        ),
    };
    let mut diagnostic = Diagnostic::error(source, message)
        .with_code(format!("alder::trait::{code}"))
        .with_primary_label(primary, primary_label);
    if let Some((region, label)) = secondary {
        diagnostic = diagnostic.with_secondary_label(region, label);
    }
    if let Some(help) = help {
        diagnostic = diagnostic.with_help(help);
    }
    diagnostic
}

fn kind_description(arity: u16) -> String {
    match arity {
        0 => "a concrete type".to_owned(),
        1 => "a one-argument type constructor".to_owned(),
        arity => format!("a {arity}-argument type constructor"),
    }
}

type CanDetails = (
    &'static str,
    String,
    Option<String>,
    Option<(Region, &'static str)>,
);

fn name_error(error: &NameError<'_>) -> CanDetails {
    match error {
        NameError::Unknown {
            qualifier,
            name,
            suggestions,
            ..
        } => {
            let qualified = qualifier.map_or_else(|| (*name).to_owned(), |q| format!("{q}.{name}"));
            let help = (!suggestions.is_empty()).then(|| {
                format!(
                    "did you mean {}?",
                    suggestions
                        .iter()
                        .map(|s| format!("`{s}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            });
            (
                "unknown_name",
                format!("unknown name `{qualified}`"),
                help,
                None,
            )
        }
        NameError::Ambiguous {
            name, candidates, ..
        } => (
            "ambiguous_name",
            format!("`{name}` is ambiguous ({} candidates)", candidates.len()),
            Some("qualify the name with its module".to_owned()),
            None,
        ),
        NameError::Private { name, .. } => {
            ("private_name", format!("`{name}` is private"), None, None)
        }
    }
}

fn import_error(error: &ImportError<'_>) -> CanDetails {
    match error {
        ImportError::Name(error) => name_error(error),
        ImportError::NameNotFound {
            name, available, ..
        } => (
            "import_name_not_found",
            format!("the imported module does not export `{name}`"),
            (!available.is_empty()).then(|| format!("available names: {}", available.join(", "))),
            None,
        ),
        ImportError::AliasCollision { name, first } => (
            "import_alias_collision",
            format!("import name `{name}` is already bound"),
            None,
            Some((*first, "first binding is here")),
        ),
        ImportError::ReexportPrivate { name, .. } => (
            "reexport_private",
            format!("cannot re-export private name `{name}`"),
            None,
            None,
        ),
    }
}

fn item_error(error: &ItemError<'_>) -> CanDetails {
    match error {
        ItemError::DuplicateDefinition { name, first, .. } => (
            "duplicate_definition",
            format!("`{name}` is defined more than once"),
            None,
            Some((*first, "first definition is here")),
        ),
        ItemError::RecursiveValue { name, cycle } => (
            "recursive_value",
            format!(
                "value `{name}` is recursively defined through {}",
                cycle.join(" -> ")
            ),
            None,
            None,
        ),
        ItemError::RecursiveAlias { name, cycle } => (
            "recursive_alias",
            format!(
                "type alias `{name}` is recursive through {}",
                cycle.join(" -> ")
            ),
            None,
            None,
        ),
        ItemError::AnnotationTooShort {
            name,
            annotated,
            parameters,
        } => (
            "annotation_too_short",
            format!(
                "annotation for `{name}` covers {annotated} arguments but the function has {parameters}"
            ),
            None,
            None,
        ),
    }
}

fn type_error(error: &TypeError<'_>) -> CanDetails {
    match error {
        TypeError::Name(error) => name_error(error),
        TypeError::BadArity {
            name,
            expected,
            actual,
        } => (
            "type_arity",
            format!("type `{name}` expects {expected} arguments, found {actual}"),
            None,
            None,
        ),
        TypeError::DuplicateParameter { name, first } => duplicate("type parameter", name, *first),
        TypeError::DuplicateField { name, first } => duplicate("record field", name, *first),
        TypeError::DuplicateTag { name, first } => duplicate("tag", name, *first),
        TypeError::UnboundVariable { name } => (
            "unbound_type_variable",
            format!("unbound type variable `{name}`"),
            None,
            None,
        ),
        TypeError::UnusedParameter { name } => (
            "unused_type_parameter",
            format!("unused type parameter `{name}`"),
            None,
            None,
        ),
        TypeError::MissingAnnotation { name, position } => {
            let message = match *position {
                "parameter" => format!("every parameter of `{name}` needs a type annotation"),
                "return type" => format!("`{name}` needs a return type annotation"),
                position => format!("`{name}` needs a type annotation for its {position}"),
            };
            ("missing_annotation", message, None, None)
        }
        TypeError::UnknownImplItem {
            trait_name,
            name,
            item_kind,
        } => (
            "unknown_impl_item",
            format!("{item_kind} `{name}` is not a member of trait `{trait_name}`"),
            None,
            None,
        ),
        TypeError::MissingImplItem {
            trait_name,
            name,
            item_kind,
        } => (
            "missing_impl_item",
            format!("impl of `{trait_name}` is missing {item_kind} `{name}`"),
            None,
            None,
        ),
        TypeError::UnknownAssocType { name } => (
            "unknown_associated_type",
            format!("unknown associated type `{name}`"),
            None,
            None,
        ),
        TypeError::AmbiguousAssocType { name, traits } => (
            "ambiguous_associated_type",
            format!(
                "associated type `{name}` is declared by {} traits",
                traits.len()
            ),
            Some("qualify the associated type through its trait".to_owned()),
            None,
        ),
        TypeError::OrphanImpl(details) => (
            "orphan_impl",
            format!(
                "orphan implementation of `{trait_name}[{subject}]`: this package owns neither `{trait_name}` ({}) nor `{subject}` ({})",
                package_name(details.trait_package),
                details.type_package
                    .map(package_name)
                    .unwrap_or("no owning package"),
                trait_name = details.trait_name,
                subject = details.subject,
            ),
            Some(
                "define either the trait or subject type in this package, or move the impl to a package that does"
                    .to_owned(),
            ),
            None,
        ),
        TypeError::InvalidHole => (
            "invalid_type_hole",
            "`_` is only allowed in a partially-applied impl head".to_owned(),
            None,
            None,
        ),
    }
}

fn pattern_error(error: &PatternError<'_>) -> CanDetails {
    match error {
        PatternError::Name(error) => name_error(error),
        PatternError::DuplicateBinding { name, first } => {
            duplicate("pattern binding", name, *first)
        }
        PatternError::ConstructorArity {
            name,
            expected,
            actual,
        } => (
            "constructor_arity",
            format!(
                "constructor `{}::{}` expects {expected} values, found {actual}",
                name.enum_name, name.variant
            ),
            None,
            None,
        ),
        PatternError::ConstructorPayload {
            name,
            expected,
            actual,
        } => (
            "constructor_payload",
            format!(
                "constructor `{}::{}` has a {expected} payload, not {actual}",
                name.enum_name, name.variant
            ),
            None,
            None,
        ),
        PatternError::DuplicateField { name, first } => duplicate("pattern field", name, *first),
        PatternError::PinOutsideMatch => (
            "pin_outside_match",
            "pin patterns are only allowed in `match`".to_owned(),
            None,
            None,
        ),
    }
}

fn expression_error(error: &ExprError<'_>) -> CanDetails {
    match error {
        ExprError::Name(error) => name_error(error),
        ExprError::UnqualifiedConstructor { enum_name, variant } => (
            "unqualified_constructor",
            format!("constructor `{variant}` must be written `{enum_name}::{variant}`"),
            None,
            None,
        ),
        ExprError::PlaceholderOutsideCall => (
            "placeholder_outside_call",
            "`_` placeholders are only allowed in call arguments".to_owned(),
            None,
            None,
        ),
        ExprError::PinOutsideQuery => (
            "pin_outside_query",
            "expression pins are only allowed in queries".to_owned(),
            None,
            None,
        ),
        ExprError::AwaitRequiresTaskReturn => (
            "await_requires_task",
            "a function using `.await` must return `Task`".to_owned(),
            None,
            None,
        ),
        ExprError::MacroUnavailable { name } => (
            "macro_unavailable",
            format!("macro `{name}` is not available yet"),
            None,
            None,
        ),
        ExprError::DuplicateField { name, first } => duplicate("record field", name, *first),
        ExprError::NonAssociativeOperators { left, right } => (
            "non_associative_operators",
            format!("operators `{left}` and `{right}` cannot be chained without parentheses"),
            None,
            None,
        ),
    }
}

fn statement_error(error: &StmtError<'_>) -> CanDetails {
    match error {
        StmtError::Name(error) => name_error(error),
        StmtError::ImmutableAssignment { name, binding } => (
            "immutable_assignment",
            format!("cannot assign to immutable binding `{name}`"),
            Some("declare the binding with `let mut`".to_owned()),
            Some((*binding, "binding declared here")),
        ),
        StmtError::InvalidAssignmentTarget => (
            "invalid_assignment_target",
            "invalid assignment target".to_owned(),
            None,
            None,
        ),
        StmtError::BreakOutsideLoop => (
            "break_outside_loop",
            "`break` is only allowed inside a loop".to_owned(),
            None,
            None,
        ),
        StmtError::ContinueOutsideLoop => (
            "continue_outside_loop",
            "`continue` is only allowed inside a loop".to_owned(),
            None,
            None,
        ),
        StmtError::ReturnOutsideFunction => (
            "return_outside_function",
            "`return` is only allowed inside a function".to_owned(),
            None,
            None,
        ),
    }
}

fn attribute_error(error: &AttributeError<'_>) -> CanDetails {
    match error {
        AttributeError::InvalidExtern { reason } => (
            "invalid_extern",
            format!("invalid extern attribute: {reason}"),
            None,
            None,
        ),
        AttributeError::InvalidDerive { reason } => (
            "invalid_derive",
            format!("invalid derive attribute: {reason}"),
            None,
            None,
        ),
        AttributeError::Unknown { name } => (
            "unknown_attribute",
            format!("unknown attribute `{name}`"),
            None,
            None,
        ),
        AttributeError::MacroUnavailable => (
            "attribute_macro_unavailable",
            "attribute macros are not available yet".to_owned(),
            None,
            None,
        ),
    }
}

fn duplicate(kind: &'static str, name: &str, first: Region) -> CanDetails {
    (
        "duplicate",
        format!("duplicate {kind} `{name}`"),
        None,
        Some((first, "first declared here")),
    )
}

fn point(row: u32, column: u32) -> Region {
    Region::new(
        Position::new(row, column),
        Position::new(row, column.saturating_add(1)),
    )
}

fn impl_region(module: &Module<'_>, implementation: ImplId<'_>) -> Region {
    local_impl_region(module, implementation).unwrap_or_else(Region::one)
}

fn local_trait_region(module: &Module<'_>, trait_: alder_ast::TraitId<'_>) -> Option<Region> {
    (trait_.0.module == module.id).then_some(())?;
    module.items.iter().find_map(|item| match item.value.kind {
        ItemKind::Trait(declaration) if declaration.id == trait_ => Some(item.region),
        _ => None,
    })
}

fn local_impl_region(module: &Module<'_>, implementation: ImplId<'_>) -> Option<Region> {
    if implementation.module != module.id {
        return None;
    }
    let ordinal = match implementation.origin {
        ImplOrigin::Source { item_ordinal } => item_ordinal,
        ImplOrigin::Derived { type_ordinal, .. } | ImplOrigin::AutomaticEq { type_ordinal } => {
            type_ordinal
        }
        ImplOrigin::Builtin { .. } => return None,
    };
    module
        .items
        .get(ordinal as usize)
        .map(|item| match item.value.kind {
            ItemKind::Impl(_) | ItemKind::Enum(_) | ItemKind::ErrorGroup(_) => item.region,
            _ => item.region,
        })
}

fn impl_description(implementation: ImplId<'_>) -> String {
    let module = module_name(implementation.module);
    if implementation.module.package == PackageId::Builtin {
        return "a standard-library implementation".to_owned();
    }
    match implementation.origin {
        ImplOrigin::Source { .. } => format!("a source implementation in `{module}`"),
        ImplOrigin::Derived { .. } => format!("a derived implementation in `{module}`"),
        ImplOrigin::AutomaticEq { .. } => {
            format!("an automatic Eq implementation in `{module}`")
        }
        ImplOrigin::Builtin { .. } => "a standard-library implementation".to_owned(),
    }
}

fn package_name(package: PackageId<'_>) -> &'static str {
    match package {
        PackageId::Named(_) => "a dependency package",
        PackageId::Application => "the application",
        PackageId::ApplicationMember(_) => "an application workspace member",
        PackageId::Builtin => "the standard library",
    }
}

fn module_name(module: ModuleId<'_>) -> String {
    let path = module.path.join("/");
    match module.package {
        PackageId::Named(package) if path.is_empty() => {
            format!("{}/{}", package.author, package.project)
        }
        PackageId::Named(package) => format!("{}/{}/{path}", package.author, package.project),
        PackageId::Application if path.is_empty() => "the application root".to_owned(),
        PackageId::Application => path,
        PackageId::ApplicationMember(member) if path.is_empty() => member.to_owned(),
        PackageId::ApplicationMember(member) => format!("{member}/{path}"),
        PackageId::Builtin if path.is_empty() => "the standard library".to_owned(),
        PackageId::Builtin => format!("standard library/{path}"),
    }
}
