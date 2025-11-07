mod args;
mod symlinks;

/// # Project Purpose
/// Take czkawka duplicate file scan results and replace each duplicate instance with a symlink. This is intended to save disk
/// space while maintaining file accessibility through the symlink.
///
/// This project is cross-platform and safe fore use on Windows, Linux, and MacOS systems.
///
/// This function will start the program. It:
/// - Checks for the correct arguments
/// - Validates the file input
/// - Replaces duplicate files with symlinks
pub fn start() {
    let args: Vec<String> = std::env::args().collect();
    let valid_args = args::validate_arguments(args);

    let mut valid_args = match valid_args {
        Ok(valid_args) => valid_args,
        Err(e) => {
            eprintln!("Invalid arguments provided.");
            eprintln!("{e}");
            args::print_usage();
            return;
        }
    };

    if valid_args.allow_roots.is_empty() {
        eprintln!(
            "At least one --allow-root <PATH> must be provided to prevent destructive mistakes."
        );
        args::print_usage();
        return;
    }

    let canonical_roots = match args::canonicalize_roots(&valid_args.allow_roots) {
        Ok(roots) => roots,
        Err(e) => {
            eprintln!("Failed to validate provided allow-root paths.");
            eprintln!("{e}");
            return;
        }
    };
    valid_args.allow_roots = canonical_roots;

    let validated_files = match args::validate_files(&valid_args.input_file_path) {
        Ok(files) => files,
        Err(e) => {
            eprintln!("The provided input file is not valid.");
            eprintln!("{e}");
            return;
        }
    };

    if let Err(e) = symlinks::replace_duplicates_with_symlinks(&valid_args, &validated_files) {
        eprintln!("Failed to replace duplicates: {}", e);
        std::process::exit(1);
    }
}
