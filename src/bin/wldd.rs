extern crate dependency_runner;

use dependency_runner::{lookup_executable_dependencies, LookupContext, LookupResult};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    #[cfg(windows)]
    if args.len() < 2 {
        println!("You must pass the path to the binary!");
        return;
    }

    // TODO: proper argument passing
    #[cfg(not(windows))]
    if args.len() != 2 && args.len() != 4 {
        eprintln!("Usage: dependency_runner <executable> <system directory> <windows directory> or dependency_runner <executable> to deduce the rest");
        std::process::exit(1);
    }

    let binary_path = args.get(1).unwrap();

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

    #[cfg(not(windows))]
    let context = if args.len() == 4 {
        let sys_dir = args.get(2).unwrap();
        let win_dir = args.get(3).unwrap();
        LookupContext::new(&binary_dir, &sys_dir, &win_dir, &binary_dir)
    } else {
        LookupContext::deduce_from_executable_location(binary_path).unwrap()
    };

    #[cfg(windows)]
    let context = LookupContext::new(&binary_dir, &binary_dir);

    // we pass just the executable filename, and we rely on the fact that its own folder is first on the search path
    let executables = lookup_executable_dependencies(&binary_filename, &context, 6, true);

    let mut sorted_executables: Vec<LookupResult> = executables.values().cloned().collect();
    sorted_executables.sort_by(|e1, e2| e1.depth_first_appearance.cmp(&e2.depth_first_appearance));

    // printing in depth order

    let prefix = " ".repeat(8); // as ldd

    for e in sorted_executables {
        if !e.is_system.unwrap_or(false) {
            if let Some(folder) = e.folder {
                if let Some(full_path_str) = std::path::Path::new(&folder).join(&e.name).to_str() {
                    println!("{}{} => {}", &prefix, &e.name, full_path_str);
                } else {
                    println!("{}{} => invalid path", &prefix, &e.name);
                }
            } else {
                println!("{}{} => not found", &prefix, &e.name);
            }
        }
    }

    Ok(())
}
