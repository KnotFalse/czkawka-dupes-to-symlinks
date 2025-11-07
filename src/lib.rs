mod args;
mod symlinks;

use clap;

/// # Project Purpose
/// Take czkawka duplicate file scan results and replace each duplicate instance with a symlink. This is intended to save disk
/// space while maintaining file accessibility through the symlink.
///
/// This project is cross-platform and safe fore use on Windows, Linux, and MacOS systems.

/// This function will start the program. It:
/// - Checks for the correct arguments
/// - Validates the file input
/// - Replaces duplicate files with symlinks
pub fn start() {
    let args: Vec<String> = std::env::args().collect();
    let valid_args = args::validate_arguments(args);

    let valid_args = match valid_args {
        Ok(valid_args) => valid_args,
        Err(e) => {
            eprintln!("Invalid arguments provided.");
            eprintln!("{e}");
            args::print_usage();
            return;
        }
    };

    let file_is_valid = args::validate_files(&valid_args.input_file_path);
    match file_is_valid {
        Ok(false) => {}
        Err(e) => {
            eprintln!("The provided input file is not valid.");
            eprintln!("{e}");
            return;
        },
        _ => {}
    }

    symlinks::replace_duplicates_with_symlinks(&valid_args);
}
