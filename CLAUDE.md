# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

### Build and Run
- `just run <args>` - Build and run rv with arguments (e.g., `just run sync`)  
- `cargo run --features=cli -- <args>` - Alternative build/run command
- `just install` - Install rv binary locally
- `cargo install --path . --features=cli` - Alternative install command

### Testing
- `just test` - Run all unit tests
- `cargo test --features=cli` - Alternative test command
- Snapshot tests require R version 4.4.x to be installed

### Code Quality
- The codebase uses standard Rust formatting and linting
- No specific lint/clippy commands configured in justfile or Cargo.toml
- Use `cargo fmt` and `cargo clippy` for standard Rust code quality checks

## Project Architecture

### Core Purpose
**rv** is a fast, reproducible R package manager written in Rust that manages R dependencies through configuration files (`rproject.toml`), lock files (`rv.lock`), and project-specific package libraries.

### Key Components

**Configuration System** (`src/config.rs`)
- `rproject.toml` files define project dependencies and repositories
- Supports multiple dependency types: simple strings, git repos, local paths, URLs
- Repository aliases allow specific package sourcing (CRAN, R-Universe, etc.)
- Environment variables for package compilation

**Dependency Resolution** (`src/resolver/`)
- Multi-source resolution: local → builtin → lockfile → repositories → git/URL
- Queue-based breadth-first dependency resolution
- Version requirement satisfaction and conflict detection
- Handles R's dependency types: depends, imports, suggests, enhances, linking_to

**Synchronization** (`src/sync/`)
- `SyncHandler` orchestrates package installation/removal
- Parallel compilation and installation with staging directories
- Safety checks (prevents removing packages in use via lsof)
- System dependency tracking for Linux package requirements

**Caching** (`src/cache/`)
- `DiskCache` manages package databases, downloads, and git repos
- Organized by R version and system architecture  
- Tracks installation status and system requirements

**CLI Interface** (`src/main.rs`, `src/cli/`)
- Primary commands: `init`, `sync`, `plan`, `add`, `upgrade`, `tree`
- `CliContext` manages project state across commands
- JSON output support for programmatic usage

### Data Flow
```
rproject.toml → Config → Resolver → Resolution → SyncHandler → Library
Dependencies → Repositories → Cache → Lockfile → Staging → Installed Packages
```

### Key File Relationships
- **`rproject.toml`**: Project configuration with dependencies
- **`rv.lock`**: Exact resolved dependency tree with versions/SHAs for reproducibility
- **`rv/library/`**: Project-specific package installation directory
- **Cache directories**: Persistent storage for downloads and metadata

### R Version Handling
- Supports R version detection via `RCommandLine` 
- Library paths are namespaced by R version and architecture
- Builtin package detection through R's installed.packages()
- Uses R CMD INSTALL for package installation

### Testing Structure
- Unit tests with `cargo test`
- Snapshot testing with `insta` crate for resolver behavior
- Test fixtures in `src/tests/` including sample DESCRIPTION files, configs, and resolution scenarios
- Example projects in `example_projects/` directory

### Special Considerations
- Requires R to be installed and accessible via PATH
- Git CLI required for git-based dependencies
- System dependency detection currently Ubuntu/Debian only
- Windows support with R.bat fallback detection