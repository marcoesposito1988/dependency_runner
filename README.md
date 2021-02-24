[![crates.io](https://img.shields.io/crates/v/dependency_runner.svg)](https://crates.io/crates/dependency_runner)
[![Dependency Runner CI](https://github.com/marcoesposito1988/dependency_runner/actions/workflows/ci.yml/badge.svg)](https://github.com/marcoesposito1988/dependency_runner/actions/workflows/ci.yml)

# Dependency runner

ldd for Windows - and more!

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
    and  output formats, including to a JSON file. It can parse Dependency Walker's `.dwp` files, 
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

#### Checking the symbols     
```bash
deprun --check-symbols path/to/your/executable.exe
```

#### Limiting the scanning and output depth    
```bash
deprun --depth 4 path/to/your/executable.exe
```

#### Saving the scan results to a JSON file
```bash
deprun --output-json-path path/to/output.json path/to/your/executable.exe
```
Each executable will be represented by a single object. The dependency tree can be reconstructed from the dependency 
list of each node.

#### Print recursively all system dependencies
```bash
deprun --print-system-dlls path/to/your/executable.exe
```
WARNING: until API sets are correctly implemented, this may result in gigantic output, 
and hence performance degradation
    
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
- v 0.2.0
    - [ ] parallelization across multiple threads (if ever necessary)
    - [ ] visualization of library symbols with address/ordinal
    - [ ] support of API levels
    - [ ] support of manifests
    - [ ] support of KnownDLLs
    - [ ] release on package managers
      - [ ] crates.io
      - [ ] Chocolatey
      - [ ] WinGet?
      - [ ] APT?
      - [ ] AUR?
    - [ ] implementation of the maximal possible subset of `ldd` functionalities in `wldd`
        - [ ] subset of verbose output
        - [ ] unused symbols?
        - [ ] relocation?
- v 0.3.0
    - [ ] `dependency_runner` GUI?
        - [ ] drag-and-drop input of executables
        - [ ] PATH editing
        - [ ] saving PATH to disk, association of each PATH to executables on disk
        - [ ] monitoring of file changes

## Limitations
- `LoadLibraryEx` and similar mechanism can't be inspected without letting the program run. 
  This limitation is common to other similar tools that recursively scan executables files and parse their import tables.  
- no support yet for manifests, API sets or "known DLLs". You can take a look at [Dependencies](https://github.com/lucasg/Dependencies) instead  
- symbol imports among system DLLs are not checked by default, because they lead to performance degradation 
  (this may change once API sets are supported); however, you can usually take their correctness for granted 

## Acknowledgements

- [Dependencies](https://github.com/lucasg/Dependencies) (thanks to @lucasg for this awesome software, as well as for 
  the amount of information and documentation on the personal website)
- [pelite](https://github.com/CasualX/pelite), upon which all of this is built

## License
LGPLv3
