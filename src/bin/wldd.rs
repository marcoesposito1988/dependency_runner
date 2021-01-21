extern crate dependency_runner;

use clap::{App, Arg};

use dependency_runner::{path_to_string, osstring_to_string, decanonicalize};
use dependency_runner::{lookup, Context, Executable, Query};

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

    let binary_path = matches.value_of("INPUT").unwrap();

    let hide_system_dlls = matches.is_present("HIDE_SYS_DLLS");

    if !std::path::Path::new(binary_path).exists() {
        eprintln!("Specified file not found at {}", binary_path);
        std::process::exit(1);
    }

    let mut query = Query::deduce_from_executable_location(binary_path)?;

    if let Some(overridden_sysdir) = matches.value_of("SYSDIR") {
        query.system.sys_dir = std::fs::canonicalize(overridden_sysdir)?;
    } else {
        if verbose {
            println!("System32 directory not specified, assumed {}", path_to_string(&query.system.sys_dir));
        }
    }
    if let Some(overridden_windir) = matches.value_of("WINDIR") {
        query.system.win_dir = std::fs::canonicalize(overridden_windir)?;
        if verbose {
            println!("Windows directory not specified, assumed {}", path_to_string(&query.system.win_dir));
        }
    }

    let context = Context::new(&query);
    let executables = lookup(&query, context)?;

    // printing in depth order
    let mut sorted_executables: Vec<Executable> = executables.values().cloned().collect();
    sorted_executables.sort_by(|e1, e2| e1.depth_first_appearance.cmp(&e2.depth_first_appearance));
    debug_assert_eq!(sorted_executables.first().unwrap().name, query.target_exe.file_name().unwrap());

    let prefix = " ".repeat(8); // as ldd

    for e in sorted_executables.iter().skip(1) {
        if !(e.details.as_ref().map(|d| d.is_system).unwrap_or(false) && hide_system_dlls) {
            if e.found {
                println!("{}{} => {}", &prefix, osstring_to_string(&e.name),
                         decanonicalize(&path_to_string(e.full_path().unwrap())));
            } else {
                println!(
                    "{}{} => not found",
                    &prefix,
                    e.name.to_str().unwrap_or(format!("{:?}", e.name).as_ref())
                );
            }
        }
    }

    Ok(())
}
