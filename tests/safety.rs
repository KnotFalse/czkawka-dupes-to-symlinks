use std::fs::{self, File};
use std::io::Write;

use czkawka_dupe_to_symlinks::{
    Args, OriginalToKeep, replace_duplicates_with_symlinks, validate_files,
};
use serde_json::json;
use tempfile::TempDir;

fn write_json(path: &std::path::Path, entries: serde_json::Value) {
    let mut file = File::create(path).expect("Failed to create json file");
    file.write_all(entries.to_string().as_bytes())
        .expect("Failed to write json");
}

fn canonicalize(path: &std::path::Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).expect("Failed to canonicalize path")
}

#[test]
fn fails_when_duplicate_folder_cannot_be_staged() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path();
    let data_dir = root.join("data");
    fs::create_dir(&data_dir).expect("create data dir");

    let original = data_dir.join("original.bin");
    fs::write(&original, b"orig").expect("write original");

    let duplicate = data_dir.join("dup.bin");
    fs::write(&duplicate, b"dup").expect("write dup");

    let json_path = root.join("input.json");
    write_json(
        &json_path,
        json!({
            "3": [[
                {
                    "path": original.to_string_lossy(),
                    "modified_date": 0,
                    "size": 3,
                    "hash": "hash123"
                },
                {
                    "path": duplicate.to_string_lossy(),
                    "modified_date": 0,
                    "size": 3,
                    "hash": "hash123"
                }
            ]]
        }),
    );

    let args = Args {
        input_file_path: json_path.to_string_lossy().into_owned(),
        dry_run: false,
        original_to_keep: OriginalToKeep::First,
        allow_roots: vec![canonicalize(root)],
    };

    let files = validate_files(&args.input_file_path).expect("validate files");

    // Drop write permissions to force move_to_backup to fail.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&data_dir).expect("metadata").permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&data_dir, perms).expect("set perms");
    }
    #[cfg(windows)]
    {
        let mut perms = fs::metadata(&data_dir).expect("metadata").permissions();
        perms.set_readonly(true);
        fs::set_permissions(&data_dir, perms).expect("set perms");
    }

    let result = replace_duplicates_with_symlinks(&args, &files);
    assert!(result.is_err(), "Expected symlink run to fail");

    // Restore permissions so TempDir cleanup succeeds.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&data_dir).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&data_dir, perms).expect("restore perms");
    }
    #[cfg(windows)]
    {
        let mut perms = fs::metadata(&data_dir).expect("metadata").permissions();
        perms.set_readonly(false);
        fs::set_permissions(&data_dir, perms).expect("restore perms");
    }
}

#[test]
fn errors_when_path_outside_allow_root() {
    let allowed = TempDir::new().expect("allowed");
    let outside = TempDir::new().expect("outside");

    let original = allowed.path().join("original.bin");
    fs::write(&original, b"orig").expect("write original");

    let duplicate_outside = outside.path().join("dup.bin");
    fs::write(&duplicate_outside, b"dup").expect("write dup");

    let json_path = allowed.path().join("input.json");
    write_json(
        &json_path,
        json!({
            "3": [[
                {
                    "path": original.to_string_lossy(),
                    "modified_date": 0,
                    "size": 3,
                    "hash": "hash123"
                },
                {
                    "path": duplicate_outside.to_string_lossy(),
                    "modified_date": 0,
                    "size": 3,
                    "hash": "hash123"
                }
            ]]
        }),
    );

    let args = Args {
        input_file_path: json_path.to_string_lossy().into_owned(),
        dry_run: false,
        original_to_keep: OriginalToKeep::First,
        allow_roots: vec![canonicalize(allowed.path())],
    };

    let files = validate_files(&args.input_file_path).expect("validate");
    let result = replace_duplicates_with_symlinks(&args, &files);
    assert!(result.is_err(), "Expected allow-root violation");
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("outside the configured allow-root"),
        "Unexpected error message: {}",
        err
    );
}
