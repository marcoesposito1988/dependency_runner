#[cfg(windows)]
extern crate winapi;

use std::ffi::OsString;
#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use crate::LookupError;

// supported DLL search modes: standard for desktop application, safe or unsafe, as specified by the registry (if running on Windows)
// TODO: read HKEY_LOCAL_MACHINE\System\CurrentControlSet\Control\Session Manager\SafeDllSearchMode  and pick mode accordingly
// the other modes are activated programmatically, and there is no hope to be able to handle that properly
// https://docs.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-search-order#standard-search-order-for-desktop-applications

// description of a Windows system
// if running from within Win: we extract system directory paths from Win32 APIs, and read the
// PATH env var (the user can override everything later if necessary)
// if running in another OS: we can only guess the directories, and can't do anything about the PATH
pub struct WindowsSystem {
    pub safe_dll_search_mode_on: Option<bool>,
    pub known_dlls: Option<Vec<PathBuf>>,
    pub win_dir: PathBuf,
    pub sys_dir: PathBuf,
    // sys16_dir ignored, since it is not supported on 64-bit systems
    pub path: Option<Vec<PathBuf>>,
}

impl WindowsSystem {
    #[cfg(windows)]
    pub fn current() -> Self {
        // TODO: read known dlls from HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager\KnownDLLs,
        // and mark their dependencies (which are not listed there) as known DLLs as well
        // https://lucasg.github.io/2017/06/07/listing-known-dlls/
        // TODO: read dll safe mode on/off from HKEY_LOCAL_MACHINE\System\CurrentControlSet\Control\Session Manager\SafeDllSearchMode (if it doesn't exist, it's 1)
        Self {
            safe_dll_search_mode_on: None,
            known_dlls: None,
            win_dir: get_windows_directory(),
            sys_dir: get_system_directory(),
            path: std::env::var("PATH")
                .and_then(|s| s.split(";").map(|subs| subs.into()).collect()),
        }
    }

    pub fn from_exe_location<P: AsRef<Path>>(p: P) -> Result<Self, LookupError> {
        if let Some(root) = Self::find_root(&p) {
            Ok(Self::from_root(root))
        } else {
            Err(LookupError::ContextDeductionError(
                "Couldn't find Windows filesystem root for executable ".to_owned()
                    + p.as_ref().to_str().unwrap_or(""),
            ))
        }
    }

    #[cfg(not(windows))]
    fn find_root<P: AsRef<Path>>(p: P) -> Option<PathBuf> {
        for a in p.as_ref().parent()?.ancestors() {
            if Self::is_root(a) {
                return Some(a.to_owned());
            }
        }
        None
    }

    #[cfg(not(windows))]
    fn is_root<P: AsRef<Path>>(p: P) -> bool {
        let s = Self::from_root(p);
        s.win_dir.exists() && s.sys_dir.exists()
    }

    #[cfg(not(windows))]
    pub fn from_root<P: AsRef<Path>>(root_path: P) -> Self {
        // TODO: wrap hivex?
        // read known dlls from HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager\KnownDLLs,
        // and mark their dependencies (which are not listed there) as known DLLs as well
        // https://lucasg.github.io/2017/06/07/listing-known-dlls/
        // read dll safe mode on/off from HKEY_LOCAL_MACHINE\System\CurrentControlSet\Control\Session Manager\SafeDllSearchMode (if it doesn't exist, it's 1)
        // read system path from HKEY_LOCAL_MACHINE\System\CurrentControlSet\Control\Session Manager\Environment ?
        // read user path from C:\Users\<username>\NTUSER.DAT \Environment ?
        let win_dir = root_path.as_ref().join("Windows");
        let sys_dir = win_dir.join("System32");
        Self {
            safe_dll_search_mode_on: None,
            known_dlls: None,
            win_dir,
            sys_dir,
            path: None,
        }
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

pub(crate) fn test_file_in_path_case_insensitive<P: AsRef<Path>>(
    filename: P,
    path: P,
) -> Result<Option<PathBuf>, LookupError> {
    let matching_entries: Vec<OsString> = std::fs::read_dir(path)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.metadata().map_or_else(|_| false, |m| m.is_file()))
        .filter_map(|entry| {
            entry
                .file_name().into()
        })
        .filter(|s|
            // s.eq_ignore_ascii_case(&filename)) // TODO as soon as it's stable
            s.to_str().map(|s2| s2.to_lowercase() == filename.as_ref().to_str().unwrap_or("").to_lowercase()).unwrap_or(false)
        )
        .collect();
    if matching_entries.len() == 1 {
        if let Some(s) = matching_entries.first() {
            return Ok(Some(s.into()));
        }
    }
    Ok(None)
}