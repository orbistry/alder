//! Port of Elm's `Type.Constrain.Module`.
//!
//! Alder has no ports or effect managers, so this is just the declaration
//! walk terminated by `CSaveTheEnvironment`.

use alder_ast::{Decls, Module as CanModule};
use bumpalo::Bump;

use crate::expression;
use crate::type_::Constraint;
use crate::union_find::UnionFind;

pub fn constrain<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    module: &CanModule<'a>,
) -> Constraint<'a> {
    constrain_decls(bump, uf, module.decls, Constraint::SaveTheEnvironment)
}

fn constrain_decls<'a>(
    bump: &'a Bump,
    uf: &mut UnionFind<'a>,
    decls: &Decls<'a>,
    final_constraint: Constraint<'a>,
) -> Constraint<'a> {
    match decls {
        Decls::Declare { definition, next } => {
            let next_con = constrain_decls(bump, uf, next, final_constraint);
            expression::constrain_def(bump, uf, &expression::Rtv::new(), definition, next_con)
        }

        Decls::DeclareRec {
            definition,
            following,
            next,
        } => {
            let next_con = constrain_decls(bump, uf, next, final_constraint);
            let mut defs = Vec::with_capacity(1 + following.len());
            defs.push(*definition);
            defs.extend(following.iter().copied());
            expression::constrain_recursive_defs(bump, uf, &expression::Rtv::new(), &defs, next_con)
        }

        Decls::Empty => final_constraint,
    }
}
