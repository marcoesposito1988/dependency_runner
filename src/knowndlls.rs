//! Utilities to read the list of known DLLs for a Windows installation

use std::ffi::OsString;
use std::mem::size_of;
use std::os::windows::prelude::*;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{
    HANDLE, NTSTATUS, RtlNtStatusToDosError, STATUS_BUFFER_TOO_SMALL, STATUS_INSUFFICIENT_RESOURCES,
    STATUS_INVALID_PARAMETER, STATUS_MORE_ENTRIES, STATUS_OBJECT_NAME_INVALID,
    STATUS_OBJECT_NAME_NOT_FOUND, STATUS_OBJECT_PATH_NOT_FOUND, STATUS_OBJECT_PATH_SYNTAX_BAD,
    STATUS_SUCCESS, UNICODE_STRING,
};

const DIRECTORY_QUERY: u32 = 0x0001;

#[repr(C)]
struct OBJECT_ATTRIBUTES {
    pub length: u32,
    pub root_directory: HANDLE,
    pub object_name: *const UNICODE_STRING,
    pub attributes: u32,
    pub security_descriptor: *mut std::ffi::c_void,
    pub security_quality_of_service: *mut std::ffi::c_void,
}

#[repr(C)]
struct OBJECT_DIRECTORY_INFORMATION {
    pub name: UNICODE_STRING,
    pub type_name: UNICODE_STRING,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtOpenDirectoryObject(
        directory_handle: *mut HANDLE,
        desired_access: u32,
        object_attributes: *const OBJECT_ATTRIBUTES,
    ) -> NTSTATUS;

    fn NtQueryDirectoryObject(
        directory_handle: HANDLE,
        buffer: *mut std::ffi::c_void,
        length: u32,
        return_single_entry: u8,
        restart_scan: u8,
        context: *mut u32,
        return_length: *mut u32,
    ) -> NTSTATUS;

    fn NtClose(handle: HANDLE) -> NTSTATUS;
}

/// Convert a `UNICODE_STRING` safely to `OsString` given its validated buffer and length.
///
/// # Safety
/// The caller must ensure that `unicode_str.Buffer` is non-null, properly aligned,
/// and points to at least `unicode_str.Length` bytes of readable memory.
unsafe fn unicode_string_to_os_string(unicode_str: &UNICODE_STRING) -> Option<OsString> {
    if unicode_str.Buffer.is_null() || unicode_str.Length == 0 {
        return None;
    }
    let u16_len = (unicode_str.Length / 2) as usize;
    let slice = unsafe { std::slice::from_raw_parts(unicode_str.Buffer, u16_len) };
    Some(OsString::from_wide(slice))
}

// according to https://lucasg.github.io/2017/06/07/listing-known-dlls/,
// in contrast to reading the HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager\KnownDLLs key,
// listing \KnownDlls gives us the entire list of DLLs, so we don't need to look up the dependencies of each DLL

use crate::common::LookupError;

/// Returns the complete list of KnownDlls
///
/// These DLLs are mapped into each process' memory as
/// copy-on-write for performance (and a sprinkle of security) reasons.
///
/// They are all located in the system directory and can't be overridden/hijacked.
pub fn get_known_dlls() -> Result<Vec<String>, LookupError> {
    const KNOWN_DLLS_NAME_BUFFER: &[u16] = &[
        b'\\' as u16,
        b'K' as u16,
        b'n' as u16,
        b'o' as u16,
        b'w' as u16,
        b'n' as u16,
        b'D' as u16,
        b'l' as u16,
        b'l' as u16,
        b's' as u16,
    ];

    let known_dlls_name = UNICODE_STRING {
        Length: (KNOWN_DLLS_NAME_BUFFER.len() * size_of::<u16>()) as u16,
        MaximumLength: (KNOWN_DLLS_NAME_BUFFER.len() * size_of::<u16>()) as u16,
        Buffer: KNOWN_DLLS_NAME_BUFFER.as_ptr() as *mut u16,
    };

    let oa = OBJECT_ATTRIBUTES {
        length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        root_directory: null_mut(),
        object_name: &known_dlls_name,
        attributes: 0,
        security_descriptor: null_mut(),
        security_quality_of_service: null_mut(),
    };

    let mut ret = Vec::new();
    let mut known_dll_dir_handle: HANDLE = null_mut();

    let status = unsafe {
        NtOpenDirectoryObject(&mut known_dll_dir_handle, DIRECTORY_QUERY, &oa)
    };

    if status != STATUS_SUCCESS {
        let raw_err = std::io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32);
        tracing::warn!("Failed to open KnownDll directory object: {}", raw_err);
        match status {
            STATUS_INSUFFICIENT_RESOURCES => tracing::warn!("STATUS_INSUFFICIENT_RESOURCES"),
            STATUS_INVALID_PARAMETER => tracing::warn!("STATUS_INVALID_PARAMETER"),
            STATUS_OBJECT_NAME_INVALID => tracing::warn!("STATUS_OBJECT_NAME_INVALID"),
            STATUS_OBJECT_NAME_NOT_FOUND => tracing::warn!("STATUS_OBJECT_NAME_NOT_FOUND"),
            STATUS_OBJECT_PATH_NOT_FOUND => tracing::warn!("STATUS_OBJECT_PATH_NOT_FOUND"),
            STATUS_OBJECT_PATH_SYNTAX_BAD => tracing::warn!("STATUS_OBJECT_PATH_SYNTAX_BAD"),
            _ => tracing::warn!("NTSTATUS error code: 0x{:08X}", status as u32),
        }
        return Ok(ret);
    }

    // RAII handle guard to ensure NtClose is always called
    struct HandleGuard(HANDLE);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    NtClose(self.0);
                }
            }
        }
    }
    let _guard = HandleGuard(known_dll_dir_handle);

    let mut first_time: u8 = 1;
    let mut context: u32 = 0;
    let mut buffer_size: u32 = 0x1000; // 4KB initial buffer
    let mut return_length: u32 = 0;
    let mut buffer_vec: Vec<u8> = vec![0; buffer_size as usize];

    unsafe {
        loop {
            let mut status;
            loop {
                status = NtQueryDirectoryObject(
                    known_dll_dir_handle,
                    buffer_vec.as_mut_ptr() as *mut std::ffi::c_void,
                    buffer_size,
                    0, // return all entries that fit
                    first_time,
                    &mut context,
                    &mut return_length,
                );

                if status == STATUS_BUFFER_TOO_SMALL
                    || (status == STATUS_MORE_ENTRIES
                        && (*(buffer_vec.as_ptr() as *const OBJECT_DIRECTORY_INFORMATION))
                            .name
                            .Buffer
                            .is_null())
                {
                    buffer_size = buffer_size.saturating_mul(2);
                    buffer_vec = vec![0; buffer_size as usize];
                } else {
                    break;
                }
            }

            if status != STATUS_SUCCESS && status != STATUS_MORE_ENTRIES {
                break;
            }

            let entry_size = size_of::<OBJECT_DIRECTORY_INFORMATION>();
            let mut i: usize = 0;
            let buffer_base = buffer_vec.as_ptr();

            while (i + 1) * entry_size <= buffer_vec.len() {
                let info = (buffer_base.add(i * entry_size)) as *const OBJECT_DIRECTORY_INFORMATION;
                if (*info).name.Buffer.is_null() {
                    break;
                }

                if let Some(type_name) = unicode_string_to_os_string(&(*info).type_name) {
                    if type_name == "Section" {
                        if let Some(dll_name) = unicode_string_to_os_string(&(*info).name) {
                            if let Ok(name_str) = dll_name.into_string() {
                                ret.push(name_str);
                            }
                        }
                    }
                }

                i += 1;
            }

            if status != STATUS_MORE_ENTRIES {
                break;
            }

            first_time = 0;
        }
    }

    Ok(ret)
}

#[cfg(test)]
mod tests {
    use crate::common::LookupError;
    use crate::knowndlls::get_known_dlls;

    #[cfg(windows)]
    #[test]
    fn list_known_dlls() -> Result<(), LookupError> {
        let known_dlls = get_known_dlls()?;
        assert!(!known_dlls.is_empty());
        assert!(known_dlls.contains(&"ntdll.dll".to_string()));
        Ok(())
    }
}
