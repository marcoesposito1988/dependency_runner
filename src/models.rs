use crate::{Executable, LookupResult};

// tree view of nodes referencing Executables in a LookupResult
// this is necessary for the QAbstractItemModel, because that requires that every node has a single parent
// in our Executables DAG, a node can have multiple parents (and appear at multiple depths)
// this class just provides a reified tree view of the DAG

pub struct LookupResultTreeNode {
    pub name: String,
    pub parent: Option<String>,
    pub depth: usize,
    pub dependencies: Vec<String>,
}

// ordered depth-first: root is first node
pub struct LookupResultTreeView {
    pub arena: Vec<LookupResultTreeNode>,
    pub index: std::collections::HashMap<String, usize>,
    pub executables: LookupResult,
}

impl LookupResultTreeView {
    fn add_to_arena(
        &mut self,
        parent: Option<String>,
        depth: usize,
        lr: &Executable,
        exes: &LookupResult,
    ) {
        let this_index = self.arena.len();
        self.arena.push(LookupResultTreeNode {
            name: lr.name.clone(),
            depth,
            parent,
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
                if let Some(dep_lr) = exes.get(&dep.to_lowercase()) {
                    self.add_to_arena(Some(lr.name.clone()), depth + 1, dep_lr, exes);
                    this_deps.push(dep.clone());
                }
            }
        }

        self.arena[this_index].dependencies = this_deps;
        self.index.insert(lr.name.clone(), this_index);
    }

    pub fn new(exes: &LookupResult) -> Self {
        let root_nodes: Vec<&Executable> = exes
            .values()
            .filter(|le| le.depth_first_appearance == 0)
            .collect();

        if root_nodes.len() > 1 {
            panic!("Found multiple root nodes in the Executables");
            // TODO: list found root nodes, proper error handling
        }

        if root_nodes.len() == 0 {
            panic!("No root node found in the Executables");
            // TODO: list found root nodes, proper error handling
        }

        let root_node = root_nodes.first().unwrap();

        let mut ret = Self {
            arena: Vec::new(),
            index: std::collections::HashMap::new(),
            executables: exes.clone(),
        };

        ret.add_to_arena(None, 0, root_node, &exes);

        ret
    }

    pub fn visit_depth_first(&self, f: impl Fn(&LookupResultTreeNode) -> ()) {
        // the arena currently holds a depth-first linearization of the tree
        for n in &self.arena {
            f(n)
        }
    }
}
