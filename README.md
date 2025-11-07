# Czkawka Dupe to Symlinks

_A careful CLI for reclaiming disk space by turning duplicate files into symlinks._

This project started as a companion for [Czkawka](https://github.com/qarmin/czkawka)'s `czkawka_cli` JSON duplicate reports, but it works with **any** JSON document that matches the schema enforced below. If you produce the same structure from another tool, this binary will happily deduplicate it.

## Why?

- **No more silent deletions** – every duplicate is staged, replaced with a symlink, and automatically rolled back if anything fails.
- **Secure by default** – you _must_ declare one or more `--allow-root` directories so the tool never touches unexpected paths.
- **Deterministic** – JSON is validated against a schema, hashes are rechecked, and newest/oldest selection re-stats files before deciding.

## Quick Start

```bash
# Build from source
cargo install --path .

# Run against a Czkawka JSON export
czkawka_dupe_to_symlinks \
  --input-file-path ~/czkawka_duplicates.json \
  --allow-root /srv/media --allow-root /srv/backups
```

Add `--dry-run` to preview actions without touching the filesystem.

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
| `-o, --original-to-keep <first|last|oldest|newest>` | Strategy for choosing the canonical copy (default `newest`) |

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
2. **Validate** – Each file is checked for MIME type, parsed, and validated against the schema above. Invalid files abort the run.
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

## Caveats

- Creating symlinks on Windows requires either Developer Mode or elevated privileges.
- `--allow-root` paths must already exist; canonicalization will fail otherwise.
- Czkawka outputs absolute paths by default. If you generate relative paths, they’re interpreted relative to the filesystem entry itself on Unix and relative to the process on Windows—consider canonicalizing upstream.

## License

Dual-licensed under [Apache 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT). Pick the one that works for you.
