//! Constructor-specializing pattern usefulness. Rows contain only unconditional
//! patterns; guards and pins are handled by the caller, never evaluated here.

#[derive(Clone, Debug)]
pub(crate) enum Pattern {
    Any,
    Constructor {
        key: String,
        /// None denotes an infinite or open family (numbers, strings, open rows).
        alternatives: Option<Vec<(String, usize)>>,
        arguments: Vec<Pattern>,
    },
}

pub(crate) fn useful(matrix: &[Vec<Pattern>], query: &[Pattern]) -> bool {
    if query.iter().any(|pattern| !pattern.possible()) {
        return false;
    }
    if matrix.is_empty() {
        return true;
    }
    if query.is_empty() {
        return false;
    }
    if matrix
        .iter()
        .any(|row| row.iter().all(|p| matches!(p, Pattern::Any)))
    {
        return false;
    }
    match &query[0] {
        Pattern::Constructor { key, arguments, .. } => {
            let specialized = specialize(matrix, key, arguments.len());
            let mut next = arguments.clone();
            next.extend_from_slice(&query[1..]);
            useful(&specialized, &next)
        }
        Pattern::Any => {
            let mut constructors = std::collections::BTreeMap::new();
            for row in matrix {
                if let Pattern::Constructor {
                    key,
                    alternatives,
                    arguments,
                } = &row[0]
                {
                    constructors.insert(key, (alternatives.as_ref(), arguments.len()));
                }
            }
            let complete = constructors.values().next().is_some_and(|(cases, _)| {
                cases.is_some_and(|cases| cases.len() == constructors.len())
            });
            if complete {
                constructors.into_iter().any(|(key, (_, arity))| {
                    let mut next = vec![Pattern::Any; arity];
                    next.extend_from_slice(&query[1..]);
                    useful(&specialize(matrix, key, arity), &next)
                })
            } else {
                let defaults = matrix
                    .iter()
                    .filter(|row| matches!(row[0], Pattern::Any))
                    .map(|row| row[1..].to_vec())
                    .collect::<Vec<_>>();
                useful(&defaults, &query[1..])
            }
        }
    }
}

/// Return at most `limit` uncovered pattern vectors. Like usefulness, this
/// consumes a matrix column at each default step, and specializes only the
/// finite constructor syntax already present in the matrix (not recursive types).
pub(crate) fn missing(matrix: &[Vec<Pattern>], columns: usize, limit: usize) -> Vec<Vec<Pattern>> {
    if limit == 0 || !useful(matrix, &vec![Pattern::Any; columns]) {
        return vec![];
    }
    if matrix.is_empty() {
        return vec![vec![Pattern::Any; columns]];
    }
    let mut seen = std::collections::BTreeMap::new();
    let mut family = None;
    for row in matrix {
        if let Pattern::Constructor {
            key,
            alternatives,
            arguments,
        } = &row[0]
        {
            seen.insert(key.clone(), arguments.len());
            family = alternatives.as_ref();
        }
    }
    if family.is_some_and(|cases| cases.len() == seen.len()) {
        let mut result = Vec::new();
        for (key, arity) in seen {
            for mut row in missing(
                &specialize(matrix, &key, arity),
                columns - 1 + arity,
                limit - result.len(),
            ) {
                let rest = row.split_off(arity);
                let head = Pattern::Constructor {
                    key: key.clone(),
                    alternatives: family.cloned(),
                    arguments: row,
                };
                result.push(std::iter::once(head).chain(rest).collect());
            }
        }
        result
    } else {
        let defaults = matrix
            .iter()
            .filter(|row| matches!(row[0], Pattern::Any))
            .map(|row| row[1..].to_vec())
            .collect::<Vec<_>>();
        let tails = missing(&defaults, columns - 1, limit);
        let heads = family
            .map(|cases| {
                cases
                    .iter()
                    .filter(|(key, _)| !seen.contains_key(key))
                    .map(|(key, arity)| Pattern::Constructor {
                        key: key.clone(),
                        alternatives: family.cloned(),
                        arguments: vec![Pattern::Any; *arity],
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![Pattern::Any]);
        heads
            .into_iter()
            .flat_map(|head| {
                tails.iter().map(move |tail| {
                    std::iter::once(head.clone())
                        .chain(tail.iter().cloned())
                        .collect()
                })
            })
            .take(limit)
            .collect()
    }
}

impl Pattern {
    /// Failed guards and pins may mutate the scrutinee through an alias. Enum
    /// identities and primitive values are stable, but refinements of mutable
    /// fields, tuple slots, or array length/elements cannot survive that step.
    pub(crate) fn stable_after_effects(&self) -> bool {
        match self {
            Self::Any => true,
            Self::Constructor { key, arguments, .. } => match key.as_str() {
                "#nil" | "#cons" => false,
                "#tuple" => arguments.iter().all(|p| matches!(p, Self::Any)),
                _ if key.starts_with("#record:") => {
                    arguments.iter().all(|p| matches!(p, Self::Any))
                }
                _ => arguments.iter().all(Self::stable_after_effects),
            },
        }
    }

    fn possible(&self) -> bool {
        match self {
            Self::Any => true,
            Self::Constructor {
                key,
                alternatives,
                arguments,
            } => {
                alternatives
                    .as_ref()
                    .is_none_or(|cases| cases.iter().any(|(case, _)| case == key))
                    && arguments.iter().all(Self::possible)
            }
        }
    }

    pub(crate) fn render(&self) -> String {
        let Self::Constructor { key, arguments, .. } = self else {
            return "_".into();
        };
        let args = arguments.iter().map(Self::render).collect::<Vec<_>>();
        match key.as_str() {
            "#tuple" => format!("({})", args.join(", ")),
            "#nil" => "[]".into(),
            "#cons" => {
                let mut items = Vec::new();
                let mut tail = self;
                while let Self::Constructor { key, arguments, .. } = tail {
                    if key != "#cons" {
                        break;
                    }
                    items.push(arguments[0].render());
                    tail = &arguments[1];
                }
                if !matches!(tail, Self::Constructor { key, .. } if key == "#nil") {
                    items.push("..".into());
                }
                format!("[{}]", items.join(", "))
            }
            _ if !key.starts_with('"') && key.contains('#') => {
                let (name, fields) = key.strip_prefix("#record:").map_or_else(
                    || key.split_once('#').expect("record constructor key"),
                    |fields| ("", fields),
                );
                let fields = fields
                    .split(',')
                    .zip(args)
                    .map(|(name, value)| format!("{name}: {value}"))
                    .collect::<Vec<_>>();
                let prefix = if name.is_empty() {
                    String::new()
                } else {
                    format!("{name} ")
                };
                format!("{prefix}{{ {} }}", fields.join(", "))
            }
            _ if args.is_empty() => key.clone(),
            _ => format!("{key}({})", args.join(", ")),
        }
    }
}

fn specialize(matrix: &[Vec<Pattern>], key: &str, arity: usize) -> Vec<Vec<Pattern>> {
    matrix
        .iter()
        .filter_map(|row| {
            let mut head = match &row[0] {
                Pattern::Any => vec![Pattern::Any; arity],
                Pattern::Constructor {
                    key: candidate,
                    arguments,
                    ..
                } if candidate == key => arguments.clone(),
                _ => return None,
            };
            head.extend_from_slice(&row[1..]);
            Some(head)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boolean(value: bool) -> Pattern {
        Pattern::Constructor {
            key: value.to_string(),
            alternatives: Some(vec![("false".into(), 0), ("true".into(), 0)]),
            arguments: vec![],
        }
    }

    #[test]
    fn combinations_are_checked_not_just_individual_columns() {
        let mut rows = vec![
            vec![boolean(true), boolean(true)],
            vec![boolean(false), boolean(false)],
        ];
        assert!(useful(&rows, &[Pattern::Any, Pattern::Any]));
        rows.push(vec![boolean(true), boolean(false)]);
        rows.push(vec![boolean(false), boolean(true)]);
        assert!(!useful(&rows, &[Pattern::Any, Pattern::Any]));
    }

    #[test]
    fn wildcard_rows_cover_specialized_payloads() {
        let rows = vec![
            vec![Pattern::Any, boolean(true)],
            vec![Pattern::Any, boolean(false)],
        ];
        assert!(!useful(&rows, &[boolean(true), Pattern::Any]));
        assert!(!useful(&rows, &[Pattern::Any, Pattern::Any]));
    }

    #[test]
    fn usefulness_agrees_with_every_boolean_pair_subset() {
        let values = [(false, false), (false, true), (true, false), (true, true)];
        for subset in 0..16 {
            let matrix = values
                .iter()
                .enumerate()
                .filter(|(index, _)| subset & (1 << index) != 0)
                .map(|(_, (a, b))| vec![boolean(*a), boolean(*b)])
                .collect::<Vec<_>>();
            for a in [None, Some(false), Some(true)] {
                for b in [None, Some(false), Some(true)] {
                    let expected = values.iter().enumerate().any(|(index, (x, y))| {
                        subset & (1 << index) == 0
                            && a.is_none_or(|a| a == *x)
                            && b.is_none_or(|b| b == *y)
                    });
                    let query = [
                        a.map_or(Pattern::Any, boolean),
                        b.map_or(Pattern::Any, boolean),
                    ];
                    assert_eq!(
                        useful(&matrix, &query),
                        expected,
                        "subset {subset}, query {query:?}"
                    );
                }
            }
            assert_eq!(missing(&matrix, 2, 4).is_empty(), subset == 15);
            for witness in missing(&matrix, 2, 4) {
                assert!(useful(&matrix, &witness));
            }
        }
    }
}
