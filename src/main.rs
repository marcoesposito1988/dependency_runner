#[cfg(windows)]
mod rupencies;

#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;

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

    let system_directory = rupencies::get_system_directory();
    println!("path to the system directory: {:?}", system_directory);

    let windows_directory = rupencies::get_windows_directory();
    println!("path to the windows directory: {:?}", windows_directory);

    println!("path env variable: {:?}", path);

    rupencies::file_map(binary_path);
}

// this is not going to work on other platforms
#[cfg(not(windows))]
fn main() {
    println!("This package is only going to work on Windows. Sorry!")
}
