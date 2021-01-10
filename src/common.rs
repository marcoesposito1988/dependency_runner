use thiserror::Error;

use crate::system::WindowsSystem;
use pelite::pe64::{Pe, PeFile};
use serde::Serialize;
use std::collections::hash_map::Values;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

#[derive(Error, Debug)]
pub enum LookupError {
    #[error("Read error")]
    CouldNotOpenFile { source: std::io::Error },

    #[error("PE file parse error")]
    ProcessingError { source: pelite::Error },

    #[error("File system access error while scanning")]
    ScanError(String),

    #[error("Visual Studio User settings file parse error")]
    ParseError(String),

    #[error("Lookup context building error")]
    ContextDeductionError(String),

    #[error(transparent)]
    RegexError(#[from] regex::Error),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    PEError(#[from] pelite::Error),
}

pub struct Query {
    pub system: WindowsSystem,
    pub target_exe: PathBuf,
    pub app_dir: PathBuf,
    pub working_dir: PathBuf,
    pub max_depth: Option<usize>,
    pub skip_system_dlls: bool,
}

impl Query {
    // autodetects the settings with sensible defaults
    #[cfg(windows)]
    pub fn deduce_from_executable_location<P: AsRef<Path>>(
        target_exe: P,
    ) -> Result<Self, LookupError> {
        let app_dir = target_exe
            .as_ref()
            .parent()
            .ok_or(LookupError::ContextDeductionError(
                "Could not find application directory for given executable ".to_owned()
                    + target_exe,
            ))?;
        Ok(Self {
            search_mode: DllSearchMode::SafeDllSearchModeOn,
            system: WindowsSystem::current(),
            target_exe: target_exe.into(),
            app_dir: app_dir.to_owned(),
            working_dir: app_dir.to_owned(),
            max_depth: None,
            skip_system_dlls: true,
        })
    }

    // autodetects the settings with sensible defaults
    #[cfg(not(windows))]
    pub fn deduce_from_executable_location<P: AsRef<Path>>(
        target_exe: P,
    ) -> Result<Self, LookupError> {
        let app_dir = target_exe
            .as_ref()
            .parent()
            .ok_or(LookupError::ContextDeductionError(
                "Could not find application directory for given executable ".to_owned()
                    + target_exe.as_ref().to_str().unwrap_or(""),
            ))?;
        Ok(Self {
            system: WindowsSystem::from_exe_location(&target_exe)?,
            target_exe: target_exe.as_ref().to_owned(),
            app_dir: app_dir.to_owned(),
            working_dir: app_dir.to_owned(),
            max_depth: None,
            skip_system_dlls: true,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutableDetails {
    /// located in a system directory (Win or Sys dir)
    pub is_system: bool,
    /// because it is among the KnownDLLs list, or a dependency thereof
    pub is_known_dll: bool,
    pub folder: PathBuf,
    pub dependencies: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Executable {
    pub name: OsString,
    pub depth_first_appearance: usize,
    pub found: bool,
    pub details: Option<ExecutableDetails>,
}

impl Executable {
    pub fn full_path(&self) -> Option<PathBuf> {
        self.details.as_ref().map(|d| d.folder.join(&self.name))
    }
}

#[derive(Debug, Clone)]
pub struct Executables {
    index: std::collections::HashMap<OsString, Executable>,
}

impl Executables {
    pub fn new() -> Self {
        Self {
            index: std::collections::HashMap::new(),
        }
    }

    pub fn insert(&mut self, lr: Executable) {
        if let Some(filename) = lr.name.to_str() {
            self.index.insert(filename.to_lowercase().into(), lr);
        }
        // self.index.insert(lr.name.to_ascii_lowercase(), lr); // TODO as soon as it's stable
    }

    pub fn get(&self, name: &OsStr) -> Option<&Executable> {
        if let Some(filename) = name.to_str() {
            let lowercase_filename_os: OsString = OsString::from(filename.to_lowercase());
            self.index.get(&lowercase_filename_os)
        } else {
            None
        }
        // self.index.get(&name.to_lowercase()) // TODO as soon as it's stable
    }

    pub fn contains(&self, name: &OsStr) -> bool {
        if let Some(filename) = name.to_str() {
            let lowercase_filename_os: OsString = OsString::from(filename.to_lowercase());
            self.index.contains_key(&lowercase_filename_os)
        } else {
            false
        }
        // self.index.contains_key(&name.to_lowercase())  // TODO as soon as it's stable
    }

    pub fn values(&self) -> Values<'_, OsString, Executable> {
        self.index.values()
    }
}

pub fn read_dependencies<P: AsRef<Path> + ?Sized>(path: &P) -> Result<Vec<String>, LookupError> {
    use LookupError::{CouldNotOpenFile, ProcessingError};
    let path = path.as_ref();
    let map = pelite::FileMap::open(path).map_err(|e| CouldNotOpenFile { source: e })?;
    let file = PeFile::from_bytes(&map).map_err(|e| ProcessingError { source: e })?;

    // Access the import directory
    let imports = file.imports().map_err(|e| ProcessingError { source: e })?;

    let names: Vec<&pelite::util::CStr> = imports
        .iter()
        .map(|desc| desc.dll_name())
        .collect::<Result<Vec<&pelite::util::CStr>, pelite::Error>>()
        .map_err(|e| ProcessingError { source: e })?;
    Ok(names
        .iter()
        .filter_map(|s| s.to_str().ok())
        .map(|s| s.to_string())
        .collect::<Vec<String>>())
}
