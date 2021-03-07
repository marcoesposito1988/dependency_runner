use crate::common::LookupError;
use crate::executable::{Executable, ExecutableDetails, ExecutableSymbols, Executables};
use crate::lookup_path::{LookupPath, LookupPathEntry};
use crate::query::LookupQuery;
use crate::{pe, readable_canonical_path};

#[derive(Debug)]
struct Job {
    pub dllname: String,
    pub depth: usize,
}

/// Finds the dependencies of the specified executable within the given context
/// The dependencies are resolved recursively, in a breadth-first fashion.
pub(crate) struct Runner {
    query: LookupQuery,
    context: LookupPath,
    executables_to_lookup: Vec<Job>,
    executables_found: Executables, // using lowercase filename as key, assuming that we can only find a DLL given a name; if this changes, use the path instead
}

impl Runner {
    pub(crate) fn new(query: &LookupQuery, context: LookupPath) -> Self {
        Self {
            context,
            query: query.clone(),
            executables_to_lookup: Vec::new(),
            executables_found: Executables::new(query.clone()),
        }
    }

    // the user enqueues an executable; the workers enqueue the dependencies of those that were found
    // (skip the dependencies that have already been found)
    fn enqueue(&mut self, executable_name: &str, depth: usize) {
        if !self.executables_found.contains(executable_name) {
            self.executables_to_lookup.push(Job {
                dllname: executable_name.to_owned(),
                depth,
            })
        }
    }

    // the workers fetch work to be done (the name of a DLL to be found)
    fn pop(&mut self) -> Option<Job> {
        self.executables_to_lookup.pop()
    }

    // the workers register the executable that was found for the given name; the function checks for uniqueness
    fn register_finding(&mut self, new_finding: Executable) {
        if let Some(older_finding) = self.executables_found.get(&new_finding.dllname) {
            eprintln!(
                "Found two DLLs with the same name! {:?} and {:?}",
                new_finding
                    .details
                    .as_ref()
                    .map(|d| readable_canonical_path(&d.full_path).ok())
                    .flatten()
                    .unwrap_or(new_finding.dllname),
                older_finding
                    .details
                    .as_ref()
                    .map(|d| readable_canonical_path(&d.full_path).ok())
                    .flatten()
                    .unwrap_or(older_finding.dllname.clone()),
            );
        } else {
            self.executables_found.insert(new_finding);
        }
    }

    pub fn run(&mut self) -> Result<Executables, LookupError> {
        let filename = self
            .query
            .target_exe
            .file_name()
            .map(|s| s.to_str())
            .flatten()
            .ok_or(LookupError::ScanError(
                "could not open file ".to_owned() + self.query.target_exe.to_str().unwrap_or(""),
            ))?
            .to_owned();

        self.enqueue(&filename, 0);

        while let Some(lookup_query) = self.pop() {
            if lookup_query.depth <= self.query.max_depth.unwrap_or(usize::MAX) {
                // don't search again if we already found the executable
                if self.executables_found.contains(&lookup_query.dllname) {
                    continue;
                }
                if let Some(r) = self
                    .context
                    .search_dll(&lookup_query.dllname)
                    .unwrap_or(None)
                {
                    let filemap =
                        pelite::FileMap::open(&r.fullpath).map_err(|e| LookupError::IOError(e))?;
                    let pefile = pelite::pe64::PeFile::from_bytes(&filemap)
                        .map_err(|e| LookupError::PEError(e))?;

                    let dllname =
                        pe::read_dll_name(&pefile).unwrap_or(lookup_query.dllname.clone());
                    let is_system = r.location.is_system();
                    let is_api_set = std::matches!(r.location, LookupPathEntry::ApiSet);
                    let dependencies = if is_api_set {
                        self.context
                            .apiset_map
                            .as_ref()
                            .map(|am| am.get(dllname.trim_end_matches(".dll")).cloned())
                            .flatten()
                    } else {
                        if r.location.is_system() && r.location != LookupPathEntry::ApiSet {
                            // system DLLs have just too many dependencies
                            None
                        } else {
                            Some(pe::read_dependencies(&pefile)?)
                        }
                    };
                    let symbols = if !is_api_set && self.query.extract_symbols {
                        Some(ExecutableSymbols {
                            exported: pe::read_exports(&pefile)?,
                            imported: pe::read_imports(&pefile)?,
                        })
                    } else {
                        None
                    };

                    if let Some(deps) = &dependencies {
                        for d in deps {
                            self.enqueue(&d, lookup_query.depth + 1);
                        }
                    }
                    self.register_finding(Executable {
                        dllname,
                        depth_first_appearance: lookup_query.depth,
                        found: true,
                        details: Some(ExecutableDetails {
                            is_api_set,
                            is_system,
                            is_known_dll: false, // TODO
                            full_path: r.fullpath,
                            dependencies,
                            symbols,
                        }),
                    });
                } else {
                    self.register_finding(Executable {
                        dllname: lookup_query.dllname,
                        depth_first_appearance: lookup_query.depth,
                        found: false,
                        details: None,
                    });
                }
            }
        }

        Ok(self.executables_found.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::lookup_path::LookupPath;
    use crate::query::LookupQuery;
    use crate::runner::Runner;
    use crate::LookupError;
    use std::collections::HashSet;
    use std::iter::FromIterator;

    #[test]
    fn run_build_same_output() -> Result<(), LookupError> {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path =
            d.join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");

        let mut query = LookupQuery::deduce_from_executable_location(exe_path)?;
        query.skip_system_dlls = true;
        let context = LookupPath::new(&query);
        let mut runner = Runner::new(&query, context);
        let res = runner.run()?;
        let sorted = res.sorted_by_first_appearance();
        let sorted_names: HashSet<&str> = sorted
            .iter()
            .filter(|e| e.details.as_ref().map(|d| !d.is_system).unwrap_or(false))
            .map(|e| e.dllname.as_ref())
            .collect();
        let expected_names: HashSet<&str> =
            HashSet::from_iter(["DepRunTestLib.dll", "DepRunTest.exe"].iter().map(|&s| s));
        assert_eq!(sorted_names, expected_names);

        Ok(())
    }
}
