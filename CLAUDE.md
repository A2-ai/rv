# CLAUDE.md

Guidance for Claude Code when working with this repository.

## Quick Reference

**rv** is a fast, reproducible R package manager written in Rust. It manages R dependencies through `rproject.toml` config files, `rv.lock` lock files, and project-specific package libraries.

## Development Commands

```bash
# Build and run
just run <args>                        # e.g., just run sync
cargo run --features=cli -- <args>     # alternative

# Testing
just test                              # unit tests
cargo test --all-features              # CI command

# Code quality
cargo fmt                              # format
cargo clippy                           # lint
```

**Key details:**
- The `cli` feature flag separates library code from the CLI binary
- CI runs on R version 4.5 (check `.github/workflows/ci.yaml` for current version)
- Snapshot tests use the `insta` crate

## Project Structure

```
rproject.toml → Config → Resolver → Resolution → SyncHandler → Library
```

**Core modules:**
- `src/config.rs` - Configuration parsing (`rproject.toml`)
- `src/resolver/` - Dependency resolution
- `src/sync/` - Package installation/removal
- `src/cache/` - Package database and download caching
- `src/context.rs` - Project context (`Context` struct)
- `src/main.rs` - CLI entry point and commands

**Project files:**
- `rproject.toml` - Project configuration
- `rv.lock` - Resolved dependency tree
- `rv/library/` - Project package library

## Environment Variables

All environment variable names are defined in `src/consts.rs`.

### Cache Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `RV_CACHE_DIR` | OS cache dir | Override user cache directory location |
| `RV_GLOBAL_CACHE_DIR` | unset | Path to shared cache for multi-user systems. Directory must exist |
| `PKGCACHE_TIMEOUT` | 3600 (1 hour) | Package database cache TTL in seconds. Compatible with R's pkgcache |

### Performance Tuning

| Variable | Default | Description |
|----------|---------|-------------|
| `RV_NUM_CPUS` | auto-detected | Max worker threads for parallel sync operations |
| `RV_COPY_THREADS` | 4-16 (by file count) | Thread count for parallel file copying on NFS |
| `RV_LINK_MODE` | `clone` (macOS), `hardlink` (Linux) | How packages are linked from cache to library (see below) |

**RV_LINK_MODE values** (case-insensitive, see `src/sync/link.rs`):
- `copy` - Full file copy. Slowest but works on any filesystem
- `clone` - Copy-on-Write via reflink. Requires filesystem support: APFS (macOS), Btrfs/XFS (Linux), ReFS (Windows). Default on macOS where APFS is standard
- `hardlink` - Hard links for files. Cache and library must be on same filesystem. Default on Linux (ext4 doesn't support reflinks)
- `symlink` - Symlink to package directory. Auto-selected on network filesystems (NFS/Lustre on Linux)

Priority: env var → network FS detection → OS default. Falls back to `copy` if selected mode fails.

### Git Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `RV_SUBMODULE_UPDATE_DISABLE` | false | Set to `true` or `1` to skip git submodule updates |

### System Dependencies (Linux)

| Variable | Default | Description |
|----------|---------|-------------|
| `RV_SYS_REQ_URL` | Posit API | Override the system requirements API URL |
| `RV_SYS_DEPS_CHECK_IN_PATH` | unset | Additional tools to check via PATH for sysdeps |

### Safety

| Variable | Default | Description |
|----------|---------|-------------|
| `RV_NO_CHECK_OPEN_FILE` | false | Set to `true` or `1` to skip lsof check before removing packages. Use when lsof is slow or unavailable |

## CLI Commands

Run `rv --help` for full documentation. Primary commands:

- `init`, `sync`, `plan`, `add`, `upgrade` - Core workflow
- `tree`, `info`, `summary` - Information
- `configure` - Repository configuration
- `migrate`, `activate`, `deactivate` - Project setup

## Testing

- Unit tests: `src/` modules with `#[cfg(test)]`
- Integration tests: `tests/` directory
- Snapshot tests: `src/*/snapshots/` and `tests/snapshots/`
- Test fixtures: `src/tests/` (DESCRIPTION files, PACKAGES files, etc.)
- Example projects: `example_projects/` (working configurations)

## Code Style Guidelines

- Follow existing patterns in the codebase
- Use `thiserror` for library errors, `anyhow` for CLI errors
- Use `fs_err` instead of `std::fs` for better error messages
- Prefer explicit error handling over `.unwrap()`

## Important Notes

- R must be installed and accessible via PATH
- Git CLI required for git-based dependencies
- System dependency detection supports multiple Linux distros (see `src/system_req.rs`)
- Package databases use MessagePack serialization (`.mp` files)

## Documentation

- User documentation: https://a2-ai.github.io/rv-docs/
- Example configurations: `example_projects/` directory
