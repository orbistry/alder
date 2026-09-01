//! Port of the data half of Elm's `Type.Error`: the tree that type errors
//! describe types with, after unification variables have been resolved.
//!
//! The `toDoc`/`toComparison` rendering machinery is deferred along with the
//! rest of error reporting (alder stores error data only, like `alder-parse`
//! and `alder-can`).

use alder_ast::ModuleName;

/// Elm's `Type.Error.Type`. Bump-allocated; maps become name-sorted slices.
#[derive(Clone, Copy, Debug)]
pub enum ErrorType<'a> {
    Lambda(
        &'a ErrorType<'a>,
        &'a ErrorType<'a>,
        &'a [&'a ErrorType<'a>],
    ),
    Infinite,
    Error,
    FlexVar(&'a str),
    FlexSuper(Super, &'a str),
    RigidVar(&'a str),
    RigidSuper(Super, &'a str),
    Type {
        home: ModuleName<'a>,
        name: &'a str,
        args: &'a [&'a ErrorType<'a>],
    },
    Record {
        fields: &'a [(&'a str, &'a ErrorType<'a>)],
        ext: Extension<'a>,
    },
    Unit,
    Tuple(
        &'a ErrorType<'a>,
        &'a ErrorType<'a>,
        Option<&'a ErrorType<'a>>,
    ),
    Alias {
        home: ModuleName<'a>,
        name: &'a str,
        args: &'a [(&'a str, &'a ErrorType<'a>)],
        real: &'a ErrorType<'a>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Super {
    Number,
    Comparable,
    Appendable,
    CompAppend,
}

#[derive(Clone, Copy, Debug)]
pub enum Extension<'a> {
    Closed,
    FlexOpen(&'a str),
    RigidOpen(&'a str),
}

pub fn iterated_dealias<'a>(tipe: &'a ErrorType<'a>) -> &'a ErrorType<'a> {
    match tipe {
        ErrorType::Alias { real, .. } => iterated_dealias(real),
        _ => tipe,
    }
}

// IS TYPE?

pub fn is_int(home: ModuleName<'_>, name: &str) -> bool {
    home == crate::type_::basics() && name == "Int"
}

pub fn is_float(home: ModuleName<'_>, name: &str) -> bool {
    home == crate::type_::basics() && name == "Float"
}

pub fn is_string(home: ModuleName<'_>, name: &str) -> bool {
    home == crate::type_::string_home() && name == "String"
}

pub fn is_char(home: ModuleName<'_>, name: &str) -> bool {
    home == crate::type_::char_home() && name == "Char"
}

pub fn is_list(home: ModuleName<'_>, name: &str) -> bool {
    home == crate::type_::list_home() && name == "List"
}
