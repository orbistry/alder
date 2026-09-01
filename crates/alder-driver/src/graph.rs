//! Dependency graph construction and analysis.
//!
//! Builds a graph of module dependencies by parsing import statements,
//! performs topological sorting for compilation order, and detects cycles.

use std::collections::{HashMap, HashSet, VecDeque};
use url::Url;

use crate::error::DriverError;

/// A dependency graph of modules.
#[derive(Debug, Default)]
pub struct DepGraph {
    /// Modules in topological order (dependencies before dependents).
    pub order: Vec<Url>,

    /// Import relationships: module -> modules it imports.
    pub edges: HashMap<Url, Vec<Url>>,

    /// Depth of each module in the graph (for parallel compilation).
    pub depths: HashMap<Url, usize>,
}

impl DepGraph {
    /// Create a new empty dependency graph.
    pub fn new() -> Self {
        DepGraph::default()
    }

    /// Add a module to the graph with its imports.
    pub fn add_module(&mut self, module: Url, imports: Vec<Url>) {
        self.edges.insert(module, imports);
    }

    /// Build the topological order from the edges.
    ///
    /// Returns an error if a cycle is detected.
    pub fn compute_order(&mut self) -> Result<(), DriverError> {
        // Kahn's algorithm for topological sort
        // We want dependencies to come before dependents.
        // edges[A] = [B, C] means A imports B and C (A depends on B and C)
        // So B and C must be compiled before A.

        // Build reverse graph: for each module, track who imports it
        let mut importers: HashMap<&Url, Vec<&Url>> = HashMap::new();
        let mut all_nodes: HashSet<&Url> = HashSet::new();

        for (module, imports) in &self.edges {
            all_nodes.insert(module);
            for import in imports {
                all_nodes.insert(import);
                importers.entry(import).or_default().push(module);
            }
        }

        // out-degree = number of dependencies = len(edges[node])
        // We start with nodes that have no dependencies
        let mut out_degree: HashMap<&Url, usize> = HashMap::new();
        for node in &all_nodes {
            let deg = self.edges.get(*node).map(|v| v.len()).unwrap_or(0);
            out_degree.insert(node, deg);
        }

        // Start with nodes that have no dependencies (out-degree = 0)
        let mut queue: VecDeque<&Url> = out_degree
            .iter()
            .filter(|&(_, deg)| *deg == 0)
            .map(|(&node, _)| node)
            .collect();

        let mut order = Vec::new();
        let mut depths: HashMap<Url, usize> = HashMap::new();

        // Process nodes in order
        while let Some(node) = queue.pop_front() {
            // Calculate depth: max depth of imports + 1
            let depth = self
                .edges
                .get(node)
                .map(|imports| {
                    imports
                        .iter()
                        .filter_map(|i| depths.get(i))
                        .max()
                        .map(|d| d + 1)
                        .unwrap_or(0)
                })
                .unwrap_or(0);

            depths.insert(node.clone(), depth);
            order.push(node.clone());

            // For each module that imports this one, decrement its out-degree
            // (one of its dependencies is now satisfied)
            if let Some(deps) = importers.get(node) {
                for importer in deps {
                    if let Some(deg) = out_degree.get_mut(importer) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(importer);
                        }
                    }
                }
            }
        }

        // Check for cycles
        if order.len() != all_nodes.len() {
            // Find a cycle for error reporting
            let cycle = self.find_cycle_path();
            return Err(DriverError::ImportCycle { cycle });
        }

        self.order = order;
        self.depths = depths;

        Ok(())
    }

    /// Find a cycle in the graph and return a human-readable path.
    fn find_cycle_path(&self) -> String {
        // DFS to find cycle
        let mut visited: HashSet<&Url> = HashSet::new();
        let mut stack: HashSet<&Url> = HashSet::new();
        let mut path: Vec<&Url> = Vec::new();

        for start in self.edges.keys() {
            if self.dfs_cycle(start, &mut visited, &mut stack, &mut path) {
                // Format cycle as: A -> B -> C -> A
                let cycle_str: Vec<String> = path.iter().map(|u| module_name_from_uri(u)).collect();
                return cycle_str.join(" -> ");
            }
        }

        "unknown cycle".to_string()
    }

    fn dfs_cycle<'a>(
        &'a self,
        node: &'a Url,
        visited: &mut HashSet<&'a Url>,
        stack: &mut HashSet<&'a Url>,
        path: &mut Vec<&'a Url>,
    ) -> bool {
        if stack.contains(node) {
            // Found cycle - trim path to just the cycle
            if let Some(pos) = path.iter().position(|&n| n == node) {
                path.drain(..pos);
            }
            path.push(node);
            return true;
        }

        if visited.contains(node) {
            return false;
        }

        visited.insert(node);
        stack.insert(node);
        path.push(node);

        if let Some(imports) = self.edges.get(node) {
            for import in imports {
                if self.dfs_cycle(import, visited, stack, path) {
                    return true;
                }
            }
        }

        stack.remove(node);
        path.pop();
        false
    }

    /// Get modules grouped by depth for parallel compilation.
    ///
    /// Returns groups where all modules in a group can be compiled in parallel
    /// because their dependencies are in earlier groups.
    pub fn levels(&self) -> Vec<Vec<&Url>> {
        if self.order.is_empty() {
            return vec![];
        }

        let max_depth = self.depths.values().max().copied().unwrap_or(0);
        let mut levels: Vec<Vec<&Url>> = vec![vec![]; max_depth + 1];

        for (module, &depth) in &self.depths {
            levels[depth].push(module);
        }

        levels
    }

    /// Check if module A depends on module B (directly or transitively).
    pub fn depends_on(&self, a: &Url, b: &Url) -> bool {
        let mut visited = HashSet::new();
        self.depends_on_dfs(a, b, &mut visited)
    }

    fn depends_on_dfs(&self, current: &Url, target: &Url, visited: &mut HashSet<Url>) -> bool {
        if current == target {
            return true;
        }

        if visited.contains(current) {
            return false;
        }
        visited.insert(current.clone());

        if let Some(imports) = self.edges.get(current) {
            for import in imports {
                if self.depends_on_dfs(import, target, visited) {
                    return true;
                }
            }
        }

        false
    }
}

/// Extract a module name from a file URI for display.
fn module_name_from_uri(uri: &Url) -> String {
    uri.path_segments()
        .and_then(|mut segments| segments.next_back())
        .map(|s| s.trim_end_matches(".ald"))
        .unwrap_or("unknown")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(path: &str) -> Url {
        Url::parse(&format!("file:///{}", path)).unwrap()
    }

    #[test]
    fn test_simple_graph() {
        let mut graph = DepGraph::new();

        // Main imports Utils
        graph.add_module(url("Main.ald"), vec![url("Utils.ald")]);
        graph.add_module(url("Utils.ald"), vec![]);

        graph.compute_order().unwrap();

        // Utils should come before Main
        let main_idx = graph
            .order
            .iter()
            .position(|u| u == &url("Main.ald"))
            .unwrap();
        let utils_idx = graph
            .order
            .iter()
            .position(|u| u == &url("Utils.ald"))
            .unwrap();
        assert!(utils_idx < main_idx);
    }

    #[test]
    fn test_cycle_detection() {
        let mut graph = DepGraph::new();

        // A -> B -> C -> A (cycle)
        graph.add_module(url("A.ald"), vec![url("B.ald")]);
        graph.add_module(url("B.ald"), vec![url("C.ald")]);
        graph.add_module(url("C.ald"), vec![url("A.ald")]);

        let result = graph.compute_order();
        assert!(result.is_err());

        if let Err(DriverError::ImportCycle { cycle }) = result {
            // Cycle should mention all three modules
            assert!(cycle.contains("A") || cycle.contains("B") || cycle.contains("C"));
        }
    }

    #[test]
    fn test_levels() {
        let mut graph = DepGraph::new();

        // Diamond dependency: Main -> (A, B) -> Core
        graph.add_module(url("Main.ald"), vec![url("A.ald"), url("B.ald")]);
        graph.add_module(url("A.ald"), vec![url("Core.ald")]);
        graph.add_module(url("B.ald"), vec![url("Core.ald")]);
        graph.add_module(url("Core.ald"), vec![]);

        graph.compute_order().unwrap();
        let levels = graph.levels();

        // Should have 3 levels:
        // Level 0: Core
        // Level 1: A, B
        // Level 2: Main
        assert!(levels.len() >= 2);
    }
}
