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
    pub(crate) fn register_finding(&mut self, found: LookupResult) {
        if self.executables_found.contains(&found.name) {
            if found.folder != self.executables_found.get(&found.name).unwrap().folder {
                panic!(
                    "Found two DLLs with the same name! {:?} and {:?}",
                    found.folder,
                    self.executables_found.get(&found.name).unwrap().folder
                )
            }
        } else {
            self.executables_found.insert(&found.name.clone(), found);
        }
    }
}
