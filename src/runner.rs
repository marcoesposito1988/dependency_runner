//! Routine to perform a recursive lookup according to the parameters in the user-provided query and
//! the lookup path computed from it (and eventually adjusted by the user)

use crate::common::{readable_canonical_path, LookupError};
use crate::executable::{Executable, ExecutableDetails, ExecutableSymbols, Executables};
use crate::path::{LookupPath, LookupPathEntry};
use crate::pe;
use crate::query::LookupQuery;

#[derive(Debug)]
struct Job {
    pub dllname: String,
    pub depth: usize,
}

/// Find the dependencies of the specified executable within the given path
/// The dependencies are resolved recursively, in a breadth-first fashion.
pub fn run(query: &LookupQuery, lookup_path: &LookupPath) -> Result<Executables, LookupError> {
    let mut executables_to_lookup: Vec<Job> = Vec::new();
    let mut executables_found = Executables::new();

    let filename = query
        .target
        .target_exe
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            LookupError::ScanError(
                "could not open file ".to_owned() + query.target.target_exe.to_str().unwrap_or(""),
            )
        })?
        .to_owned();

    executables_to_lookup.push(Job {
        dllname: filename,
        depth: 0,
    });

    while let Some(lookup_query) = executables_to_lookup.pop() {
        if lookup_query.depth <= query.parameters.max_depth.unwrap_or(usize::MAX) {
            // don't search again if we already found the executable
            if executables_found.contains(&lookup_query.dllname) {
                continue;
            }
            if let Some(r) = lookup_path
                .search_dll(&lookup_query.dllname, &query.system)
                .ok()
                .flatten()
            {
                let pefilemap = pe::PEFileMap::new(&r.fullpath)?;
                let pefile = pe::PEFile::new(&pefilemap)?;

                let dllname = pefile
                    .read_dll_name()
                    .unwrap_or_else(|_| lookup_query.dllname.clone());
                let is_system = r.location.is_system();
                let is_api_set = std::matches!(r.location, LookupPathEntry::ApiSet);
                let is_known_dll = std::matches!(r.location, LookupPathEntry::KnownDLLs);
                let dependencies = if is_api_set {
                    query
                        .system
                        .as_ref()
                        .and_then(|s| s.apiset_map.as_ref())
                        .and_then(|am| am.get(dllname.trim_end_matches(".dll")).cloned())
                } else if r.location.is_system()
                    && !std::matches!(r.location, LookupPathEntry::ApiSet)
                {
                    // system DLLs have just too many dependencies
                    None
                } else {
                    Some(pefile.read_dependencies()?)
                };
                let symbols = if !is_api_set && query.parameters.extract_symbols {
                    let exported = pefile.read_exports();
                    let imported = pefile.read_imports();
                    if let (Ok(exported), Ok(imported)) = (exported, imported) {
                        Some(ExecutableSymbols {
                            exported,
                            imported,
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
                executables_found.insert(Executable {
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
                });
            } else {
                executables_found.insert(Executable {
                    dllname: lookup_query.dllname,
                    depth_first_appearance: lookup_query.depth,
                    found: false,
                    details: None,
                });
            }
        }
    }

    Ok(executables_found)
}

#[cfg(test)]
mod tests {
    use crate::common::LookupError;
    use crate::path::LookupPath;
    use crate::query::LookupQuery;
    use crate::runner::run;
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
            HashSet::from_iter(["DepRunTestLib.dll", "DepRunTest.exe"].iter().copied());
        assert_eq!(sorted_names, expected_names);

        Ok(())
    }
}
