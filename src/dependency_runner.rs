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
}

fn test_executable_in_path(filename: &str, path: &str) -> Result<bool, Error> {
    use crate::dependency_runner::Error::CouldNotOpenFile;
    let fullpath = Path::new(path).join(filename);
    let attr = std::fs::metadata(fullpath).map_err(|e| CouldNotOpenFile(e))?;
    Ok(attr.is_file())
}

#[derive(Debug)]
pub enum LookupResult {
    Found(String),
    // the string is the containing directory
    NotFound,
}

#[derive(Debug)]
pub struct Executable {
    name: String,
    pub(crate) folder: Option<String>,
    pub(crate) dependencies: Option<Vec<String>>,
}

type Executables = std::collections::HashMap<String, Executable>;

struct Workqueue {
    executables_to_lookup: Vec<String>,
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
    fn enqueue(&mut self, executable_name: &str) {
        if !self.executables_found.contains_key(executable_name) {
            self.executables_to_lookup.push(executable_name.to_string())
        }
    }

    // the workers fetch work to be done (the name of a DLL to be found)
    fn pop(&mut self) -> Option<String> {
        self.executables_to_lookup.pop()
    }

    // the workers register the executable that was found for the given name; the function checks for uniqueness
    fn register_finding(&mut self, found: Executable) {
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

// returns the directory containing the executable, if found
pub fn lookup_executable(filename: &str, context: &Context) -> Result<LookupResult, Error> {
    let search_path = context.search_path();
    for d in search_path {
        if let Ok(found) = test_executable_in_path(filename, &d) {
            if found {
                return Ok(LookupResult::Found(d));
            }
        }
    }

    Ok(LookupResult::NotFound)
}

pub fn lookup_executable_dependencies(filename: &str, context: &Context) -> Executables {
    println!("inspecting {}", filename);

    let mut workqueue = Workqueue::new();
    workqueue.enqueue(filename);

    while let Some(executable) = workqueue.pop() {
        if let Ok(l) = lookup_executable(&executable, &context) {
            if let LookupResult::Found(folder) = l {
                let fullpath = folder.to_string() + "/" + &executable;
                if let Ok(dependencies) = dlls_imported_by_executable(&fullpath) {
                    for d in &dependencies {
                        workqueue.enqueue(d);
                    }

                    workqueue.register_finding(Executable {
                        name: executable.clone(),
                        folder: Some(folder),
                        dependencies: Some(dependencies),
                    });
                } else {
                    workqueue.register_finding(Executable {
                        name: executable.clone(),
                        folder: None,
                        dependencies: None,
                    });
                }
            } else {
                workqueue.register_finding(Executable {
                    name: executable.clone(),
                    folder: None,
                    dependencies: None,
                });
            }
        } else {
            workqueue.register_finding(Executable {
                name: executable.clone(),
                folder: None,
                dependencies: None,
            });
        }
    }

    workqueue.executables_found
}
