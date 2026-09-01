//! Port of Elm's `Type.Occurs`: does a variable occur in its own structure?

use alder_constrain::{Content, FlatType, UnionFind, Variable};

pub fn occurs(uf: &mut UnionFind<'_>, var: Variable) -> bool {
    occurs_help(uf, &mut Vec::new(), var, false)
}

fn occurs_help(
    uf: &mut UnionFind<'_>,
    seen: &mut Vec<Variable>,
    var: Variable,
    found_cycle: bool,
) -> bool {
    if seen.contains(&var) {
        return true;
    }

    let content = uf.get(var).content.clone();
    match content {
        Content::FlexVar(_)
        | Content::FlexSuper(_, _)
        | Content::RigidVar(_)
        | Content::RigidSuper(_, _)
        | Content::Error => found_cycle,

        Content::Structure(term) => {
            seen.push(var);
            let result = match term {
                FlatType::App1(_, _, args) => args
                    .iter()
                    .fold(found_cycle, |acc, arg| occurs_help(uf, seen, *arg, acc)),

                FlatType::Fun1(a, b) => {
                    let acc = occurs_help(uf, seen, b, found_cycle);
                    occurs_help(uf, seen, a, acc)
                }

                FlatType::EmptyRecord1 => found_cycle,

                FlatType::Record1(fields, ext) => {
                    let acc = fields
                        .values()
                        .fold(found_cycle, |acc, field| occurs_help(uf, seen, *field, acc));
                    occurs_help(uf, seen, ext, acc)
                }

                FlatType::Unit1 => found_cycle,

                FlatType::Tuple1(a, b, maybe_c) => {
                    let acc = match maybe_c {
                        None => found_cycle,
                        Some(c) => occurs_help(uf, seen, c, found_cycle),
                    };
                    let acc = occurs_help(uf, seen, b, acc);
                    occurs_help(uf, seen, a, acc)
                }
            };
            seen.pop();
            result
        }

        Content::Alias { args, .. } => {
            seen.push(var);
            let result = args.iter().fold(found_cycle, |acc, (_, arg)| {
                occurs_help(uf, seen, *arg, acc)
            });
            seen.pop();
            result
        }
    }
}
