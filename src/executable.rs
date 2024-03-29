//! This crate contains the data structures used to hold the results of a dependency scan
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::Serialize;

use crate::common::{LookupError, readable_canonical_path};

/// Information about a DLL that was mentioned as target for the search
/// If the file was actually found, additional info is available. Otherwise it represents a
/// missing/broken dependency.
#[derive(Debug, Clone, Serialize)]
pub struct Executable {
    /// Name as it appears in the import table
    pub dllname: String,
    /// depth at which the file was first mentioned in the dependency tree
    pub depth_first_appearance: usize,
    /// if the file was found on the PATH
    pub found: bool,
    /// metadata extracted from the actual executable file
    pub details: Option<ExecutableDetails>,
}

/// Metadata for a found executable file
#[derive(Debug, Clone, Serialize)]
pub struct ExecutableDetails {
    /// virtual DLL which just forwards to an implementation
    pub is_api_set: bool,
    /// located in a system directory (Win or Sys dir)
    pub is_system: bool,
    /// it is among the KnownDLLs list, or a dependency thereof
    pub is_known_dll: bool,
    /// full path
    pub full_path: PathBuf,
    /// names of the DLLs this executable file depends on
    pub dependencies: Option<Vec<String>>,
    /// Symbols import / export table
    pub symbols: Option<ExecutableSymbols>,
}

/// Symbols information for a found executable file
#[derive(Debug, Clone, Serialize)]
pub struct ExecutableSymbols {
    /// Exported symbols
    pub exported: HashSet<String>,
    /// Imported symbols, grouped by DLL
    pub imported: HashMap<String, HashSet<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutablesCheckReport {
    /// Map from dependent to list of non found dependees
    pub not_found_libraries: HashMap<String, HashSet<String>>,
    /// Map from importer to list of non found imported symbols, grouped by dependent DLL
    pub not_found_symbols: Option<HashMap<String, HashMap<String, HashSet<String>>>>,
}

impl Default for ExecutablesCheckReport {
    fn default() -> Self {
        ExecutablesCheckReport::new()
    }
}

impl ExecutablesCheckReport {
    pub fn new() -> Self {
        Self {
            not_found_libraries: HashMap::new(),
            not_found_symbols: None,
        }
    }

    pub fn extend(&mut self, other: ExecutablesCheckReport) {
        self.not_found_libraries.extend(other.not_found_libraries);

        if let Some(other_symbols) = other.not_found_symbols {
            if let Some(our_symbols) = self.not_found_symbols.as_mut() {
                our_symbols.extend(other_symbols)
            } else {
                self.not_found_symbols = Some(other_symbols)
            }
        }
    }
}

/// Collection of Executable objects, result of a DLL search
#[derive(Debug, Clone)]
pub struct Executables {
    index: HashMap<String, Executable>,
}

impl Default for Executables {
    fn default() -> Self {
        Executables::new()
    }
}

impl Executables {
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn get(&self, dllname: &str) -> Option<&Executable> {
        self.index.get(&dllname.to_lowercase())
    }

    pub fn contains(&self, dllname: &str) -> bool {
        self.index.contains_key(&dllname.to_lowercase())
    }

    /// Get the root executable file (i.e. the only one with depth equal to zero)
    pub fn get_root(&self) -> Result<Option<&Executable>, LookupError> {
        if self.index.is_empty() {
            return Ok(None);
        }
        let root_candidates: Vec<&Executable> = self
            .index
            .values()
            .filter(|v| v.depth_first_appearance == 0)
            .collect();
        if root_candidates.is_empty() {
            return Err(LookupError::ScanError(
                "The executable tree has no roots".to_string(),
            ));
        }
        if root_candidates.len() > 1 {
            let names: Vec<&str> = root_candidates.iter().map(|n| n.dllname.as_ref()).collect();
            return Err(LookupError::ScanError(format!(
                "The executable tree has multiple roots: {}",
                names.join(";")
            )));
        }
        Ok(root_candidates.first().copied())
    }

    pub fn sorted_by_first_appearance(&self) -> Vec<&Executable> {
        let mut sorted_executables: Vec<_> = self.index.values().collect();
        sorted_executables
            .sort_by(|e1, e2| e1.depth_first_appearance.cmp(&e2.depth_first_appearance));
        sorted_executables
    }

    /// Add a new executable
    pub(crate) fn insert(&mut self, new_exe: Executable) {
        if let Some(older_finding) = self.get(&new_exe.dllname) {
            eprintln!(
                "Found two DLLs with the same name! {:?} and {:?}",
                new_exe
                    .details
                    .as_ref()
                    .and_then(|d| readable_canonical_path(&d.full_path).ok())
                    .unwrap_or(new_exe.dllname),
                older_finding
                    .details
                    .as_ref()
                    .and_then(|d| readable_canonical_path(&d.full_path).ok())
                    .unwrap_or_else(|| older_finding.dllname.clone()),
            );
        } else {
            self.index.insert(new_exe.dllname.to_lowercase(), new_exe);
        }
    }

    /// Check that all referenced DLLs are found, and (if available) that imported symbols are present
    pub fn check(&self, extract_symbols: bool) -> Result<ExecutablesCheckReport, LookupError> {
        let mut report = ExecutablesCheckReport::new();

        if extract_symbols {
            let symbols_report = self
                .index
                .values()
                .map(|e| self.check_imports(&e.dllname))
                .fold(ExecutablesCheckReport::new(), |mut r, pr| {
                    if let Ok(rr) = pr {
                        r.extend(rr);
                    }
                    r
                });
            report.extend(symbols_report);
        }

        Ok(report)
    }

    /// Check that every dependency exports the symbols imported by this file
    fn check_imports(&self, name: &str) -> Result<ExecutablesCheckReport, LookupError> {
        let exe = self
            .get(name)
            .ok_or_else(|| LookupError::ScanError(format!("Could not find file {name}")))?;

        if exe.details.as_ref().map(|d| d.is_api_set).unwrap_or(true) {
            return Ok(ExecutablesCheckReport::new());
        }

        let imported_symbols = &exe
            .details
            .as_ref()
            .ok_or_else(|| {
                LookupError::ScanError(format!("Could not find details for file {name}"))
            })?
            .symbols
            .as_ref()
            .ok_or_else(|| {
                LookupError::ScanError(format!("Could not find symbols for file {name}"))
            })?
            .imported;

        let mut missing_imports = ExecutablesCheckReport::new();

        for dll_name in imported_symbols.keys() {
            if let Some(dll_exe) = self.get(dll_name) {
                // TODO: following should distinguish if not found (in case report missing library), or if system/api set
                if dll_exe.found {
                    if !dll_exe
                        .details
                        .as_ref()
                        .map(|d| d.is_system)
                        .unwrap_or(true)
                    {
                        let res = self.check_symbols(name, dll_name)?;
                        missing_imports.extend(res);
                    }
                } else {
                    missing_imports
                        .not_found_libraries
                        .entry(name.to_owned())
                        .or_default()
                        .insert(dll_name.clone());
                }
            } else {
                // TODO: it was not looked up
            }
        }

        Ok(missing_imports)
    }

    fn get_notfound_children(&self, e: &Executable) -> Vec<Executable> {
        if !e.found {
            return vec![e.clone()];
        }

        if let Some(details) = &e.details {
            if let Some(dependencies) = &details.dependencies {
                let mut deps_with_notfound_children: Vec<Executable> = dependencies
                    .iter()
                    .flat_map(|d| {
                        if let Some(c) = self.get(d) {
                            self.get_notfound_children(c)
                        } else {
                            vec![]
                        }
                    })
                    .collect();

                if !deps_with_notfound_children.is_empty() {
                    deps_with_notfound_children.extend(vec![e.clone()])
                }
                deps_with_notfound_children
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    }

    pub fn filter_only_notfound(&self) -> Result<Executables, LookupError> {
        let mut ret = Executables::new();

        if let Some(root) = self.get_root()? {
            for e in self.get_notfound_children(root) {
                ret.insert(e.clone())
            }
        }

        Ok(ret)
    }

    /// Check that the exporting DLL has all symbols imported by the importing executable file
    fn check_symbols(
        &self,
        importer: &str,
        exporter: &str,
    ) -> Result<ExecutablesCheckReport, LookupError> {
        let exe = self
            .get(importer)
            .ok_or_else(|| LookupError::ScanError(format!("Could not find file {importer}")))?;
        let imported_symbols = &exe
            .details
            .as_ref()
            .ok_or_else(|| {
                LookupError::ScanError(format!("Could not find details for file {importer}"))
            })?
            .symbols
            .as_ref()
            .ok_or_else(|| {
                LookupError::ScanError(format!("Could not find symbols for file {importer}"))
            })?
            .imported;
        let imported_symbols_this_dep = imported_symbols.get(exporter).ok_or_else(|| {
            LookupError::ScanError(format!(
                "Could not find list of symbols imported by {importer} from {exporter}"))
        })?;

        let dep_exe = self
            .get(exporter)
            .ok_or_else(|| LookupError::ScanError(format!("Could not find file {exporter}")))?;
        let exported_symbols = &dep_exe
            .details
            .as_ref()
            .ok_or_else(|| {
                LookupError::ScanError(format!("Could not find details for file {exporter}"))
            })?
            .symbols
            .as_ref()
            .ok_or_else(|| {
                LookupError::ScanError(format!("Could not find symbols for file {exporter}"))
            })?
            .exported;

        let mut missing_symbols: HashSet<String> = HashSet::new();

        for d in imported_symbols_this_dep {
            if !exported_symbols.contains(d) {
                missing_symbols.insert(d.clone());
            }
        }

        let not_found_symbols = if missing_symbols.is_empty() {
            None
        } else {
            Some(
                vec![(
                    importer.to_owned(),
                    vec![(exporter.to_owned(), missing_symbols)]
                        .into_iter()
                        .collect(),
                )]
                    .into_iter()
                    .collect(),
            )
        };

        Ok(ExecutablesCheckReport {
            not_found_libraries: HashMap::new(),
            not_found_symbols,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::iter::FromIterator;

    use fs_err as fs;

    use crate::common::LookupError;
    use crate::executable::Executables;
    use crate::path::LookupPath;
    use crate::query::LookupQuery;
    use crate::runner::run;

    #[test]
    fn empty_executables() -> Result<(), LookupError> {
        let exes = Executables::new();
        assert!(!exes.contains("NonExistingExecutable.exe"));

        assert!(exes.get("NonExistingExecutable.exe").is_none());

        assert!(exes.get_root()?.is_none());

        assert!(exes.sorted_by_first_appearance().is_empty());

        Ok(())
    }

    #[test]
    fn executables() -> Result<(), LookupError> {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path =
            d.join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");

        let mut query = LookupQuery::deduce_from_executable_location(&exe_path)?;
        query.parameters.skip_system_dlls = true;
        let context = LookupPath::deduce(&query);
        let exes = run(&query, &context)?;

        assert!(exes.contains("DepRunTest.exe"));
        assert!(exes.contains("depruntest.exe"));
        assert!(!exes.contains("NonExistingExecutable.exe"));

        assert!(exes.get("NonExistingExecutable.exe").is_none());
        assert!(exes.get("DepRunTest.exe").is_some());

        assert_eq!(exes.get_root()?.unwrap().dllname, "DepRunTest.exe");

        let sorted = exes.sorted_by_first_appearance();
        let sorted_names: HashSet<&str> = sorted
            .iter()
            .filter(|e| e.details.as_ref().map(|d| !d.is_system).unwrap_or(false))
            .map(|e| e.dllname.as_ref())
            .collect();
        let expected_names: HashSet<&str> =
            HashSet::from_iter(["DepRunTestLib.dll", "DepRunTest.exe"].iter().copied());
        assert_eq!(sorted_names, expected_names);

        let exe_p = &exes
            .get_root()?
            .unwrap()
            .details
            .as_ref()
            .unwrap()
            .full_path;
        assert_eq!(exe_p, &fs::canonicalize(exe_path)?);

        Ok(())
    }
}
