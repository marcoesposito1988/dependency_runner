//! Utilities to read the list of known DLLs for a Windows installation

extern crate ntapi;
extern crate winapi;

use ntapi::ntobapi::{
    DIRECTORY_QUERY, OBJECT_DIRECTORY_INFORMATION, POBJECT_DIRECTORY_INFORMATION,
};
use ntapi::ntrtl::RtlNtStatusToDosError;
use std::ffi::OsString;
use std::mem::size_of;
use std::os::windows::prelude::*;
use std::ptr::null_mut;
use winapi::shared::ntdef::{
    FALSE, HANDLE, NTSTATUS, NT_SUCCESS, OBJECT_ATTRIBUTES, TRUE, ULONG, UNICODE_STRING, USHORT,
    WCHAR,
};
use winapi::shared::ntstatus;

unsafe fn u16_ptr_to_string(ptr: *const u16) -> OsString {
    let len = (0..).take_while(|&i| *ptr.offset(i) != 0).count();
    let slice = std::slice::from_raw_parts(ptr, len);

    OsString::from_wide(slice)
}

// according to https://lucasg.github.io/2017/06/07/listing-known-dlls/,
// in contrast to reading the HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager\KnownDLLs key,
// listing \KnownDlls gives us the entire list of DLLs, so we don't need to look up the dependencies of each DLL

/// Returns the complete list of KnownDlls
///
/// These DLLs are mapped into each process' memory as
/// copy-on-write for performance (and a sprinkle of security) reasons.
///
/// They are all located in the system directory and can't be overridden/hijacked.
pub fn get_known_dlls() -> anyhow::Result<Vec<String>> {
    const KNOWN_DLLS_NAME_BUFFER: &[WCHAR] = &[
        '\\' as _, 'K' as _, 'n' as _, 'o' as _, 'w' as _, 'n' as _, 'D' as _, 'l' as _, 'l' as _,
        's' as _,
    ];

    const KNOWN_DLLS_NAME: UNICODE_STRING = UNICODE_STRING {
        Length: (KNOWN_DLLS_NAME_BUFFER.len() * size_of::<WCHAR>()) as USHORT,
        MaximumLength: (KNOWN_DLLS_NAME_BUFFER.len() * size_of::<WCHAR>()) as USHORT,
        Buffer: KNOWN_DLLS_NAME_BUFFER.as_ptr() as *mut _,
    };

    let mut oa: OBJECT_ATTRIBUTES = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as ULONG,
        RootDirectory: null_mut(),
        ObjectName: &KNOWN_DLLS_NAME as *const _ as *mut _,
        Attributes: 0,
        SecurityDescriptor: null_mut(),
        SecurityQualityOfService: null_mut(),
    };

    let mut ret = Vec::new();

    let mut known_dll_dir_handle: HANDLE = null_mut();
    let mut status: NTSTATUS;
    unsafe {
        status = ntapi::ntobapi::NtOpenDirectoryObject(
            &mut known_dll_dir_handle,
            DIRECTORY_QUERY,
            &mut oa,
        );
        if status != ntstatus::STATUS_SUCCESS {
            let raw_err = std::io::Error::from_raw_os_error(RtlNtStatusToDosError(status) as i32);
            eprintln!("Failed to open KnownDll: {}", raw_err);
        }
    }
    if !NT_SUCCESS(status) {
        match status {
            ntstatus::STATUS_INSUFFICIENT_RESOURCES => eprintln!("Insufficient resources"),
            ntstatus::STATUS_INVALID_PARAMETER => eprintln!("INVALID_PARAMETER"),
            ntstatus::STATUS_OBJECT_NAME_INVALID => eprintln!("OBJECT_NAME_INVALID"),
            ntstatus::STATUS_OBJECT_NAME_NOT_FOUND => eprintln!("OBJECT_NAME_NOT_FOUND"),
            ntstatus::STATUS_OBJECT_PATH_NOT_FOUND => eprintln!("OBJECT_PATH_NOT_FOUND"),
            ntstatus::STATUS_OBJECT_PATH_SYNTAX_BAD => eprintln!("OBJECT_PATH_SYNTAX_BAD"),
            _ => eprintln!("Error: other"),
        }
    }

    let mut first_time = TRUE;
    let mut context: ULONG = 0;
    let mut buffer_size: u32 = 0x200;
    let mut return_length: u32 = 0;
    let mut buffer_vec: Vec<u8> = vec![0; buffer_size as usize];
    let buffer: POBJECT_DIRECTORY_INFORMATION =
        buffer_vec.as_mut_ptr() as POBJECT_DIRECTORY_INFORMATION;
    unsafe {
        loop {
            loop {
                status = ntapi::ntobapi::NtQueryDirectoryObject(
                    known_dll_dir_handle,
                    buffer as *mut winapi::ctypes::c_void,
                    buffer_size,
                    FALSE,
                    first_time,
                    &mut context,
                    &mut return_length,
                );
                if status != ntstatus::STATUS_MORE_ENTRIES {
                    break;
                }

                // Check if we have at least one entry. If not, we'll double the buffer size and try
                // again.

                if (*buffer).Name.Buffer != null_mut() {
                    break;
                }

                buffer_size *= 2;
                buffer_vec = vec![0; buffer_size as usize];
            }

            let mut i: usize = 0;

            loop {
                let info: POBJECT_DIRECTORY_INFORMATION = buffer_vec
                    .as_ptr()
                    .offset((size_of::<OBJECT_DIRECTORY_INFORMATION>() * i) as isize)
                    as POBJECT_DIRECTORY_INFORMATION;

                if (*info).Name.Buffer == null_mut() {
                    break;
                }

                let section_str: OsString = std::ffi::OsString::from("Section");
                let section_str_2 = u16_ptr_to_string((*info).TypeName.Buffer);

                if section_str == section_str_2 {
                    let ffis = u16_ptr_to_string((*info).Name.Buffer);
                    ret.push(ffis.to_str().unwrap().to_owned())
                }

                i += 1;
            }

            if status != ntstatus::STATUS_MORE_ENTRIES {
                break;
            }

            first_time = FALSE;
        }
    }

    Ok(ret)
}

#[cfg(test)]
mod tests {
    use crate::knowndlls::get_known_dlls;
    use crate::common::LookupError;

    #[cfg(windows)]
    #[test]
    fn list_known_dlls() -> Result<(), LookupError> {
        let known_dlls = get_known_dlls()?;
        assert!(!known_dlls.is_empty());
        assert!(known_dlls.contains(&"ntdll.dll".to_string()));
        Ok(())
    }
}
