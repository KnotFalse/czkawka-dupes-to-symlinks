# Czkawka Dupe to Symlinks

[![Crates.io](https://img.shields.io/crates/v/czkawka-dupes-to-symlinks.svg)](https://crates.io/crates/czkawka-dupes-to-symlinks)
[![docs.rs](https://img.shields.io/docsrs/czkawka-dupes-to-symlinks)](https://docs.rs/czkawka-dupes-to-symlinks)
[![License](https://img.shields.io/crates/l/czkawka-dupes-to-symlinks?style=flat)](https://crates.io/crates/czkawka-dupes-to-symlinks)

_A careful CLI for reclaiming disk space by turning duplicate files into symlinks._

This project started as a companion for [Czkawka](https://github.com/qarmin/czkawka)'s `czkawka_cli` JSON duplicate reports, but it works with **any** JSON document that matches the schema enforced below. If you produce the same structure from another tool, this binary will happily deduplicate it.

## Why?

- **No more silent deletions** – every duplicate is staged, replaced with a symlink, and automatically rolled back if anything fails.
- **Secure by default** – you _must_ declare one or more `--allow-root` directories so the tool never touches unexpected paths.
- **Deterministic** – JSON is validated against a schema, hashes are rechecked, and newest/oldest selection re-stats files before deciding.

## Quick Start

```bash
# Install from crates.io
cargo install czkawka-dupes-to-symlinks

# (Optional) install from a local checkout
cargo install --path .

# Run against a Czkawka JSON export
czkawka-dupes-to-symlinks \
  --input-file-path ~/czkawka_duplicates.json \
  --allow-root /srv/media --allow-root /srv/backups
```

Add `--dry-run` to preview actions without touching the filesystem.

> _Planning ahead_: Once we publish prebuilt archives you will also be able to use `cargo binstall czkawka-dupes-to-symlinks` for instant installs.

## Installation Options

- `cargo install czkawka-dupes-to-symlinks` – current release from Crates.io
- `cargo binstall czkawka-dupes-to-symlinks` – fast installs from GitHub Release artifacts
- `cargo install --git https://github.com/KnotFalse/czkawka-dupes-to-symlinks` – grab the latest main branch without waiting for a release.
- Download the platform-specific archive from the GitHub Release page and place the binary on your `$PATH`.

### Exit semantics

| Exit Code | Meaning | Typical Cause |
|-----------|---------|---------------|
| `0` | Every duplicate was processed and/or skipped intentionally | Happy path or dry-run |
| `1` | One or more duplicates could not be processed | Missing files, symlink permission errors, outside allow-root, invalid JSON |

## CLI Reference

| Flag | Description |
|------|-------------|
| `-i, --input-file-path <PATH>` | Path to a JSON file _or directory_ of JSON files to process |
| `-a, --allow-root <PATH>` (repeatable, required) | Directories that the tool is allowed to modify. Paths are canonicalized and enforced for every duplicate |
| `-d, --dry-run` | Log replacements without touching the filesystem |
| `-o, --original-to-keep <first\|last\|oldest\|newest>` | Strategy for choosing the canonical copy (default `newest`) |

## JSON Schema

Any producer that emits the following structure can be consumed.

<details>
<summary>Click to expand schema</summary>

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Czkawka Duplicates Report",
  "description": "Schema for duplicate groups keyed by file size.",
  "type": "object",
  "propertyNames": {
    "description": "Decimal representation of the file size in bytes.",
    "pattern": "^[0-9]+$"
  },
  "additionalProperties": {
    "description": "A list of groups, each containing files with identical hashes.",
    "type": "array",
    "items": {
      "description": "Files that share the same hash.",
      "type": "array",
      "minItems": 2,
      "items": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "path": { "type": "string" },
          "modified_date": { "type": "integer", "minimum": 0 },
          "size": { "type": "integer", "minimum": 0 },
          "hash": { "type": "string" }
        },
        "required": ["path", "modified_date", "size", "hash"]
      }
    }
  }
}
```

</details>

_A minimal example:_

```json
{
  "8229": [[
    {
      "path": "/var/www/uploads/a.jpg",
      "modified_date": 1724175436,
      "size": 8229,
      "hash": "a5b042..."
    },
    {
      "path": "/var/www/uploads/b.jpg",
      "modified_date": 1724175383,
      "size": 8229,
      "hash": "a5b042..."
    }
  ]]
}
```

## How It Works

1. **Discover inputs** – The CLI accepts either a single JSON file or a directory tree of JSON files.
2. **Validate** – Each file must be readable text; after that we parse and validate the JSON against the schema above. MIME sniffing is only used to block obvious binary blobs—the JSON parser + schema are the final gatekeepers. Invalid files abort the run.
3. **Enforce sandbox** – Every path must live under one of the canonicalized `--allow-root` directories.
4. **Replace safely** – For each duplicate group:
   - ensure hashes still match
   - stage each duplicate by renaming it to `*.czkawka-bak[-N]`
   - create the symlink to the selected canonical file
   - remove the backup only after the symlink succeeds (or restore it otherwise)
5. **Report** – Any per-file failure is aggregated and returned; the process exits non-zero with a detailed summary.

## Development

```bash
# Format + lint
cargo fmt && cargo clippy --all-targets --all-features

# Run regression tests (includes integration tests in tests/safety.rs)
cargo test
```

The integration tests spin up temporary directories with real files, so they work on macOS, Linux, and Windows.

### Optional git hook

You can have `cargo fmt --all -- --check` run before every commit:

```bash
ln -s ../../scripts/git-hooks/pre-commit .git/hooks/pre-commit
```

Adjust the relative path if your Git tooling stores hooks elsewhere.

## Caveats

- Creating symlinks on Windows requires either Developer Mode or elevated privileges.
- `--allow-root` paths must already exist; canonicalization will fail otherwise.
- Czkawka outputs absolute paths by default. If you generate relative paths, they’re interpreted relative to the filesystem entry itself on Unix and relative to the process on Windows—consider canonicalizing upstream.

## Troubleshooting

- **"At least one --allow-root must be provided"** – double-check that you passed `--allow-root` at least once and that paths don’t expand to blank strings (quote shell globs).
- **"Path ... is outside the allow-root sandbox"** – canonicalization happens before processing; ensure the JSON file only references directories you explicitly added via `--allow-root`.
- **Windows symlink failures** – enable Developer Mode or run an elevated PowerShell terminal so `std::os::windows::fs::symlink_file` can create links.
- **"Failed to validate provided allow-root paths"** – each directory must exist before launching the tool; create empty placeholder directories if needed.

## License

Dual-licensed under [Apache 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT). Pick the one that works for you.
