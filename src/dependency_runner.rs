#[cfg(windows)]
extern crate winapi;

extern crate pelite;
extern crate serde;
extern crate thiserror;

use pelite::pe64::{Pe, PeFile};
use serde::Serialize;
use std::path::Path;

use std::collections::hash_map::Values;
use std::collections::HashMap;
use std::ffi::OsString;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LookupError {
    #[error("Read error")]
    CouldNotOpenFile { source: std::io::Error },

    #[error("PE file parse error")]
    ProcessingError { source: pelite::Error },

    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    PEError(#[from] pelite::Error),
}

#[cfg(windows)]
fn get_winapi_directory(
    a: unsafe extern "system" fn(
        winapi::um::winnt::LPWSTR,
        winapi::shared::minwindef::UINT,
    ) -> winapi::shared::minwindef::UINT,
) -> Result<String, std::io::Error> {
    use std::io::Error;

    const BFR_SIZE: usize = 512;
    let mut bfr: [u16; BFR_SIZE] = [0; BFR_SIZE];

    let ret: u32 = unsafe { a(bfr.as_mut_ptr(), BFR_SIZE as u32) };
    if ret == 0 {
        Err(Error::last_os_error())
    } else {
        let valid_bfr = &bfr[..ret as usize];
        let valid_str = OsString::from_wide(valid_bfr);
        match valid_str.into_string() {
            Ok(s) => Ok(s),
            Err(_) => Err(Error::new(std::io::ErrorKind::Other, "oh no!")),
        }
    }
}

#[cfg(windows)]
pub fn get_system_directory() -> Result<String, std::io::Error> {
    return get_winapi_directory(winapi::um::sysinfoapi::GetSystemDirectoryW);
}

#[cfg(windows)]
pub fn get_windows_directory() -> Result<String, std::io::Error> {
    return get_winapi_directory(winapi::um::sysinfoapi::GetWindowsDirectoryW);
}

pub fn dlls_imported_by_executable<P: AsRef<Path> + ?Sized>(
    path: &P,
) -> Result<Vec<String>, LookupError> {
    use crate::dependency_runner::LookupError::{CouldNotOpenFile, ProcessingError};
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

#[derive(Debug)]
pub struct LookupContext {
    pub app_dir: String,
    pub sys_dir: String,
    pub win_dir: String,
    pub app_wd: String,
    pub env_path: Vec<String>,
}

impl LookupContext {
    #[cfg(windows)]
    pub fn new(app_dir: &str, app_wd: &str) -> Self {
        let app_dir = app_dir.to_string();
        let sys_dir = get_system_directory().unwrap();
        let win_dir = get_windows_directory().unwrap();
        let app_wd = app_wd.to_string();

        let path_str = std::env::var_os("PATH")
            .unwrap_or(OsString::from(""))
            .to_str()
            .unwrap()
            .to_string();
        let env_path: Vec<String> = path_str.split(";").map(|s| s.to_string()).collect();

        Self {
            app_dir,
            sys_dir,
            win_dir,
            app_wd,
            env_path,
        }
    }

    #[cfg(not(windows))]
    pub fn new(app_dir: &str, sys_dir: &str, win_dir: &str, app_wd: &str) -> Self {
        let app_dir = app_dir.to_string();
        let sys_dir = sys_dir.to_string();
        let win_dir = win_dir.to_string();
        let app_wd = app_wd.to_string();

        let path_str = std::env::var_os("PATH")
            .unwrap_or(OsString::from(""))
            .to_str()
            .unwrap()
            .to_string();
        let env_path: Vec<String> = path_str.split(";").map(|s| s.to_string()).collect();

        Self {
            app_dir,
            sys_dir,
            win_dir,
            app_wd,
            env_path,
        }
    }

    // deduces sensible default values
    // working dir same as app dir, Windows and Windows/System32 in same partition as the exe
    // look for those from the app dir upwards
    // extra path same as current environment (TODO: read from the registry of the partition with hivex?)
    pub fn deduce_from_executable_location(exe_path: &str) -> Result<Self, String> {
        let exe_path = Path::new(exe_path).parent().ok_or("Exe not found")?;

        let app_dir = exe_path.to_str().ok_or("Exe not found")?.to_string();
        let app_wd = app_dir.clone();

        let root_path_candidates: Vec<_> = exe_path
            .ancestors()
            .filter(|p| {
                let mut pbuf = p.to_path_buf();
                pbuf.push("Windows");
                pbuf.push("System32");
                pbuf.exists()
            })
            .collect();

        // just take the first for now
        let partition_root_pathbuf = root_path_candidates[0].to_path_buf();
        let win_dir_pathbuf = partition_root_pathbuf.join("Windows");
        let sys_dir_pathbuf = win_dir_pathbuf.join("System32");

        let win_dir = win_dir_pathbuf
            .to_str()
            .ok_or("Windows folder not found")?
            .to_string();
        let sys_dir = sys_dir_pathbuf
            .to_str()
            .ok_or("System folder not found")?
            .to_string();

        let path_str = std::env::var_os("PATH")
            .unwrap_or(OsString::from(""))
            .to_str()
            .ok_or("Could not read PATH")?
            .to_string();
        let env_path: Vec<String> = path_str.split(";").map(|s| s.to_string()).collect();

        Ok(Self {
            app_dir,
            sys_dir,
            win_dir,
            app_wd,
            env_path,
        })
    }

    /*
    Standard DLL search order for Desktop Applications (safe mode)
    https://docs.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-search-order#standard-search-order-for-desktop-applications

    1) application directory
    2) system directory (GetSystemDirectory())
    3) DEPRECATED: 16-bit system directory
    4) Windows directory (GetWindowsDirectory())
    5) Current directory
    6) PATH environment variable
    */
    pub fn search_path(&self) -> Vec<String> {
        let mut ret: Vec<String> = vec![
            self.app_dir.clone(),
            self.sys_dir.clone(),
            self.win_dir.clone(),
            self.app_wd.clone(),
        ];
        ret.extend(self.env_path.iter().cloned());

        let downlevel = self.sys_dir.clone() + "/downlevel";
        ret.insert(0, downlevel); // TODO: remove hack for API sets

        ret
    }

    fn is_system_dir(&self, dir: &str) -> bool {
        //TODO: remove hack for API sets
        let downlevel = self.sys_dir.clone() + "/downlevel";
        if dir == downlevel {
            return true;
        }

        dir == self.sys_dir || dir == self.win_dir
    }
}

fn test_file_in_path_case_insensitive(
    filename: &str,
    path: &str,
) -> Result<Option<String>, LookupError> {
    let lower_filename = filename.to_lowercase();
    let matching_entries: Vec<_> = std::fs::read_dir(path)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.metadata().map_or_else(|_| false, |m| m.is_file()))
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .map_or_else(|| None, |s| Some(s.to_owned()))
        })
        .filter(|s| s.to_lowercase() == lower_filename)
        .collect();
    if matching_entries.len() == 1 {
        Ok(matching_entries.first().cloned())
    } else {
        Ok(None)
    }
}

#[derive(Debug)]
pub struct LookupQuery {
    name: String,
    depth: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LookupResult {
    pub(crate) name: String,
    pub(crate) depth_first_appearance: usize,
    pub(crate) is_system: Option<bool>,
    pub(crate) folder: Option<String>,
    pub(crate) dependencies: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct Executables {
    index: std::collections::HashMap<String, LookupResult>,
}

impl Executables {
    pub fn new() -> Self {
        Self {
            index: std::collections::HashMap::new(),
        }
    }

    pub fn insert(&mut self, name: &str, lr: LookupResult) {
        self.index.insert(name.to_lowercase(), lr);
    }

    pub fn get(&self, name: &str) -> Option<&LookupResult> {
        self.index.get(&name.to_lowercase())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(&name.to_lowercase())
    }

    pub fn values(&self) -> Values<'_, String, LookupResult> {
        self.index.values()
    }
}

struct Workqueue {
    executables_to_lookup: Vec<LookupQuery>,
    executables_found: Executables, // using lowercase filename as key, assuming that we can only find a DLL given a name; if this changes, use the path instead
}

impl Workqueue {
    fn new() -> Self {
        Self {
            executables_to_lookup: Vec::new(),
            executables_found: Executables::new(),
        }
    }

    // the user enqueues an executable; the workers enqueue the dependencies of those that were found
    // (skip the dependencies that have already been found)
    fn enqueue(&mut self, executable_name: &str, depth: usize) {
        if !self.executables_found.contains(executable_name) {
            self.executables_to_lookup.push(LookupQuery {
                name: executable_name.to_string(),
                depth,
            })
        }
    }

    // the workers fetch work to be done (the name of a DLL to be found)
    fn pop(&mut self) -> Option<LookupQuery> {
        self.executables_to_lookup.pop()
    }

    // the workers register the executable that was found for the given name; the function checks for uniqueness
    fn register_finding(&mut self, found: LookupResult) {
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

// returns the actual full path to the executable, if found
pub fn search_file(filename: &str, context: &LookupContext) -> Result<Option<String>, LookupError> {
    let search_path = context.search_path();
    for d in search_path {
        if let Ok(found) = test_file_in_path_case_insensitive(filename, &d) {
            if let Some(actual_filename) = found {
                let mut p = std::path::PathBuf::new();
                p.push(d);
                p.push(actual_filename);
                return Ok(p.to_str().map(|s| s.to_owned()));
            }
        }
    }

    Ok(None)
}

pub fn lookup_executable_dependencies(
    filename: &str,
    context: &LookupContext,
    max_depth: usize,
    skip_system_dlls: bool,
) -> Executables {
    println!("inspecting {}", filename);

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
            if let Ok(l) = search_file(&executable, &context) {
                if let Some(fullpath) = l {
                    let folder = Path::new(&fullpath).parent().unwrap().to_str().unwrap();
                    let actual_name = Path::new(&fullpath).file_name().unwrap().to_str().unwrap();
                    let is_system = context.is_system_dir(folder);

                    if let Ok(dependencies) = dlls_imported_by_executable(&fullpath) {
                        if !(skip_system_dlls && is_system) {
                            for d in &dependencies {
                                workqueue.enqueue(d, depth + 1);
                            }
                        }

                        workqueue.register_finding(LookupResult {
                            name: actual_name.to_owned(),
                            depth_first_appearance: depth,
                            is_system: Some(is_system),
                            folder: Some(folder.to_owned()),
                            dependencies: Some(dependencies),
                        });
                    } else {
                        workqueue.register_finding(LookupResult {
                            name: executable.clone(),
                            depth_first_appearance: depth,
                            is_system: Some(is_system),
                            folder: None,
                            dependencies: None,
                        });
                    }
                } else {
                    workqueue.register_finding(LookupResult {
                        name: executable.clone(),
                        depth_first_appearance: depth,
                        is_system: None,
                        folder: None,
                        dependencies: None,
                    });
                }
            } else {
                workqueue.register_finding(LookupResult {
                    name: executable.clone(),
                    depth_first_appearance: depth,
                    is_system: None,
                    folder: None,
                    dependencies: None,
                });
            }
        }
    }
    println!("finished inspecting {}", filename);

    workqueue.executables_found
}

// tree view of nodes referencing LookupResults in an Executables
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
        parent: Option<String>,
        depth: usize,
        lr: &LookupResult,
        exes: &Executables,
    ) {
        let this_index = self.arena.len();
        self.arena.push(ExecutablesTreeNode {
            name: lr.name.clone(),
            depth,
            parent,
            dependencies: Vec::new(), // will fill this later in new()
        });

        let mut this_deps: Vec<String> = Vec::new();

        if let Some(deps) = &lr.dependencies {
            for dep in deps {
                if let Some(dep_lr) = exes.get(&dep.to_lowercase()) {
                    self.add_to_arena(Some(lr.name.clone()), depth + 1, dep_lr, exes);
                    this_deps.push(dep.clone());
                }
            }
        }

        self.arena[this_index].dependencies = this_deps;
        self.index.insert(lr.name.clone(), this_index);
    }

    pub fn new(exes: &Executables) -> Self {
        let root_nodes: Vec<&LookupResult> = exes
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
            index: HashMap::new(),
            executables: exes.clone(),
        };

        ret.add_to_arena(None, 0, root_node, &exes);

        ret
    }

    pub fn visit_depth_first(&self, f: impl Fn(&ExecutablesTreeNode) -> ()) {
        // the arena currently holds a depth-first linearization of the tree
        for n in &self.arena {
            f(n)
        }
    }
}
