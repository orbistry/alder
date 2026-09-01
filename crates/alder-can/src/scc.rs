pub struct Node<'a, T> {
    pub key: &'a str,
    pub value: T,
    pub deps: Vec<&'a str>,
}

pub enum Scc<T> {
    Acyclic(T),
    Cyclic(Vec<T>),
}

/// Exact port of Haskell's `Data.Graph.stronglyConnComp` so component
/// order and member order match Elm's canonicalizer:
///
/// - `graphFromEdges` numbers vertices in key-sorted order; unknown deps
///   are dropped, duplicate edges and self-edges are kept.
/// - `scc g = dfs g (reverse (postOrd (transposeG g)))` (Kosaraju): a DFS
///   forest over the transposed graph gives a reversed postorder, then a
///   DFS over the original graph in that order yields one tree per SCC,
///   with members in tree preorder.
/// - A singleton component is `Acyclic` unless it has a self-edge.
pub fn strongly_connected_components<T>(nodes: Vec<Node<'_, T>>) -> Vec<Scc<T>> {
    let n = nodes.len();

    // graphFromEdges sorts nodes by key; vertex v = sorted position.
    let mut sorted: Vec<usize> = (0..n).collect();
    sorted.sort_by_key(|&i| nodes[i].key);

    let key_vertex =
        |key: &str| -> Option<usize> { sorted.binary_search_by(|&i| nodes[i].key.cmp(key)).ok() };

    // Adjacency in vertex space, preserving each node's dep order and
    // duplicates (`mapMaybe key_vertex ks`).
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for v in 0..n {
        for dep in &nodes[sorted[v]].deps {
            if let Some(w) = key_vertex(dep) {
                adj[v].push(w);
            }
        }
    }

    // transposeG via buildG/accumArray (flip (:)): edges enumerated in
    // ascending vertex order are *prepended*, so each incoming list ends
    // up in descending enumeration order.
    let mut transposed: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (v, targets) in adj.iter().enumerate() {
        for &w in targets {
            transposed[w].push(v);
        }
    }
    for list in &mut transposed {
        list.reverse();
    }

    // reverse (postOrd (transposeG g)): postorder of the DFS forest over
    // the transpose, visiting roots 0..n-1, then reversed.
    let mut post_order: Vec<usize> = Vec::with_capacity(n);
    let mut visited = vec![false; n];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for root in 0..n {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        stack.push((root, 0));
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if let Some(&w) = transposed[v].get(*next) {
                *next += 1;
                if !visited[w] {
                    visited[w] = true;
                    stack.push((w, 0));
                }
            } else {
                post_order.push(v);
                stack.pop();
            }
        }
    }

    // dfs g (reverse post_order): each tree is one SCC, members in preorder.
    let mut components: Vec<Vec<usize>> = Vec::new();
    let mut visited = vec![false; n];
    for &root in post_order.iter().rev() {
        if visited[root] {
            continue;
        }
        let mut component = Vec::new();
        visited[root] = true;
        stack.push((root, 0));
        component.push(root);
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if let Some(&w) = adj[v].get(*next) {
                *next += 1;
                if !visited[w] {
                    visited[w] = true;
                    component.push(w);
                    stack.push((w, 0));
                }
            } else {
                stack.pop();
            }
        }
        components.push(component);
    }

    let has_self_edge: Vec<bool> = (0..n).map(|v| adj[v].contains(&v)).collect();

    let mut values: Vec<Option<T>> = nodes.into_iter().map(|node| Some(node.value)).collect();
    let mut take = |v: usize| {
        values[sorted[v]]
            .take()
            .expect("each vertex appears in exactly one SCC")
    };

    components
        .into_iter()
        .map(|component| {
            if component.len() == 1 && !has_self_edge[component[0]] {
                Scc::Acyclic(take(component[0]))
            } else {
                Scc::Cyclic(component.into_iter().map(&mut take).collect())
            }
        })
        .collect()
}
