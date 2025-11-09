#![doc(html_root_url = "https://docs.rs/czkawka-dupes-to-symlinks/0.1.0")]
//! # Czkawka Dupes to Symlinks
//!
//! A safe, deterministic way to reclaim disk space after running
//! [Czkawka](https://github.com/qarmin/czkawka)'s duplicate scanner. The CLI
//! replaces redundant files with symlinks and automatically rolls back whenever a
//! step fails, while the library API exposes the same safety guarantees for
//! custom tooling.
//!
//! ## When to use this crate
//! - Run the **CLI** via `cargo install czkawka-dupes-to-symlinks` to process Czkawka JSON reports.
//! - Embed the **library** when you want to validate JSON reports programmatically
//!   and invoke the symlink replacement engine from your own UI/automation.
//!
//! ## Quick start (CLI)
//! ```text
//! czkawka-dupes-to-symlinks \
//!     --input-file-path /tmp/czkawka.json \
//!     --allow-root /srv/media --allow-root /srv/backups
//! ```
//!
//! Add `--dry-run` to preview changes without touching the filesystem.
//!
//! ## Quick start (library)
//! ```no_run
//! use czkawka_dupe_to_symlinks::{
//!     replace_duplicates_with_symlinks, validate_files, Args, OriginalToKeep,
//! };
//!
//! # fn main() -> anyhow::Result<()> {
//! let args = Args {
//!     input_file_path: "/tmp/czkawka.json".into(),
//!     dry_run: false,
//!     original_to_keep: OriginalToKeep::Newest,
//!     allow_roots: vec!["/srv/media".into(), "/srv/backups".into()],
//! };
//!
//! let files = validate_files(&args.input_file_path)?;
//! replace_duplicates_with_symlinks(&args, &files)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## JSON expectations
//! Reports must match the structure that Czkawka emits:
//! - the root object is keyed by **file size** (as decimal strings)
//! - every value is a list of duplicate groups; each group contains ≥ 2 entries
//! - every entry includes `path`, `modified_date` (Unix epoch seconds), `size`,
//!   and a content `hash`
//! - hashes inside a group must match—`replace_duplicates_with_symlinks`
//!   re-checks them as a guardrail
//!
//! See [`validate_files`] for details on the runtime schema validation.
//!
//! ## Safety guardrails
//! - All file operations are restricted to the canonicalized `--allow-root`
//!   directories.
//! - Every replacement stages a `*.czkawka-bak` backup and restores it if the
//!   symlink cannot be created.
//! - Dry runs (`--dry-run`) exercise the entire pipeline but leave the
//!   filesystem untouched.
//!
//! ## Exit semantics
//! | Code | Meaning |
//! |------|---------|
//! | `0` | All duplicates were processed or intentionally skipped. |
//! | `1` | At least one duplicate could not be processed (outside sandbox, missing file, permission error, etc.). |

mod args;
mod symlinks;

pub use args::{Args, OriginalToKeep, validate_files};
pub use symlinks::replace_duplicates_with_symlinks;

/// Run the CLI entrypoint.
///
/// This helper mirrors what `src/main.rs` does: parse the command-line arguments,
/// enforce the safety guardrails, and exit with a descriptive non-zero status if
/// any duplicate fails to process.
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
