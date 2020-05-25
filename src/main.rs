use crate::dependency_runner::{Context, lookup_executable_dependencies, LookupResult};

mod dependency_runner;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        println!("You must pass the path to the binary!");
        return;
    }

    // TODO: proper argument passing
    #[cfg(not(windows))]
    if args.len() < 4 {
        println!("Usage: dependency_runner <executable> <system directory> <windows directory>");
        return;
    }

    let binary_path = args.get(1).unwrap();

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
        let context = {
        let sys_dir = args.get(2).unwrap();
        let win_dir = args.get(3).unwrap();
        Context::new(&binary_dir, &sys_dir, &win_dir, &binary_dir)
    };

    #[cfg(windows)]
        let context = Context::new(&binary_dir, &binary_dir);


    println!("Looking for dependencies of binary {}\n", binary_filename);
    println!("Assuming working directory: {}\n", binary_dir);

    // we pass just the executable filename, and we rely on the fact that its own folder is first on the search path
    let executables = lookup_executable_dependencies(&binary_filename, &context, 2, true);

    let mut sorted_executables: Vec<LookupResult> = executables.values().cloned().collect();
    sorted_executables.sort_by(|e1, e2| e1.depth.cmp(&e2.depth));

    for e in sorted_executables {
        if !e.is_system.unwrap_or(false) {
            if let Some(folder) = e.folder {
                println!("Found executable {}\n", &e.name);
                println!("\tDepth: {}", &e.depth);
                println!("\tcontaining folder: {}", folder);

                if let Some(deps) = e.dependencies {
                    println!("\tdependencies:");
                    for d in deps {
                        println!("\t\t{}", d);
                    }
                }
            } else {
                println!("Executable {} not found\n", &e.name);
            }
            println!();

        }
    }
}