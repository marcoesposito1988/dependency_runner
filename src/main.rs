#[cfg(windows)]
extern crate winapi;

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

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
fn get_system_directory() -> Result<String, std::io::Error> {
    return get_winapi_directory(winapi::um::sysinfoapi::GetSystemDirectoryW);
}

#[cfg(windows)]
fn get_windows_directory() -> Result<String, std::io::Error> {
    return get_winapi_directory(winapi::um::sysinfoapi::GetWindowsDirectoryW);
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

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = std::env::var_os("PATH").unwrap_or(OsString::from(""));

    if args.len() < 2 {
        println!("You must pass the path to the binary!");
        return;
    }

    let binary_path = args.get(1).unwrap();
    println!("path to the binary: {:?}", binary_path);

    let system_directory = get_system_directory();
    println!("path to the system directory: {:?}", system_directory);

    let windows_directory = get_windows_directory();
    println!("path to the windows directory: {:?}", windows_directory);

    println!("path env variable: {:?}", path);
}

// this is not going to work on other platforms
#[cfg(not(windows))]
fn main() {
    println!("This package is only going to work on Windows. Sorry!")
}
