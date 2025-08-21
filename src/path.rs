//! This is the workhorse of the library: the LookupPath contains the list of possible locations for 
//! a dependency, performs the actual lookup and caching of the results and of all filesystem access.

use crate::apiset;
use crate::common::LookupError;
use crate::query::LookupQuery;
use crate::system::{KnownDLLList, WinFileSystemCache, WindowsSystem};
#[cfg(windows)]
use fs_err as fs;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Directory/set of DLLs to be searched, and relative metadata
#[derive(Eq, PartialEq, Debug, Clone)]
pub enum LookupPathEntry<'a> {
    /// The DLL is implicitly loaded by the OS for every process and not looked up every time
    KnownDLLs(&'a KnownDLLList),
    /// Directory where the root executable sits
    ExecutableDir(PathBuf),
    /// Directory containing the "proxy" DLLs that implement the API set feature
    ApiSet(&'a apiset::ApisetMap),
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

impl<'a> LookupPathEntry<'a> {
    pub fn is_system(&self) -> bool {
        matches!(
            self,
            Self::KnownDLLs(_) | Self::ApiSet(_) | Self::WindowsDir(_) | Self::SystemDir(_)
        )
    }

    pub fn get_path(&self) -> Option<PathBuf> {
        match self {
            // we have a fixed list, no need to scan
            Self::KnownDLLs(_) => None,
            Self::ApiSet(_) => None,
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
pub struct LookupResult<'a> {
    pub location: LookupPathEntry<'a>,
    pub fullpath: PathBuf,
}

/// Linearized lookup path
/// Contains a list of entries that describes the logic used by the operating system to resolve a
/// DLL/executable name. Such entries can correspond to a physical location, such as one or more
/// directories containing DLL libraries, or to a collection of virtual mappings from a DLL name
/// to one or more actual executable files.
/// The path is computed from the user-provided query before scanning the dependency tree. It acts
/// as a reification of the computed path itself, as an interface to look up executables across it
/// and a cache for the metadata of the DLLs found through it.
pub struct LookupPath<'a> {
    /// Sorted list of directories to be looked up when searching for a DLL
    /// It is built from a query, depending on the current system configuration
    /// (availability of a Windows root, and its configuration that influences the lookup)
    pub entries: Vec<LookupPathEntry<'a>>,
    /// Cache of file lookup on disk
    /// (filesystem access is the true bottleneck in DLL dependency resolution)
    fs_cache: std::cell::RefCell<WinFileSystemCache>,
}

impl<'a> LookupPath<'a> {
    /// Deduces the lookup path from the given user query applying sensible defaults
    /// The user can still manipulate the entries afterward in a manual fashion
    pub fn deduce(query: &'a LookupQuery) -> Self {
        let entries = if let Some(system) = query.system.as_ref() {
            let knowndlls_entry = if let Some(known_dlls) = system.known_dlls.as_ref() {
                vec![LookupPathEntry::KnownDLLs(known_dlls)]
            } else {
                vec![]
            };
            let apiset_entry = if let Some(apiset_map) = system.apiset_map.as_ref() {
                vec![LookupPathEntry::ApiSet(apiset_map)]
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
                    vec![LookupPathEntry::ExecutableDir(query.target.app_dir.clone())],
                    system_entries,
                    vec![LookupPathEntry::WorkingDir(
                        query.target.working_dir.clone(),
                    )],
                    Self::system_path_entries(system),
                    Self::user_path_entries(query),
                ]
                .concat()
            } else {
                // if HKEY_LOCAL_MACHINE\System\CurrentControlSet\Control\Session Manager\SafeDllSearchMode is 0
                [
                    knowndlls_entry,
                    apiset_entry,
                    vec![
                        LookupPathEntry::ExecutableDir(query.target.app_dir.clone()),
                        LookupPathEntry::WorkingDir(query.target.working_dir.clone()),
                    ],
                    system_entries,
                    Self::system_path_entries(system),
                    Self::user_path_entries(query),
                ]
                .concat()
            }
        } else {
            [
                vec![
                    LookupPathEntry::ExecutableDir(query.target.app_dir.clone()),
                    LookupPathEntry::WorkingDir(query.target.working_dir.clone()),
                ],
                Self::user_path_entries(query),
            ]
            .concat()
        };

        Self {
            // system: sys,
            entries,
            fs_cache: std::cell::RefCell::new(WinFileSystemCache::new()),
        }
    }

    /// Parse an entry in a .dwp file
    #[cfg(windows)]
    fn dwp_string_to_context_entry(
        s: &str,
        q: &'a LookupQuery,
    ) -> Result<Vec<LookupPathEntry<'a>>, LookupError> {
        if s.is_empty() {
            return Ok(vec![]);
        }
        if [':', ';', '/', '\'', '#'].contains(&s.chars().next().unwrap()) {
            // comment
            return Ok(vec![]);
        }
        match s {
            "SxS" => Ok(vec![]), // TODO?
            "KnownDLLs" => {
                if let Some(kd) = q.system.as_ref().and_then(|s| s.known_dlls.as_ref()) {
                    Ok(vec![LookupPathEntry::KnownDLLs(kd)])
                } else {
                    Ok(vec![])
                }
            }
            "AppDir" => Ok(vec![LookupPathEntry::ExecutableDir(
                q.target.app_dir.clone(),
            )]),
            "32BitSysDir" => Ok(if let Some(system) = &q.system {
                vec![LookupPathEntry::SystemDir(system.sys_dir.clone())]
            } else {
                vec![]
            }),
            "16BitSysDir" => Ok(vec![]), // ignored
            "OSDir" => Ok(if let Some(system) = &q.system {
                vec![LookupPathEntry::WindowsDir(system.win_dir.clone())]
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

    /// Build a LookupPath from the content of a Dependency Walker .dwp file
    #[cfg(windows)]
    pub fn from_dwp_file<P: AsRef<Path>>(
        dwp_path: P,
        query: &'a LookupQuery,
    ) -> Result<Self, LookupError> {
        // https://www.dependencywalker.com/help/html/path_files.htm
        let comment_chars = [':', ';', '/', '\'', '#'];
        let lines: Vec<String> = fs::read_to_string(dwp_path)?
            .lines()
            .filter(|s| !(s.is_empty() || comment_chars.contains(&s.chars().next().unwrap())))
            .map(str::to_owned)
            .collect();
        let entries_vecs = lines
            .iter()
            .map(|e| Self::dwp_string_to_context_entry(e, &query))
            .collect::<Result<Vec<Vec<LookupPathEntry>>, LookupError>>()?;
        Ok(Self {
            entries: entries_vecs.concat(),
            fs_cache: std::cell::RefCell::new(WinFileSystemCache::new()),
        })
    }

    /// linearize the lookup context into a single vector of directories
    pub fn search_path(&self) -> Vec<PathBuf> {
        self.entries.iter().flat_map(|e| e.get_path()).collect()
    }

    /// look for a DLL by name across the entries
    pub fn search_dll(&self, library: &str) -> Result<Option<LookupResult<'_>>, LookupError> {
        for e in &self.entries {
            match e {
                LookupPathEntry::KnownDLLs(kd) => {
                    if let Ok(Some(lp)) = kd.search_dll_in_known_dlls(library) {
                        let ret = Some(LookupResult {
                            location: LookupPathEntry::KnownDLLs(kd),
                            fullpath: lp,
                        });
                        return Ok(ret);
                    }
                }
                LookupPathEntry::ApiSet(apis) => {
                    let apiset_name = library.to_lowercase().trim_end_matches(".dll").to_owned();
                    if apis.contains_key(&apiset_name) {
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
                                    location: e.clone(),
                                    fullpath: p?,
                                })
                            });
                        }
                    }
                }
                LookupPathEntry::ExecutableDir(p)
                | LookupPathEntry::SystemDir(p)
                | LookupPathEntry::WindowsDir(p)
                | LookupPathEntry::SystemPath(p)
                | LookupPathEntry::UserPath(p)
                | LookupPathEntry::WorkingDir(p) => {
                    if let Some(r) = self.search_file_in_folder(OsStr::new(library), p)? {
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

    /// Look for a DLL in a concrete filesystem folder
    fn search_file_in_folder<P: AsRef<Path>>(
        &self,
        filename: &OsStr,
        p: P,
    ) -> Result<Option<PathBuf>, LookupError> {
        self.fs_cache
            .borrow_mut()
            .test_file_in_folder_case_insensitive(filename, p.as_ref())
    }

    /// Get the PATH entries specified by the system
    fn system_path_entries(system: &'_ WindowsSystem) -> Vec<LookupPathEntry<'_>> {
        system
            .system_path
            .as_ref()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|s| LookupPathEntry::SystemPath(s.clone()))
            .collect::<Vec<_>>()
    }

    /// Get the PATH entries that were provided by the user when running the program
    fn user_path_entries(q: &'_ LookupQuery) -> Vec<LookupPathEntry<'_>> {
        q.target
            .user_path
            .iter()
            .map(|s| LookupPathEntry::UserPath(s.clone()))
            .collect::<Vec<_>>()
    }
}

#[cfg(windows)]
#[cfg(test)]
mod tests {
    use crate::common::LookupError;
    use crate::path::{LookupPath, LookupPathEntry};
    use crate::query::LookupQuery;

    #[test]
    fn parse_dwp() -> Result<(), LookupError> {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let relative_path = "test_data/dwp/lookup_path.dwp";
        let dwp_file_path = d.join(relative_path);

        let exe_relative_path =
            "test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe";
        let exe_path = d.join(exe_relative_path);

        let query = LookupQuery::deduce_from_executable_location(&exe_path)?;

        let path = LookupPath::from_dwp_file(&dwp_file_path, &query)?;

        if query.system.is_some() {
            assert!(std::matches!(path.entries.first().unwrap(), LookupPathEntry::KnownDLLs(_)));
            assert!(std::matches!(path.entries[1], LookupPathEntry::ExecutableDir(_)));
            assert!(std::matches!(path.entries[2], LookupPathEntry::SystemDir(_)));
            assert!(std::matches!(path.entries[3], LookupPathEntry::WindowsDir(_)));
            assert!(std::matches!(path.entries[4], LookupPathEntry::UserPath(_)));
        } else {
            assert!(std::matches!(path.entries.first().unwrap(), LookupPathEntry::KnownDLLs(_)));
            assert!(std::matches!(path.entries[1], LookupPathEntry::ExecutableDir(_)));
            assert!(std::matches!(path.entries[2], LookupPathEntry::SystemDir(_)));
        }

        Ok(())
    }
}
