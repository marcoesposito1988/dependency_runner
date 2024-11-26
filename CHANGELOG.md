# 1.3.1

- Made all types used in API public

# 1.3.0

- Updated dependencies
- Removed obsolete FFI usage

# 1.2.4

- Fixed compilation on nightly 1.72.x

# 1.2.3

- Fixed wldd CLI (issue #3)

# 1.2.2

- Disabled skim on Windows

# 1.2.1

- Updated Cargo dependencies

# 1.2.0

- Added fuzzy search of DLLs and imported/exported symbols via skim

# 1.1.0

- Added goblin-based backend

# 1.0.0

- Reworked the CLI
- Added an option to only report missing symbols/DLLs
- Refactored the data structures for easier C bindings generation
- Switched to Rust 2021

# 0.2.0

- Added support for API sets
- Added support for KnownDLLs 

# 0.1.0

First released version
- `deprun` executable with ergonomic CLI
- `wldd` executable for `ldd` compatibility
- Complete Rust API
- Recursive resolution of DLL dependencies
- Recursive check of imported/exported symbols
- Smart detection of lookup path
- Manual specification of lookup path
- Parsing Dependency Walker's .dwp files
- Parsing Visual Studio .vcxproj and .vcxproj.user files