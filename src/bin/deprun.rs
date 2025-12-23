extern crate dependency_runner;

#[cfg(windows)]
use dependency_runner::common::LookupError;
use dependency_runner::path::LookupPath;
#[cfg(windows)]
use dependency_runner::vcx::{parse_vcxproj, parse_vcxproj_user};

use anyhow::Context;
use clap::Parser;
#[cfg(not(windows))]
use dependency_runner::common::path_to_string;
use dependency_runner::common::{decanonicalize, readable_canonical_path};
use dependency_runner::executable::{Executable, Executables};
use dependency_runner::pe::demangle_symbol;
use dependency_runner::query::LookupQuery;
#[cfg(not(windows))]
use dependency_runner::skim::{skim_dlls, skim_symbols};
#[cfg(not(windows))]
use dependency_runner::system::WindowsSystem;
use fs_err as fs;
use std::path::PathBuf;

#[cfg(windows)]
fn pick_configuration(
    configs: &Vec<&String>,
    user_config: &Option<String>,
    file_path: &str,
) -> Result<String, LookupError> {
    if let Some(vcx_config) = user_config {
        if configs.contains(&&vcx_config.to_string()) {
            Ok(vcx_config.to_owned().to_string())
        } else {
            Err(LookupError::ContextDeductionError(format!(
                "The specified configuration {} was not found in project file {}\n\
                Available configurations: {:?}",
                vcx_config, file_path, configs
            )))
        }
    } else if configs.len() == 1 {
        let single_config = configs.last().unwrap();
        eprintln!(
            "Visual Studio configuration not specified, using {} for file {}",
            single_config, file_path
        );
        Ok(single_config.to_owned().to_string())
    } else {
        Err(LookupError::ContextDeductionError(format!(
            "Must specify a configuration with --vcx-config=<CONFIG> for project file {}\n\
            Available configurations: {:?}",
            file_path, configs
        )))
    }
}

fn visit_depth_first(
    e: &Executable,
    current_depth: usize,
    max_depth: Option<usize>,
    exes: &Executables,
    print_system_dlls: bool,
    filter: &Option<String>,
) -> bool {
    if !((print_system_dlls || !e.details.as_ref().map(|d| d.is_system).unwrap_or(false))
        && max_depth.map(|d| current_depth < d).unwrap_or(true))
    {
        return false;
    }

    // Check if any child matches the filter
    let mut any_child_matches = false;
    let mut child_outputs = Vec::new();

    if let Some(details) = &e.details {
        if let Some(dependencies) = &details.dependencies {
            for d in dependencies {
                if let Some(de) = exes.get(d) {
                    let mut child_output = Vec::new();
                    let matches = visit_depth_first_to_buffer(
                        de,
                        current_depth + 1,
                        max_depth,
                        exes,
                        print_system_dlls,
                        filter,
                        &mut child_output,
                    );
                    if matches {
                        any_child_matches = true;
                        child_outputs.push(child_output);
                    }
                }
            }
        }
    }

    // Check if this node matches
    let this_matches = if let Some(filter_str) = filter {
        e.dllname.to_lowercase().contains(&filter_str.to_lowercase())
    } else {
        true
    };

    // Print this node if it matches or if any child matches
    if this_matches || any_child_matches {
        let folder = if !e.found {
            "not found".to_owned()
        } else if let Some(details) = &e.details {
            readable_canonical_path(details.full_path.parent().unwrap())
                .unwrap_or_else(|_| "INVALID".to_owned())
        } else {
            "not searched".to_owned()
        };
        let extra_tag = if e.details.as_ref().map(|d| d.is_known_dll).unwrap_or(false) {
            "[Known DLL]"
        } else {
            ""
        };
        println!(
            "{}{} => {} {}",
            "\t".repeat(current_depth),
            e.dllname,
            folder,
            extra_tag
        );

        // Print all matching children
        for child_output in child_outputs {
            for line in child_output {
                println!("{}", line);
            }
        }

        return true;
    }

    false
}

fn visit_depth_first_to_buffer(
    e: &Executable,
    current_depth: usize,
    max_depth: Option<usize>,
    exes: &Executables,
    print_system_dlls: bool,
    filter: &Option<String>,
    buffer: &mut Vec<String>,
) -> bool {
    if !((print_system_dlls || !e.details.as_ref().map(|d| d.is_system).unwrap_or(false))
        && max_depth.map(|d| current_depth < d).unwrap_or(true))
    {
        return false;
    }

    // Check if any child matches the filter
    let mut any_child_matches = false;
    let mut child_outputs = Vec::new();

    if let Some(details) = &e.details {
        if let Some(dependencies) = &details.dependencies {
            for d in dependencies {
                if let Some(de) = exes.get(d) {
                    let mut child_output = Vec::new();
                    let matches = visit_depth_first_to_buffer(
                        de,
                        current_depth + 1,
                        max_depth,
                        exes,
                        print_system_dlls,
                        filter,
                        &mut child_output,
                    );
                    if matches {
                        any_child_matches = true;
                        child_outputs.push(child_output);
                    }
                }
            }
        }
    }

    // Check if this node matches
    let this_matches = if let Some(filter_str) = filter {
        e.dllname.to_lowercase().contains(&filter_str.to_lowercase())
    } else {
        true
    };

    // Add this node to buffer if it matches or if any child matches
    if this_matches || any_child_matches {
        let folder = if !e.found {
            "not found".to_owned()
        } else if let Some(details) = &e.details {
            readable_canonical_path(details.full_path.parent().unwrap())
                .unwrap_or_else(|_| "INVALID".to_owned())
        } else {
            "not searched".to_owned()
        };
        let extra_tag = if e.details.as_ref().map(|d| d.is_known_dll).unwrap_or(false) {
            "[Known DLL]"
        } else {
            ""
        };
        buffer.push(format!(
            "{}{} => {} {}",
            "\t".repeat(current_depth),
            e.dllname,
            folder,
            extra_tag
        ));

        // Add all matching children
        for child_output in child_outputs {
            buffer.extend(child_output);
        }

        return true;
    }

    false
}

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct DeprunCli {
    #[clap(value_parser, required = true)]
    /// Target file(s) (.exe, .dll or .vcxproj). Supports glob patterns like "*.dll"
    inputs: Vec<String>,
    #[clap(value_parser, short, long)]
    /// Path for output in JSON format
    output_json_path: Option<String>,
    #[clap(value_parser, short, long)]
    /// Maximum recursion depth (default: unlimited)
    max_depth: Option<usize>,
    #[clap(short, long)]
    /// Activate verbose output
    verbose: bool,
    #[clap(short = 'e', long)]
    /// Only show executables with missing dependencies or missing symbols
    errors_only: bool,
    #[clap(short, long)]
    /// Include system DLLs in the output
    print_system_dlls: bool,
    #[clap(short, long)]
    /// Check that all imported symbols are found within the (non-system) dependencies
    check_symbols: bool,
    #[cfg(not(windows))]
    #[clap(short, long)]
    /// Start a fuzzy search on the found DLLs, then on the symbols of the selected DLL
    skim: bool,
    #[cfg(not(windows))]
    #[clap(long)]
    /// Start a fuzzy search on the symbols of all found DLLs
    skim_symbols: bool,
    #[clap(value_parser, short, long)]
    /// Working directory to be considered in the DLL lookup path (default: same as the shell deprun runs in)
    working_directory: Option<String>,
    #[clap(value_parser, short, long)]
    /// User path to be considered in the DLL lookup path (default: same as the shell deprun runs in)
    user_path: Option<String>,
    #[clap(value_parser, short, long)]
    /// Filter output to show only DLLs matching this string (case-insensitive substring match). Parent DLLs in the dependency tree are still shown to preserve the tree structure.
    filter: Option<String>,
    #[clap(value_parser, short, long)]
    /// When filtering, if passed multiple targets, omit the output of non-matching DLLs
    quiet: bool,
    #[cfg(windows)]
    #[clap(value_parser, long)]
    /// Read the complete DLL lookup path from a .dwp file (Dependency Walker's format)
    dwp_path: Option<String>,
    #[cfg(windows)]
    #[clap(value_parser, long, conflicts_with = "dwp_path")]
    /// Path to a .vcxproj.user file to parse for PATH entries to be added to the search path
    vcxproj_user_path: Option<String>,
    #[cfg(windows)]
    #[clap(value_parser, long, conflicts_with = "dwp_path")]
    /// Configuration to use (Debug, Release, ...) if the target is a .vcxproj file, or if a .vcxproj.user was provided
    vcxproj_configuration: Option<String>,
    #[cfg(not(windows))]
    #[clap(value_parser, long)]
    /// Windows partition to use for system DLLs lookup (if not specified, the partition where INPUT lies will be tested and used if valid)
    windows_root: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = DeprunCli::parse();

    // Expand glob patterns and collect all binary paths
    let mut binary_paths = Vec::new();
    for input in &args.inputs {
        let glob_matches = glob::glob(input)
            .with_context(|| format!("Invalid glob pattern: {}", input))?;

        let mut found_match = false;
        for entry in glob_matches {
            match entry {
                Ok(path) => {
                    if path.is_file() {
                        binary_paths.push(path);
                        found_match = true;
                    } else if path.is_dir() {
                        eprintln!(
                            "Skipping directory: {}",
                            path.to_str().unwrap_or("<invalid path>")
                        );
                    }
                }
                Err(e) => eprintln!("Error processing glob match: {}", e),
            }
        }

        if !found_match {
            eprintln!(
                "No files found matching pattern '{}'\nCurrent working directory: {}",
                input,
                std::env::current_dir()?.to_str().unwrap_or("<invalid>"),
            );
        }
    }

    if binary_paths.is_empty() {
        eprintln!("No valid files to process");
        std::process::exit(1);
    }

    // Process each binary
    let max_path_len = binary_paths.iter().map(|p| p.to_string_lossy().len()).max().unwrap_or(0);
    let message_len = 22 + max_path_len;

    for (idx, binary_path) in binary_paths.iter().enumerate() {
        if binary_paths.len() > 1 && !args.quiet {
            println!("\n{}\nProcessing {} / {} : {}\n{}\n",
                     "=".repeat(message_len), idx + 1, binary_paths.len(),
                     binary_path.to_str().unwrap_or("<invalid>"),
                     "=".repeat(message_len));
        }

        let binary_path = fs::canonicalize(binary_path)?;

        #[cfg(not(windows))]
        let mut query = LookupQuery::deduce_from_executable_location(&binary_path)?;

        #[cfg(windows)]
        let mut query = if binary_path
            .extension()
            .map(|e| e == "vcxproj")
            .unwrap_or(false)
        {
            let vcxproj_path = &binary_path;
            let vcx_exe_info_per_config = parse_vcxproj(&vcxproj_path)?;
            let vcx_config_to_use = pick_configuration(
                &vcx_exe_info_per_config.keys().collect::<Vec<_>>(),
                &args.vcxproj_configuration,
                vcxproj_path
                    .to_str()
                    .ok_or(LookupError::ContextDeductionError(format!(
                        "Could not open {:?} as a .vcxproj file",
                        vcxproj_path
                    )))?,
            )?;
            let vcx_exe_info = &vcx_exe_info_per_config[&vcx_config_to_use];

            LookupQuery::read_from_vcx_executable_information(vcx_exe_info)?
        } else {
            let mut query = LookupQuery::deduce_from_executable_location(&binary_path)?;

            if let Some(vcxproj_user_path_str) = &args.vcxproj_user_path {
                let vcxproj_user_path = std::path::Path::new(&vcxproj_user_path_str);
                if !vcxproj_user_path.exists() || vcxproj_user_path.is_dir() {
                    eprintln!(
                        "Specified vcxproj.user file not found at {}",
                        vcxproj_user_path_str,
                    );
                    std::process::exit(1);
                }

                let vcx_debug_info_per_config = parse_vcxproj_user(&vcxproj_user_path)?;
                let config_to_use = pick_configuration(
                    &vcx_debug_info_per_config.keys().collect::<Vec<_>>(),
                    &args.vcxproj_configuration,
                    &vcxproj_user_path_str,
                )?;
                let vcx_debug_info = &vcx_debug_info_per_config[&config_to_use];

                query.update_from_vcx_debugging_configuration(vcx_debug_info);
            }
            query
        };

        if let Some(max_depth) = args.max_depth {
            query.parameters.max_depth = Some(max_depth);
        }

        #[cfg(not(windows))]
        {
            query.parameters.extract_symbols = args.check_symbols || args.skim_symbols || args.skim;
        }

        #[cfg(windows)]
        {
            query.parameters.extract_symbols = args.check_symbols;
        }

        // overrides (must be last)

        #[cfg(not(windows))]
        if let Some(overridden_winroot) = &args.windows_root {
            query.system = WindowsSystem::from_root(overridden_winroot.clone());
        } else if args.verbose {
            if let Some(system) = &query.system {
                println!(
                    "Windows partition root not specified, assumed {}",
                    path_to_string(&system.sys_dir)
                );
            } else {
                println!("Windows partition root not specified, and executable doesn't lie in one; system DLL imports will not be resolved");
            }
        }

        if let Some(overridden_workdir) = &args.working_directory {
            query.target.working_dir = PathBuf::from(overridden_workdir);
        } else if args.verbose {
            println!(
                "Working directory not specified, assuming directory of executable: {}",
                decanonicalize(query.target.working_dir.to_str().unwrap_or("---"))
            );
        }
        if let Some(overridden_path) = &args.user_path {
            let canonicalized_path: Vec<PathBuf> = overridden_path
                .split(';')
                .filter_map(|s| {
                    let p = std::path::Path::new(s);
                    if p.exists() {
                        Some(fs::canonicalize(s))
                    } else {
                        eprintln!("Skipping non-existing path entry {s}");
                        None
                    }
                })
                .collect::<Result<Vec<_>, std::io::Error>>()?;
            query.target.user_path.extend(canonicalized_path);
        } else if args.verbose {
            #[cfg(windows)]
            {
                let decanonicalized_path: Vec<String> = query
                    .target
                    .user_path
                    .iter()
                    .map(|p| decanonicalize(p.to_str().unwrap()))
                    .collect();
                println!(
                    "User path not specified, taken that of current shell: {}",
                    decanonicalized_path.join(", ")
                );
            }
            #[cfg(not(windows))]
            println!(
                "User path not specified, assumed: {:?}",
                query.target.user_path
            );
        };

        #[cfg(not(windows))]
        let lookup_path = LookupPath::deduce(&query);

        #[cfg(windows)]
        let lookup_path = if let Some(dwp_file_path) = &args.dwp_path {
            LookupPath::from_dwp_file(dwp_file_path.clone(), &query)?
        } else {
            LookupPath::deduce(&query)
        };

        if args.verbose {
            println!(
                "Looking for dependencies of binary {}",
                readable_canonical_path(&binary_path)?
            );
            if let Some(kd) = query.system.as_ref().and_then(|s| s.known_dlls.as_ref()) {
                println!("Known DLLs: {:?}", kd.entries.keys());
            }
            if query
                .system
                .as_ref()
                .map(|s| s.apiset_map.is_some())
                .unwrap_or(false)
            {
                println!("API set map available");
            }
            let lookup_path = LookupPath::deduce(&query);
            let decanonicalized_path: Vec<String> = lookup_path
                .search_path()
                .iter()
                .map(|p| decanonicalize(p.to_str().unwrap()))
                .collect();
            println!("Search path: {}\n", decanonicalized_path.join(", "));
        }

        let mut executables = dependency_runner::runner::run(&query, &lookup_path)?;

        if args.errors_only {
            executables = executables.filter_only_notfound()?;
            if executables.is_empty() {
                println!("No missing DLLs identified");
            }
        }

        let sorted_executables = executables.sorted_by_first_appearance();

        #[cfg(not(windows))]
        let do_skim = args.skim;
        #[cfg(not(windows))]
        let do_skim_symbols = args.skim_symbols;
        #[cfg(windows)]
        let do_skim = false;
        #[cfg(windows)]
        let do_skim_symbols = false;

        // print results
        if !(do_skim || do_skim_symbols) {
            // printing in depth order // TODO: arg to choose output format
            //
            // for e in sorted_executables {
            //     if !e.is_system.unwrap_or(false) {
            //         if let Some(folder) = e.folder {
            //             println!("Found executable {}\n", &e.name);
            //             println!("\tDepth: {}", &e.depth);
            //             println!("\tcontaining folder: {}", folder);
            //
            //             if let Some(deps) = e.dependencies {
            //                 println!("\tdependencies:");
            //                 for d in deps {
            //                     println!("\t\t{}", d);
            //                 }
            //             }
            //         } else {
            //             println!("Executable {} not found\n", &e.name);
            //         }
            //         println!();
            //
            //     }
            // }

            // printing depth-first
            // println!();
            if let Some(root) = executables.get_root()? {
                visit_depth_first(
                    root,
                    0,
                    query.parameters.max_depth,
                    &executables,
                    args.print_system_dlls,
                    &args.filter,
                );
            }

            if args.check_symbols {
                println!("\nChecking symbols...\n");

                let sym_check = executables.check(query.parameters.extract_symbols);
                match sym_check {
                    Ok(report) => {
                        if !report.not_found_libraries.is_empty() {
                            println!("Missing libraries detected!");
                            println!("[Importing executable, missing dependencies]\n");
                            for (importer, missing_dependencies) in report.not_found_libraries.iter() {
                                if !missing_dependencies.is_empty() {
                                    println!("{importer}");
                                    for missing_import_dll in missing_dependencies {
                                        println!("\t{missing_import_dll}");
                                    }
                                }
                            }
                            println!();
                        } else {
                            println!("No missing libraries detected");
                        }

                        if let Some(missing_symbols) = report.not_found_symbols {
                            println!("\nMissing symbols detected!");
                            println!("[Importing executable, exporting executable, missing symbols]\n");
                            for (filename, missing_imports) in missing_symbols.iter() {
                                if !missing_imports.is_empty() {
                                    println!("{filename}");
                                    for (missing_import_dll, missing_symbols) in missing_imports {
                                        println!("\t{missing_import_dll}");
                                        for missing_symbol in missing_symbols {
                                            println!(
                                                "\t\t{}",
                                                demangle_symbol(missing_symbol)
                                                    .as_ref()
                                                    .unwrap_or(missing_symbol)
                                            );
                                        }
                                    }
                                }
                            }
                        } else {
                            println!("No missing symbols detected");
                        }
                    }
                    Err(sym_check_error) => println!("{sym_check_error:?}"),
                }
            }
        }

        // skimming
        #[cfg(not(windows))]
        if args.skim {
            while let Some(selected_dlls) = skim_dlls(&executables) {
                skim_symbols(&executables, Some(selected_dlls));
            }
        } else if args.skim_symbols {
            skim_symbols(&executables, None);
        }

        // JSON representation

        if let Some(json_output_path) = &args.output_json_path {
            // Generate filename with index for multiple files
            let json_path = if binary_paths.len() > 1 {
                let stem = std::path::Path::new(json_output_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("output");
                let ext = std::path::Path::new(json_output_path)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("json");
                let parent = std::path::Path::new(json_output_path).parent();
                let filename = format!("{}_{}.{}", stem, idx, ext);
                if let Some(p) = parent {
                    p.join(filename)
                } else {
                    PathBuf::from(filename)
                }
            } else {
                PathBuf::from(json_output_path)
            };

            let js = serde_json::to_string(&sorted_executables).context("Error serializing")?;

            use std::io::prelude::*;
            let display = json_path.display();

            // Open a file in write-only mode, returns `io::Result<File>`
            let mut file = fs::File::create(&json_path).context(format!("couldn't create {display}"))?;

            // Write to `file`, returns `io::Result<()>`
            file.write_all(js.as_bytes())
                .context(format!("couldn't write to {display}"))?;

            if args.verbose {
                println!("successfully wrote to {display}");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Once;

    static INIT: Once = Once::new();

    fn ensure_binary_built() {
        INIT.call_once(|| {
            let output = std::process::Command::new("cargo")
                .args(&["build", "--bin", "deprun"])
                .output()
                .expect("Failed to build deprun binary");

            if !output.status.success() {
                panic!(
                    "Failed to build deprun binary:\n{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        });
    }

    #[test]
    fn test_deprun_basic() {
        ensure_binary_built();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = manifest_dir
            .join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");

        if !exe_path.exists() {
            eprintln!("Test file not found, skipping test: {:?}", exe_path);
            return;
        }

        let deprun_bin = std::env::var("CARGO_BIN_EXE_deprun")
            .unwrap_or_else(|_| "target/debug/deprun".to_string());

        let output = std::process::Command::new(&deprun_bin)
            .arg(exe_path.to_str().unwrap())
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute deprun at {:?}. Ensure the binary is built.", deprun_bin));

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "deprun command failed:\n{}", stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Check that output contains the executable and its dependency
        assert!(stdout.contains("DepRunTest.exe"));
        assert!(stdout.contains("DepRunTestLib.dll"));
    }

    #[test]
    fn test_deprun_with_filter() {
        ensure_binary_built();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = manifest_dir
            .join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");

        if !exe_path.exists() {
            eprintln!("Test file not found, skipping test: {:?}", exe_path);
            return;
        }

        let deprun_bin = std::env::var("CARGO_BIN_EXE_deprun")
            .unwrap_or("target/debug/deprun".to_string());

        let output = std::process::Command::new(&deprun_bin)
            .arg(exe_path.to_str().unwrap())
            .arg("--filter")
            .arg("DepRunTestLib")
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute deprun at {:?}. Ensure the binary is built.", deprun_bin));

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "deprun command failed:\n{}", stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Check that filtered output contains both the parent and the matching DLL
        assert!(stdout.contains("DepRunTest.exe"), "Parent DLL should be shown");
        assert!(stdout.contains("DepRunTestLib.dll"), "Matching DLL should be shown");
    }

    #[test]
    fn test_deprun_with_filter_no_match() {
        ensure_binary_built();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = manifest_dir
            .join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");

        if !exe_path.exists() {
            eprintln!("Test file not found, skipping test: {:?}", exe_path);
            return;
        }

        let deprun_bin = std::env::var("CARGO_BIN_EXE_deprun")
            .unwrap_or("target/debug/deprun".to_string());

        let output = std::process::Command::new(deprun_bin)
            .arg(exe_path.to_str().unwrap())
            .arg("--filter")
            .arg("NonExistentDLL")
            .output()
            .expect("Failed to execute deprun");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "deprun command failed:\n{}", stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        // With no matches, the root should not be printed either
        assert!(!stdout.contains("DepRunTestLib.dll"));
    }

    #[test]
    fn test_deprun_max_depth() {
        ensure_binary_built();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = manifest_dir
            .join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");

        if !exe_path.exists() {
            eprintln!("Test file not found, skipping test: {:?}", exe_path);
            return;
        }

        let deprun_bin = std::env::var("CARGO_BIN_EXE_deprun")
            .unwrap_or("target/debug/deprun".to_string());

        let output = std::process::Command::new(deprun_bin)
            .arg(exe_path.to_str().unwrap())
            .arg("--max-depth")
            .arg("1")
            .output()
            .expect("Failed to execute deprun");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "deprun command failed:\n{}", stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        // With depth 1, should only show the root executable
        assert!(stdout.contains("DepRunTest.exe"));
        // Should not show dependencies
        assert!(!stdout.contains("DepRunTestLib.dll"));
    }

    #[test]
    fn test_deprun_errors_only() {
        ensure_binary_built();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = manifest_dir
            .join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");

        if !exe_path.exists() {
            eprintln!("Test file not found, skipping test: {:?}", exe_path);
            return;
        }

        let deprun_bin = std::env::var("CARGO_BIN_EXE_deprun")
            .unwrap_or("target/debug/deprun".to_string());

        let output = std::process::Command::new(&deprun_bin)
            .arg(exe_path.to_str().unwrap())
            .arg("--errors-only")
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute deprun at {:?}. Ensure the binary is built.", deprun_bin));

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "deprun command failed:\n{}", stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        #[cfg(windows)]
        {
            // Since this is a valid executable with all dependencies, should show no errors
            assert!(stdout.contains("No missing DLLs identified"));
        }
        #[cfg(not(windows))]
        {
            assert!(stdout.contains("DepRunTestLib.dll") && !stdout.contains("DepRunTestLib.dll => not found"));
        }
    }

    #[test]
    fn test_deprun_json_output() {
        ensure_binary_built();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = manifest_dir
            .join("test_data/test_project1/DepRunTest/build-same-output/bin/Debug/DepRunTest.exe");

        if !exe_path.exists() {
            eprintln!("Test file not found, skipping test: {:?}", exe_path);
            return;
        }

        let temp_dir = std::env::temp_dir();
        let json_output = temp_dir.join("deprun_test_output.json");

        let deprun_bin = std::env::var("CARGO_BIN_EXE_deprun")
            .unwrap_or("target/debug/deprun".to_string());

        let output = std::process::Command::new(&deprun_bin)
            .arg(exe_path.to_str().unwrap())
            .arg("--output-json-path")
            .arg(json_output.to_str().unwrap())
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute deprun at {:?}. Ensure the binary is built.", deprun_bin));

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "deprun command failed:\n{}", stderr);
        assert!(json_output.exists(), "JSON output file should be created");

        // Read and validate JSON
        let json_content = std::fs::read_to_string(&json_output).expect("Failed to read JSON output");
        assert!(json_content.contains("DepRunTest.exe"));
        assert!(json_content.contains("DepRunTestLib.dll"));

        // Clean up
        let _ = std::fs::remove_file(json_output);
    }

    #[cfg(windows)]
    #[test]
    fn test_deprun_vcxproj() {
        if std::env::var("CI").is_ok() {
            eprintln!("Skipping test in CI environment");
            return;
        }

        ensure_binary_built();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let vcxproj_path = manifest_dir
            .join("test_data/test_project1/DepRunTest/build/DepRunTest/DepRunTest.vcxproj");

        if !vcxproj_path.exists() {
            eprintln!("Test file not found, skipping test: {:?}", vcxproj_path);
            return;
        }

        let deprun_bin = std::env::var("CARGO_BIN_EXE_deprun")
            .unwrap_or("target/debug/deprun".to_string());

        let output = std::process::Command::new(deprun_bin)
            .arg(vcxproj_path.to_str().unwrap())
            .arg("--vcxproj-configuration")
            .arg("Debug")
            .output()
            .expect("Failed to execute deprun");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "deprun command failed:\n{}", stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Check that output contains the executable
        assert!(stdout.contains("DepRunTest.exe"));
    }

    #[cfg(windows)]
    #[test]
    fn test_deprun_vcxproj_user() {
        if std::env::var("CI").is_ok() {
            eprintln!("Skipping test in CI environment");
            return;
        }

        ensure_binary_built();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let exe_path = manifest_dir
            .join("test_data/test_project1/DepRunTest/build-vcxproj-user/DepRunTest/Debug/DepRunTest.exe");
        let vcxproj_user_path = manifest_dir
            .join("test_data/test_project1/DepRunTest/build-vcxproj-user/DepRunTest/DepRunTest.vcxproj.user");

        if !exe_path.exists() || !vcxproj_user_path.exists() {
            eprintln!("Test files not found, skipping test");
            return;
        }

        let deprun_bin = std::env::var("CARGO_BIN_EXE_deprun")
            .unwrap_or("target/debug/deprun".to_string());

        let output = std::process::Command::new(deprun_bin)
            .arg(exe_path.to_str().unwrap())
            .arg("--vcxproj-user-path")
            .arg(vcxproj_user_path.to_str().unwrap())
            .arg("--vcxproj-configuration")
            .arg("Debug")
            .output()
            .expect("Failed to execute deprun");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "deprun command failed:\n{}", stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        assert!(stdout.contains("DepRunTest.exe"));
        assert!(stdout.contains("DepRunTestLib.dll"));
    }
}
