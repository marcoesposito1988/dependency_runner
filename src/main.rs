use crate::rupencies::{lookup_executable_dependencies, Context};

#[cfg(windows)]
mod rupencies;

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        println!("You must pass the path to the binary!");
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

    let context = Context::new(&binary_dir, &binary_dir);

    println!("Looking for dependencies of binary {}\n", binary_filename);
    println!("Assuming working directory: {}\n", binary_dir);

    lookup_executable_dependencies(&binary_path, &context);
}

// this is not going to work on other platforms
#[cfg(not(windows))]
fn main() {
    println!("This package is only going to work on Windows. Sorry!")
}
