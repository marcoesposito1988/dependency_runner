extern crate dependency_runner;

#[cfg(windows)]
use dependency_runner::vcx::{parse_vcxproj, parse_vcxproj_user};
#[cfg(windows)]
use dependency_runner::LookupError;
use dependency_runner::{
    decanonicalize, demangle_symbol, lookup, path_to_string, readable_canonical_path, Executable,
    Executables, LookupQuery, WindowsSystem,
};

use anyhow::Context;
use clap::{value_t, App, Arg};
use fs_err as fs;
use std::path::PathBuf;

#[cfg(windows)]
fn pick_configuration(
    configs: &Vec<&String>,
    user_config: &Option<&str>,
    file_path: &str,
) -> Result<String, LookupError> {
    if let Some(vcx_config) = user_config {
        if configs.contains(&&vcx_config.to_string()) {
            Ok(vcx_config.to_owned().to_string())
        } else {
            return Err(LookupError::ContextDeductionError(format!(
                "The specified configuration {} was not found in project file {}\n\
                Available configurations: {:?}",
                vcx_config, file_path, configs
            )));
        }
    } else {
        if configs.len() == 1 {
            let single_config = configs.last().unwrap();
            eprintln!(
                "Visual Studio configuration not specified, using {} for file {}",
                single_config, file_path
            );
            Ok(single_config.to_owned().to_string())
        } else {
            return Err(LookupError::ContextDeductionError(format!(
                "Must specify a configuration with --vcx-config=<CONFIG> for project file {}\n\
                Available configurations: {:?}",
                file_path, configs
            )));
        }
    }
}

fn visit_depth_first(
    e: &Executable,
    current_depth: usize,
    exes: &Executables,
    query: &LookupQuery,
    print_system_dlls: bool,
) {
    if query.max_depth.map(|d| current_depth < d).unwrap_or(true) {
        if !(e.details.as_ref().map(|d| d.is_system).unwrap_or(false) && !print_system_dlls) {
            let folder = if !e.found {
                "not found".to_owned()
            } else {
                if let Some(details) = &e.details {
                    readable_canonical_path(&details.full_path.parent().unwrap())
                        .unwrap_or("INVALID".to_owned())
                } else {
                    "not searched".to_owned()
                }
            };
            println!("{}{} => {}", "\t".repeat(current_depth), e.dllname, folder);

            if let Some(details) = &e.details {
                if let Some(dependencies) = &details.dependencies {
                    for d in dependencies {
                        if let Some(de) = exes.get(&d) {
                            visit_depth_first(
                                de,
                                current_depth + 1,
                                exes,
                                query,
                                print_system_dlls,
                            );
                        }
                    }
                }
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args = App::new("dependency_runner")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Marco Esposito <marcoesposito1988@gmail.com>")
        .about("ldd for Windows - and more!")
        .arg(
            Arg::with_name("INPUT")
                .help("Sets the input file to use")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::with_name("OUTPUT_JSON_PATH")
                .short("j")
                .long("output-json-path")
                .value_name("OUTPUT_JSON_PATH")
                .help("Sets the path for the output JSON file")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("MAX_DEPTH")
                .short("d")
                .long("max-depth")
                .value_name("MAX_DEPTH")
                .help("Maximum recursion depth (default: unlimited)")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("VERBOSE")
                .short("v")
                .long("verbose")
                .multiple(true)
                .help("Sets the level of verbosity"),
        )
        .arg(
            Arg::with_name("PRINT_SYS_DLLS")
                .long("print-system-dlls")
                .takes_value(false)
                .help("Include system DLLs in the output"),
        )
        .arg(
            Arg::with_name("CHECK_SYMBOLS")
                .long("check-symbols")
                .takes_value(false)
                .help("Check that all imported symbols are found within the (non-system) dependencies"),
        );

    let args = {
        #[cfg(windows)]
        {
            args
                .arg(
                    Arg::with_name("WORKDIR")
                        .short("k")
                        .long("workdir")
                        .value_name("WORKDIR")
                        .help(
                            "Specify a current working directory other than that of the current shell",
                        )
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("PATH")
                        .short("a")
                        .long("userpath")
                        .value_name("PATH")
                        .help("Specify a user path different from that of the current shell")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("DWP_FILE_PATH")
                        .long("dwp-file-path")
                        .value_name("DWP_FILE_PATH")
                        .help("Read the search path from a .dwp file (Dependency Walker's format)")
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("VCXPROJ_USER_PATH")
                        .long("vcxuser-path")
                        .value_name("VCXPROJ_USER_PATH")
                        .help("Path to a .vcxproj.user file to parse for PATH entries to be added to the search path")
                        .takes_value(true)
                        .conflicts_with("DWP_FILE_PATH"),
                )
                .arg(
                    Arg::with_name("VCXPROJ_CONFIGURATION")
                        .long("vcx-config")
                        .value_name("VCXPROJ_CONFIGURATION")
                        .help("Configuration to use (Debug, Release, ...) if the target is a .vcxproj file, or a .vcxproj.user was provided")
                        .takes_value(true)
                        .conflicts_with("DWP_FILE_PATH"),
                )
        }

        #[cfg(not(windows))]
        {
            args
                .arg(Arg::with_name("Windows root")
                    .short("w")
                    .long("windows-root")
                    .value_name("WINROOT")
                    .help("Specify a Windows partition (if not specified, the partition where INPUT lies will be tested and used)")
                    .takes_value(true))
                .arg(Arg::with_name("WORKDIR")
                    .short("k")
                    .long("workdir")
                    .value_name("WORKDIR")
                    .help("Specify a current working directory other than that of the current shell")
                    .takes_value(true))
                .arg(
                    Arg::with_name("PATH")
                        .short("a")
                        .long("userpath")
                        .value_name("PATH")
                        .help("Specify a user path")
                        .takes_value(true),
                )
        }
    };

    let matches = args.get_matches();

    let verbose = matches.occurrences_of("VERBOSE") > 0;

    let binary_path = PathBuf::from(matches.value_of("INPUT").unwrap());

    if !binary_path.exists() {
        eprintln!(
            "Specified file not found at {}\nCurrent working directory: {}",
            binary_path.to_str().unwrap(),
            std::env::current_dir()?.to_str().unwrap(),
        );
        std::process::exit(1);
    }

    if binary_path.is_dir() {
        eprintln!(
            "The specified path is a directory, not a PE executable file: {}",
            binary_path.to_str().unwrap(),
        );
        std::process::exit(1);
    }

    let binary_path = fs::canonicalize(binary_path)?;

    let print_system_dlls = matches.is_present("PRINT_SYS_DLLS");

    let check_symbols = matches.is_present("CHECK_SYMBOLS");

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
            &matches.value_of("VCXPROJ_CONFIGURATION"),
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

        if let Some(vcxproj_user_path_str) = matches.value_of("VCXPROJ_USER_PATH") {
            let vcxproj_user_path = std::path::Path::new(vcxproj_user_path_str);
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
                &matches.value_of("VCXPROJ_CONFIGURATION"),
                vcxproj_user_path_str,
            )?;
            let vcx_debug_info = &vcx_debug_info_per_config[&config_to_use];

            query.update_from_vcx_debugging_configuration(vcx_debug_info);
        }
        query
    };

    if let Ok(max_depth) = value_t!(matches.value_of("MAX_DEPTH"), usize) {
        query.max_depth = Some(max_depth);
    }

    query.extract_symbols = check_symbols;

    // overrides (must be last)

    if let Some(overridden_sysdir) = matches.value_of("WINROOT") {
        query.system = WindowsSystem::from_root(overridden_sysdir);
    } else {
        if verbose {
            if let Some(system) = &query.system {
                println!(
                    "Windows partition root not specified, assumed {}",
                    path_to_string(&system.sys_dir)
                );
            } else {
                println!("Windows partition root not specified, and executable doesn't lie in one; system DLL imports will not be resolved");
            }
        }
    }

    if let Some(overridden_workdir) = matches.value_of("WORKDIR") {
        query.working_dir = PathBuf::from(overridden_workdir);
    } else {
        if verbose {
            println!(
                "Working directory not specified, assuming directory of executable: {}",
                decanonicalize(query.working_dir.to_str().unwrap_or("---"))
            );
        }
    }
    if let Some(overridden_path) = matches.value_of("PATH") {
        let canonicalized_path: Vec<PathBuf> = overridden_path
            .split(";")
            .filter_map(|s| {
                let p = std::path::Path::new(s);
                if p.exists() {
                    Some(fs::canonicalize(s))
                } else {
                    eprintln!("Skipping non-existing path entry {}", s);
                    None
                }
            })
            .collect::<Result<Vec<_>, std::io::Error>>()?;
        query.user_path.extend(canonicalized_path);
    } else {
        if verbose {
            #[cfg(windows)]
            {
                let decanonicalized_path: Vec<String> = query
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
            println!("User path not specified, assumed: {:?}", query.user_path);
        }
    };

    if verbose {
        println!(
            "Looking for dependencies of binary {}\n",
            readable_canonical_path(&binary_path)?
        );
        let ctx = dependency_runner::LookupPath::new(&query);
        let decanonicalized_path: Vec<String> = ctx
            .search_path()
            .iter()
            .map(|p| decanonicalize(p.to_str().unwrap()))
            .collect();
        println!("Search path: {}\n", decanonicalized_path.join(", "));
    }

    #[cfg(not(windows))]
    let context = dependency_runner::lookup_path::LookupPath::new(&query);

    #[cfg(windows)]
    let context = if let Some(dwp_file_path) = matches.value_of("DWP_FILE_PATH") {
        dependency_runner::lookup_path::LookupPath::from_dwp_file(dwp_file_path, &query)?
    } else {
        dependency_runner::lookup_path::LookupPath::new(&query)
    };

    let executables = lookup(&query, context)?;

    let sorted_executables = executables.sorted_by_first_appearance();

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
    println!();
    if let Some(root) = executables.get_root()? {
        visit_depth_first(root, 0, &executables, &query, print_system_dlls);
    }

    if check_symbols {
        println!("\nChecking symbols...\n");

        let sym_check = executables.check();
        match sym_check {
            Ok(report) => {
                if !report.not_found_libraries.is_empty() {
                    println!("Missing libraries detected!");
                    println!("[Importing executable, missing dependencies]\n");
                    for (importer, missing_dependencies) in report.not_found_libraries.iter() {
                        if !missing_dependencies.is_empty() {
                            println!("{}", importer);
                            for missing_import_dll in missing_dependencies {
                                println!("\t{}", missing_import_dll);
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
                            println!("{}", filename);
                            for (missing_import_dll, missing_symbols) in missing_imports {
                                println!("\t{}", missing_import_dll);
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
            Err(sym_check_error) => println!("{:?}", sym_check_error),
        }
    }

    // JSON representation

    if let Some(json_output_path) = matches.value_of("OUTPUT_JSON_PATH") {
        let js = serde_json::to_string(&sorted_executables).context("Error serializing")?;

        use std::io::prelude::*;
        let path = std::path::Path::new(json_output_path);
        let display = path.display();

        // Open a file in write-only mode, returns `io::Result<File>`
        let mut file = fs::File::create(&path).context(format!("couldn't create {}", display))?;

        // Write to `file`, returns `io::Result<()>`
        file.write_all(js.as_bytes())
            .context(format!("couldn't write to {}", display))?;

        if verbose {
            println!("successfully wrote to {}", display);
        }
    }

    Ok(())
}
