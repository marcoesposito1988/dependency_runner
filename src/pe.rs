use std::path::Path;
use crate::LookupError;
use pelite::pe64::{Pe, PeFile};

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
    use crate::{LookupError};
    use super::read_dependencies;
    use std::collections::HashSet;

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
