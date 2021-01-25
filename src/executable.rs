
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use serde::Serialize;
use crate::LookupError;

/// Metadata for a found executable file
#[derive(Debug, Clone, Serialize)]
pub struct ExecutableDetails {
    /// located in a system directory (Win or Sys dir)
    pub is_system: bool,
    /// it is among the KnownDLLs list, or a dependency thereof
    pub is_known_dll: bool,
    /// containing folder
    pub folder: PathBuf,
    /// names of the DLLs this executable file depends on
    pub dependencies: Option<Vec<String>>,
}

/// Information about a DLL that was mentioned as target for the search
#[derive(Debug, Clone, Serialize)]
pub struct Executable {
    /// name as it appears in the import table
    pub name: OsString,
    /// depth at which the file was first mentioned in the dependency tree
    pub depth_first_appearance: usize,
    /// if the file was found on the PATH
    pub found: bool,
    /// metadata extracted from the actual executable file
    pub details: Option<ExecutableDetails>,
}

impl Executable {
    pub fn full_path(&self) -> Option<PathBuf> {
        self.details.as_ref().map(|d| d.folder.join(&self.name))
    }
}

/// Collection of Executable objects, result of a DLL search
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

    pub fn get_root(&self) -> Result<Option<&Executable>, LookupError> {
        if self.index.is_empty() {
            return Ok(None);
        }
        let root_candidates: Vec<&Executable> = self.index.values().filter(|v| v.depth_first_appearance == 0).collect();
        if root_candidates.is_empty() {
            return Err(LookupError::ScanError(format!("The executable tree has no roots")));
        }
        if root_candidates.len() > 1 {
            let names: Vec<&str> =  root_candidates.iter().map(|n| n.name.to_str().unwrap_or_default()).collect();
            return Err(LookupError::ScanError(format!(
                "The executable tree has multiple roots: {}",
               names.join(";"))));
        }
        Ok(root_candidates.first().map(|&e| e))
    }

    pub fn sorted_by_first_appearance(&self) -> Vec<&Executable> {
        let mut sorted_executables: Vec<_> = self.index.values().collect();
        sorted_executables
            .sort_by(|e1, e2| e1.depth_first_appearance.cmp(&e2.depth_first_appearance));
        sorted_executables
    }
}