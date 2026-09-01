//! Port of Elm's `Type.Instantiate`: turn a canonical type into an
//! inference `Type`, substituting free type variables.

use std::collections::BTreeMap;

use alder_ast::{AliasType as CanAliasType, Type as CanType};
use alder_region::Located;
use bumpalo::Bump;

use crate::type_::Type;

pub type FreeVars<'a> = BTreeMap<&'a str, &'a Type<'a>>;

pub fn from_src_type<'a>(
    bump: &'a Bump,
    free_vars: &FreeVars<'a>,
    src_type: &Located<CanType<'a>>,
) -> &'a Type<'a> {
    match &src_type.value {
        CanType::Lambda { from, to } => bump.alloc(Type::FunN(
            from_src_type(bump, free_vars, from),
            from_src_type(bump, free_vars, to),
        )),

        CanType::Var(name) => free_vars
            .get(name)
            .expect("canonical types only mention their free variables"),

        CanType::Named { reference, args } => bump.alloc(Type::AppN {
            home: reference.home,
            name: reference.name,
            args: bump
                .alloc_slice_fill_iter(args.iter().map(|arg| from_src_type(bump, free_vars, arg))),
        }),

        CanType::Alias {
            reference,
            arguments,
            target,
        } => {
            let targs = bump.alloc_slice_fill_iter(
                arguments
                    .iter()
                    .map(|arg| (arg.name, from_src_type(bump, free_vars, arg.typ))),
            );
            let real = match target {
                CanAliasType::Filled(real_type) => from_src_type(bump, free_vars, real_type),
                CanAliasType::Open(real_type) => {
                    let arg_vars: FreeVars<'a> = targs.iter().copied().collect();
                    from_src_type(bump, &arg_vars, real_type)
                }
            };
            bump.alloc(Type::AliasN {
                home: reference.home,
                name: reference.name,
                args: targs,
                real,
            })
        }

        CanType::Tuple {
            first,
            second,
            rest,
        } => bump.alloc(Type::TupleN(
            from_src_type(bump, free_vars, first),
            from_src_type(bump, free_vars, second),
            rest.first()
                .map(|third| from_src_type(bump, free_vars, third)),
        )),

        CanType::Unit => bump.alloc(Type::UnitN),

        CanType::Record { fields, ext } => bump.alloc(Type::RecordN {
            fields: bump.alloc_slice_fill_iter(
                fields
                    .iter()
                    .map(|field| (field.field, from_src_type(bump, free_vars, field.typ))),
            ),
            ext: match ext {
                None => bump.alloc(Type::EmptyRecordN),
                Some(ext_name) => free_vars
                    .get(ext_name)
                    .expect("canonical types only mention their free variables"),
            },
        }),
    }
}
