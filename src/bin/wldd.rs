extern crate dependency_runner;

use clap::{App, Arg};

use dependency_runner::{lookup_executable_dependencies_recursive, Executable, LookupContext};

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

    #[cfg(windows)]
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

    // we pass just the executable filename, and we rely on the fact that its own folder is first on the search path
    let executables =
        lookup_executable_dependencies_recursive(&binary_filename, &context, 6, true)?;

    let mut sorted_executables: Vec<Executable> = executables.values().cloned().collect();
    sorted_executables.sort_by(|e1, e2| e1.depth_first_appearance.cmp(&e2.depth_first_appearance));

    // printing in depth order

    let prefix = " ".repeat(8); // as ldd

    for e in sorted_executables {
        if !(e.details.as_ref().map(|d| d.is_system).unwrap_or(true) && hide_system_dlls) {
            if e.found {
                println!("{}{} => {}", &prefix, &e.name, e.full_path());
            } else {
                println!("{}{} => not found", &prefix, &e.name);
            }
        }
    }

    Ok(())
}
