//! Name resolution and canonicalization for Alder source modules.
//!
//! The implementation follows the contract in `docs/canonical-internals.md`.

mod canonicalize;
pub mod environment;
mod error;
pub mod expression;
mod interface;
pub mod pattern;
mod scc;
pub mod types;
mod value_scc;
mod warning;

pub use canonicalize::{CanResult, Context, canonicalize, canonicalize_headers};
pub use error::{
    AttributeError, Error, ErrorKind, ExprError, ImportError, ItemError, NameError, PatternError,
    StmtError, TypeError,
};
pub use interface::{builtin_trait_interface, from_module, headers_from_module};
pub use warning::{Warning, WarningKind};

/// Type annotations produced for top-level values by the solver.
pub type Annotations<'a> =
    std::collections::BTreeMap<alder_ast::QualifiedName<'a>, &'a alder_ast::Annotation<'a>>;
