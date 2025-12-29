[![crates.io](https://img.shields.io/crates/v/dependency_runner.svg)](https://crates.io/crates/dependency_runner)
[![Dependency Runner CI](https://github.com/marcoesposito1988/dependency_runner/actions/workflows/ci.yml/badge.svg)](https://github.com/marcoesposito1988/dependency_runner/actions/workflows/ci.yml)

# Dependency runner

ldd for Windows - and more!

## Features
- portable: debug Windows DLL loading issues from Linux or macOS!
- ergonomic CLI
- readable report of missing libraries and symbols
- browsing of DLLs and symbols with fuzzy search (Unix targets only)
- JSON output
- support for API sets and KnownDLLs (Windows only, at least for now)
- support for Dependency Walker's `.dwp` files
- support for Microsoft Visual Studio's `.vcxproj` and `.vcxproj.user` files


Try it out:
```text
dependency_runner> cargo run --bin deprun -- test_data\test_project1\DepRunTest\build\DepRunTest\Debug\DepRunTest.exe --check-symbols --userpath test_data\test_project1\DepRunTestLibWrong\build\Debug

DepRunTest.exe => C:\Users\Marco Esposito\Projects\personal\dependency_runner\test_data\test_project1\DepRunTest\build\DepRunTest\Debug
        DepRunTestLib.dll => C:\Users\Marco Esposito\Projects\personal\dependency_runner\test_data\test_project1\DepRunTestLibWrong\build\Debug

Checking symbols...

No missing libraries detected

Missing symbols detected!
[Importing executable, exporting executable, missing symbols]

DepRunTest.exe
	DepRunTestLib.dll
		public: float TestClass::testMethod(int)

```

This repository contains tools to analyze the dependencies of a Windows Portable Executable (PE) 
file, usually in order to debug application startup problems.

These tools are: 
- `wldd`, a reimplementation of GNU `ldd` for Windows PE executables (`exe` and `dll` files). 
    An effort is made in order to keep the output similar to that of the original tool, so that 
    existing scripts targeting Linux executables can be reused easily. The current 
    API may be extended in the future to include new features allowed by the Win32 executable format.
    However, priority will be given to avoiding breaking changes in the output format.
    While `ldd` invokes the loader and inspects the result in memory, `wldd` doesn't. The 
    Windows loading process is emulated, thus the address at which each library is loaded is not 
    included into the output. This may change in the future.
- `deprun`, a further CLI tool that, in contrast to `wldd`, is not limited by the 
    constraint of keeping compatibility with `ldd`. By default, dependencies are printed as a tree 
    for better readability. It supports multiple lookup path specifications 
    and  output formats, including to a JSON file. It includes a DLL and symbol browser with fuzzy search integrated 
    through skim. It can parse Dependency Walker's `.dwp` files, 
    as well as Visual Studio `.vcxproj` and `.vcxproj.user` files to read the executable location, 
    working directory and user path.
- both tools are based on the same Rust library, which can be included in Rust 
    applications. A C API is also planned to allow straightforward usage of the 
    library from most other languages.
    
All these tools target Windows PE exe files, but are designed to be portable. The default 
behavior attempts to guess sane defaults to make it easy to inspect executables located 
on a neighboring Windows installation from another operating system, or to ignore missing 
system libraries if no such partition is available on the system. The example above should 
work on any operating system.


## Getting started
### Binary releases (any OS)
- download the binaries for your OS from the [GitHub Releases page](https://github.com/marcoesposito1988/dependency_runner/releases)
- copy the binaries somewhere on your PATH
    - Linux: `/usr/local/bin` is a good place
    - Windows: create your own directory somewhere and add it to the PATH variable through the control panel

### Installation from source with Cargo (any OS)
- download and execute rustup
- check out this repository and `cd` into it
- `cargo build --release`
- copy the statically linked binaries from `target/release` to somewhere on your PATH 
  - Linux: `/usr/local/bin` is a good place
  - Windows: create your own directory somewhere and add it to the PATH variable through the control panel 

## Usage
### deprun

#### Printing the dependency tree
```bash
deprun path/to/your/executable.exe
```
Default behavior:
- Windows
  - `C:\Windows` and `C:\Windows\System32` as "Windows" and "System" directories
  - the shell's current directory is also used as `cwd`
  - the content of the current shell's PATH is used as user path
- Linux/macOS
  - if the executable is located in a mounted Windows partition, its `C:\Windows` and `C:\Windows\System32` directories will be used
  - the shell's current directory is also used as `cwd`
  - the PATH is empty
    
<!-- TODO
#### Overriding the guessed PATH  
#### Extending the guessed PATH  
    
- overriding guessed PATH
- extending the system PATH
    - env var
    - config file
    - vcxproj.user
    - vcxproj

-->

#### Limiting the scanning and output depth
```bash
deprun --depth 4 path/to/your/executable.exe
```

#### Looking for DLLs depending on a given one
```bash
deprun --filter mydep path/to/your/executable.exe
```

or also:

```bash
deprun --filter mydep path/to/your/dlls/*.dll
```

#### Saving the scan results to a JSON file
```bash
deprun --output-json-path path/to/output.json path/to/your/executable.exe
```
Each executable will be represented by a single object. The dependency tree can be reconstructed from the dependency
list of each node.

#### Printing recursively all system dependencies
```bash
deprun --print-system-dlls path/to/your/executable.exe
```

#### Browsing the DLLs with fuzzy search
```bash
deprun --skim path/to/your/executable.exe
```

### Lookup path

#### Defining the whole DLL lookup path with a .dwp file (Dependency Walker format)
```bash
deprun --dwp_path=path/to/config.dwp path/to/your/executable.exe
```

#### Scanning the executable produced by a given .vcxproj (Visual Studio) project
```bash
deprun --vcx-config=Release path/to/visual_studio_solution/executable.vcxproj
```

The configuration must only be provided if more than one are listed in the vcxproj file.


#### Extending the DLL lookup user path as in the .vcxproj.user file
```bash
deprun --vcx-config=Release --vcxproj_user_path=path/to/visual_studio_solution/executable.vcxproj.user path/to/visual_studio_solution/executable.vcxproj
```

The configuration must only be provided if more than one are listed in the vcxproj file. 
The executable can also be referred to directly, instead of providing the path to the .vcxproj file.

### DLL symbols

#### Checking for missing symbols     
```bash
deprun --check-symbols path/to/your/executable.exe
```

#### Browsing the symbols imported/exported by the all found DLLs (not supported yet on Windows)
```bash
deprun --skim-symbols path/to/your/executable.exe
```
    
### wldd
a subset of the above, check with `-h`

## Roadmap
Help is welcome in the form of issues and pull request!
- v 0.1.0
    - [x] minimal, non-parallelized PE dependency scanning library
    - [x] implementation of a meaningful subset of `ldd` functionalities in `wldd`
        - [x] compatible output for non-verbose mode
    - [x] ergonomic CLI  
    - [x] JSON output
    - [x] specification of lookup path through a `.dwp` file
    - [x] specification of PATH through `.vcxproj.user` files, picking configuration
    - [x] specification of executable and working directory through `.vcxproj` files, picking configuration
    - [x] extraction of symbols from DLLs
    - [x] check of imported/exported symbols correspondency down the dependency tree
  - [x] release on package managers
      - [x] crates.io
- v 0.2.0
    - [x] support of API sets
    - [x] support of KnownDLLs
- v 1.0.0
    - [x] API refactor
    - [x] documentation improvement
- v 1.1.0
    - [x] add goblin PE parser for robustness to alignment issues
- v 1.2.0
    - [x] add fuzzy search based on skim 
- v 1.3.0
    - [x] add support for multiple targets and filtering DLL names
- v 1.4.0
    - [x] Rust 2024 support
- v 1.5.0
    - [ ] support of manifests
    - [ ] visualization of library symbols with address/ordinal
    - [ ] release on package managers
      - [ ] Chocolatey
      - [ ] WinGet?
      - [ ] APT?
      - [ ] AUR?
    - [ ] implementation of the maximal possible subset of `ldd` functionalities in `wldd`
        - [ ] subset of verbose output
        - [ ] unused symbols?
        - [ ] relocation?
- v 1.x.0
    - [ ] parallelization across multiple threads (if ever necessary)
    - [ ] `dependency_runner` GUI?
        - [ ] drag-and-drop input of executables
        - [ ] PATH editing
        - [ ] saving PATH to disk, association of each PATH to executables on disk
        - [ ] monitoring of file changes

## Limitations
- `LoadLibraryEx` and similar mechanism can't be inspected without letting the program run. 
  This limitation is common to other similar tools that recursively scan executables files and parse their import tables.  
- no support yet for application manifests. You can take a look at [Dependencies](https://github.com/lucasg/Dependencies) instead  
- the dependencies of system DLLs are not recursed into, for performance reasons; however, you can usually take their correctness for granted 

## Acknowledgements

- [Dependencies](https://github.com/lucasg/Dependencies) (thanks to @lucasg for this awesome software, as well as for 
  the amount of information and documentation on the personal website)
- [pelite](https://github.com/CasualX/pelite) and [goblin](https://github.com/m4b/goblin), upon which all of this is built
- [skim](https://github.com/lotabout/skim) for the great fuzzy search library

## License
LGPLv3
