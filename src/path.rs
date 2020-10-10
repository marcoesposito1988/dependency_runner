use crate::LookupContext;
use crate::LookupError;

#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;

#[cfg(windows)]
extern crate winapi;

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
