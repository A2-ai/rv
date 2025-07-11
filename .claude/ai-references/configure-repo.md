# AI Assistant Reference - rv configure repository

## Quick Implementation Guide

### Purpose
The `rv configure repository` command manages R package repositories in `rproject.toml` files. This is the canonical reference for AI assistants working on this feature.

### Key Files to Check First
- `src/configure.rs` - Main implementation (authoritative source)
- `src/main.rs` - CLI integration (ConfigureSubcommand enum)
- `src/lib.rs` - Module exports

### Command Pattern
```bash
rv configure repository --alias <name> --url <url> [positioning/operation flags]
```

### Critical Implementation Details

**Current API**: `execute_repository_action()` + `parse_repository_action()` in `src/configure.rs`
- Clean separation: CLI parsing → Action enum → execution  
- Type-safe operations with `RepositoryAction` enum
- Main.rs uses this improved API
- Returns `Result<(), ConfigureError>`

**Type-safe Architecture**:
- `RepositoryAction` enum with operation-specific variants
- `RepositoryPositioning` enum for precise placement control  
- `RepositoryOperation` enum for type-safe responses
- `CliArgs` struct for organized parameter passing

**TOML Handling**: Uses `toml_edit::DocumentMut` 
- Preserves formatting and comments
- Function: `get_mut_repositories_array()` gets mutable access

**Repository Format**: Inline tables in TOML
```toml
repositories = [
    { alias = "cran", url = "https://cran.r-project.org" },
]
```

### Positioning Logic (mutually exclusive)
- `--first` → index 0
- `--last` → array.len() (default)
- `--before <alias>` → find_index(alias)
- `--after <alias>` → find_index(alias) + 1

### Operations (mutually exclusive with positioning)
- `--replace <alias>` → replace existing
- `--remove <alias>` → remove by alias
- `--clear` → empty array

### Validation Rules
1. **Duplicate aliases**: Check before adding (not for replace with same alias)
2. **URL validation**: Use `url::Url::parse()`
3. **Reference validation**: Ensure before/after targets exist

### Output Formats
- **Text**: Detailed success messages
- **JSON**: `ConfigureRepositoryResponse` struct with operation details

### Testing
- Location: `src/configure.rs` test module
- Uses `cargo insta` snapshots
- Tests operate on `DocumentMut`, not CLI

### Common Gotchas for AI
1. **Don't assume URLs**: Always validate with `url` crate
2. **Preserve TOML formatting**: Use `toml_edit`, not `toml`
3. **Check duplicates correctly**: Skip check when replacing with same alias
4. **Repository structure**: Always inline tables, never full tables
5. **Array formatting**: Call `format_repositories_array()` after modifications
6. **Error handling**: Uses `thiserror` pattern, returns `Result<(), ConfigureError>`

### Error Types
Check `ConfigureErrorKind` enum in `src/configure.rs` for all error cases.

### Integration Points
- CLI: `ConfigureSubcommand::Repository` in `src/main.rs`
- Config loading: Uses existing `read_and_verify_config()`
- JSON flag: Handled at CLI level, passed to function

### When Making Changes
1. **Read current implementation first** - this doc may be outdated
2. **Check test snapshots** for expected behavior
3. **Run `cargo insta test`** to verify changes
4. **Update this doc** if core patterns change

---
*This document focuses on implementation patterns, not examples. Check `docs/` for usage examples.*