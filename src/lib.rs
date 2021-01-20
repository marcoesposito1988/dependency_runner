extern crate thiserror;

use crate::workqueue::Workqueue;
use pelite::pe64::{Pe, PeFile};
use std::path::Path;

mod common;
mod path;
mod workqueue;

pub mod context;
pub mod models;

use crate::common::Details;
pub use crate::common::{Executables, LookupError, LookupQuery, LookupResult};
pub use crate::context::LookupContext;

pub fn lookup_executable_dependencies<P: AsRef<Path> + ?Sized>(
    path: &P,
) -> Result<Vec<String>, LookupError> {
    use LookupError::{CouldNotOpenFile, ProcessingError};
    let path = path.as_ref();
    let map = pelite::FileMap::open(path).map_err(|e| CouldNotOpenFile { source: e })?;
    let file = PeFile::from_bytes(&map).map_err(|e| ProcessingError { source: e })?;

    // Access the import directory
    let imports = file.imports().map_err(|e| ProcessingError { source: e })?;

    let names: Vec<&pelite::util::CStr> = imports
        .iter()
        .map(|desc| desc.dll_name())
        .collect::<Result<Vec<&pelite::util::CStr>, pelite::Error>>()
        .map_err(|e| ProcessingError { source: e })?;
    Ok(names
        .iter()
        .filter_map(|s| s.to_str().ok())
        .map(|s| s.to_string())
        .collect::<Vec<String>>())
}

pub fn lookup_executable_dependencies_recursive(
    filename: &str,
    context: &LookupContext,
    max_depth: usize,
    skip_system_dlls: bool,
) -> Result<Executables, LookupError> {
    let mut workqueue = Workqueue::new();
    workqueue.enqueue(filename, 0);

    while let Some(lookup_query) = workqueue.pop() {
        if lookup_query.depth <= max_depth {
            let executable = lookup_query.name;
            let depth = lookup_query.depth;
            // don't search again if we already found the executable
            if workqueue.executables_found.contains(&executable) {
                continue;
            }
            if let Some(fullpath) = path::search_file(&executable, &context).unwrap_or(None) {
                let folder = Path::new(&fullpath).parent().unwrap().to_str().unwrap();
                let actual_name = Path::new(&fullpath).file_name().unwrap().to_str().unwrap();
                let is_system = context.is_system_dir(folder);

                if let Ok(dependencies) = lookup_executable_dependencies(&fullpath) {
                    if !(skip_system_dlls && is_system) {
                        for d in &dependencies {
                            workqueue.enqueue(d, depth + 1);
                        }
                    }

                    workqueue.register_finding(LookupResult {
                        name: actual_name.to_owned(),
                        depth_first_appearance: depth,
                        found: true,
                        details: Some(Details {
                            is_system,
                            folder: folder.to_owned(),
                            dependencies: Some(dependencies),
                        }),
                    });
                } else {
                    workqueue.register_finding(LookupResult {
                        name: executable.clone(),
                        depth_first_appearance: depth,
                        found: true,
                        details: Some(Details {
                            is_system,
                            folder: folder.to_owned(),
                            dependencies: None,
                        }),
                    });
                }
            } else {
                workqueue.register_finding(LookupResult {
                    name: executable.clone(),
                    depth_first_appearance: depth,
                    found: false,
                    details: None,
                });
            }
        }
    }

    Ok(workqueue.executables_found)
}
