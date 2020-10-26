extern crate dependency_runner;

use dependency_runner::models::{ExecutablesTreeNode, ExecutablesTreeView};
use dependency_runner::{lookup_executable_dependencies_recursive, LookupContext, LookupResult};

use anyhow::Context;
use clap::{App, Arg};

fn main() -> anyhow::Result<()> {
    let matches = App::new("dependency_runner")
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
                .long("output_json_path")
                .value_name("OUTPUT_JSON_PATH")
                .help("Sets the path for the output JSON file")
                .takes_value(true)
        )
        .arg(Arg::with_name("System directory")
            .short("s")
            .long("sysdir")
            .value_name("SYSDIR")
            .help("Specify a Windows System32 directory other than X:\\Windows\\System32 (X is the partition where INPUT lies)")
            .takes_value(true))
        .arg(Arg::with_name("Windows directory")
            .short("w")
            .long("windir")
            .value_name("WINDIR")
            .help("Specify a Windows directory other than X:\\Windows (X is the partition where INPUT lies)")
            .takes_value(true))
        .arg(Arg::with_name("v")
            .short("v")
            .multiple(true)
            .help("Sets the level of verbosity"))
        .get_matches();

    let verbose = matches.occurrences_of("v") > 0;

    let binary_path = matches.value_of("INPUT").unwrap();

    if !std::path::Path::new(binary_path).exists() {
        eprintln!("Specified file not found at {}", binary_path);
        std::process::exit(1);
    }

    let binary_dir = std::path::Path::new(binary_path)
        .parent()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let binary_filename = std::path::Path::new(binary_path)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let context = {
        #[cfg(not(windows))]
        let mut context = LookupContext::deduce_from_executable_location(binary_path).unwrap();

        #[cfg(windows)]
        let mut context = LookupContext::new(&binary_dir, &binary_dir);

        if let Some(overridden_sysdir) = matches.value_of("SYSDIR") {
            context.sys_dir = overridden_sysdir.to_string();
        } else {
            if verbose {
                println!(
                    "System32 directory not specified, assumed {}",
                    context.sys_dir
                );
            }
        }
        if let Some(overridden_windir) = matches.value_of("WINDIR") {
            context.win_dir = overridden_windir.to_string();
            if verbose {
                println!(
                    "Windows directory not specified, assumed {}",
                    context.win_dir
                );
            }
        }
        context
    };

    if verbose {
        println!("Looking for dependencies of binary {}\n", binary_filename);
        println!("Assuming working directory: {}\n", binary_dir);
        println!("Search path: {:?}", context.search_path());
    }

    // we pass just the executable filename, and we rely on the fact that its own folder is first on the search path
    let executables = lookup_executable_dependencies_recursive(&binary_filename, &context, 6, true);

    let mut sorted_executables: Vec<LookupResult> = executables.values().cloned().collect();
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
    let exe_tree = ExecutablesTreeView::new(&executables);
    exe_tree.visit_depth_first(|n: &ExecutablesTreeNode| {
        if let Some(lr) = executables.get(&n.name) {
            if lr.is_system.is_some() && !lr.is_system.unwrap() {
                println!(
                    "{}{} => {}",
                    "\t".repeat(n.depth),
                    n.name,
                    lr.folder.as_ref().unwrap()
                );
            }
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
