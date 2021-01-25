use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::common::{read_dependencies, LookupError};
use crate::executable::{ExecutableDetails, Executable, Executables};
use crate::lookup_path::{LookupPath};
use crate::query::LookupQuery;

#[derive(Debug)]
struct Job {
    pub name: OsString,
    pub depth: usize,
}

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
            executables_found: Executables::new(),
        }
    }

    // the user enqueues an executable; the workers enqueue the dependencies of those that were found
    // (skip the dependencies that have already been found)
    fn enqueue(&mut self, executable_name: &OsStr, depth: usize) {
        if !self.executables_found.contains(executable_name) {
            self.executables_to_lookup.push(Job {
                name: executable_name.to_owned(),
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
        if let Some(older_finding) = self.executables_found.get(&new_finding.name) {
            eprintln!(
                "Found two DLLs with the same name! {:?} and {:?}",
                new_finding
                    .full_path()
                    .unwrap_or(PathBuf::from(new_finding.name)),
                older_finding
                    .full_path()
                    .unwrap_or(PathBuf::from(&older_finding.name))
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
            .ok_or(LookupError::ScanError(
                "could not open file ".to_owned() + self.query.target_exe.to_str().unwrap_or(""),
            ))?
            .to_owned();

        self.enqueue(&filename, 0);

        while let Some(lookup_query) = self.pop() {
            if lookup_query.depth <= self.query.max_depth.unwrap_or(9999) {
                let executable = lookup_query.name;
                let depth = lookup_query.depth;
                // don't search again if we already found the executable
                if self.executables_found.contains(&executable) {
                    continue;
                }
                if let Some(r) = self.context.search_file(&executable).unwrap_or(None) {
                    let folder = r.fullpath.parent().unwrap();
                    let actual_name = Path::new(&r.fullpath).file_name().unwrap_or("".as_ref());
                    let is_system = r.location.is_system();

                    if let Ok(dependencies) = read_dependencies(&r.fullpath) {
                        if !(self.query.skip_system_dlls && is_system) {
                            for d in &dependencies {
                                let dos = OsString::from(d);
                                self.enqueue(&dos, depth + 1);
                            }
                        }

                        self.register_finding(Executable {
                            name: actual_name.to_owned(),
                            depth_first_appearance: depth,
                            found: true,
                            details: Some(ExecutableDetails {
                                is_system,
                                is_known_dll: false, // TODO
                                folder: folder.into(),
                                dependencies: Some(dependencies),
                            }),
                        });
                    } else {
                        self.register_finding(Executable {
                            name: executable.clone(),
                            depth_first_appearance: depth,
                            found: true,
                            details: Some(ExecutableDetails {
                                is_system,
                                is_known_dll: false, // TODO
                                folder: folder.into(),
                                dependencies: None,
                            }),
                        });
                    }
                } else {
                    self.register_finding(Executable {
                        name: executable.clone(),
                        depth_first_appearance: depth,
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
    use crate::LookupError;
    use crate::query::LookupQuery;
    use crate::lookup_path::LookupPath;
    use crate::runner::Runner;
    use std::collections::HashSet;
    use std::iter::FromIterator;

    #[test]
    fn run_build_same_output() -> Result<(), LookupError> {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = d.join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");

        let mut query = LookupQuery::deduce_from_executable_location(exe_path)?;
        query.skip_system_dlls = true;
        let context = LookupPath::new(&query);
        let mut runner = Runner::new(&query, context);
        let res = runner.run()?;
        let sorted = res.sorted_by_first_appearance();
        let sorted_names: HashSet<&str> = sorted.iter()
            .filter(|e| e.details.as_ref().map(|d| !d.is_system).unwrap_or(false))
            .map(|e| e.name.to_str().unwrap()).collect();
        let expected_names: HashSet<&str> = HashSet::from_iter(
            ["DepRunTestLib.dll", "DepRunTest.exe",].iter().map(|&s|s));
        assert_eq!(sorted_names, expected_names);

        Ok(())
    }
}
