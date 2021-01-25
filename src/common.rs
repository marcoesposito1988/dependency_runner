
use std::path::Path;
use std::ffi::OsStr;
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

pub fn path_to_string<P: AsRef<Path>>(p: P) -> String {
    p.as_ref().to_str().unwrap_or(format!("{:?}", p.as_ref()).as_ref()).to_owned()
}

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
