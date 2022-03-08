use fs_err as fs;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LookupError {
    #[error("File system access error while scanning: {}", .0)]
    ScanError(String),

    #[error("Visual Studio User settings file parse error: {}", .0)]
    ParseError(String),

    #[error("Error trying to render a file path in readable form: {}", .0)]
    PathConversionError(String),

    #[error("Lookup context building error: {}", .0)]
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
    Ok(decanonicalize(fs::canonicalize(&p)?.to_str().ok_or_else(
        || {
            LookupError::PathConversionError(format!(
                "Can't compute canonic path for {:?}",
                p.as_ref()
            ))
        },
    )?))
}

/// Shorthand to get some kind of readable representation of a path
pub fn path_to_string<P: AsRef<Path>>(p: P) -> String {
    p.as_ref()
        .to_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{:?}", p.as_ref()))
}

/// Shorthand to get some kind of readable representation of an OsStr
pub fn osstring_to_string(p: &OsStr) -> String {
    p.to_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{:?}", p))
}

#[cfg(test)]
mod tests {
    use crate::common::{decanonicalize, readable_canonical_path, LookupError};
    use fs_err as fs;

    #[test]
    fn decanonicalize_removes_prefix() -> Result<(), LookupError> {
        let cargo_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let cargo_dir_canon = fs::canonicalize(&cargo_dir)?;

        let cargo_dir_decanon = decanonicalize(cargo_dir_canon.to_str().unwrap());
        assert!(!cargo_dir_decanon.contains(r"\\?\"));

        let cargo_dir_decanon2 = readable_canonical_path(&cargo_dir)?;
        assert!(!cargo_dir_decanon2.contains(r"\\?\"));
        assert_eq!(cargo_dir_decanon, cargo_dir_decanon2);

        Ok(())
    }
}
