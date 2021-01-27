
use std::path::Path;
use std::ffi::{OsStr, OsString};
use thiserror::Error;
use pelite::pe64::{Pe, PeFile};

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

    #[error("Error trying to render a file path in readable form")]
    PathConversionError(String),

    #[error("Lookup context building error")]
    ContextDeductionError(String),

    #[error("OsString could not be converted into a string")]
    OsStringConversionError(OsString),

    #[error(transparent)]
    VarError(#[from] std::env::VarError),
    #[error(transparent)]
    RegexError(#[from] regex::Error),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    PEError(#[from] pelite::Error),
    #[error(transparent)]
    InternalError(#[from] anyhow::Error),
}

/// Remove the extended path prefix (\\?\) for readability
pub fn decanonicalize(s: &str) -> String {
    s.replacen(r"\\?\", "", 1)
}

/// Provide the canonical form of the Path as a string, or die trying
pub fn readable_canonical_path<P: AsRef<Path>>(p: P) -> Result<String, LookupError> {
    Ok(decanonicalize(std::fs::canonicalize(&p)?
        .to_str()
        .ok_or(LookupError::PathConversionError(
            format!("Can't compute canonic path for {:?}", p.as_ref())))?))
}

/// Shorthand to get some kind of readable representation of a path
pub fn path_to_string<P: AsRef<Path>>(p: P) -> String {
    p.as_ref().to_str().unwrap_or(format!("{:?}", p.as_ref()).as_ref()).to_owned()
}

/// Shorthand to get some kind of readable representation of an OsStr
pub fn osstring_to_string(p: &OsStr) -> String {
    p.to_str().unwrap_or(format!("{:?}", p).as_ref()).to_owned()
}

/// read the names of the DLLs this executable depends on
pub fn read_dependencies<P: AsRef<Path> + ?Sized>(path: &P) -> Result<Vec<String>, LookupError> {
    use LookupError::{CouldNotOpenFile, ProcessingError};
    let map = pelite::FileMap::open(path.as_ref()).map_err(|e| CouldNotOpenFile { source: e })?;
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

#[cfg(test)]
mod tests {
    use crate::{LookupError, decanonicalize, readable_canonical_path};
    use crate::common::read_dependencies;
    use std::collections::HashSet;

    #[test]
    fn decanonicalize_removes_prefix() -> Result<(), LookupError> {
        let cargo_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let cargo_dir_canon = std::fs::canonicalize(&cargo_dir)?;

        let cargo_dir_decanon = decanonicalize(cargo_dir_canon.to_str().unwrap());
        assert!(!cargo_dir_decanon.contains(r"\\?\"));

        let cargo_dir_decanon2 = readable_canonical_path(&cargo_dir)?;
        assert!(!cargo_dir_decanon2.contains(r"\\?\"));
        assert_eq!(cargo_dir_decanon, cargo_dir_decanon2);

        Ok(())
    }

    #[test]
    fn read_dependencies_test_exe() -> Result<(), LookupError> {
        let cargo_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = cargo_dir.join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");

        let expected_exe_deps: HashSet<String> = ["DepRunTestLib.dll", "VCRUNTIME140D.dll", "ucrtbased.dll", "KERNEL32.dll"]
            .iter().map(|&s| s.to_owned()).collect();
        let exe_deps: HashSet<String> = read_dependencies(&exe_path)?.into_iter().collect();
        assert_eq!(exe_deps, expected_exe_deps);

        Ok(())
    }

    #[test]
    fn read_dependencies_test_dll() -> Result<(), LookupError> {
        let cargo_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let lib_path = cargo_dir.join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTestLib.dll");

        let expected_lib_deps: HashSet<String> = ["KERNEL32.dll", "MSVCP140D.dll", "VCRUNTIME140D.dll", "VCRUNTIME140_1D.dll", "ucrtbased.dll"]
            .iter().map(|&s| s.to_owned()).collect();
        let lib_deps: HashSet<String> = read_dependencies(&lib_path)?.into_iter().collect();
        assert_eq!(lib_deps, expected_lib_deps);

        Ok(())
    }
}
