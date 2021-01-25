use crate::{Executable, Executables, LookupError};
use std::ffi::OsString;

// tree view of nodes referencing Executables in a LookupResult
// this is necessary for the QAbstractItemModel, because that requires that every node has a single parent
// in our Executables DAG, a node can have multiple parents (and appear at multiple depths)
// this class just provides a reified tree view of the DAG

pub struct ExecutablesTreeNode {
    pub name: String,
    pub parent: Option<String>,
    pub depth: usize,
    pub dependencies: Vec<String>,
}

// ordered depth-first: root is first node
pub struct ExecutablesTreeView {
    pub arena: Vec<ExecutablesTreeNode>,
    pub index: std::collections::HashMap<String, usize>,
    pub executables: Executables,
}

impl ExecutablesTreeView {
    fn add_to_arena(
        &mut self,
        parent: Option<OsString>,
        depth: usize,
        lr: &Executable,
        exes: &Executables,
    ) {
        if let Some(name) = lr.name.to_str() {
            let this_index = self.arena.len();
            self.arena.push(ExecutablesTreeNode {
                name: name.to_owned(),
                depth,
                parent: parent.map(|p| p.to_str().unwrap_or("INVALID").to_owned()),
                dependencies: Vec::new(), // will fill this later in new()
            });

            let mut this_deps: Vec<String> = Vec::new();

            if let Some(dependencies) = &lr
                .details
                .as_ref()
                .map(|det| &det.dependencies)
                .unwrap_or(&None)
            {
                for dep in dependencies {
                    let dep_lr: OsString = dep.to_lowercase().into();
                    // if let Some(dep_lr) = exes.get(&dep.to_lowercase()) {
                    if let Some(dep_lr) = exes.get(&dep_lr) {
                        self.add_to_arena(Some(lr.name.clone()), depth + 1, dep_lr, exes);
                        this_deps.push(dep.clone());
                    }
                }
            }

            self.arena[this_index].dependencies = this_deps;
            self.index.insert(name.to_owned(), this_index);
        }
    }

    pub fn new(exes: &Executables) -> Result<Self, LookupError> {
        let mut ret = Self {
            arena: Vec::new(),
            index: std::collections::HashMap::new(),
            executables: exes.clone(),
        };

        if let Some(root) = exes.get_root()? {
            ret.add_to_arena(None, 0, root, &exes);
        }

        Ok(ret)
    }

    pub fn visit_depth_first(&self, f: impl Fn(&ExecutablesTreeNode) -> ()) {
        // the arena currently holds a depth-first linearization of the tree
        for n in &self.arena {
            f(n)
        }
    }
}
