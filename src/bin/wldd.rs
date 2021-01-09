extern crate dependency_runner;

use clap::{App, Arg};

use dependency_runner::{lookup, Executable, Query};

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
        .arg(Arg::with_name("v")
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

    let verbose = matches.occurrences_of("v") > 0;

    let binary_path = matches.value_of("INPUT").unwrap();

    let hide_system_dlls = matches.is_present("HIDE_SYS_DLLS");

    if !std::path::Path::new(binary_path).exists() {
        eprintln!("Specified file not found at {}", binary_path);
        std::process::exit(1);
    }

    // we pass just the executable filename, and we rely on the fact that its own folder is first on the search path
    let mut query = Query::deduce_from_executable_location(binary_path)?;

    if let Some(overridden_sysdir) = matches.value_of("SYSDIR") {
        query.system.sys_dir = overridden_sysdir.to_string().parse()?;
    } else {
        if verbose {
            println!(
                "System32 directory not specified, assumed {}",
                query.system.sys_dir.to_str().unwrap_or("---")
            );
        }
    }
    if let Some(overridden_windir) = matches.value_of("WINDIR") {
        query.system.win_dir = overridden_windir.to_string().parse()?;
        if verbose {
            println!(
                "Windows directory not specified, assumed {}",
                query.system.win_dir.to_str().unwrap_or("---")
            );
        }
    }

    let executables = lookup(query)?;

    // printing in depth order
    let mut sorted_executables: Vec<Executable> = executables.values().cloned().collect();
    sorted_executables.sort_by(|e1, e2| e1.depth_first_appearance.cmp(&e2.depth_first_appearance));

    let prefix = " ".repeat(8); // as ldd

    for e in sorted_executables {
        if !(e.details.as_ref().map(|d| d.is_system).unwrap_or(true) && hide_system_dlls) {
            if e.found {
                println!(
                    "{}{} => {:?}",
                    &prefix,
                    e.name.to_str().unwrap_or("---"),
                    e.full_path()
                        .and_then(|p| { p.to_str().map(|f| f.to_owned()) })
                        .unwrap_or("INVALID PATH".to_owned())
                );
            } else {
                println!(
                    "{}{} => not found",
                    &prefix,
                    e.name.to_str().unwrap_or("---")
                );
            }
        }
    }

    Ok(())
}
