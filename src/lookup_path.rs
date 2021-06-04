use crate::query::LookupQuery;
use crate::system::WinFileSystemCache;
use crate::LookupError;
#[cfg(windows)]
use fs_err as fs;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

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
/// It is built from a query, depending on the current system configuration
/// (availability of a Windows root, and its configuration that influences the lookup)
pub struct LookupPath {
    pub query: LookupQuery,
    pub entries: Vec<LookupPathEntry>,
    fs_cache: std::cell::RefCell<WinFileSystemCache>,
}

impl LookupPath {
    pub fn new(query: LookupQuery) -> Self {
        let entries = if let Some(system) = &query.system {
            let knowndlls_entry = if system.known_dlls.is_some() {
                vec![LookupPathEntry::KnownDLLs]
            } else {
                vec![]
            };
            let apiset_entry = if system.apiset_map.is_some() {
                vec![LookupPathEntry::ApiSet]
            } else {
                vec![]
            };
            let system_entries = vec![
                LookupPathEntry::SystemDir(system.sys_dir.clone()),
                // 16-bit system directory ignored
                LookupPathEntry::WindowsDir(system.win_dir.clone()),
            ];

            if system.safe_dll_search_mode_on.unwrap_or(true) {
                // default mode (assume if not specified)
                [
                    knowndlls_entry,
                    apiset_entry,
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
                    knowndlls_entry,
                    apiset_entry,
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
            query,
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
        query: LookupQuery,
    ) -> Result<Self, LookupError> {
        // https://www.dependencywalker.com/help/html/path_files.htm
        let comment_chars = [':', ';', '/', '\'', '#'];
        let lines: Vec<String> = fs::read_to_string(dwp_path)?
            .lines()
            .filter(|s| !(s.is_empty() || comment_chars.contains(&s.chars().nth(0).unwrap())))
            .map(str::to_owned)
            .collect();
        let entries_vecs = lines
            .iter()
            .map(|e| Self::dwp_string_to_context_entry(e, &query))
            .collect::<Result<Vec<Vec<LookupPathEntry>>, LookupError>>()?;
        Ok(Self {
            query,
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
        for e in &self.entries {
            match e {
                LookupPathEntry::KnownDLLs => {
                    if let Ok(Some(ret)) = self.search_dll_in_known_dlls(library) {
                        return Ok(Some(ret));
                    }
                }
                LookupPathEntry::ApiSet => {
                    if let Ok(Some(ret)) = self.search_dll_in_apiset_map(library) {
                        return Ok(Some(ret));
                    }
                }
                LookupPathEntry::ExecutableDir(p)
                | LookupPathEntry::SystemDir(p)
                | LookupPathEntry::WindowsDir(p)
                | LookupPathEntry::SystemPath(p)
                | LookupPathEntry::UserPath(p)
                | LookupPathEntry::WorkingDir(p) => {
                    if let Some(r) = self.search_file_in_folder(OsStr::new(library), &p)? {
                        return Ok(Some(LookupResult {
                            location: e.clone(),
                            fullpath: r,
                        }));
                    }
                }
            }
        }
        Ok(None)
    }

    // looks for a DLL by name
    // first looks in the known dlls, then in the api set, then in the concrete entries
    fn search_dll_in_known_dlls(&self, library: &str) -> Result<Option<LookupResult>, LookupError> {
        if let Some(kd) = self
            .query
            .system
            .as_ref()
            .and_then(|s| s.known_dlls.as_ref())
        {
            if let Some(lp) = kd.get(&library.to_ascii_lowercase()) {
                return Ok(Some(LookupResult {
                    location: LookupPathEntry::KnownDLLs,
                    fullpath: lp.clone(),
                }));
            } else {
                // DLL not found among the KnownDLLs
                Ok(None)
            }
        } else {
            // TODO: error? we don't have a known dlls list available, so there should be no entry to lead us here
            Ok(None)
        }
    }

    // looks for a DLL by name
    // first looks in the known dlls, then in the api set, then in the concrete entries
    fn search_dll_in_apiset_map(&self, library: &str) -> Result<Option<LookupResult>, LookupError> {
        // API set: return location of DLL on disk, although useless, to show it in the results
        if let Some(apisetmap) = self
            .query
            .system
            .as_ref()
            .and_then(|s| s.apiset_map.as_ref())
        {
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
                } else {
                    // TODO error? we don't have access to a system32 directory, so we shouldn't have access to an apiset map either
                    Ok(None)
                }
            } else {
                // not found
                Ok(None)
            }
        } else {
            // TODO: error? we don't have an apisetmap available, so there should be no entry to lead us here
            Ok(None)
        }
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
