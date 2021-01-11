extern crate dependency_runner;

use dependency_runner::models::{LookupResultTreeNode, LookupResultTreeView};
use dependency_runner::system::decanonicalize;
use dependency_runner::{lookup, Executable, Query};

use anyhow::Context;

use clap::{App, Arg};
use std::path::PathBuf;

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
            Arg::with_name("v")
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
        );

    let args = {
        #[cfg(windows)]
        {
            args.arg(
                Arg::with_name("SYSDIR")
                    .short("s")
                    .long("sysdir")
                    .value_name("SYSDIR")
                    .help("Specify a Windows System32 directory other than C:\\Windows\\System32")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("WINDIR")
                    .short("w")
                    .long("windir")
                    .value_name("WINDIR")
                    .help("Specify a Windows directory other than C:\\Windows")
                    .takes_value(true),
            )
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
        }

        #[cfg(not(windows))]
        {
            args
                    .arg(Arg::with_name("SYSDIR")
                        .short("s")
                        .long("sysdir")
                        .value_name("SYSDIR")
                        .help("Specify a Windows System32 directory other than X:\\Windows\\System32 (X is the partition where INPUT lies)")
                        .takes_value(true))
                    .arg(Arg::with_name("WINDIR")
                        .short("w")
                        .long("windir")
                        .value_name("WINDIR")
                        .help("Specify a Windows directory other than X:\\Windows (X is the partition where INPUT lies)")
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

    let verbose = matches.occurrences_of("v") > 0;

    let binary_path = std::fs::canonicalize(matches.value_of("INPUT").unwrap())?;

    let print_system_dlls = matches.is_present("PRINT_SYS_DLLS");

    if !binary_path.exists() {
        eprintln!("Specified file not found at {:?}", binary_path);
        std::process::exit(1);
    }

    let query = {
        let mut query = Query::deduce_from_executable_location(&binary_path)?;

        if let Some(overridden_sysdir) = matches.value_of("SYSDIR") {
            query.system.sys_dir = PathBuf::from(overridden_sysdir);
        } else {
            if verbose {
                println!(
                    "System32 directory not specified, assumed {}",
                    decanonicalize(query.system.sys_dir.to_str().unwrap_or("---"))
                );
            }
        }
        if let Some(overridden_windir) = matches.value_of("WINDIR") {
            query.system.win_dir = PathBuf::from(overridden_windir);
        } else {
            if verbose {
                println!(
                    "Windows directory not specified, assumed {}",
                    decanonicalize(query.system.win_dir.to_str().unwrap_or("---"))
                );
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
                .map(|s| std::fs::canonicalize(s))
                .collect::<Result<Vec<_>, std::io::Error>>()?;
            query.system.path = Some(canonicalized_path);
        } else {
            if verbose {
                #[cfg(windows)]
                {
                    let decanonicalized_path: Vec<String> = query
                        .system
                        .path
                        .as_ref()
                        .unwrap_or(&Vec::new())
                        .iter()
                        .map(|p| decanonicalize(p.to_str().unwrap()))
                        .collect();
                    println!(
                        "User path not specified, taken that of current shell: {}",
                        decanonicalized_path.join(", ")
                    );
                }
                #[cfg(not(windows))]
                println!("User path not specified, assumed: {:?}", query.system.path);
            }
        }
        query
    };

    if verbose {
        println!(
            "Looking for dependencies of binary {}\n",
            decanonicalize(&binary_path.to_str().unwrap())
        );
        let ctx = dependency_runner::Context::new(&query);
        let decanonicalized_path: Vec<String> = ctx
            .search_path()
            .iter()
            .map(|p| decanonicalize(p.to_str().unwrap()))
            .collect();
        println!("Search path: {}\n", decanonicalized_path.join(", "));
    }

    // we pass just the executable filename, and we rely on the fact that its own folder is first on the search path
    let context = dependency_runner::context::Context::new(&query);
    let executables = lookup(query, context)?;

    let mut sorted_executables: Vec<Executable> = executables.values().cloned().collect();
    sorted_executables.sort_by(|e1, e2| e1.depth_first_appearance.cmp(&e2.depth_first_appearance));

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

    // printing in tree order
    //
    let exe_tree = LookupResultTreeView::new(&executables);
    exe_tree.visit_depth_first(|n: &LookupResultTreeNode| {
        if let Some(lr) = executables.get(n.name.as_ref()) {
            if !(lr.details.as_ref().map(|d| d.is_system).unwrap_or(false) && !print_system_dlls) {
                let folder = if !lr.found {
                    "not found".to_owned()
                } else {
                    if let Some(details) = &lr.details {
                        details
                            .folder
                            .to_str()
                            .map(decanonicalize)
                            .unwrap_or("INVALID".to_owned())
                    } else {
                        "not searched".to_owned()
                    }
                };
                println!("{}{} => {}", "\t".repeat(n.depth), n.name, folder);
            }
        } else {
            println!("no data for executable {}", &n.name);
        }
    });

    // JSON representation

    if let Some(json_output_path) = matches.value_of("OUTPUT_JSON_PATH") {
        let js = serde_json::to_string(&sorted_executables).context("Error serializing")?;

        use std::io::prelude::*;
        let path = std::path::Path::new(json_output_path);
        let display = path.display();

        // Open a file in write-only mode, returns `io::Result<File>`
        let mut file =
            std::fs::File::create(&path).context(format!("couldn't create {}", display))?;

        // Write to `file`, returns `io::Result<()>`
        file.write_all(js.as_bytes())
            .context(format!("couldn't write to {}", display))?;

        if verbose {
            println!("successfully wrote to {}", display);
        }
    }

    Ok(())
}
