//! Alder type inference over the canonical AST.

mod inference;
mod traits;

pub use inference::run;
pub use traits::{InstanceHeader, TraitDatabase, TraitHeader, builtin_trait_id};
