use crate::args;
use anyhow::{Context, Error, Result};
use rayon::prelude::*;
use std::collections::HashMap;

pub fn replace_duplicates_with_symlinks(args: &args::Args) {
    let all_files = args::get_all_files(&args.input_file_path)
        .expect("File(s) should have been validated by args parsing.");

    let json_by_file = all_files
        .into_par_iter()
        .map(|file_path| -> Result<CzkawkaDuplicateJsonFormat> {
            let file_contents = std::fs::read_to_string(&file_path).with_context(|| {
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

    if json_by_file.is_err() {
        eprintln!("{}", json_by_file.err().unwrap());
        return;
    }

    let json_by_file = json_by_file
        .expect("Should have been able to parse all files as JSON; already checked for error.");

    // gx todo - we should consider there may be some performance implications based on file size
    //           our structures currently index by file save *as a string* because of how czkawka json provides the data
    //           problem: `"8"` > `"22"`; string sorting doesn't work for numbers

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

                        replace_files(args, &files_that_exist);
                    });
            });
    });
}

fn confirm_hashes_match(elms: &[CzkawkaDuplicateJsonFormatElement]) -> bool {
    elms.par_iter()
        .map(|e| &e.hash)
        .collect::<Vec<&String>>()
        .windows(2)
        .all(|w| w[0] == w[1])
}

fn replace_files(args: &args::Args, elms: &[CzkawkaDuplicateJsonFormatElement]) {
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

        // Remove the duplicate file
        if let Err(e) = std::fs::remove_file(duplicate_path) {
            eprintln!(
                "Failed to remove duplicate file '{}': {}",
                duplicate_path.display(),
                e
            );
            continue;
        }

        // Create a symlink pointing to the original file
        #[cfg(target_family = "unix")]
        let symlink_result = std::os::unix::fs::symlink(&original_path, &duplicate_path);

        #[cfg(target_family = "windows")]
        let symlink_result = std::os::windows::fs::symlink_file(original_path, duplicate_path);

        if let Err(e) = symlink_result {
            eprintln!(
                "Failed to create symlink from '{}' to '{}': {}",
                duplicate_path.display(),
                original_path.display(),
                e
            );
        } else {
            println!(
                "Replaced '{}' with symlink to '{}'",
                duplicate_path.display(),
                original_path.display()
            );
        }
    }
}

fn choose_original_file<'a>(
    args: &args::Args,
    elms: &'a [CzkawkaDuplicateJsonFormatElement],
) -> &'a CzkawkaDuplicateJsonFormatElement {
    match args.original_to_keep {
        args::OriginalToKeep::First => &elms[0],
        args::OriginalToKeep::Last => &elms[elms.len() - 1],
        args::OriginalToKeep::Newest => elms
            .par_iter()
            .max_by_key(|e| e.modified_date)
            .expect("There should be at least one element; checked before calling this function."),
        args::OriginalToKeep::Oldest => elms
            .par_iter()
            .min_by_key(|e| e.modified_date)
            .expect("There should be at least one element; checked before calling this function."),
    }
}

type FileSizeKey = String;
type CzkawkaDuplicateJsonFormat = HashMap<FileSizeKey, Vec<Vec<CzkawkaDuplicateJsonFormatElement>>>;

#[derive(serde::Serialize, serde::Deserialize)]
struct CzkawkaDuplicateJsonFormatElement {
    path: String,
    modified_date: i64,
    size: i64,
    hash: String,
}
