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
        Item::Attribute(_, row, column) => syntax_problem("invalid attribute", *row, *column),
        Item::Import(_, row, column) => syntax_problem("invalid import declaration", *row, *column),
        Item::Fn(_, row, column) => syntax_problem("invalid function declaration", *row, *column),
        Item::Let(_, row, column) => syntax_problem("invalid top-level binding", *row, *column),
        Item::TypeAlias(_, row, column) => syntax_problem("invalid type alias", *row, *column),
        Item::Enum(_, row, column) => syntax_problem("invalid enum declaration", *row, *column),
        Item::ErrorDecl(_, row, column) => {
            syntax_problem("invalid error declaration", *row, *column)
        }
        Item::Component(_, row, column) => {
            syntax_problem("invalid component declaration", *row, *column)
        }
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
        Trait::Fn(_, row, column) => {
            syntax_problem("invalid trait method declaration", *row, *column)
        }
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
        Impl::Fn(_, row, column) => syntax_problem("invalid impl method", *row, *column),
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
        Type::Args(_, row, column) => syntax_problem("invalid type argument list", *row, *column),
        Type::Fn(_, row, column) => syntax_problem("invalid function type", *row, *column),
        Type::Tuple(_, row, column) => syntax_problem("invalid tuple type", *row, *column),
        Type::Record(_, row, column) => syntax_problem("invalid record type", *row, *column),
        Type::ErrorRow(_, row, column) => syntax_problem("invalid error-row type", *row, *column),
        Type::TooDeep(row, column) => SyntaxProblem {
            message: "this type is nested too deeply".to_owned(),
            row: *row,
            column: *column,
            label: "nesting limit reached here",
            help: Some("split this type into named aliases".to_owned()),
        },
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
    let mut diagnostic = Diagnostic::error(source, message)
        .with_code(format!("alder::canonicalize::{code}"))
        .with_primary_label(error.region, "reported here");
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
        SolveError::Trait(error) => trait_error(source, error),
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
    };
    Diagnostic::error(source, message)
        .with_code(format!("alder::type::{code}"))
        .with_primary_label(error.region, "type requirement originates here")
}

fn trait_error(source: Source, error: &SolveTraitError<'_>) -> Diagnostic {
    let (code, message, region, help) = match error {
        SolveTraitError::MissingInstance {
            trait_,
            subject,
            origin,
        } => (
            "missing_instance",
            format!(
                "no implementation of `{}[{subject}]` was found",
                trait_.0.name
            ),
            *origin,
            None,
        ),
        SolveTraitError::AmbiguousInstance {
            trait_,
            subject,
            origin,
            candidates,
        } => (
            "ambiguous_instance",
            format!(
                "multiple implementations of `{}[{subject}]` match ({} candidates)",
                trait_.0.name,
                candidates.len()
            ),
            *origin,
            Some("add a type annotation that selects one implementation".to_owned()),
        ),
        SolveTraitError::UnsatisfiedBound {
            trait_,
            subject,
            origin,
        } => (
            "unsatisfied_bound",
            format!("the generic type `{subject}` requires `{}`", trait_.0.name),
            *origin,
            Some(format!(
                "add a matching bound, such as `where {subject}: {}`",
                trait_.0.name
            )),
        ),
        SolveTraitError::InstanceCycle {
            trait_,
            subject,
            origin,
        } => (
            "instance_cycle",
            format!(
                "resolving `{}[{subject}]` forms an instance cycle",
                trait_.0.name
            ),
            *origin,
            None,
        ),
    };
    let mut diagnostic = Diagnostic::error(source, message)
        .with_code(format!("alder::trait::{code}"))
        .with_primary_label(region, "trait evidence is required here");
    if let Some(help) = help {
        diagnostic = diagnostic.with_help(help);
    }
    diagnostic
}

fn coherence(source: Source, module: &Module<'_>, error: &CoherenceError<'_>) -> Diagnostic {
    let (code, message, primary, secondary) = match error {
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
            Region::one(),
            None,
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
            None,
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
            Some((impl_region(module, *first), "first implementation is here")),
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
            None,
        ),
        CoherenceError::KindMismatch {
            implementation,
            parameter,
            expected_arity,
            actual_arity,
        } => (
            "kind_mismatch",
            format!(
                "trait argument {} has kind arity {actual_arity}, but arity {expected_arity} is required",
                parameter + 1
            ),
            impl_region(module, *implementation),
            None,
        ),
        CoherenceError::ProjectionCycle {
            implementation,
            assoc,
        } => (
            "projection_cycle",
            format!(
                "associated type `{}` is defined in terms of itself",
                assoc.name
            ),
            impl_region(module, *implementation),
            None,
        ),
    };
    let mut diagnostic = Diagnostic::error(source, message)
        .with_code(format!("alder::trait::{code}"))
        .with_primary_label(primary, "invalid implementation");
    if let Some((region, label)) = secondary {
        diagnostic = diagnostic.with_secondary_label(region, label);
    }
    diagnostic
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
        TypeError::MissingAnnotation { name, position } => (
            "missing_annotation",
            format!("`{name}` needs a type annotation in {position}"),
            None,
            None,
        ),
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
    if implementation.module != module.id {
        return Region::one();
    }
    let ordinal = match implementation.origin {
        ImplOrigin::Source { item_ordinal } => item_ordinal,
        ImplOrigin::Derived { type_ordinal, .. } | ImplOrigin::AutomaticEq { type_ordinal } => {
            type_ordinal
        }
        ImplOrigin::Builtin { .. } => return Region::one(),
    };
    module
        .items
        .get(ordinal as usize)
        .map_or(Region::one(), |item| match item.value.kind {
            ItemKind::Impl(_) | ItemKind::Enum(_) | ItemKind::ErrorGroup(_) => item.region,
            _ => item.region,
        })
}

fn package_name(package: PackageId<'_>) -> &'static str {
    match package {
        PackageId::Named(_) => "a dependency package",
        PackageId::Application => "the application",
        PackageId::ApplicationMember(_) => "an application workspace member",
        PackageId::Builtin => "the standard library",
    }
}

#[allow(dead_code)]
fn module_name(module: ModuleId<'_>) -> String {
    module.path.join("/")
}
