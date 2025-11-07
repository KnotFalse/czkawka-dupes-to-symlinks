use crate::args;
use anyhow::{Context, Error, Result};
use rayon::prelude::*;
use std::collections::HashMap;

pub fn replace_duplicates_with_symlinks(
    args: &args::Args,
    input_files: &[std::path::PathBuf],
) -> Result<(), Error> {
    let json_by_file = input_files
        .into_par_iter()
        .map(|file_path| -> Result<CzkawkaDuplicateJsonFormat> {
            let file_contents = std::fs::read_to_string(file_path).with_context(|| {
                format!(
                    "Failed to read input file as string: {}",
                    file_path.display()
                )
            })?;
            let parsed_json: CzkawkaDuplicateJsonFormat = serde_json::from_str(&file_contents)
                .with_context(|| {
                    format!(
                        "Failed to parse input file as JSON: {}",
                        file_path.display()
                    )
                })?;
            Ok(parsed_json)
        })
        .collect::<Result<Vec<CzkawkaDuplicateJsonFormat>, Error>>()
        .context("Failed to parse all files as JSON.");

    let json_by_file = match json_by_file {
        Ok(data) => data,
        Err(e) => {
            eprintln!("{}", e);
            return Err(e);
        }
    };

    // Using a thread-safe collection to store errors
    let errors: std::sync::Mutex<Vec<Error>> = std::sync::Mutex::new(vec![]);

    json_by_file.into_par_iter().for_each(|dupes_in_one_json_file| {
        dupes_in_one_json_file
            .into_par_iter()
            .for_each(|(_file_size, duplicate_groups)| {
                duplicate_groups
                    .into_par_iter()
                    .for_each(|duplicate_group| {
                        if duplicate_group.len() < 2 {
                            return;
                        }

                        let hashes_match = confirm_hashes_match(&duplicate_group);

                        if !hashes_match {
                            let err = anyhow::anyhow!(
                                "Hashes do not match for duplicate group: {:?}",
                                duplicate_group
                                    .iter()
                                    .map(|e| &e.path)
                                    .collect::<Vec<&String>>()
                            );
                            errors
                                .lock()
                                .expect("Should be able to unwrap lock")
                                .push(err);

                            return;
                        }

                        let hash = duplicate_group[0].hash.clone();

                        let (files_that_exist, files_that_dont_exist): (
                            Vec<CzkawkaDuplicateJsonFormatElement>,
                            Vec<CzkawkaDuplicateJsonFormatElement>,
                        ) = duplicate_group
                            .into_par_iter()
                            .partition(|e| std::path::Path::new(&e.path).exists());

                        if !files_that_dont_exist.is_empty() {
                            errors.lock().expect("Should be able to unwrap lock").push(
                                anyhow::anyhow!(
                                    "Some files specified as duplicates do not exist:\n\
                                    {hash}\n\
                                    {:?}\n\
                                    The specified duplicates that do exist were replaced with symlinks.",
                                    files_that_dont_exist
                                        .par_iter()
                                        .map(|e| &e.path)
                                        .collect::<Vec<&String>>(),
                                ),
                            );
                        }

                        if files_that_exist.is_empty() {
                            errors.lock().expect("Should be able to unwrap lock").push(
                                anyhow::anyhow!(
                                    "No files exist for duplicate group:\n\
                                    {hash}",
                                ),
                            );
                            return;
                        }

                        let mut allowed_files = Vec::new();
                        let mut disallowed_found = false;

                        for entry in files_that_exist {
                            match ensure_path_within_roots(&entry.path, &args.allow_roots) {
                                Ok(_) => allowed_files.push(entry),
                                Err(e) => {
                                    disallowed_found = true;
                                    errors
                                        .lock()
                                        .expect("Should be able to unwrap lock")
                                        .push(e);
                                }
                            }
                        }

                        if disallowed_found {
                            return;
                        }

                        if allowed_files.len() < 2 {
                            return;
                        }

                        replace_files(args, &allowed_files, &errors);
                    });
            });
    });

    let collected_errors = {
        let mut guard = errors.lock().expect("Should be able to unwrap lock");
        guard.drain(..).collect::<Vec<Error>>()
    };

    if !collected_errors.is_empty() {
        eprintln!(
            "Encountered {} error(s) during processing:",
            collected_errors.len()
        );

        let formatted = collected_errors
            .iter()
            .map(|error| {
                let line = format!("  - {}", error);
                eprintln!("{}", line);
                line
            })
            .collect::<Vec<String>>()
            .join("\n");

        return Err(anyhow::anyhow!(
            "Encountered {} error(s):\n{}",
            collected_errors.len(),
            formatted
        ));
    }

    Ok(())
}

fn confirm_hashes_match(elms: &[CzkawkaDuplicateJsonFormatElement]) -> bool {
    elms.par_iter()
        .map(|e| &e.hash)
        .collect::<Vec<&String>>()
        .windows(2)
        .all(|w| w[0] == w[1])
}

fn replace_files(
    args: &args::Args,
    elms: &[CzkawkaDuplicateJsonFormatElement],
    errors: &std::sync::Mutex<Vec<Error>>,
) {
    let original_file = choose_original_file(args, elms);
    let original_path = std::path::Path::new(&original_file.path);

    for duplicate in elms {
        let duplicate_path = std::path::Path::new(&duplicate.path);

        // Skip the original file
        if duplicate.path == original_file.path {
            continue;
        }

        if args.dry_run {
            println!(
                "[Dry Run] Would replace '{}' with symlink to '{}'",
                duplicate_path.display(),
                original_path.display()
            );
            continue;
        }

        let backup_path = match move_to_backup(duplicate_path) {
            Ok(path) => path,
            Err(e) => {
                eprintln!(
                    "Failed to stage duplicate file '{}' for replacement: {}",
                    duplicate_path.display(),
                    e
                );
                errors
                    .lock()
                    .expect("Should be able to unwrap lock")
                    .push(anyhow::anyhow!(
                        "Failed to stage duplicate file '{}' for replacement: {}",
                        duplicate_path.display(),
                        e
                    ));
                continue;
            }
        };

        let symlink_result = create_symlink(original_path, duplicate_path);

        match symlink_result {
            Ok(_) => {
                if let Err(e) = std::fs::remove_file(&backup_path) {
                    eprintln!(
                        "Symlinked '{}' but failed to delete backup '{}': {}",
                        duplicate_path.display(),
                        backup_path.display(),
                        e
                    );
                    errors
                        .lock()
                        .expect("Should be able to unwrap lock")
                        .push(anyhow::anyhow!(
                            "Symlinked '{}' but failed to delete backup '{}': {}",
                            duplicate_path.display(),
                            backup_path.display(),
                            e
                        ));
                }

                println!(
                    "Replaced '{}' with symlink to '{}'",
                    duplicate_path.display(),
                    original_path.display()
                );
            }
            Err(e) => {
                eprintln!(
                    "Failed to create symlink from '{}' to '{}': {}",
                    duplicate_path.display(),
                    original_path.display(),
                    e
                );
                errors
                    .lock()
                    .expect("Should be able to unwrap lock")
                    .push(anyhow::anyhow!(
                        "Failed to create symlink from '{}' to '{}': {}",
                        duplicate_path.display(),
                        original_path.display(),
                        e
                    ));

                if let Err(restore_err) = std::fs::rename(&backup_path, duplicate_path) {
                    eprintln!(
                        "Also failed to restore original file from backup '{}': {}",
                        backup_path.display(),
                        restore_err
                    );
                    errors
                        .lock()
                        .expect("Should be able to unwrap lock")
                        .push(anyhow::anyhow!(
                            "Also failed to restore original file from backup '{}': {}",
                            backup_path.display(),
                            restore_err
                        ));
                }
            }
        }
    }
}

fn create_symlink(
    original_path: &std::path::Path,
    duplicate_path: &std::path::Path,
) -> Result<(), std::io::Error> {
    #[cfg(target_family = "windows")]
    let original_path = original_path.canonicalize()?;

    #[cfg(target_family = "unix")]
    {
        std::os::unix::fs::symlink(original_path, duplicate_path)
    }

    #[cfg(target_family = "windows")]
    {
        std::os::windows::fs::symlink_file(&original_path, duplicate_path)
    }
}

fn move_to_backup(path: &std::path::Path) -> Result<std::path::PathBuf, std::io::Error> {
    let mut counter = 0u32;
    loop {
        let suffix = if counter == 0 {
            "czkawka-bak".to_string()
        } else {
            format!("czkawka-bak-{}", counter)
        };

        let candidate = path.with_extension(suffix);

        if !candidate.exists() {
            std::fs::rename(path, &candidate)?;
            return Ok(candidate);
        }

        counter += 1;
    }
}

fn ensure_path_within_roots(path: &str, allowed_roots: &[std::path::PathBuf]) -> Result<(), Error> {
    if allowed_roots.is_empty() {
        anyhow::bail!("No allow-root paths configured.");
    }

    let canonical_path = std::fs::canonicalize(path)
        .with_context(|| format!("Failed to canonicalize path '{}'.", path))?;

    let is_allowed = allowed_roots
        .iter()
        .any(|root| canonical_path.starts_with(root));

    if is_allowed {
        Ok(())
    } else {
        let roots = allowed_roots
            .iter()
            .map(|root| root.display().to_string())
            .collect::<Vec<String>>()
            .join(", ");
        anyhow::bail!(
            "Path '{}' is outside the configured allow-root directories: {}",
            path,
            roots
        );
    }
}

fn choose_original_file<'a>(
    args: &args::Args,
    elms: &'a [CzkawkaDuplicateJsonFormatElement],
) -> &'a CzkawkaDuplicateJsonFormatElement {
    match args.original_to_keep {
        args::OriginalToKeep::First => &elms[0],
        args::OriginalToKeep::Last => &elms[elms.len() - 1],
        args::OriginalToKeep::Newest => select_by_mtime(elms, true),
        args::OriginalToKeep::Oldest => select_by_mtime(elms, false),
    }
}

fn select_by_mtime(
    elms: &[CzkawkaDuplicateJsonFormatElement],
    newest: bool,
) -> &CzkawkaDuplicateJsonFormatElement {
    use std::cmp::Ordering;

    let mut best = &elms[0];
    let mut best_time = file_timestamp(best);

    for entry in &elms[1..] {
        let candidate_time = file_timestamp(entry);

        let should_replace = match best_time.cmp(&candidate_time) {
            Ordering::Less => newest,
            Ordering::Greater => !newest,
            Ordering::Equal => false,
        };

        if should_replace {
            best = entry;
            best_time = candidate_time;
        }
    }

    best
}

fn file_timestamp(entry: &CzkawkaDuplicateJsonFormatElement) -> i128 {
    use std::time::UNIX_EPOCH;

    let path = std::path::Path::new(&entry.path);

    if let Ok(metadata) = std::fs::metadata(path)
        && let Ok(modified) = metadata.modified()
        && let Ok(duration) = modified.duration_since(UNIX_EPOCH)
    {
        let nanos =
            duration.as_secs() as i128 * 1_000_000_000i128 + duration.subsec_nanos() as i128;
        return nanos;
    }

    eprintln!(
        "Falling back to scan timestamp for '{}'; live metadata unavailable.",
        entry.path
    );

    // Fall back to the scan timestamp when live metadata is unavailable.
    entry.modified_date as i128 * 1_000_000_000i128
}

type FileSizeKey = u64;
type CzkawkaDuplicateJsonFormat = HashMap<FileSizeKey, Vec<Vec<CzkawkaDuplicateJsonFormatElement>>>;

#[derive(serde::Serialize, serde::Deserialize)]
struct CzkawkaDuplicateJsonFormatElement {
    path: String,
    modified_date: i64,
    size: i64,
    hash: String,
}
