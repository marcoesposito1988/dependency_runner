//! This crate contains the data structures used to hold the results of a dependency scan

use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::path::PathBuf;

use petgraph::algo::{is_cyclic_directed, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use serde::{Deserialize, Serialize};

use crate::common::{readable_canonical_path, LookupError};

/// Normalized, case-insensitive DLL name wrapper.
///
/// Windows DLL resolution is case-insensitive. `DllName` preserves the original
/// string casing for display while providing case-insensitive equality, comparison,
/// and hashing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DllName(String);

impl DllName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl Deref for DllName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for DllName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for DllName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DllName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<S: Into<String>> From<S> for DllName {
    fn from(s: S) -> Self {
        Self(s.into())
    }
}

impl PartialEq for DllName {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for DllName {}

impl Hash for DllName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for byte in self.0.bytes() {
            state.write_u8(byte.to_ascii_lowercase());
        }
    }
}

impl PartialOrd for DllName {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DllName {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .to_ascii_lowercase()
            .cmp(&other.0.to_ascii_lowercase())
    }
}

impl PartialEq<str> for DllName {
    fn eq(&self, other: &str) -> bool {
        self.0.eq_ignore_ascii_case(other)
    }
}

impl PartialEq<&str> for DllName {
    fn eq(&self, other: &&str) -> bool {
        self.0.eq_ignore_ascii_case(*other)
    }
}

impl PartialEq<String> for DllName {
    fn eq(&self, other: &String) -> bool {
        self.0.eq_ignore_ascii_case(other)
    }
}

/// Information about a DLL that was mentioned as a target for the search
/// If the file was actually found, additional info is available. Otherwise, it represents a
/// missing/broken dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Executable {
    /// Name as it appears in the import table
    pub dllname: String,
    /// depth at which the file was first mentioned in the dependency tree
    pub depth_first_appearance: usize,
    /// if the file was found on the PATH
    pub found: bool,
    /// metadata extracted from the actual executable file
    pub details: Option<ExecutableDetails>,
}

/// Metadata for a found executable file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableDetails {
    /// virtual DLL, which just forwards to an implementation
    pub is_api_set: bool,
    /// located in a system directory (Win or Sys dir)
    pub is_system: bool,
    /// it is among the KnownDLLs list or a dependency thereof
    pub is_known_dll: bool,
    /// full path
    pub full_path: PathBuf,
    /// names of the DLLs this executable file depends on
    pub dependencies: Option<Vec<String>>,
    /// Symbols import / export table
    pub symbols: Option<ExecutableSymbols>,
}

/// Symbols information for a found executable file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableSymbols {
    /// Exported symbols
    pub exported: HashSet<String>,
    /// Imported symbols, grouped by DLL
    pub imported: HashMap<String, HashSet<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutablesCheckReport {
    /// Map from dependent to list of non-found dependees
    pub not_found_libraries: HashMap<String, HashSet<String>>,
    /// Map from importer to list of non-found imported symbols, grouped by dependent DLL
    pub not_found_symbols: Option<HashMap<String, HashMap<String, HashSet<String>>>>,
}

impl Default for ExecutablesCheckReport {
    fn default() -> Self {
        ExecutablesCheckReport::new()
    }
}

impl ExecutablesCheckReport {
    pub fn new() -> Self {
        Self {
            not_found_libraries: HashMap::new(),
            not_found_symbols: None,
        }
    }

    pub fn extend(&mut self, other: ExecutablesCheckReport) {
        self.not_found_libraries.extend(other.not_found_libraries);

        if let Some(other_symbols) = other.not_found_symbols {
            if let Some(our_symbols) = self.not_found_symbols.as_mut() {
                our_symbols.extend(other_symbols)
            } else {
                self.not_found_symbols = Some(other_symbols)
            }
        }
    }
}

/// Collection of Executable objects, result of a DLL search, modeled as a directed graph
#[derive(Debug, Clone)]
pub struct Executables {
    index: HashMap<DllName, Executable>,
    graph: DiGraph<DllName, ()>,
    node_indices: HashMap<DllName, NodeIndex>,
}

impl Default for Executables {
    fn default() -> Self {
        Executables::new()
    }
}

impl Executables {
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
            graph: DiGraph::new(),
            node_indices: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn get(&self, dllname: &str) -> Option<&Executable> {
        self.index.get(&DllName::from(dllname))
    }

    pub fn contains(&self, dllname: &str) -> bool {
        self.index.contains_key(&DllName::from(dllname))
    }

    /// Access the underlying directed dependency graph (`petgraph::graph::DiGraph`)
    /// where directed edges represent `importer -> dependency`.
    pub fn graph(&self) -> &DiGraph<DllName, ()> {
        &self.graph
    }

    /// Get the `NodeIndex` for a DLL in the petgraph representation, if present.
    pub fn node_index(&self, dllname: &str) -> Option<NodeIndex> {
        self.node_indices.get(&DllName::from(dllname)).copied()
    }

    /// Iterator over all executables in the collection.
    pub fn iter(&self) -> impl Iterator<Item = &Executable> {
        self.index.values()
    }

    /// Get the root executable file (i.e., the only one with depth equal to zero)
    pub fn get_root(&self) -> Result<Option<&Executable>, LookupError> {
        if self.index.is_empty() {
            return Ok(None);
        }
        let root_candidates: Vec<&Executable> = self
            .index
            .values()
            .filter(|v| v.depth_first_appearance == 0)
            .collect();
        if root_candidates.is_empty() {
            return Err(LookupError::ScanError(
                "The executable tree has no roots".to_string(),
            ));
        }
        if root_candidates.len() > 1 {
            let names: Vec<&str> = root_candidates.iter().map(|n| n.dllname.as_ref()).collect();
            return Err(LookupError::ScanError(format!(
                "The executable tree has multiple roots: {}",
                names.join(";")
            )));
        }
        Ok(root_candidates.first().copied())
    }

    pub fn sorted_by_first_appearance(&self) -> Vec<&Executable> {
        let mut sorted_executables: Vec<_> = self.index.values().collect();
        sorted_executables
            .sort_by(|e1, e2| e1.depth_first_appearance.cmp(&e2.depth_first_appearance));
        sorted_executables
    }

    /// Add a new executable to the collection and update the dependency graph
    pub(crate) fn insert(&mut self, new_exe: Executable) {
        let dll_key = DllName::from(&new_exe.dllname);

        if let Some(older_finding) = self.index.get(&dll_key) {
            tracing::warn!(
                "Found two DLLs with the same name! {:?} and {:?}",
                new_exe
                    .details
                    .as_ref()
                    .and_then(|d| readable_canonical_path(&d.full_path).ok())
                    .unwrap_or(new_exe.dllname),
                older_finding
                    .details
                    .as_ref()
                    .and_then(|d| readable_canonical_path(&d.full_path).ok())
                    .unwrap_or_else(|| older_finding.dllname.clone()),
            );
            return;
        }

        let node_idx = *self
            .node_indices
            .entry(dll_key.clone())
            .or_insert_with(|| self.graph.add_node(dll_key.clone()));

        if let Some(details) = &new_exe.details {
            if let Some(dependencies) = &details.dependencies {
                for dep in dependencies {
                    let dep_key = DllName::from(dep);
                    let dep_idx = *self
                        .node_indices
                        .entry(dep_key.clone())
                        .or_insert_with(|| self.graph.add_node(dep_key));
                    self.graph.update_edge(node_idx, dep_idx, ());
                }
            }
        }

        self.index.insert(dll_key, new_exe);
    }

    /// Return executables in topological sort order (dependents before dependencies).
    /// Returns an error if a dependency cycle is detected.
    pub fn topological_sort(&self) -> Result<Vec<&Executable>, LookupError> {
        match toposort(&self.graph, None) {
            Ok(nodes) => {
                let mut result = Vec::with_capacity(nodes.len());
                for node_idx in nodes {
                    let dll_name = &self.graph[node_idx];
                    if let Some(exe) = self.index.get(dll_name) {
                        result.push(exe);
                    }
                }
                Ok(result)
            }
            Err(cycle) => {
                let cyclic_node = &self.graph[cycle.node_id()];
                Err(LookupError::ScanError(format!(
                    "Cyclic dependency detected involving {}",
                    cyclic_node
                )))
            }
        }
    }

    /// Check if the dependency graph contains any cycles.
    pub fn has_cycles(&self) -> bool {
        is_cyclic_directed(&self.graph)
    }

    /// Find all cyclic components (strongly connected components with size > 1 or self-loops).
    pub fn find_cycles(&self) -> Vec<Vec<String>> {
        let sccs = petgraph::algo::tarjan_scc(&self.graph);
        sccs.into_iter()
            .filter(|scc| {
                if scc.len() > 1 {
                    true
                } else if scc.len() == 1 {
                    let node = scc[0];
                    self.graph.contains_edge(node, node)
                } else {
                    false
                }
            })
            .map(|scc| {
                scc.into_iter()
                    .map(|idx| self.graph[idx].to_string())
                    .collect()
            })
            .collect()
    }

    /// Get all executables that `dllname` directly depends on.
    pub fn direct_dependencies(&self, dllname: &str) -> Vec<&Executable> {
        let dll_key = DllName::from(dllname);
        if let Some(&node_idx) = self.node_indices.get(&dll_key) {
            self.graph
                .neighbors_directed(node_idx, Direction::Outgoing)
                .filter_map(|dep_idx| {
                    let dep_key = &self.graph[dep_idx];
                    self.index.get(dep_key)
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get all executables that directly depend on `dllname` (reverse dependencies).
    pub fn reverse_dependencies(&self, dllname: &str) -> Vec<&Executable> {
        let dll_key = DllName::from(dllname);
        if let Some(&node_idx) = self.node_indices.get(&dll_key) {
            self.graph
                .neighbors_directed(node_idx, Direction::Incoming)
                .filter_map(|parent_idx| {
                    let parent_key = &self.graph[parent_idx];
                    self.index.get(parent_key)
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get all executables transitively reachable from `dllname` in breadth-first order.
    pub fn transitive_dependencies(&self, dllname: &str) -> Vec<&Executable> {
        let dll_key = DllName::from(dllname);
        let Some(&start_idx) = self.node_indices.get(&dll_key) else {
            return Vec::new();
        };

        let mut bfs = petgraph::visit::Bfs::new(&self.graph, start_idx);
        let mut result = Vec::new();

        // Skip start node itself
        let _ = bfs.next(&self.graph);

        while let Some(nx) = bfs.next(&self.graph) {
            let key = &self.graph[nx];
            if let Some(exe) = self.index.get(key) {
                result.push(exe);
            }
        }
        result
    }

    /// Get all executables that transitively depend on `dllname` (transitive reverse dependencies).
    pub fn transitive_reverse_dependencies(&self, dllname: &str) -> Vec<&Executable> {
        let dll_key = DllName::from(dllname);
        let Some(&start_idx) = self.node_indices.get(&dll_key) else {
            return Vec::new();
        };

        let reversed = petgraph::visit::Reversed(&self.graph);
        let mut bfs = petgraph::visit::Bfs::new(&reversed, start_idx);
        let mut result = Vec::new();

        // Skip start node itself
        let _ = bfs.next(&reversed);

        while let Some(nx) = bfs.next(&reversed) {
            let key = &self.graph[nx];
            if let Some(exe) = self.index.get(key) {
                result.push(exe);
            }
        }
        result
    }

    /// Find the shortest path between `from` and `to` binaries in the dependency graph.
    pub fn shortest_path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        let from_key = DllName::from(from);
        let to_key = DllName::from(to);
        let start_idx = *self.node_indices.get(&from_key)?;
        let target_idx = *self.node_indices.get(&to_key)?;

        if start_idx == target_idx {
            return Some(vec![self.graph[start_idx].to_string()]);
        }

        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut predecessor = HashMap::new();

        queue.push_back(start_idx);
        visited.insert(start_idx);

        let mut found = false;
        while let Some(curr) = queue.pop_front() {
            if curr == target_idx {
                found = true;
                break;
            }
            for neighbor in self.graph.neighbors_directed(curr, Direction::Outgoing) {
                if visited.insert(neighbor) {
                    predecessor.insert(neighbor, curr);
                    queue.push_back(neighbor);
                }
            }
        }

        if !found {
            return None;
        }

        let mut path = Vec::new();
        let mut curr = target_idx;
        while curr != start_idx {
            path.push(self.graph[curr].to_string());
            curr = *predecessor.get(&curr)?;
        }
        path.push(self.graph[start_idx].to_string());
        path.reverse();
        Some(path)
    }

    /// Find the shortest path from the root executable to each missing (unresolved) dependency.
    pub fn shortest_missing_paths(&self) -> HashMap<String, Vec<String>> {
        let mut result = HashMap::new();
        let root = match self.get_root() {
            Ok(Some(r)) => &r.dllname,
            _ => return result,
        };

        for exe in self.index.values() {
            if !exe.found {
                if let Some(path) = self.shortest_path(root, &exe.dllname) {
                    result.insert(exe.dllname.clone(), path);
                }
            }
        }
        result
    }

    /// Filter the graph to retain only missing executables and all ancestors on paths leading to them.
    pub fn filter_only_notfound(&self) -> Result<Executables, LookupError> {
        let mut ret = Executables::new();

        let root = match self.get_root()? {
            Some(r) => r,
            None => return Ok(ret),
        };

        let missing_exes: Vec<&Executable> = self.index.values().filter(|e| !e.found).collect();
        if missing_exes.is_empty() {
            return Ok(ret);
        }

        let mut included = HashSet::new();
        for missing in &missing_exes {
            included.insert(DllName::from(&missing.dllname));
            for ancestor in self.transitive_reverse_dependencies(&missing.dllname) {
                included.insert(DllName::from(&ancestor.dllname));
            }
        }

        if included.contains(&DllName::from(&root.dllname)) {
            for dll_key in &included {
                if let Some(exe) = self.index.get(dll_key) {
                    ret.insert(exe.clone());
                }
            }
        }

        Ok(ret)
    }

    /// Check that all referenced DLLs are found, and (if available) that imported symbols are present
    pub fn check(&self, extract_symbols: bool) -> Result<ExecutablesCheckReport, LookupError> {
        let mut report = ExecutablesCheckReport::new();

        if extract_symbols {
            let symbols_report = self
                .index
                .values()
                .map(|e| self.check_imports(&e.dllname))
                .fold(ExecutablesCheckReport::new(), |mut r, pr| {
                    if let Ok(rr) = pr {
                        r.extend(rr);
                    }
                    r
                });
            report.extend(symbols_report);
        }

        Ok(report)
    }

    /// Check that every dependency exports the symbols imported by this file
    fn check_imports(&self, name: &str) -> Result<ExecutablesCheckReport, LookupError> {
        let exe = self
            .get(name)
            .ok_or_else(|| LookupError::ScanError(format!("Could not find file {name}")))?;

        if exe.details.as_ref().map(|d| d.is_api_set).unwrap_or(true) {
            return Ok(ExecutablesCheckReport::new());
        }

        let imported_symbols = &exe
            .details
            .as_ref()
            .ok_or_else(|| {
                LookupError::ScanError(format!("Could not find details for file {name}"))
            })?
            .symbols
            .as_ref()
            .ok_or_else(|| {
                LookupError::ScanError(format!("Could not find symbols for file {name}"))
            })?
            .imported;

        let mut missing_imports = ExecutablesCheckReport::new();

        for dll_name in imported_symbols.keys() {
            if let Some(dll_exe) = self.get(dll_name) {
                if dll_exe.found {
                    if !dll_exe
                        .details
                        .as_ref()
                        .map(|d| d.is_system)
                        .unwrap_or(true)
                    {
                        let res = self.check_symbols(name, dll_name)?;
                        missing_imports.extend(res);
                    }
                } else {
                    missing_imports
                        .not_found_libraries
                        .entry(name.to_owned())
                        .or_default()
                        .insert(dll_name.clone());
                }
            }
        }

        Ok(missing_imports)
    }

    /// Check that the exporting DLL has all symbols imported by the importing executable file
    fn check_symbols(
        &self,
        importer: &str,
        exporter: &str,
    ) -> Result<ExecutablesCheckReport, LookupError> {
        let exe = self
            .get(importer)
            .ok_or_else(|| LookupError::ScanError(format!("Could not find file {importer}")))?;
        let imported_symbols = &exe
            .details
            .as_ref()
            .ok_or_else(|| {
                LookupError::ScanError(format!("Could not find details for file {importer}"))
            })?
            .symbols
            .as_ref()
            .ok_or_else(|| {
                LookupError::ScanError(format!("Could not find symbols for file {importer}"))
            })?
            .imported;
        let imported_symbols_this_dep = imported_symbols
            .iter()
            .find(|(k, _)| DllName::from(*k) == DllName::from(exporter))
            .map(|(_, v)| v)
            .ok_or_else(|| {
                LookupError::ScanError(format!(
                    "Could not find list of symbols imported by {importer} from {exporter}"))
            })?;

        let dep_exe = self
            .get(exporter)
            .ok_or_else(|| LookupError::ScanError(format!("Could not find file {exporter}")))?;
        let exported_symbols = &dep_exe
            .details
            .as_ref()
            .ok_or_else(|| {
                LookupError::ScanError(format!("Could not find details for file {exporter}"))
            })?
            .symbols
            .as_ref()
            .ok_or_else(|| {
                LookupError::ScanError(format!("Could not find symbols for file {exporter}"))
            })?
            .exported;

        let mut missing_symbols: HashSet<String> = HashSet::new();

        for d in imported_symbols_this_dep {
            if !exported_symbols.contains(d) {
                missing_symbols.insert(d.clone());
            }
        }

        let not_found_symbols = if missing_symbols.is_empty() {
            None
        } else {
            Some(
                vec![(
                    importer.to_owned(),
                    vec![(exporter.to_owned(), missing_symbols)]
                        .into_iter()
                        .collect(),
                )]
                    .into_iter()
                    .collect(),
            )
        };

        Ok(ExecutablesCheckReport {
            not_found_libraries: HashMap::new(),
            not_found_symbols,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::iter::FromIterator;
    use std::path::PathBuf;

    use fs_err as fs;

    use crate::common::LookupError;
    use crate::executable::{DllName, Executable, ExecutableDetails, Executables};
    use crate::path::LookupPath;
    use crate::query::LookupQuery;
    use crate::runner::run;

    #[test]
    fn test_dll_name_case_insensitivity() {
        let name1 = DllName::new("Kernel32.dll");
        let name2 = DllName::new("KERNEL32.DLL");
        let name3 = DllName::new("user32.dll");

        assert_eq!(name1, name2);
        assert_ne!(name1, name3);
        assert_eq!(name1, "kernel32.dll");
        assert_eq!(name1, "KERNEL32.DLL");

        let mut set = HashSet::new();
        set.insert(name1.clone());
        assert!(set.contains(&DllName::new("kernel32.dll")));
    }

    #[test]
    fn empty_executables() -> Result<(), LookupError> {
        let exes = Executables::new();
        assert!(!exes.contains("NonExistingExecutable.exe"));

        assert!(exes.get("NonExistingExecutable.exe").is_none());

        assert!(exes.get_root()?.is_none());

        assert!(exes.sorted_by_first_appearance().is_empty());

        assert!(!exes.has_cycles());
        assert!(exes.find_cycles().is_empty());

        Ok(())
    }

    #[test]
    fn executables() -> Result<(), LookupError> {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path =
            d.join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");

        let mut query = LookupQuery::deduce_from_executable_location(&exe_path)?;
        query.parameters.skip_system_dlls = true;
        let context = LookupPath::deduce(&query);
        let exes = run(&query, &context)?;

        assert!(exes.contains("DepRunTest.exe"));
        assert!(exes.contains("depruntest.exe"));
        assert!(!exes.contains("NonExistingExecutable.exe"));

        assert!(exes.get("NonExistingExecutable.exe").is_none());
        assert!(exes.get("DepRunTest.exe").is_some());

        assert_eq!(exes.get_root()?.unwrap().dllname, "DepRunTest.exe");

        let sorted = exes.sorted_by_first_appearance();
        let sorted_names: HashSet<&str> = sorted
            .iter()
            .filter(|e| e.details.as_ref().map(|d| !d.is_system).unwrap_or(false))
            .map(|e| e.dllname.as_ref())
            .collect();
        let expected_names: HashSet<&str> =
            HashSet::from_iter(["DepRunTestLib.dll", "DepRunTest.exe"].iter().copied());
        assert_eq!(sorted_names, expected_names);

        let exe_p = &exes
            .get_root()?
            .unwrap()
            .details
            .as_ref()
            .unwrap()
            .full_path;
        assert_eq!(exe_p, &fs::canonicalize(exe_path)?);

        // Test graph APIs on resolved executable
        assert!(!exes.has_cycles());
        let toposorted = exes.topological_sort()?;
        assert!(!toposorted.is_empty());

        let rev_deps = exes.reverse_dependencies("DepRunTestLib.dll");
        assert!(rev_deps.iter().any(|e| e.dllname == "DepRunTest.exe"));

        let dir_deps = exes.direct_dependencies("DepRunTest.exe");
        assert!(dir_deps.iter().any(|e| e.dllname == "DepRunTestLib.dll"));

        let path = exes.shortest_path("DepRunTest.exe", "DepRunTestLib.dll");
        assert_eq!(
            path,
            Some(vec![
                "DepRunTest.exe".to_string(),
                "DepRunTestLib.dll".to_string()
            ])
        );

        Ok(())
    }

    #[test]
    fn test_graph_cycle_detection_and_missing_paths() -> Result<(), LookupError> {
        let mut exes = Executables::new();

        // Node A (root) depends on B and C
        exes.insert(Executable {
            dllname: "A.exe".to_string(),
            depth_first_appearance: 0,
            found: true,
            details: Some(ExecutableDetails {
                is_api_set: false,
                is_system: false,
                is_known_dll: false,
                full_path: PathBuf::from("A.exe"),
                dependencies: Some(vec!["B.dll".to_string(), "C.dll".to_string()]),
                symbols: None,
            }),
        });

        // Node B depends on D (missing)
        exes.insert(Executable {
            dllname: "B.dll".to_string(),
            depth_first_appearance: 1,
            found: true,
            details: Some(ExecutableDetails {
                is_api_set: false,
                is_system: false,
                is_known_dll: false,
                full_path: PathBuf::from("B.dll"),
                dependencies: Some(vec!["D.dll".to_string()]),
                symbols: None,
            }),
        });

        // Node C is found and has no dependencies
        exes.insert(Executable {
            dllname: "C.dll".to_string(),
            depth_first_appearance: 1,
            found: true,
            details: Some(ExecutableDetails {
                is_api_set: false,
                is_system: false,
                is_known_dll: false,
                full_path: PathBuf::from("C.dll"),
                dependencies: Some(vec![]),
                symbols: None,
            }),
        });

        // Node D is missing
        exes.insert(Executable {
            dllname: "D.dll".to_string(),
            depth_first_appearance: 2,
            found: false,
            details: None,
        });

        assert!(!exes.has_cycles());
        let missing_paths = exes.shortest_missing_paths();
        assert_eq!(
            missing_paths.get("D.dll"),
            Some(&vec![
                "A.exe".to_string(),
                "B.dll".to_string(),
                "D.dll".to_string()
            ])
        );

        let filtered = exes.filter_only_notfound()?;
        assert!(filtered.contains("A.exe"));
        assert!(filtered.contains("B.dll"));
        assert!(filtered.contains("D.dll"));
        assert!(!filtered.contains("C.dll"));

        // Now introduce a cycle: D (or new node E) -> A
        let mut cyclic_exes = exes.clone();
        cyclic_exes.insert(Executable {
            dllname: "E.dll".to_string(),
            depth_first_appearance: 3,
            found: true,
            details: Some(ExecutableDetails {
                is_api_set: false,
                is_system: false,
                is_known_dll: false,
                full_path: PathBuf::from("E.dll"),
                dependencies: Some(vec!["A.exe".to_string()]),
                symbols: None,
            }),
        });
        // Connect B -> E
        cyclic_exes.insert(Executable {
            dllname: "B_cycle.dll".to_string(),
            depth_first_appearance: 1,
            found: true,
            details: Some(ExecutableDetails {
                is_api_set: false,
                is_system: false,
                is_known_dll: false,
                full_path: PathBuf::from("B_cycle.dll"),
                dependencies: Some(vec!["E.dll".to_string()]),
                symbols: None,
            }),
        });

        // Add edge A -> B_cycle -> E -> A
        let mut cycle_root = Executables::new();
        cycle_root.insert(Executable {
            dllname: "X.dll".to_string(),
            depth_first_appearance: 0,
            found: true,
            details: Some(ExecutableDetails {
                is_api_set: false,
                is_system: false,
                is_known_dll: false,
                full_path: PathBuf::from("X.dll"),
                dependencies: Some(vec!["Y.dll".to_string()]),
                symbols: None,
            }),
        });
        cycle_root.insert(Executable {
            dllname: "Y.dll".to_string(),
            depth_first_appearance: 1,
            found: true,
            details: Some(ExecutableDetails {
                is_api_set: false,
                is_system: false,
                is_known_dll: false,
                full_path: PathBuf::from("Y.dll"),
                dependencies: Some(vec!["X.dll".to_string()]),
                symbols: None,
            }),
        });

        assert!(cycle_root.has_cycles());
        assert!(cycle_root.topological_sort().is_err());
        let cycles = cycle_root.find_cycles();
        assert!(!cycles.is_empty());

        Ok(())
    }
}
