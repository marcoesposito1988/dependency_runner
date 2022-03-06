use crate::common::LookupError;
use crate::executable::{Executable, ExecutableDetails, ExecutableSymbols, Executables};
use crate::path::{LookupPath, LookupPathEntry};
use crate::{pe, readable_canonical_path, LookupQuery};

#[derive(Debug)]
struct Job {
    pub dllname: String,
    pub depth: usize,
}

/// Finds the dependencies of the specified executable within the given context
/// The dependencies are resolved recursively, in a breadth-first fashion.

pub fn run(query: &LookupQuery, lookup_path: &LookupPath) -> Result<Executables, LookupError> {
    let mut executables_to_lookup: Vec<Job> = Vec::new();
    let mut executables_found = Executables::new();

    let filename = query
        .target
        .target_exe
        .file_name()
        .map(|s| s.to_str())
        .flatten()
        .ok_or(LookupError::ScanError(
            "could not open file ".to_owned() + query.target.target_exe.to_str().unwrap_or(""),
        ))?
        .to_owned();

    executables_to_lookup.push(Job {
        dllname: filename.to_owned(),
        depth: 0,
    });

    while let Some(lookup_query) = executables_to_lookup.pop() {
        if lookup_query.depth <= query.parameters.max_depth.unwrap_or(usize::MAX) {
            // don't search again if we already found the executable
            if executables_found.contains(&lookup_query.dllname) {
                continue;
            }
            if let Some(r) = lookup_path
                .search_dll(&lookup_query.dllname)
                .unwrap_or(None)
            {
                let filemap =
                    pelite::FileMap::open(&r.fullpath).map_err(|e| LookupError::IOError(e))?;
                let pefile = pelite::pe64::PeFile::from_bytes(&filemap)
                    .map_err(|e| LookupError::PEError(e))?;

                let dllname = pe::read_dll_name(&pefile).unwrap_or(lookup_query.dllname.clone());
                let is_system = r.location.is_system();
                let is_api_set = std::matches!(r.location, LookupPathEntry::ApiSet(_));
                let is_known_dll = std::matches!(r.location, LookupPathEntry::KnownDLLs(_));
                let dependencies = if is_api_set {
                    query
                        .system
                        .as_ref()
                        .and_then(|s| s.apiset_map.as_ref())
                        .map(|am| am.get(dllname.trim_end_matches(".dll")).cloned())
                        .flatten()
                } else {
                    if r.location.is_system()
                        && !std::matches!(r.location, LookupPathEntry::ApiSet(_))
                    {
                        // system DLLs have just too many dependencies
                        None
                    } else {
                        Some(pe::read_dependencies(&pefile)?)
                    }
                };
                let symbols = if !is_api_set && query.parameters.extract_symbols {
                    let exported = pe::read_exports(&pefile);
                    let imported = pe::read_imports(&pefile);
                    if exported.is_ok() && imported.is_ok() {
                        Some(ExecutableSymbols {
                            exported: exported.unwrap(),
                            imported: imported.unwrap(),
                        })
                    } else {
                        eprintln!(
                            "Error extracting symbols of library {}",
                            readable_canonical_path(&r.fullpath)?
                        );
                        None
                    }
                } else {
                    None
                };

                if let Some(deps) = &dependencies {
                    for d in deps {
                        if !executables_found.contains(d.as_ref()) {
                            executables_to_lookup.push(Job {
                                dllname: d.to_owned(),
                                depth: lookup_query.depth + 1,
                            })
                        }
                    }
                }
                register_finding(
                    &mut executables_found,
                    Executable {
                        dllname,
                        depth_first_appearance: lookup_query.depth,
                        found: true,
                        details: Some(ExecutableDetails {
                            is_api_set,
                            is_system,
                            is_known_dll,
                            full_path: r.fullpath,
                            dependencies,
                            symbols,
                        }),
                    },
                );
            } else {
                register_finding(
                    &mut executables_found,
                    Executable {
                        dllname: lookup_query.dllname,
                        depth_first_appearance: lookup_query.depth,
                        found: false,
                        details: None,
                    },
                );
            }
        }
    }

    Ok(executables_found)
}

fn register_finding(executables_found: &mut Executables, new_finding: Executable) {
    if let Some(older_finding) = executables_found.get(&new_finding.dllname) {
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
        executables_found.insert(new_finding);
    }
}

#[cfg(test)]
mod tests {
    use crate::path::LookupPath;
    use crate::query::LookupQuery;
    use crate::runner::run;
    use crate::LookupError;
    use std::collections::HashSet;
    use std::iter::FromIterator;

    #[test]
    fn run_build_same_output() -> Result<(), LookupError> {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path =
            d.join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");

        let mut query = LookupQuery::deduce_from_executable_location(exe_path)?;
        query.parameters.skip_system_dlls = true;
        let lookup_path = LookupPath::deduce(&query);
        let res = run(&query, &lookup_path)?;
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
