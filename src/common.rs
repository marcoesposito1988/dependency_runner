
use std::path::Path;
use std::ffi::{OsStr, OsString};
use thiserror::Error;

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

    #[error("Could not demangle symbol")]
    DemanglingError(String),

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
    Utf8Error(#[from] std::str::Utf8Error),
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
}
