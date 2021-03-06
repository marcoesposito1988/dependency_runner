extern crate msvc_demangler;
extern crate thiserror;
use crate::LookupError;
use pelite::pe64::{Pe, PeFile};
use std::collections::{HashMap, HashSet};

/// Read the DLL name as specified in the PE file headers
///
/// This should match the dependency name specified in the import table of the file depending on
/// this DLL
pub fn read_dll_name(file: &PeFile) -> Result<String, LookupError> {
    Ok(file.exports()?.dll_name()?.to_string())
}

/// read the names of the DLLs this executable depends on
pub fn read_dependencies(file: &PeFile) -> Result<Vec<String>, LookupError> {
    // Access the import directory
    let imports = file.imports().map_err(|e| LookupError::PEError(e))?;

    let names: Vec<&pelite::util::CStr> = imports
        .iter()
        .map(|desc| desc.dll_name())
        .collect::<Result<Vec<&pelite::util::CStr>, pelite::Error>>()
        .map_err(|e| LookupError::PEError(e))?;

    Ok(names
        .iter()
        .filter_map(|s| s.to_str().ok())
        .map(|s| s.to_string())
        .collect::<Vec<String>>())
}

/// Get the list of symbols imported by this file from each of its dependencies
pub(crate) fn read_imports(file: &PeFile) -> Result<HashMap<String, HashSet<String>>, LookupError> {
    use LookupError::PEError;
    // Access the import directory
    let imports = file.imports().map_err(|e| PEError(e))?;

    let mut ret = HashMap::new();

    use pelite::pe32::imports::Import;

    for desc in imports.iter() {
        // Import Address Table and Import Name Table for this imported DLL
        let dllname = desc.dll_name()?;
        let importednames: HashSet<_> = desc
            .int()?
            .flat_map(|imp| match imp {
                Ok(Import::ByName { hint: _, name }) => Ok(name.to_string()),
                Ok(Import::ByOrdinal { ord: _ }) => {
                    // println!("by ordinal");
                    Ok("".to_owned()) // TODO apparently we can't check much here...
                }
                Err(err) => {
                    eprintln!("Error parsing import: {}", err);
                    Err(err)
                }
            })
            .collect();

        ret.insert(dllname.to_str()?.to_owned(), importednames);
    }

    Ok(ret)
}

/// Get the list of symbols exported by this DLL
pub(crate) fn read_exports(file: &PeFile) -> Result<HashSet<String>, LookupError> {
    // To query the exports
    let exports = match file.exports() {
        Ok(exports) => exports,
        // there is no export directory, e.g. in case of an executable
        Err(pelite::Error::Null) => return Ok(HashSet::new()),
        Err(e) => return Err(LookupError::PEError(e)),
    };
    let by = exports.by()?;

    Ok(by
        .iter_names()
        .map(|(name, _)| name.unwrap().to_str().unwrap().to_owned())
        .collect())
}

/// Get a humanly-readable version of the (imported or exported) symbol
pub fn demangle_symbol(symbol: &str) -> Result<String, LookupError> {
    let flags =
        msvc_demangler::DemangleFlags::llvm() | msvc_demangler::DemangleFlags::NO_MS_KEYWORDS;
    msvc_demangler::demangle(symbol, flags)
        .map_err(|_| LookupError::DemanglingError(symbol.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::read_dependencies;
    use crate::LookupError;
    use std::collections::HashSet;

    #[test]
    fn read_dependencies_test_exe() -> Result<(), LookupError> {
        let cargo_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = cargo_dir
            .join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");
        let filemap = pelite::FileMap::open(&exe_path).map_err(|e| LookupError::IOError(e))?;
        let pefile =
            pelite::pe64::PeFile::from_bytes(&filemap).map_err(|e| LookupError::PEError(e))?;

        let expected_exe_deps: HashSet<String> = [
            "DepRunTestLib.dll",
            "VCRUNTIME140D.dll",
            "ucrtbased.dll",
            "KERNEL32.dll",
        ]
        .iter()
        .map(|&s| s.to_owned())
        .collect();
        let exe_deps: HashSet<String> = read_dependencies(&pefile)?.into_iter().collect();
        assert_eq!(exe_deps, expected_exe_deps);

        Ok(())
    }

    #[test]
    fn read_dependencies_test_dll() -> Result<(), LookupError> {
        let cargo_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let lib_path = cargo_dir.join(
            "test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTestLib.dll",
        );

        let filemap = pelite::FileMap::open(&lib_path).map_err(|e| LookupError::IOError(e))?;
        let pefile =
            pelite::pe64::PeFile::from_bytes(&filemap).map_err(|e| LookupError::PEError(e))?;

        let expected_lib_deps: HashSet<String> = [
            "KERNEL32.dll",
            "MSVCP140D.dll",
            "VCRUNTIME140D.dll",
            "VCRUNTIME140_1D.dll",
            "ucrtbased.dll",
        ]
        .iter()
        .map(|&s| s.to_owned())
        .collect();
        let lib_deps: HashSet<String> = read_dependencies(&pefile)?.into_iter().collect();
        assert_eq!(lib_deps, expected_lib_deps);

        Ok(())
    }
}
