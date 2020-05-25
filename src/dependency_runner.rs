#[cfg(windows)]
extern crate winapi;

use std::ffi::OsString;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;

#[derive(Debug)]
pub enum Error {
    CouldNotOpenFile(std::io::Error),
    ProcessingError(pelite::Error),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::CouldNotOpenFile(e)
    }
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

use pelite::pe64::{Pe, PeFile};
use std::path::Path;

pub fn dlls_imported_by_executable<P: AsRef<Path> + ?Sized>(
    path: &P,
) -> Result<Vec<String>, Error> {
    use crate::dependency_runner::Error::{CouldNotOpenFile, ProcessingError};
    let path = path.as_ref();
    let map = pelite::FileMap::open(path).map_err(|e| CouldNotOpenFile(e))?;
    let file = PeFile::from_bytes(&map).map_err(|e| ProcessingError(e))?;

    // Access the import directory
    let imports = file.imports().map_err(|e| ProcessingError(e))?;

    let names: Vec<&pelite::util::CStr> = imports
        .iter()
        .map(|desc| desc.dll_name())
        .collect::<Result<Vec<&pelite::util::CStr>, pelite::Error>>()
        .map_err(|e| ProcessingError(e))?;
    Ok(names
        .iter()
        .filter_map(|s| s.to_str().ok())
        .map(|s| s.to_string())
        .collect::<Vec<String>>())
}

pub struct Context {
    app_dir: String,
    sys_dir: String,
    win_dir: String,
    app_wd: String,
    env_path: Vec<String>,
}

impl Context {
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
        ret
    }

    fn is_system_dir(&self, dir: &str) -> bool {
        dir == self.sys_dir || dir == self.win_dir
    }
}

fn test_file_in_path_case_insensitive(filename: &str, path: &str) -> Result<Option<String>, Error> {
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

#[derive(Debug, Clone)]
pub struct LookupResult {
    pub(crate) name: String,
    pub(crate) depth: usize,
    pub(crate) is_system: Option<bool>,
    pub(crate) folder: Option<String>,
    pub(crate) dependencies: Option<Vec<String>>,
}

type Executables = std::collections::HashMap<String, LookupResult>;

struct Workqueue {
    executables_to_lookup: Vec<LookupQuery>,
    executables_found: Executables, // using filename as key, assuming that we can only find a DLL given a name; if this changes, use the path instead
}

impl Workqueue {
    fn new() -> Self {
        Self {
            executables_to_lookup: Vec::new(),
            executables_found: std::collections::HashMap::new(),
        }
    }

    // the user enqueues an executable; the workers enqueue the dependencies of those that were found
    // (skip the dependencies that have already been found)
    fn enqueue(&mut self, executable_name: &str, depth: usize) {
        if !self.executables_found.contains_key(executable_name) {
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
        if self.executables_found.contains_key(&found.name) {
            if found.folder != self.executables_found[&found.name].folder {
                panic!(
                    "Found two DLLs with the same name! {:?} and {:?}",
                    found.folder, self.executables_found[&found.name].folder
                )
            }
        } else {
            self.executables_found.insert(found.name.clone(), found);
        }
    }
}

// returns the actual full path to the executable, if found
pub fn search_file(filename: &str, context: &Context) -> Result<Option<String>, Error> {
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
    context: &Context,
    max_depth: usize,
    skip_system_dlls_deps: bool,
) -> Executables {
    println!("inspecting {}", filename);

    let mut workqueue = Workqueue::new();
    workqueue.enqueue(filename, 0);

    while let Some(lookup_query) = workqueue.pop() {
        if lookup_query.depth <= max_depth {
            let executable = lookup_query.name;
            let depth = lookup_query.depth;
            if let Ok(l) = search_file(&executable, &context) {
                if let Some(fullpath) = l {
                    let folder = Path::new(&fullpath).parent().unwrap().to_str().unwrap();
                    let actual_name = Path::new(&fullpath).file_name().unwrap().to_str().unwrap();
                    let is_system = context.is_system_dir(folder);
                    if let Ok(dependencies) = dlls_imported_by_executable(&fullpath) {
                        if !(skip_system_dlls_deps && is_system) {
                            for d in &dependencies {
                                workqueue.enqueue(d, depth + 1);
                            }
                        }

                        workqueue.register_finding(LookupResult {
                            name: actual_name.to_owned(),
                            depth,
                            is_system: Some(is_system),
                            folder: Some(folder.to_owned()),
                            dependencies: Some(dependencies),
                        });
                    } else {
                        workqueue.register_finding(LookupResult {
                            name: executable.clone(),
                            depth,
                            is_system: Some(is_system),
                            folder: None,
                            dependencies: None,
                        });
                    }
                } else {
                    workqueue.register_finding(LookupResult {
                        name: executable.clone(),
                        depth,
                        is_system: None,
                        folder: None,
                        dependencies: None,
                    });
                }
            } else {
                workqueue.register_finding(LookupResult {
                    name: executable.clone(),
                    depth,
                    is_system: None,
                    folder: None,
                    dependencies: None,
                });
            }
        }
    }

    workqueue.executables_found
}
