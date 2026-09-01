mod accumulate;
pub mod environment;
mod error;
pub mod expression;
mod interface;
mod module;
pub mod pattern;
mod scc;
pub mod types;
pub mod warning;

pub use crate::error::{BadArityContext, DuplicatePatternContext, Error, PossibleNames, VarKind};
pub use crate::interface::{
    AliasVisibility, Annotations, Interface, InterfaceAlias, InterfaceBinop, InterfaceUnion,
    InterfaceValue, UnionVisibility, deep_copy as deep_copy_interface, from_module,
};
pub use crate::module::{CanResult, Context, canonicalize};
pub use crate::warning::{Warning, WarningContext};
