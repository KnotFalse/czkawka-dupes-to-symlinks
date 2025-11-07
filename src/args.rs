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

pub fn validate_files(input_file_path: &str) -> Result<Vec<PathBuf>, Error> {
    let all_files = get_all_files(input_file_path)?;

    let json_mimes = [
        mimetype_detector::APPLICATION_JSON,
        mimetype_detector::APPLICATION_JSON_BASE,
        mimetype_detector::APPLICATION_JSON_HAR,
        mimetype_detector::APPLICATION_JSON_UTF16,
    ];

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

            if !json_mimes.contains(&mime.to_string().as_str()) {
                anyhow::bail!(
                    "Input file must be JSON; type is: {}",
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
pub struct Args {
    /// Input file path. Can be relative or absolute. Can be a file or directory. A directory will use all files in the directory, recursively.
    #[arg(short, long)]
    pub input_file_path: String,

    /// Determines if the duplicate files should actually be replaced with symlinks.
    #[arg(short, long, default_value_t = false)]
    pub dry_run: bool,

    /// Sets the method to use for determining which duplicate file to keep (aka: the original).
    #[arg(short, long, value_enum, default_value_t = OriginalToKeep::Newest)]
    pub original_to_keep: OriginalToKeep,

    /// Restricts file modifications to the provided root directories.
    #[arg(long = "allow-root", value_name = "PATH", num_args = 1.., value_parser = clap::value_parser!(PathBuf))]
    pub allow_roots: Vec<PathBuf>,
}

#[derive(ValueEnum, Clone)]
pub enum OriginalToKeep {
    First,
    Last,
    Oldest,
    Newest,
}
