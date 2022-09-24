//! This library contains utilities to scan the dependencies of Windows executable files and DLLs.
//!
//! # Quick Start
//!
//! You can use this library to implement ldd for Windows: this repository indeed includes such a
//! binary, called wldd. The following snippet shows how to recursively scan the dependencies of a
//! target executable in a DLL lookup path built using sensible defaults.
//! You can refer to that file for a usage example of more advanced functionalities.
//!
//! The basic workflow is to first create a LookupQuery, which contains the path to the root
//! executable whose dependencies should be found, a reference to the Windows root partition to use
//! as reference for the scan, and various parameters for performing the scan.
//!
//! Then the LookupPath can be computed given the query. The path contains a list of entries, which
//! will be probed for a DLL with the name registered in the import table as dependency for the
//! target executable. This search is performed recursively. The path can be freely manipulated
//! after deduction for advanced use cases. It can also be built from a DependencyRunner dwp file,
//! or from a Visual Studio vcxproj or vcxproj.user file.
//!
//! Once all information is available, the recursive DLL lookup can be performed to obtain a
//! list of interdependent executables. This list represents a directed acyclic graph through the
//! dependency list for each node, and can be visited according to various strategies.
//!
//! Sanity checks can be run on the list of executables to find missing DLL dependencies or
//! symbols therein.  
//!
//! ```
//!
//! let exe_path = "path/to/some/executable.exe";
//! # let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
//! # let relative_path =
//! #     "test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe";
//! # let exe_path = d.join(relative_path);
//! let mut query = dependency_runner::query::LookupQuery::deduce_from_executable_location(exe_path).unwrap();
//! query.parameters.extract_symbols = true;
//! let lookup_path = dependency_runner::path::LookupPath::deduce(&query);
//! let executables = dependency_runner::runner::run(&query, &lookup_path).unwrap();
//! let sym_check = executables.check(true);
//! ```

extern crate thiserror;

mod apiset;
pub mod common;
pub mod executable;
#[cfg(windows)]
mod knowndlls;
pub mod path;
pub mod pe;
pub mod query;
pub mod runner;
pub mod system;
pub mod vcx;
