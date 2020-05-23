extern crate winapi;

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

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

pub fn get_system_directory() -> Result<String, std::io::Error> {
    return get_winapi_directory(winapi::um::sysinfoapi::GetSystemDirectoryW);
}

pub fn get_windows_directory() -> Result<String, std::io::Error> {
    return get_winapi_directory(winapi::um::sysinfoapi::GetWindowsDirectoryW);
}

use pelite::pe64::{Pe, PeFile};
use std::path::Path;

fn example(file: PeFile<'_>) -> pelite::Result<()> {
    // Access the import directory
    let imports = file.imports()?;

    // Iterate over the import descriptors
    for desc in imports {
        // DLL being imported from
        let dll_name = desc.dll_name()?;
        println!("{}", dll_name);

        // Import Address Table and Import Name Table for this imported DLL
        // let iat = desc.iat()?;
        // let int = desc.int()?;

        // Iterate over the imported functions from this DLL
        // for (va, import) in Iterator::zip(iat, int) {}
    }

    // Iterate over the IAT
    // for (va, import) in file.iat()?.iter() {
    //     // The IAT may contains Null entries where the IAT of imported modules join
    //     if let Ok(import) = import {}
    // }

    Ok(())
}

pub fn file_map<P: AsRef<Path> + ?Sized>(path: &P) -> pelite::Result<()> {
    let path = path.as_ref();
    if let Ok(map) = pelite::FileMap::open(path) {
        let file = PeFile::from_bytes(&map)?;

        // Access the file contents through the Pe trait
        let image_base = file.optional_header().ImageBase;
        println!(
            "The preferred load address of {:?} is {}.",
            path, image_base
        );

        // See the respective modules to access other parts of the PE file.
        example(file);
    }
    Ok(())
}
