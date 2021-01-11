use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::common::Query;
use crate::context::ContextEntryType::{ExecutableDir, KnownDLLs, SystemDir, UserPath, WindowsDir};
use crate::system::WinFileSystemCache;
use crate::LookupError;

#[derive(Eq, PartialEq, Debug, Clone)]
pub enum ContextEntryType {
    KnownDLLs,
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

            [system_entries, Self::system_path_entries(&query)].concat()
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

            [system_entries, Self::system_path_entries(&query)].concat()
        };

        Self {
            entries,
            fs_cache: std::cell::RefCell::new(WinFileSystemCache::new()),
        }
    }

    fn system_path_entries(q: &Query) -> Vec<ContextEntry> {
        q.system
            .path
            .as_ref()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|s| ContextEntry {
                dir_type: ContextEntryType::UserPath,
                path: s.clone(),
            })
            .collect::<Vec<_>>()
    }

    fn dwp_string_to_context_entry(s: &str, q: &Query) -> Result<Vec<ContextEntry>, LookupError> {
        match s {
            "SxS" => Ok(vec![]), // TODO?
            "KnownDLLs" => Ok(vec![ContextEntry {
                dir_type: KnownDLLs,
                path: PathBuf::new(),
            }]),
            "AppDir" => Ok(vec![ContextEntry {
                dir_type: ExecutableDir,
                path: q.app_dir.clone(),
            }]),
            "32BitSysDir" => Ok(vec![ContextEntry {
                dir_type: SystemDir,
                path: q.system.sys_dir.clone(),
            }]),
            "16BitSysDir" => Ok(vec![]), // ignored
            "OSDir" => Ok(vec![ContextEntry {
                dir_type: WindowsDir,
                path: q.system.win_dir.clone(),
            }]),
            "AppPath" => Ok(vec![]), // TODO? https://docs.microsoft.com/en-us/windows/win32/shell/app-registration
            "SysPath" => Ok(vec![ContextEntry {
                dir_type: UserPath,
                path: q.system.win_dir.clone(),
            }]),
            _ if s.starts_with("UserDir ") => Ok(vec![ContextEntry {
                dir_type: UserPath,
                path: PathBuf::from(&s[8..]),
            }]),
            _ => Err(LookupError::ParseError(format!(
                "Unknown key in dwp file: {}",
                s
            ))),
        }
    }

    pub fn from_dwp_file<P: AsRef<Path>>(dwp_path: P, q: &Query) -> Result<Self, LookupError> {
        // https://www.dependencywalker.com/help/html/path_files.htm
        let comment_chars = [':', ';', '/', '\'', '#'];
        let lines: Vec<String> = std::fs::read_to_string(dwp_path)?
            .lines()
            .filter(|s| !(s.is_empty() || comment_chars.contains(&s.chars().nth(0).unwrap())))
            .map(str::to_owned)
            .collect();
        let entries_vecs = lines
            .iter()
            .map(|e| Self::dwp_string_to_context_entry(e, q))
            .collect::<Result<Vec<Vec<ContextEntry>>, LookupError>>()?;
        Ok(Self {
            entries: entries_vecs.concat(),
            fs_cache: std::cell::RefCell::new(WinFileSystemCache::new()),
        })
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
