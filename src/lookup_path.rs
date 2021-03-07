use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::apiset::ApisetMap;
use crate::query::LookupQuery;
use crate::system::WinFileSystemCache;
use crate::LookupError;

/// Directory/set of DLLs to be searched, and relative metadata
#[derive(Eq, PartialEq, Debug, Clone)]
pub enum LookupPathEntry {
    /// The DLL is implicitely loaded by the OS for every process, and not looked up every time
    KnownDLLs,
    /// Directory where the root executable sits
    ExecutableDir(PathBuf),
    /// Directory containing the "proxy" DLLs that implement the API set feature
    ApiSet,
    /// Windows System directory (typically C:\Windows\System32)
    SystemDir(PathBuf),
    // SystemDir16, // ignored
    /// Windows directory (typically C:\Windows)
    WindowsDir(PathBuf),
    /// Working directory of the (virtual) process whose DLL lookup we are simulating
    WorkingDir(PathBuf),
    /// PATH as specified by the system (value PATH variable in the shell executing the process)
    SystemPath(PathBuf),
    /// Additional path entries specified by the user
    UserPath(PathBuf),
}

impl LookupPathEntry {
    pub(crate) fn is_system(&self) -> bool {
        match self {
            Self::KnownDLLs => true,
            Self::ApiSet => true,
            Self::WindowsDir(_) => true,
            Self::SystemDir(_) => true,
            _ => false,
        }
    }

    pub(crate) fn get_path(&self) -> Option<PathBuf> {
        match self {
            // we have a fixed list, no need to scan
            Self::KnownDLLs => None,
            Self::ApiSet => None,
            // else
            Self::ExecutableDir(p)
            | Self::SystemDir(p)
            | Self::WindowsDir(p)
            | Self::WorkingDir(p)
            | Self::SystemPath(p)
            | Self::UserPath(p) => Some(p.clone()),
        }
    }
}

/// Full location of a DLL found during lookup
pub struct LookupResult {
    pub location: LookupPathEntry,
    pub fullpath: PathBuf,
}

/// Sorted list of directories to be looked up when searching for a DLL
pub struct LookupPath {
    pub(crate) apiset_map: Option<ApisetMap>,
    pub entries: Vec<LookupPathEntry>,
    fs_cache: std::cell::RefCell<WinFileSystemCache>,
}

impl LookupPath {
    pub fn new(query: &LookupQuery) -> Self {
        let entries = if let Some(system) = &query.system {
            let system_entries = vec![
                LookupPathEntry::SystemDir(system.sys_dir.clone()),
                // 16-bit system directory ignored
                LookupPathEntry::WindowsDir(system.win_dir.clone()),
            ];

            if system.safe_dll_search_mode_on.unwrap_or(true) {
                // default mode (assume if not specified)
                [
                    vec![LookupPathEntry::ExecutableDir(query.app_dir.clone())],
                    system_entries,
                    vec![LookupPathEntry::WorkingDir(query.working_dir.clone())],
                    Self::system_path_entries(&query),
                    Self::user_path_entries(&query),
                ]
                .concat()
            } else {
                // if HKEY_LOCAL_MACHINE\System\CurrentControlSet\Control\Session Manager\SafeDllSearchMode is 0
                [
                    vec![
                        LookupPathEntry::ExecutableDir(query.app_dir.clone()),
                        LookupPathEntry::WorkingDir(query.working_dir.clone()),
                    ],
                    system_entries,
                    Self::system_path_entries(&query),
                    Self::user_path_entries(&query),
                ]
                .concat()
            }
        } else {
            [
                vec![
                    LookupPathEntry::ExecutableDir(query.app_dir.clone()),
                    LookupPathEntry::WorkingDir(query.working_dir.clone()),
                ],
                Self::system_path_entries(&query),
                Self::user_path_entries(&query),
            ]
            .concat()
        };

        Self {
            apiset_map: query.system.as_ref().and_then(|s| s.apiset_map.clone()),
            entries,
            fs_cache: std::cell::RefCell::new(WinFileSystemCache::new()),
        }
    }

    /// Get the PATH entries specified by the system
    fn system_path_entries(q: &LookupQuery) -> Vec<LookupPathEntry> {
        if let Some(system) = &q.system {
            system
                .system_path
                .as_ref()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|s| LookupPathEntry::SystemPath(s.clone()))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    }

    /// Get the PATH entries that were provided by the user when running the program
    fn user_path_entries(q: &LookupQuery) -> Vec<LookupPathEntry> {
        q.user_path
            .iter()
            .map(|s| LookupPathEntry::UserPath(s.clone()))
            .collect::<Vec<_>>()
    }

    #[cfg(windows)]
    /// Parse an entry in a .dwp file
    fn dwp_string_to_context_entry(
        s: &str,
        q: &LookupQuery,
    ) -> Result<Vec<LookupPathEntry>, LookupError> {
        match s {
            "SxS" => Ok(vec![]), // TODO?
            "KnownDLLs" => Ok(vec![LookupPathEntry::KnownDLLs]),
            "AppDir" => Ok(vec![LookupPathEntry::ExecutableDir(q.app_dir.clone())]),
            "32BitSysDir" => Ok(if let Some(system) = &q.system {
                vec![LookupPathEntry::SystemDir(system.sys_dir.clone())]
            } else {
                vec![]
            }),
            "16BitSysDir" => Ok(vec![]), // ignored
            "OSDir" => Ok(if let Some(system) = &q.system {
                vec![LookupPathEntry::SystemDir(system.win_dir.clone())]
            } else {
                vec![]
            }),
            "AppPath" => Ok(vec![]), // TODO? https://docs.microsoft.com/en-us/windows/win32/shell/app-registration
            "SysPath" => Ok(
                if let Some(path) = &q.system.as_ref().and_then(|s| s.system_path.as_ref()) {
                    path.iter()
                        .map(|e| LookupPathEntry::UserPath(e.clone()))
                        .collect()
                } else {
                    vec![]
                },
            ),
            _ if s.starts_with("UserDir ") => {
                Ok(vec![LookupPathEntry::UserPath(PathBuf::from(&s[8..]))])
            }
            _ => Err(LookupError::ParseError(format!(
                "Unknown key in dwp file: {}",
                s
            ))),
        }
    }

    #[cfg(windows)]
    /// Build a LookupPath from the content of a Dependency Walker .dwp file
    pub fn from_dwp_file<P: AsRef<Path>>(
        dwp_path: P,
        q: &LookupQuery,
    ) -> Result<Self, LookupError> {
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
            .collect::<Result<Vec<Vec<LookupPathEntry>>, LookupError>>()?;
        Ok(Self {
            apiset_map: q.system.as_ref().and_then(|s| s.apiset_map.clone()),
            entries: entries_vecs.concat(),
            fs_cache: std::cell::RefCell::new(WinFileSystemCache::new()),
        })
    }

    // linearize the lookup context into a single vector of directories
    pub fn search_path(&self) -> Vec<PathBuf> {
        self.entries.iter().flat_map(|e| e.get_path()).collect()
    }

    // looks for a DLL by name
    // first looks in the known dlls, then in the api set, then in the concrete entries
    pub fn search_dll(&self, library: &str) -> Result<Option<LookupResult>, LookupError> {
        // if known_dlls.contains(library) {
        //     return known_dlls[library];
        // }
        // API set: return location of DLL on disk, although useless, to show it in the results
        if let Some(apisetmap) = self.apiset_map.as_ref() {
            let apiset_dll_name = library.to_lowercase();
            if apisetmap.contains_key(apiset_dll_name.trim_end_matches(".dll")) {
                if let Some(system32_dir) = self
                    .entries
                    .iter()
                    .find(|e| std::matches!(e, LookupPathEntry::SystemDir(_)))
                {
                    let p = self.search_file_in_folder(
                        OsStr::new(library),
                        system32_dir.get_path().unwrap().join("downlevel"),
                    );
                    return p.map(|p| {
                        Some(LookupResult {
                            location: LookupPathEntry::ApiSet,
                            fullpath: p?,
                        })
                    });
                }
            }
        }
        // search file in the lookup path as usual
        self.search_file(OsStr::new(library))
    }

    // returns the actual full path to the executable, if found
    fn search_file(&self, filename: &OsStr) -> Result<Option<LookupResult>, LookupError> {
        for e in &self.entries {
            if let Some(p) = e.get_path() {
                if let Some(r) = self.search_file_in_folder(filename, &p)? {
                    return Ok(Some(LookupResult {
                        location: e.clone(),
                        fullpath: r,
                    }));
                }
            }
        }

        Ok(None)
    }

    fn search_file_in_folder<P: AsRef<Path>>(
        &self,
        filename: &OsStr,
        p: P,
    ) -> Result<Option<PathBuf>, LookupError> {
        self.fs_cache
            .borrow_mut()
            .test_file_in_folder_case_insensitive(filename, p.as_ref())
    }
}
