extern crate dependency_runner;

use clap::Parser;
use dependency_runner::common::{decanonicalize, path_to_string, readable_canonical_path};
use dependency_runner::executable::Executable;
use fs_err as fs;

use dependency_runner::path::LookupPath;
use dependency_runner::query::LookupQuery;
use dependency_runner::runner::run;
#[cfg(not(windows))]
use dependency_runner::system::WindowsSystem;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct WlddCli {
    #[clap(value_parser)]
    /// Target file (.exe, .dll or .vcxproj)
    input: String,
    #[clap(short, long)]
    /// Activate verbose output
    verbose: bool,
    #[clap(short = 's', long)]
    /// Do not include system DLLs in the output
    hide_system_dlls: bool,
    #[cfg(not(windows))]
    #[clap(value_parser, short, long)]
    /// Windows partition to use for system DLLs lookup (if not specified, the partition where INPUT lies will be tested and used if valid)
    windows_root: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = WlddCli::parse();

    let binary_path = std::path::PathBuf::from(args.input);

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

    #[cfg(not(windows))]
    let mut query = LookupQuery::deduce_from_executable_location(binary_path)?;
    #[cfg(windows)]
    let query = LookupQuery::deduce_from_executable_location(binary_path)?;

    #[cfg(not(windows))]
    if let Some(overridden_winroot) = args.windows_root {
        query.system = WindowsSystem::from_root(overridden_winroot);
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

    let lookup_path = LookupPath::deduce(&query);
    let executables = run(&query, &lookup_path)?;

    // printing in depth order
    let sorted_executables: Vec<&Executable> = executables.sorted_by_first_appearance();

    let prefix = " ".repeat(8); // as ldd

    for e in sorted_executables.iter().skip(1) {
        if !(e.details.as_ref().map(|d| d.is_system).unwrap_or(false) && args.hide_system_dlls) {
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
                        .and_then(|d| readable_canonical_path(&d.full_path).ok())
                        .unwrap_or(format!("{:?}", e.dllname))
                );
            }
        }
    }

    Ok(())
}
