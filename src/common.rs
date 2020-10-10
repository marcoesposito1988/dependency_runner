#[cfg(windows)]
use crate::path::{get_system_directory, get_windows_directory};

use thiserror::Error;

use serde::Serialize;
use std::collections::hash_map::Values;
use std::ffi::OsString;
use std::path::Path;

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

    // #[cfg(not(windows))]
    // pub fn new(app_dir: &str, sys_dir: &str, win_dir: &str, app_wd: &str) -> Self {
    //     let app_dir = app_dir.to_string();
    //     let sys_dir = sys_dir.to_string();
    //     let win_dir = win_dir.to_string();
    //     let app_wd = app_wd.to_string();
    //
    //     let path_str = std::env::var_os("PATH")
    //         .unwrap_or(OsString::from(""))
    //         .to_str()
    //         .unwrap()
    //         .to_string();
    //     let env_path: Vec<String> = path_str.split(";").map(|s| s.to_string()).collect();
    //
    //     Self {
    //         app_dir,
    //         sys_dir,
    //         win_dir,
    //         app_wd,
    //         env_path,
    //     }
    // }

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

    pub fn is_system_dir(&self, dir: &str) -> bool {
        //TODO: remove hack for API sets
        let downlevel = self.sys_dir.clone() + "/downlevel";
        if dir == downlevel {
            return true;
        }

        dir == self.sys_dir || dir == self.win_dir
    }
}

#[derive(Debug)]
pub struct LookupQuery {
    pub name: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LookupResult {
    pub name: String,
    pub depth_first_appearance: usize,
    pub is_system: Option<bool>,
    pub folder: Option<String>,
    pub dependencies: Option<Vec<String>>,
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
