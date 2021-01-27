
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


#[cfg(test)]
mod tests {
    use crate::{LookupError, Executables};
    use crate::query::LookupQuery;
    use crate::lookup_path::LookupPath;
    use crate::runner::Runner;
    use std::collections::HashSet;
    use std::iter::FromIterator;
    use std::ffi::OsStr;

    #[test]
    fn empty_executables() -> Result<(), LookupError> {
        let exes = Executables::new();
        assert!(!exes.contains(OsStr::new("NonExistingExecutable.exe")));

        assert!(exes.get(OsStr::new("NonExistingExecutable.exe")).is_none());

        assert!(exes.get_root()?.is_none());

        assert!(exes.sorted_by_first_appearance().is_empty());

        Ok(())
    }

    #[test]
    fn executables() -> Result<(), LookupError> {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = d.join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");

        let mut query = LookupQuery::deduce_from_executable_location(&exe_path)?;
        query.skip_system_dlls = true;
        let context = LookupPath::new(&query);
        let mut runner = Runner::new(&query, context);
        let exes = runner.run()?;

        assert!(exes.contains(OsStr::new("DepRunTest.exe")));
        assert!(exes.contains(OsStr::new("depruntest.exe")));
        assert!(!exes.contains(OsStr::new("NonExistingExecutable.exe")));

        assert!(exes.get(OsStr::new("NonExistingExecutable.exe")).is_none());
        assert!(exes.get(OsStr::new("DepRunTest.exe")).is_some());

        assert_eq!(exes.get_root()?.unwrap().name, "DepRunTest.exe");

        let sorted = exes.sorted_by_first_appearance();
        let sorted_names: HashSet<&str> = sorted.iter()
            .filter(|e| e.details.as_ref().map(|d| !d.is_system).unwrap_or(false))
            .map(|e| e.name.to_str().unwrap()).collect();
        let expected_names: HashSet<&str> = HashSet::from_iter(
            ["DepRunTestLib.dll", "DepRunTest.exe",].iter().map(|&s|s));
        assert_eq!(sorted_names, expected_names);

        let exe_p = exes.get_root()?.unwrap().full_path();
        assert!(exe_p.is_some());
        assert_eq!(exe_p.unwrap(), std::fs::canonicalize(exe_path)?);

        Ok(())
    }
}
