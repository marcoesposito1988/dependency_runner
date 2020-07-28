mod dependency_runner;

use crate::dependency_runner::{
    lookup_executable_dependencies, Context, ExecutablesTreeNode, ExecutablesTreeView, LookupResult,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    #[cfg(windows)]
    if args.len() < 2 {
        println!("You must pass the path to the binary!");
        return;
    }

    // TODO: proper argument passing
    #[cfg(not(windows))]
    if args.len() != 2 && args.len() != 4 {
        println!("Usage: dependency_runner <executable> <system directory> <windows directory> or dependency_runner <executable> to deduce the rest");
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
    let context = if args.len() == 4 {
        let sys_dir = args.get(2).unwrap();
        let win_dir = args.get(3).unwrap();
        Context::new(&binary_dir, &sys_dir, &win_dir, &binary_dir)
    } else {
        Context::deduce_from_executable_location(binary_path).unwrap()
    };

    #[cfg(windows)]
    let context = Context::new(&binary_dir, &binary_dir);

    println!("Looking for dependencies of binary {}\n", binary_filename);
    println!("Assuming working directory: {}\n", binary_dir);

    println!("Search path: {:?}", context.search_path());

    // we pass just the executable filename, and we rely on the fact that its own folder is first on the search path
    let executables = lookup_executable_dependencies(&binary_filename, &context, 6, true);

    let mut sorted_executables: Vec<LookupResult> = executables.values().cloned().collect();
    sorted_executables.sort_by(|e1, e2| e1.depth_first_appearance.cmp(&e2.depth_first_appearance));

    // printing in depth order
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
                println!("{}{}", "\t".repeat(n.depth), n.name);
            }
        }
    });

    // JSON representation
    //
    let j = serde_json::to_string(&sorted_executables);
    if let Ok(js) = j {
        use std::io::prelude::*;
        let path = std::path::Path::new("/tmp/deps.json");
        let display = path.display();

        // Open a file in write-only mode, returns `io::Result<File>`
        let mut file = match std::fs::File::create(&path) {
            Err(why) => panic!("couldn't create {}: {}", display, why),
            Ok(file) => file,
        };

        // Write the `LOREM_IPSUM` string to `file`, returns `io::Result<()>`
        match file.write_all(js.as_bytes()) {
            Err(why) => panic!("couldn't write to {}: {}", display, why),
            Ok(_) => println!("successfully wrote to {}", display),
        }
    } else {
        println!("Error serializing");
    }
}
