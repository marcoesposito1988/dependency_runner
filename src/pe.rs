//! Low-level PE file format access through the goblin and pelite libraries

extern crate msvc_demangler;
extern crate multimap;
extern crate thiserror;
use crate::common::LookupError;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct PEFileMap {
    path: PathBuf,
    content: Vec<u8>,
}

impl PEFileMap {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, LookupError> {
        Ok(Self {
            path: PathBuf::from(path.as_ref()),
            content: std::fs::read(path)?,
        })
    }
}

pub struct PEFile<'a> {
    pefile: Option<pelite::PeFile<'a>>,
    peobject: Option<goblin::pe::PE<'a>>,
}

impl<'a> PEFile<'a> {
    pub fn new(filemap: &'a PEFileMap) -> Result<Self, LookupError> {
        Ok(Self {
            pefile: match pelite::PeFile::from_bytes(&filemap.content) {
                Ok(pef) => Some(pef),
                Err(e) => {
                    match e {
                        pelite::Error::BadMagic | pelite::Error::PeMagic => {
                            eprintln!("{:?}", LookupError::WrongFileFormatError(e))
                        }

                        _ => eprintln!("{:?}", LookupError::PEError(e)),
                    };
                    None
                }
            },
            peobject: match goblin::Object::parse(&filemap.content) {
                Ok(goblin::Object::PE(pef)) => Some(pef),
                Ok(ukn) => {
                    eprintln!("unexpected executable format: {ukn:?}");
                    None
                }
                Err(e) => {
                    eprintln!("{:?}", LookupError::GoblinError(e));
                    None
                }
            },
        })
    }

    /// Read the DLL name as specified in the PE file headers
    ///
    /// This should match the dependency name specified in the import table of the file depending on
    /// this DLL
    pub fn read_dll_name(&self) -> Result<String, LookupError> {
        Ok(self.pefile.unwrap().exports()?.dll_name()?.to_string())
    }

    /// read the names of the DLLs this executable depends on
    pub fn read_dependencies(&self) -> Result<Vec<String>, LookupError> {
        // prefer goblin since it seems to be less fragile
        if let Some(peo) = self.peobject.as_ref() {
            return Ok(peo.libraries.iter().map(|i| i.to_string()).collect());
        }

        // Access the import directory
        let imports = self
            .pefile
            .unwrap()
            .imports()
            .map_err(LookupError::PEError)?;

        let names: Vec<&pelite::util::CStr> = imports
            .iter()
            .map(|desc| desc.dll_name())
            .collect::<Result<Vec<&pelite::util::CStr>, pelite::Error>>()
            .map_err(LookupError::PEError)?;

        Ok(names
            .iter()
            .filter_map(|s| s.to_str().ok())
            .map(|s| s.to_string())
            .collect::<Vec<String>>())
    }

    /// Get the list of symbols imported by this file from each of its dependencies
    pub fn read_imports(&self) -> Result<HashMap<String, HashSet<String>>, LookupError> {
        // prefer goblin since it seems to be less fragile
        if let Some(peo) = self.peobject.as_ref() {
            let imports: multimap::MultiMap<&str, &str> = peo
                .imports
                .iter()
                .map(|i| (i.dll, i.name.as_ref()))
                .collect();

            let ret: HashMap<String, HashSet<String>> = imports
                .iter_all()
                .map(|(k, v)| (k.to_string(), v.iter().map(ToString::to_string).collect()))
                .collect();

            return Ok(ret);
        }

        use LookupError::PEError;
        // Access the import directory
        let imports = self.pefile.unwrap().imports().map_err(PEError)?;

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
                        eprintln!("Error parsing import: {err}");
                        Err(err)
                    }
                })
                .collect();

            ret.insert(dllname.to_str()?.to_owned(), importednames);
        }

        Ok(ret)
    }

    /// Get the list of symbols exported by this DLL
    pub fn read_exports(&self) -> Result<HashSet<String>, LookupError> {
        // prefer goblin since it seems to be less fragile
        if let Some(peo) = self.peobject.as_ref() {
            return Ok(peo
                .exports
                .iter()
                .map(|i| i.name.unwrap_or("<unnamed>").to_string())
                .collect());
        }

        // To query the exports
        let exports = match self.pefile.unwrap().exports() {
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
    use crate::common::LookupError;
    use crate::pe::PEFile;
    use std::collections::HashSet;
    use std::fs;

    #[test]
    fn read_dependencies_test_exe_goblin() -> Result<(), LookupError> {
        let cargo_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = cargo_dir
            .join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");
        let exe_content = fs::read(exe_path)?;
        let filemap = goblin::Object::parse(&exe_content)?;
        let goblin_pe = match filemap {
            goblin::Object::PE(pe) => pe,
            _ => return Err(LookupError::ScanError("Unexpected format".to_owned())),
        };
        let goblin_pe_deps = goblin_pe.libraries;

        let expected_exe_deps: HashSet<String> = [
            "DepRunTestLib.dll",
            "VCRUNTIME140D.dll",
            "ucrtbased.dll",
            "KERNEL32.dll",
        ]
        .iter()
        .map(|&s| s.to_owned())
        .collect();
        let exe_deps: HashSet<String> = goblin_pe_deps.into_iter().map(String::from).collect();
        assert_eq!(exe_deps, expected_exe_deps);

        Ok(())
    }

    #[test]
    fn read_dependencies_test_exe() -> Result<(), LookupError> {
        let cargo_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = cargo_dir
            .join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");
        let pefilemap = crate::pe::PEFileMap::new(exe_path)?;
        let pefile = PEFile::new(&pefilemap)?;

        let expected_exe_deps: HashSet<String> = [
            "DepRunTestLib.dll",
            "VCRUNTIME140D.dll",
            "ucrtbased.dll",
            "KERNEL32.dll",
        ]
        .iter()
        .map(|&s| s.to_owned())
        .collect();
        let exe_deps: HashSet<String> = pefile.read_dependencies()?.into_iter().collect();
        assert_eq!(exe_deps, expected_exe_deps);

        Ok(())
    }

    #[test]
    fn read_dependencies_test_dll() -> Result<(), LookupError> {
        let cargo_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let lib_path = cargo_dir.join(
            "test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTestLib.dll",
        );
        let pefilemap = crate::pe::PEFileMap::new(lib_path)?;
        let pefile = PEFile::new(&pefilemap)?;

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
        let lib_deps: HashSet<String> = pefile.read_dependencies()?.into_iter().collect();
        assert_eq!(lib_deps, expected_lib_deps);

        Ok(())
    }
}
