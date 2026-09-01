//! Constraint generation for alder type inference: a port of Elm's
//! `Type.Constrain.*` plus the shared vocabulary from `Type.Type`,
//! `Type.UnionFind`, `Type.Error`, and `Reporting.Error.Type`.
//!
//! Where Elm creates unification variables in ambient `IO`, alder threads an
//! explicit [`UnionFind`] store: `constrain` fills it with fresh variables
//! and `alder-solve` mutates it while solving the returned [`Constraint`].

pub mod error;
pub mod error_type;
pub mod instantiate;
pub mod pattern;
pub mod type_;

mod expression;
mod module;
mod union_find;

pub use crate::error::{
    Category, Context, Error, Expected, MaybeName, PCategory, PContext, PExpected, SubContext,
};
pub use crate::error_type::{ErrorType, Extension, Super};
pub use crate::module::constrain;
pub use crate::type_::{Constraint, Content, Descriptor, FlatType, Mark, SuperType, Type};
pub use crate::union_find::{UnionFind, Variable};
