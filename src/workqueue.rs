use crate::{Executables, LookupQuery, LookupResult};

pub(crate) struct Workqueue {
    executables_to_lookup: Vec<LookupQuery>,
    pub(crate) executables_found: Executables, // using lowercase filename as key, assuming that we can only find a DLL given a name; if this changes, use the path instead
}

impl Workqueue {
    pub(crate) fn new() -> Self {
        Self {
            executables_to_lookup: Vec::new(),
            executables_found: Executables::new(),
        }
    }

    // the user enqueues an executable; the workers enqueue the dependencies of those that were found
    // (skip the dependencies that have already been found)
    pub(crate) fn enqueue(&mut self, executable_name: &str, depth: usize) {
        if !self.executables_found.contains(executable_name) {
            self.executables_to_lookup.push(LookupQuery {
                name: executable_name.to_string(),
                depth,
            })
        }
    }

    // the workers fetch work to be done (the name of a DLL to be found)
    pub(crate) fn pop(&mut self) -> Option<LookupQuery> {
        self.executables_to_lookup.pop()
    }

    // the workers register the executable that was found for the given name; the function checks for uniqueness
    pub(crate) fn register_finding(&mut self, new_finding: LookupResult) {
        if let Some(older_finding) = self.executables_found.get(&new_finding.name) {
            eprintln!(
                "Found two DLLs with the same name! {:?} and {:?}",
                new_finding.full_path(),
                older_finding.full_path()
            );
        } else {
            self.executables_found
                .insert(&new_finding.name.clone(), new_finding);
        }
    }
}
