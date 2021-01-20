#[cfg(windows)]
use crate::path::{get_system_directory, get_windows_directory};

use crate::LookupError;

use std::ffi::OsString;
#[cfg(not(windows))]
use std::path::Path;

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
#[derive(Debug)]
pub struct LookupContext {
    pub app_dir: String,
    pub sys_dir: String,
    pub win_dir: String,
    pub app_wd: String,
    pub env_path: Vec<String>,
}

impl LookupContext {
    // create a lookup context explicitely
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

    // creates a lookup context that mirrors the behavior of the system when running the executable
    // from the current shell, including:
    // - the app directory
    // - the C:\Windows\System32 directory
    // - the C:\Windows directory
    // - the current shell working directory
    // - the directories listed in the current value of the PATH environment variable
    #[cfg(windows)]
    pub fn deduce_from_executable_location(app_dir: &str) -> Result<Self, LookupError> {
        let app_dir = app_dir.to_string();
        let sys_dir = get_system_directory()?;
        let win_dir = get_windows_directory().unwrap();
        let app_wd = std::env::current_dir()?
            .to_str()
            .ok_or(LookupError::ContextDeductionError(
                "Could not get app working directory".to_string(),
            ))?
            .to_string();

        let path_str = std::env::var_os("PATH")
            .unwrap_or(OsString::from(""))
            .to_str()
            .unwrap()
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

    // creates a lookup context that mirrors the behavior of the system when running the executable
    // from the Windows partition the executable lies on, including:
    // - the app directory
    // - the C:\Windows\System32 directory
    // - the C:\Windows directory
    // - the current shell working directory
    // - the directories listed in the current value of the PATH environment variable
    #[cfg(not(windows))]
    pub fn deduce_from_executable_location(exe_path: &str) -> Result<Self, LookupError> {
        let exe_path = Path::new(exe_path)
            .parent()
            .ok_or(LookupError::ContextDeductionError(
                "Exe not found".to_string(),
            ))?;

        let app_dir = exe_path
            .to_str()
            .ok_or(LookupError::ContextDeductionError(
                "Exe not found".to_string(),
            ))?
            .to_string();
        let app_wd = std::env::current_dir()?
            .to_str()
            .ok_or(LookupError::ContextDeductionError(
                "Could not get cwd path".to_string(),
            ))?
            .to_string();

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
            .ok_or(LookupError::ContextDeductionError(
                "Windows folder not found".to_string(),
            ))?
            .to_string();
        let sys_dir = sys_dir_pathbuf
            .to_str()
            .ok_or(LookupError::ContextDeductionError(
                "System folder not found".to_string(),
            ))?
            .to_string();

        // it makes no sense to add the non-Windows path to the search path
        // (TODO: read from the registry of the partition with hivex?)
        let env_path = Vec::new();

        Ok(Self {
            app_dir,
            sys_dir,
            win_dir,
            app_wd,
            env_path,
        })
    }

    // linearize the lookup context into a single vector of directories
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

    // return true if the given path is considered as a system directory for the current configuration
    pub fn is_system_dir(&self, dir: &str) -> bool {
        //TODO: remove hack for API sets
        let downlevel = self.sys_dir.clone() + "/downlevel";
        if dir == downlevel {
            return true;
        }

        dir == self.sys_dir || dir == self.win_dir
    }
}
