use std::ffi::OsStr;
use std::path::PathBuf;

use crate::common::Query;
use crate::system::WinFileSystemCache;
#[cfg(windows)]
use crate::system::{get_system_directory, get_windows_directory};
use crate::LookupError;

#[derive(Eq, PartialEq, Debug, Clone)]
pub enum ContextEntryType {
    ExecutableDir,
    SystemDir,
    // SystemDir16, // ignored
    WindowsDir,
    WorkingDir,
    UserPath,
}

#[derive(Eq, PartialEq, Debug, Clone)]
pub struct ContextEntry {
    pub dir_type: ContextEntryType,
    pub path: PathBuf,
}

impl ContextEntry {
    pub(crate) fn is_system(&self) -> bool {
        [
            ContextEntryType::WindowsDir,
            ContextEntryType::SystemDir,
            // ContextEntryType::SystemDir16,
        ]
        .contains(&self.dir_type)
    }
}

pub struct ContextLookupResult {
    pub location: ContextEntry,
    pub fullpath: PathBuf,
}

pub struct Context {
    pub entries: Vec<ContextEntry>,
    fs_cache: std::cell::RefCell<WinFileSystemCache>,
}

impl Context {
    pub fn new(query: &Query) -> Self {
        let user_path_entries = query
            .system
            .path
            .as_ref()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|s| ContextEntry {
                dir_type: ContextEntryType::UserPath,
                path: s.clone(),
            })
            .collect::<Vec<_>>();

        let entries = if query.system.safe_dll_search_mode_on.unwrap_or(true) {
            // default mode (assume if not specified)
            let system_entries = vec![
                ContextEntry {
                    dir_type: ContextEntryType::ExecutableDir,
                    path: query.app_dir.clone(),
                },
                ContextEntry {
                    dir_type: ContextEntryType::SystemDir,
                    path: query.system.sys_dir.clone(),
                },
                // TODO: we should resolve API sets properly as in https://lucasg.github.io/2017/10/15/Api-set-resolution/
                // for now, we just add the /downlevel directory and call it a day
                ContextEntry {
                    dir_type: ContextEntryType::SystemDir,
                    path: query.system.sys_dir.join("downlevel"),
                },
                // 16-bit system directory ignored
                ContextEntry {
                    dir_type: ContextEntryType::WindowsDir,
                    path: query.system.win_dir.clone(),
                },
                ContextEntry {
                    dir_type: ContextEntryType::WorkingDir,
                    path: query.working_dir.clone(),
                },
            ];

            [system_entries, user_path_entries].concat()
        } else {
            // if HKEY_LOCAL_MACHINE\System\CurrentControlSet\Control\Session Manager\SafeDllSearchMode is 0
            let system_entries = vec![
                ContextEntry {
                    dir_type: ContextEntryType::ExecutableDir,
                    path: query.app_dir.clone(),
                },
                ContextEntry {
                    dir_type: ContextEntryType::WorkingDir,
                    path: query.working_dir.clone(),
                },
                ContextEntry {
                    dir_type: ContextEntryType::SystemDir,
                    path: query.system.sys_dir.clone(),
                },
                // TODO: we should resolve API sets properly as in https://lucasg.github.io/2017/10/15/Api-set-resolution/
                // for now, we just add the /downlevel directory and call it a day
                ContextEntry {
                    dir_type: ContextEntryType::SystemDir,
                    path: query.system.sys_dir.join("downlevel"),
                },
                // 16-bit system directory ignored
                ContextEntry {
                    dir_type: ContextEntryType::WindowsDir,
                    path: query.system.win_dir.clone(),
                },
            ];

            [system_entries, user_path_entries].concat()
        };

        Self {
            entries,
            fs_cache: std::cell::RefCell::new(WinFileSystemCache::new()),
        }
    }

    // linearize the lookup context into a single vector of directories
    pub fn search_path(&self) -> Vec<PathBuf> {
        let mut ret: Vec<PathBuf> = self.entries.iter().map(|e| e.path.clone()).collect();

        if let Some(sys_dir) = self
            .entries
            .iter()
            .find(|e| e.dir_type == ContextEntryType::SystemDir)
        {
            ret.insert(0, sys_dir.path.join("downlevel")); // TODO: remove hack for API sets
        }

        ret
    }

    // returns the actual full path to the executable, if found
    pub fn search_file(
        &self,
        filename: &OsStr,
    ) -> Result<Option<ContextLookupResult>, LookupError> {
        for e in &self.entries {
            if let Ok(found) = self
                .fs_cache
                .borrow_mut()
                .test_file_in_folder_case_insensitive(filename, e.path.as_ref())
            {
                if let Some(actual_filename) = found {
                    let mut p = std::path::PathBuf::new();
                    p.push(e.path.clone());
                    p.push(actual_filename);
                    return Ok(Some(ContextLookupResult {
                        fullpath: p,
                        location: e.clone(),
                    }));
                }
            }
        }

        Ok(None)
    }
}
