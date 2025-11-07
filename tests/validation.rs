use std::fs;

use czkawka_dupe_to_symlinks::validate_files;
use serde_json::json;
use tempfile::TempDir;

fn padded_report_content(entry_count_padding: usize) -> String {
    let temp_entries = json!({
        "3": [[
            {
                "path": "/tmp/original",
                "modified_date": 0,
                "size": 3,
                "hash": "hash123"
            },
            {
                "path": "/tmp/duplicate",
                "modified_date": 0,
                "size": 3,
                "hash": "hash123"
            }
        ]]
    });

    let json_string = temp_entries.to_string();
    assert!(json_string.starts_with('{'));
    format!(
        "{{\n{}{}",
        " ".repeat(entry_count_padding),
        &json_string[1..]
    )
}

#[test]
fn validate_files_accepts_large_json_classified_as_text() {
    let temp_dir = TempDir::new().expect("temp dir");
    let json_path = temp_dir.path().join("input.json");
    let content = padded_report_content(1024);
    fs::write(&json_path, content).expect("write json");

    let result = validate_files(json_path.to_str().expect("utf8 path"));
    assert!(
        result.is_ok(),
        "Expected large JSON to validate: {:?}",
        result
    );
    let files = result.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0], json_path);
}

#[test]
fn validate_files_rejects_non_json_text() {
    let temp_dir = TempDir::new().expect("temp dir");
    let text_path = temp_dir.path().join("input.txt");
    fs::write(&text_path, "just some text without json structure").expect("write text");

    let err = validate_files(text_path.to_str().expect("utf8 path"))
        .expect_err("Expected invalid JSON to fail validation");
    let msg = format!("{}", err);
    assert!(
        msg.contains("Failed to parse input file as JSON") || msg.contains("JSON validation error"),
        "Unexpected error: {}",
        msg
    );
}
