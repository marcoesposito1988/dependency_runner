extern crate dependency_runner;

use clap::{App, Arg};
use fs_err as fs;

use dependency_runner::path::LookupPath;
use dependency_runner::runner::run;
use dependency_runner::{
    decanonicalize, path_to_string, readable_canonical_path, Executable, LookupQuery, WindowsSystem,
};

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
        .arg(Arg::with_name("Windows root")
            .short("w")
            .long("windows-root")
            .value_name("WINROOT")
            .help("Specify a Windows partition (if not specified, the partition where INPUT lies will be tested and used)")
            .takes_value(true))
        .arg(Arg::with_name("VERBOSE")
            .short("v")
            .multiple(true)
            .help("Sets the level of verbosity"))
        .arg(
            Arg::with_name("HIDE_SYS_DLLS")
                .long("hide-system-dlls")
                .takes_value(false)
                .help("Hide system DLLs in the output"),
        )
        .get_matches();

    let verbose = matches.occurrences_of("VERBOSE") > 0;

    let binary_path = std::path::PathBuf::from(matches.value_of("INPUT").unwrap());

    if !binary_path.exists() {
        eprintln!(
            "Specified file not found at {}",
            binary_path.to_str().unwrap()
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

    let hide_system_dlls = matches.is_present("HIDE_SYS_DLLS");

    let mut query = LookupQuery::deduce_from_executable_location(binary_path)?;

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

    let lookup_path = LookupPath::deduce(&query);
    let executables = run(&query, &lookup_path)?;

    // printing in depth order
    let sorted_executables: Vec<&Executable> = executables.sorted_by_first_appearance();

    let prefix = " ".repeat(8); // as ldd

    for e in sorted_executables.iter().skip(1) {
        if !(e.details.as_ref().map(|d| d.is_system).unwrap_or(false) && hide_system_dlls) {
            if e.found {
                println!(
                    "{}{} => {}",
                    &prefix,
                    &e.dllname,
                    decanonicalize(&path_to_string(
                        e.details.as_ref().map(|d| &d.full_path).unwrap()
                    ))
                );
            } else {
                println!(
                    "{}{} => not found",
                    &prefix,
                    e.details
                        .as_ref()
                        .map(|d| readable_canonical_path(&d.full_path).ok())
                        .flatten()
                        .unwrap_or(format!("{:?}", e.dllname))
                );
            }
        }
    }

    Ok(())
}
