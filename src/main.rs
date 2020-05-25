use crate::rupencies::{Context, lookup_executable_dependencies};

mod rupencies;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        println!("You must pass the path to the binary!");
        return;
    }

    // TODO: proper argument passing
    #[cfg(not(windows))]
    if args.len() < 4 {
        println!("Usage: rupencies <executable> <system directory> <windows directory>");
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

    lookup_executable_dependencies(&binary_path, &context);
}