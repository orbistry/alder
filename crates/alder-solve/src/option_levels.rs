//! Joint outer-Option depths for contextual lifting, not general subtyping.
//!
//! Node zero has depth zero. Each edge means `to >= from + offset`; its slack
//! is the number of Some wrappers inserted at that source site. Prefer the
//! minimum slack at every site simultaneously. If those individual minima
//! cannot coexist, choosing one would introduce an arbitrary call-order policy.

#[derive(Clone, Copy, Debug)]
pub(crate) struct Edge {
    pub from: usize,
    pub to: usize,
    pub offset: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Inconsistent,
    Ambiguous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FailureAt {
    pub kind: Failure,
    /// Earliest input edge in the failing component, not an unrelated site.
    pub edge: usize,
}

pub(crate) fn solve(nodes: usize, edges: &[Edge]) -> Result<Vec<usize>, FailureAt> {
    let mut incident = vec![Vec::new(); nodes];
    for (index, edge) in edges.iter().enumerate() {
        if edge.from != 0 {
            incident[edge.from].push(index);
        }
        if edge.to != 0 && edge.to != edge.from {
            incident[edge.to].push(index);
        }
    }
    if let Some(edge) = edges
        .iter()
        .position(|edge| edge.from == 0 && edge.to == 0 && edge.offset > 0)
    {
        return Err(FailureAt {
            kind: Failure::Inconsistent,
            edge,
        });
    }
    let mut levels = vec![0; nodes];
    let mut seen = vec![false; nodes];
    let mut seen_edges = vec![false; edges.len()];
    let mut ambiguous: Option<FailureAt> = None;
    for first in 1..nodes {
        if seen[first] {
            continue;
        }
        let mut members = vec![0];
        let mut edge_indices = Vec::new();
        let mut pending = vec![first];
        seen[first] = true;
        while let Some(node) = pending.pop() {
            members.push(node);
            for index in &incident[node] {
                if seen_edges[*index] {
                    continue;
                }
                seen_edges[*index] = true;
                edge_indices.push(*index);
                let edge = edges[*index];
                for next in [edge.from, edge.to] {
                    if next != 0 && !seen[next] {
                        seen[next] = true;
                        pending.push(next);
                    }
                }
            }
        }
        let indices = members
            .iter()
            .enumerate()
            .map(|(index, node)| (*node, index))
            .collect::<std::collections::BTreeMap<_, _>>();
        let component_edges = edge_indices
            .iter()
            .map(|index| {
                let edge = edges[*index];
                Edge {
                    from: indices[&edge.from],
                    to: indices[&edge.to],
                    offset: edge.offset,
                }
            })
            .collect::<Vec<_>>();
        let component = match solve_connected(members.len(), &component_edges) {
            Ok(component) => component,
            Err(kind) => {
                let failure = FailureAt {
                    kind,
                    edge: *edge_indices
                        .iter()
                        .min()
                        .expect("a failed component has edges"),
                };
                if kind == Failure::Inconsistent {
                    return Err(failure);
                }
                // An impossible component makes the whole problem impossible,
                // even if an earlier component has competing valid minima.
                if ambiguous.is_none_or(|previous| failure.edge < previous.edge) {
                    ambiguous = Some(failure);
                }
                continue;
            }
        };
        for (node, level) in members.into_iter().zip(component) {
            levels[node] = level;
        }
    }
    if let Some(failure) = ambiguous {
        Err(failure)
    } else {
        Ok(levels)
    }
}

fn solve_connected(nodes: usize, edges: &[Edge]) -> Result<Vec<usize>, Failure> {
    if let Some(levels) = solve_direct(nodes, edges) {
        return Ok(levels);
    }
    solve_dense(nodes, edges)
}

// Zero slack at every site is the absolute minimum. If these equalities are
// consistent, relative depths need only one graph traversal, not all-pairs
// closure. Unanchored components choose the smallest nonnegative translation.
// Failure here does not reject a program: positive minimum slack may be valid.
fn solve_direct(nodes: usize, edges: &[Edge]) -> Option<Vec<usize>> {
    let mut adjacent = vec![Vec::new(); nodes];
    for edge in edges {
        adjacent[edge.from].push((edge.to, edge.offset));
        adjacent[edge.to].push((edge.from, edge.offset.checked_neg()?));
    }
    let mut relative = vec![None::<i64>; nodes];
    let mut levels = vec![0; nodes];
    for first in 0..nodes {
        if relative[first].is_some() {
            continue;
        }
        relative[first] = Some(0);
        let mut pending = vec![first];
        let mut members = Vec::new();
        let mut minimum = 0;
        while let Some(node) = pending.pop() {
            let depth = relative[node]?;
            minimum = minimum.min(depth);
            members.push(node);
            for (next, offset) in &adjacent[node] {
                let expected = depth.checked_add(*offset)?;
                if let Some(actual) = relative[*next] {
                    if actual != expected {
                        return None;
                    }
                } else {
                    relative[*next] = Some(expected);
                    pending.push(*next);
                }
            }
        }
        // Node zero is visited first, so its component alone is anchored.
        let shift = if first == 0 {
            if minimum < 0 {
                return None;
            }
            0
        } else {
            minimum.checked_neg()?
        };
        for node in members {
            levels[node] = usize::try_from(relative[node]?.checked_add(shift)?).ok()?;
        }
    }
    Some(levels)
}

fn solve_dense(nodes: usize, edges: &[Edge]) -> Result<Vec<usize>, Failure> {
    let mut bounds = vec![vec![None; nodes]; nodes];
    for (index, row) in bounds.iter_mut().enumerate() {
        row[index] = Some(0);
    }
    // Type variables cannot have a negative number of Option layers.
    for bound in &mut bounds[0] {
        *bound = Some(0);
    }
    for edge in edges {
        strengthen(&mut bounds[edge.from][edge.to], edge.offset);
    }
    close(&mut bounds).map_err(|()| Failure::Inconsistent)?;

    // Closure gives the tight lower bound for each difference. The reverse
    // edge makes it an equality, requesting the fewest wrappers at that site.
    let minima = edges
        .iter()
        .map(|edge| bounds[edge.from][edge.to].expect("original edge is reachable"))
        .collect::<Vec<_>>();
    for (edge, minimum) in edges.iter().zip(minima) {
        strengthen(&mut bounds[edge.to][edge.from], -minimum);
    }
    close(&mut bounds).map_err(|()| Failure::Ambiguous)?;
    Ok(bounds[0]
        .iter()
        .map(|bound| {
            usize::try_from(bound.expect("every depth has a zero lower bound"))
                .expect("depths are nonnegative")
        })
        .collect())
}

fn strengthen(bound: &mut Option<i64>, value: i64) {
    *bound = Some(bound.map_or(value, |previous| previous.max(value)));
}

fn close(bounds: &mut [Vec<Option<i64>>]) -> Result<(), ()> {
    for via in 0..bounds.len() {
        for from in 0..bounds.len() {
            for to in 0..bounds.len() {
                if let (Some(left), Some(right)) = (bounds[from][via], bounds[via][to]) {
                    strengthen(&mut bounds[from][to], left.checked_add(right).ok_or(())?);
                }
            }
        }
        if (0..bounds.len()).any(|index| bounds[index][index].is_some_and(|value| value > 0)) {
            return Err(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Edge, Failure, FailureAt, solve_dense, solve_direct};

    // Most graph tests compare semantic outcomes; provenance has its own tests.
    fn solve(nodes: usize, edges: &[Edge]) -> Result<Vec<usize>, Failure> {
        super::solve(nodes, edges).map_err(|failure| failure.kind)
    }

    #[test]
    fn failures_identify_their_component_in_original_edge_order() {
        let unrelated = Edge {
            from: 0,
            to: 0,
            offset: -1,
        };
        assert_eq!(
            super::solve(
                1,
                &[
                    unrelated,
                    Edge {
                        from: 0,
                        to: 0,
                        offset: 1
                    }
                ]
            ),
            Err(FailureAt {
                kind: Failure::Inconsistent,
                edge: 1
            }),
        );
        assert_eq!(
            super::solve(
                3,
                &[
                    unrelated,
                    Edge {
                        from: 1,
                        to: 2,
                        offset: 0
                    },
                    Edge {
                        from: 2,
                        to: 1,
                        offset: -1
                    },
                ]
            ),
            Err(FailureAt {
                kind: Failure::Ambiguous,
                edge: 1
            }),
        );
        assert_eq!(
            super::solve(
                4,
                &[
                    Edge {
                        from: 1,
                        to: 0,
                        offset: -1
                    },
                    Edge {
                        from: 2,
                        to: 3,
                        offset: 0
                    },
                    Edge {
                        from: 3,
                        to: 2,
                        offset: -1
                    },
                ]
            ),
            Err(FailureAt {
                kind: Failure::Ambiguous,
                edge: 1
            }),
        );
        assert_eq!(
            super::solve(
                3,
                &[
                    unrelated,
                    Edge {
                        from: 1,
                        to: 2,
                        offset: 1
                    },
                    // Synthetic zero-depth constraints come after source edges.
                    Edge {
                        from: 1,
                        to: 0,
                        offset: 0
                    },
                    Edge {
                        from: 2,
                        to: 0,
                        offset: 0
                    },
                ]
            ),
            Err(FailureAt {
                kind: Failure::Inconsistent,
                edge: 1
            }),
        );
    }

    #[test]
    fn shared_actual_uses_the_tightest_upper_depth() {
        let mut edges = [
            Edge {
                from: 1,
                to: 0,
                offset: -1,
            },
            Edge {
                from: 1,
                to: 0,
                offset: -2,
            },
        ];
        assert_eq!(solve(2, &edges), Ok(vec![0, 1]));
        edges.reverse();
        assert_eq!(solve(2, &edges), Ok(vec![0, 1]));
    }

    #[test]
    fn shared_expected_payload_uses_the_strongest_lower_depth() {
        let mut edges = [
            Edge {
                from: 0,
                to: 1,
                offset: -1,
            },
            Edge {
                from: 0,
                to: 1,
                offset: 1,
            },
        ];
        assert_eq!(solve(2, &edges), Ok(vec![0, 1]));
        edges.reverse();
        assert_eq!(solve(2, &edges), Ok(vec![0, 1]));
    }

    #[test]
    fn generic_direct_match_retains_one_option_relationship() {
        assert_eq!(
            solve(
                3,
                &[Edge {
                    from: 1,
                    to: 2,
                    offset: -1
                }]
            ),
            Ok(vec![0, 1, 0]),
        );
    }

    #[test]
    fn incompatible_per_site_minima_are_ambiguous() {
        let mut edges = [
            Edge {
                from: 1,
                to: 2,
                offset: 0,
            },
            Edge {
                from: 2,
                to: 1,
                offset: -1,
            },
        ];
        assert_eq!(solve(3, &edges), Err(Failure::Ambiguous));
        edges.reverse();
        assert_eq!(solve(3, &edges), Err(Failure::Ambiguous));
    }

    #[test]
    fn lifting_never_removes_option_layers() {
        assert_eq!(
            solve(
                1,
                &[Edge {
                    from: 0,
                    to: 0,
                    offset: 1
                }]
            ),
            Err(Failure::Inconsistent),
        );
    }

    #[test]
    fn inconsistent_components_take_precedence_over_ambiguous_components() {
        for (ambiguous, inconsistent) in [(1, 2), (2, 1)] {
            let edges = [
                Edge {
                    from: 0,
                    to: ambiguous,
                    offset: -1,
                },
                Edge {
                    from: ambiguous,
                    to: 0,
                    offset: -1,
                },
                Edge {
                    from: inconsistent,
                    to: 0,
                    offset: 1,
                },
            ];
            assert_eq!(solve(3, &edges), Err(Failure::Inconsistent));
        }
    }

    #[test]
    fn connected_depths_preserve_all_direct_matches() {
        let count = 256;
        let edges = (1..count)
            .map(|node| Edge {
                from: node,
                to: node + 1,
                offset: if node % 2 == 1 { 1 } else { -1 },
            })
            .collect::<Vec<_>>();
        let expected = std::iter::once(0)
            .chain((1..=count).map(|node| (node + 1) % 2))
            .collect::<Vec<_>>();
        assert_eq!(solve(count + 1, &edges), Ok(expected));
    }

    #[test]
    fn large_connected_direct_matches_do_not_need_dense_closure() {
        let count = 10_000;
        let edges = (1..count)
            .map(|node| Edge {
                from: node,
                to: node + 1,
                offset: if node % 2 == 1 { 1 } else { -1 },
            })
            .collect::<Vec<_>>();
        let expected = std::iter::once(0)
            .chain((1..=count).map(|node| (node + 1) % 2))
            .collect::<Vec<_>>();
        // Exercise the sparse routine directly so a fast-path regression
        // cannot make this test allocate a 10,001-square fallback matrix.
        assert_eq!(solve_direct(count + 1, &edges), Some(expected));
    }

    #[test]
    fn direct_matches_agree_with_dense_closure_on_small_graphs() {
        let choices = (0..3)
            .flat_map(|from| {
                (0..3).flat_map(move |to| (-1..=1).map(move |offset| Edge { from, to, offset }))
            })
            .collect::<Vec<_>>();
        for first in &choices {
            for second in &choices {
                for third in &choices {
                    let edges = [*first, *second, *third];
                    let dense = solve_dense(3, &edges);
                    if let Some(direct) = solve_direct(3, &edges) {
                        assert_eq!(dense, Ok(direct), "{edges:?}");
                    }
                    assert_eq!(solve(3, &edges), dense, "{edges:?}");
                }
            }
        }
    }

    #[test]
    fn many_independent_depths_do_not_require_one_dense_problem() {
        let count = 10_000;
        let edges = (1..=count)
            .map(|node| Edge {
                from: node,
                to: 0,
                offset: -((node % 3) as i64),
            })
            .collect::<Vec<_>>();
        let expected = (0..=count).map(|node| node % 3).collect::<Vec<_>>();
        assert_eq!(solve(count + 1, &edges), Ok(expected));
    }

    #[test]
    fn small_depth_graphs_agree_with_exhaustive_assignments() {
        let choices = (0..3)
            .flat_map(|from| {
                (0..3).flat_map(move |to| (-1..=1).map(move |offset| Edge { from, to, offset }))
            })
            .collect::<Vec<_>>();
        for first in &choices {
            for second in &choices {
                let edges = [*first, *second];
                // With two unit-weight edges and two free nodes, depth four
                // covers both individual extrema and all simultaneous minima.
                let feasible = (0..=4)
                    .flat_map(|left| (0..=4).map(move |right| [0, left, right]))
                    .filter_map(|levels| {
                        let slack =
                            edges.map(|edge| levels[edge.to] - levels[edge.from] - edge.offset);
                        slack.iter().all(|value| *value >= 0).then_some(slack)
                    })
                    .collect::<Vec<_>>();
                let actual = solve(3, &edges);
                if feasible.is_empty() {
                    assert_eq!(actual, Err(Failure::Inconsistent), "{edges:?}");
                    continue;
                }
                let minima = [
                    feasible.iter().map(|slack| slack[0]).min().unwrap(),
                    feasible.iter().map(|slack| slack[1]).min().unwrap(),
                ];
                if !feasible.contains(&minima) {
                    assert_eq!(actual, Err(Failure::Ambiguous), "{edges:?}");
                } else {
                    let levels = actual.unwrap();
                    let slack = edges.map(|edge| {
                        levels[edge.to] as i64 - levels[edge.from] as i64 - edge.offset
                    });
                    assert_eq!(slack, minima, "{edges:?}");
                }
            }
        }
    }
}
