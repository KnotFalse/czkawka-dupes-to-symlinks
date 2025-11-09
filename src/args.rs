//! Argument parsing and validation utilities shared by the CLI and the library
//! helpers. Everything defined here funnels through the single exported surface
//! re-exported by `lib.rs`, which keeps the public API intentionally small.

use anyhow::{Context, Error, Result};
use clap::{CommandFactory, Parser, ValueEnum};
use rayon::prelude::*;
use serde_json::json;
use std::path::PathBuf;

pub fn validate_arguments(args: Vec<String>) -> Result<Args, Error> {
    Args::try_parse_from(args).context("Failed attempt at parsing args")
}

pub fn print_usage() {
    Args::command()
        .print_help()
        .context("Failed to print usage")
        .unwrap();
}

/// Expand and validate one or more JSON reports produced by Czkawka (or any
/// generator that emits the identical schema).
///
/// The input may be a single file or a directory; when a directory is supplied we
/// walk it recursively and validate **every** JSON file we find.
///
/// # Returns
/// A list of file paths that passed MIME sniffing, JSON parsing, and schema
/// validation. The caller can pass the returned slice directly to
/// [`crate::replace_duplicates_with_symlinks`].
///
/// # Errors
/// - the path does not exist or is not a file/directory
/// - MIME detection reports non-text content
/// - JSON parsing fails or the document violates the enforced schema
///
/// # Examples
/// ```no_run
/// use czkawka_dupe_to_symlinks::validate_files;
///
/// # fn main() -> anyhow::Result<()> {
/// let files = validate_files("/tmp/czkawka-reports")?;
/// # Ok(())
/// # }
/// ```
pub fn validate_files(input_file_path: &str) -> Result<Vec<PathBuf>, Error> {
    let all_files = get_all_files(input_file_path)?;

    let json_schema = jsonschema::draft202012::new(&czkawka_duplicate_file_json_schema())
        .context("Failed to create json schema validator")?;

    let (_, errs): (Vec<_>, Vec<_>) = all_files
        .par_iter()
        .map(|f| -> Result<(), Error> {
            let mime = mimetype_detector::detect_file(f)
                .with_context(|| format!("Failed to detect mimetype for file: {}", f.display()))?;

            if !mime.kind().is_text() {
                anyhow::bail!(
                    "Input file must be text; type is: {}",
                    mime.kind().to_string().to_lowercase().replace("_", "")
                );
            }

            let file_contents =
                std::fs::read_to_string(f).context("Failed to read input file as string")?;
            let parsed_json = serde_json::from_str(&file_contents)
                .context("Failed to parse input file as JSON")?;
            json_schema
                .validate(&parsed_json)
                .map_err(|e| anyhow::anyhow!("JSON validation error: {}", e))?;

            Ok(())
        })
        .partition(|result| result.is_ok());

    if !errs.is_empty() {
        let errors: Vec<Error> = errs.into_par_iter().map(|r| r.unwrap_err()).collect();

        return Err(anyhow::anyhow!(
            "Found {} validation error(s):\n{}",
            errors.len(),
            errors
                .iter()
                .map(|e| format!("  - {}", e))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    Ok(all_files)
}

pub fn canonicalize_roots(roots: &[PathBuf]) -> Result<Vec<PathBuf>, Error> {
    if roots.is_empty() {
        anyhow::bail!("At least one --allow-root path is required.");
    }

    roots
        .iter()
        .map(|root| {
            if !root.exists() {
                anyhow::bail!("Allow-root path does not exist: {}", root.display());
            }

            std::fs::canonicalize(root).with_context(|| {
                format!("Failed to canonicalize allow-root path: {}", root.display())
            })
        })
        .collect()
}

pub(crate) fn get_all_files(input_file_path: &str) -> Result<Vec<PathBuf>, Error> {
    let path = std::path::Path::new(input_file_path);

    if !path.exists() {
        return Err(anyhow::anyhow!("Input path does not exist"));
    }

    if !(path.is_file() || path.is_dir()) {
        return Err(anyhow::anyhow!("Input path must be a file or directory"));
    }

    let all_files = if path.is_file() {
        vec![path.to_path_buf()]
    } else {
        let mut files = Vec::new();
        for entry in walkdir::WalkDir::new(path) {
            let entry = entry.context("Failed to read directory entry")?;
            if entry.path().is_file() {
                files.push(entry.path().to_path_buf());
            }
        }
        files
    };

    Ok(all_files)
}

fn czkawka_duplicate_file_json_schema() -> serde_json::Value {
    json!({
      "$schema": "https://json-schema.org/draft/2020-12/schema",
      "title": "Czkawka Duplicates Report",
      "description": "Schema for the JSON output of Czkawka duplicate finder, where files are grouped by size, then by hash.",
      "type": "object",
      "propertyNames": {
        "description": "Each property name must be the decimal representation of the file size in bytes.",
        "pattern": "^[0-9]+$"
      },
      "additionalProperties": {
        "description": "An array of duplicate groups, keyed by file size. Each inner array represents a set of files with an identical hash.",
        "type": "array",
        "items": {
          "description": "A single group of duplicate files (which all have the same hash).",
          "type": "array",
          "items": {
            "$ref": "#/$defs/duplicateFileEntry"
          },
          "minItems": 2
        }
      },
      "$defs": {
        "duplicateFileEntry": {
          "title": "Duplicate File Entry",
          "description": "Details of a single file.",
          "type": "object",
          "properties": {
            "path": {
              "description": "The full path to the file.",
              "type": "string"
            },
            "modified_date": {
              "description": "The file's last modified timestamp (Unix epoch).",
              "type": "integer",
              "minimum": 0
            },
            "size": {
              "description": "The file size in bytes.",
              "type": "integer",
              "minimum": 0
            },
            "hash": {
              "description": "The hash of the file content.",
              "type": "string"
            }
          },
          "required": [
            "path",
            "modified_date",
            "size",
            "hash"
          ],
          "additionalProperties": false
        }
      }
    })
}
#[derive(clap::Parser)]
#[clap(author, version, about, long_about = None)]
/// Normalized CLI arguments that can also be constructed programmatically when
/// embedding the crate.
pub struct Args {
    /// Path to a JSON file **or** directory containing JSON reports.
    ///
    /// Directories are walked recursively so that large scans can be split across
    /// multiple documents.
    #[arg(short, long)]
    pub input_file_path: String,

    /// When enabled, logs every action but leaves the filesystem untouched.
    #[arg(short, long, default_value_t = false)]
    pub dry_run: bool,

    /// Strategy for picking the canonical file inside each duplicate group.
    #[arg(short, long, value_enum, default_value_t = OriginalToKeep::Newest)]
    pub original_to_keep: OriginalToKeep,

    /// Canonicalized directories that bound filesystem changes.
    ///
    /// Every duplicate must live under one of these roots or it will be skipped
    /// with an error.
    #[arg(long = "allow-root", value_name = "PATH", num_args = 1.., value_parser = clap::value_parser!(PathBuf))]
    pub allow_roots: Vec<PathBuf>,
}

#[derive(ValueEnum, Clone)]
/// How the canonical/original file is chosen inside a duplicate group.
pub enum OriginalToKeep {
    /// Select the first entry encountered in the JSON document (stable order).
    First,
    /// Select the last entry encountered in the JSON document (stable order).
    Last,
    /// Re-stat every path and keep the file with the oldest modification time.
    Oldest,
    /// Re-stat every path and keep the file with the newest modification time.
    Newest,
}
