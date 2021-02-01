#[cfg(windows)]
extern crate winapi;

#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use crate::LookupError;
use std::collections::HashMap;

// supported DLL search modes: standard for desktop application, safe or unsafe, as specified by the registry (if running on Windows)
// TODO: read HKEY_LOCAL_MACHINE\System\CurrentControlSet\Control\Session Manager\SafeDllSearchMode  and pick mode accordingly
// the other modes are activated programmatically, and there is no hope to be able to handle that properly
// https://docs.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-search-order#standard-search-order-for-desktop-applications

// description of a Windows system
// if running from within Win: we extract system directory paths from Win32 APIs, and read the
// PATH env var (the user can override everything later if necessary)
// if running in another OS: we can only guess the directories, and can't do anything about the PATH
#[derive(Debug, Clone)]
pub struct WindowsSystem {
    pub safe_dll_search_mode_on: Option<bool>,
    pub known_dlls: Option<Vec<PathBuf>>,
    pub win_dir: PathBuf,
    pub sys_dir: PathBuf,
    // sys16_dir ignored, since it is not supported on 64-bit systems
    pub system_path: Option<Vec<PathBuf>>,
}

impl WindowsSystem {
    #[cfg(windows)]
    pub fn current() -> Result<Self, LookupError> {
        // TODO: read known dlls from HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager\KnownDLLs,
        // and mark their dependencies (which are not listed there) as known DLLs as well
        // https://lucasg.github.io/2017/06/07/listing-known-dlls/
        // TODO: read dll safe mode on/off from HKEY_LOCAL_MACHINE\System\CurrentControlSet\Control\Session Manager\SafeDllSearchMode (if it doesn't exist, it's 1)
        let path_str = std::env::var("PATH");
        let path = path_str
            .and_then(|s| {
                Ok(s.split(";")
                    .filter_map(|subs| std::fs::canonicalize(subs).ok())
                    .collect())
            })
            .ok();
        Ok(Self {
            safe_dll_search_mode_on: None,
            known_dlls: None,
            win_dir: get_windows_directory()?,
            sys_dir: get_system_directory()?,
            system_path: path,
        })
    }

    #[cfg(not(windows))]
    pub fn from_exe_location<P: AsRef<Path>>(p: P) -> Result<Option<Self>, LookupError> {
        if let Some(root) = Self::find_root(&p) {
            Ok(Self::from_root(root))
        } else {
            Ok(None)
        }
    }

    #[cfg(not(windows))]
    fn find_root<P: AsRef<Path>>(p: P) -> Option<PathBuf> {
        for a in p.as_ref().parent()?.ancestors() {
            if Self::from_root(a).is_some() {
                return Some(a.to_owned());
            }
        }
        None
    }

    pub fn from_root<P: AsRef<Path>>(root_path: P) -> Option<Self> {
        // TODO: wrap hivex?
        // read known dlls from HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager\KnownDLLs,
        // and mark their dependencies (which are not listed there) as known DLLs as well
        // https://lucasg.github.io/2017/06/07/listing-known-dlls/
        // read dll safe mode on/off from HKEY_LOCAL_MACHINE\System\CurrentControlSet\Control\Session Manager\SafeDllSearchMode (if it doesn't exist, it's 1)
        // read system path from HKEY_LOCAL_MACHINE\System\CurrentControlSet\Control\Session Manager\Environment ?
        // read user path from C:\Users\<username>\NTUSER.DAT \Environment ?
        let win_dir = root_path.as_ref().join("Windows");
        let sys_dir = win_dir.join("System32");
        if sys_dir.exists() {
            Some(Self {
                safe_dll_search_mode_on: None,
                known_dlls: None,
                win_dir,
                sys_dir,
                system_path: None,
            })
        } else {
            None
        }
    }
}

impl PartialEq for WindowsSystem {
    fn eq(&self, other: &Self) -> bool {
        self.sys_dir == other.sys_dir
            && self.win_dir == other.win_dir
            && self.safe_dll_search_mode_on == other.safe_dll_search_mode_on
            && self.known_dlls == other.known_dlls
            && self.system_path == other.system_path
    }
}

#[cfg(windows)]
fn get_winapi_directory(
    a: unsafe extern "system" fn(
        winapi::um::winnt::LPWSTR,
        winapi::shared::minwindef::UINT,
    ) -> winapi::shared::minwindef::UINT,
) -> Result<PathBuf, std::io::Error> {
    use std::io::Error;

    const BFR_SIZE: usize = 512;
    let mut bfr: [u16; BFR_SIZE] = [0; BFR_SIZE];

    let ret: u32 = unsafe { a(bfr.as_mut_ptr(), BFR_SIZE as u32) };
    if ret == 0 {
        Err(Error::last_os_error())
    } else {
        let valid_bfr = &bfr[..ret as usize];
        std::fs::canonicalize(OsString::from_wide(valid_bfr))
    }
}

#[cfg(windows)]
fn get_system_directory() -> Result<PathBuf, std::io::Error> {
    return get_winapi_directory(winapi::um::sysinfoapi::GetSystemDirectoryW);
}

#[cfg(windows)]
fn get_windows_directory() -> Result<PathBuf, std::io::Error> {
    return get_winapi_directory(winapi::um::sysinfoapi::GetWindowsDirectoryW);
}

pub(crate) struct WinFileSystemCache {
    files_in_dirs: HashMap<String, HashMap<String, PathBuf>>,
}

impl WinFileSystemCache {
    pub(crate) fn new() -> Self {
        Self {
            files_in_dirs: HashMap::new(),
        }
    }

    pub(crate) fn test_file_in_folder_case_insensitive<P: AsRef<Path>, Q: AsRef<Path>>(
        &mut self,
        filename: P,
        folder: Q,
    ) -> Result<Option<PathBuf>, LookupError> {
        let folder_str: String = folder
            .as_ref()
            .to_str()
            .ok_or(LookupError::ScanError(format!(
                "Could not scan directory {:?}",
                &folder.as_ref().to_str()
            )))?
            .to_owned();
        if !self.files_in_dirs.contains_key(&folder_str) {
            self.scan_folder(&folder)?;
        }
        let dir = self
            .files_in_dirs
            .get(&folder_str)
            .ok_or(LookupError::ScanError(format!(
                "Could not scan directory {:?}",
                &folder.as_ref().to_str()
            )))?;
        Ok(dir
            .get(&filename.as_ref().to_str().unwrap().to_lowercase())
            .map(|p| p.to_owned()))
    }

    pub(crate) fn scan_folder<P: AsRef<Path>>(&mut self, folder: P) -> Result<(), LookupError> {
        let folder_str: String = folder
            .as_ref()
            .to_str()
            .ok_or(LookupError::ScanError(format!(
                "Could not scan directory {:?}",
                &folder.as_ref().to_str()
            )))?
            .to_owned();
        if !self.files_in_dirs.contains_key(&folder_str) {
            let matching_entries: HashMap<String, PathBuf> = std::fs::read_dir(folder)?
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.metadata().map_or_else(|_| false, |m| m.is_file()))
                .filter_map(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .map(|s| (s.to_lowercase(), entry.file_name().into()))
                })
                .collect();
            self.files_in_dirs.insert(folder_str, matching_entries);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::system::WinFileSystemCache;
    use crate::LookupError;
    use std::path::PathBuf;

    #[cfg(windows)]
    #[test]
    fn context_win10() -> Result<(), LookupError> {
        use super::WindowsSystem;
        let ctx = WindowsSystem::current()?;
        assert_eq!(ctx.win_dir, std::fs::canonicalize("C:\\Windows")?);
        assert_eq!(ctx.sys_dir, std::fs::canonicalize("C:\\Windows\\System32")?);

        // TODO: once implemented, document that it can fail if system is set otherwise
        // assert_eq!(ctx.safe_dll_search_mode_on, Some(true));

        // this changes from computer to computer, but we should get something
        let user_path = ctx.system_path;
        assert!(user_path.is_some());
        assert!(user_path
            .unwrap()
            .contains(&std::fs::canonicalize("C:\\Windows")?));
        Ok(())
    }

    #[test]
    fn fscache() -> Result<(), LookupError> {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let test_file_path =
            d.join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");
        assert!(test_file_path.exists());
        let folder = test_file_path.parent().unwrap();

        let mut fscache = WinFileSystemCache::new();
        let expected_res = Some(PathBuf::from("DepRunTest.exe"));
        assert_eq!(
            fscache.test_file_in_folder_case_insensitive("depruntest.exe", folder)?,
            expected_res
        );
        assert_eq!(
            fscache.test_file_in_folder_case_insensitive("Depruntest.exe", folder)?,
            expected_res
        );
        assert_eq!(
            fscache.test_file_in_folder_case_insensitive("somerandomstring.txt", folder)?,
            None
        );
        Ok(())
    }
}
