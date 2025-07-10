# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`rv` is a Rust-based R package manager that provides a fast, reproducible, and declarative way to manage R packages. It serves as an alternative to renv, allowing users to define project dependencies in a `rproject.toml` configuration file and synchronize their R library with lockfiles.

## Development Commands

### Building and Running
```bash
# Build and run with arguments
just run <args>
# or alternatively
cargo run --features=cli --release -- <args>

# Examples
just run sync
just run add --dry-run
just run plan
```

### Testing
```bash
# Run all tests
just test
# or alternatively
cargo test --features=cli

# Note: Snapshot tests require R version 4.4.x
```

### Installation
```bash
# Install as binary
just install
# or alternatively
cargo install --path . --features cli
```

## Architecture Overview

### Core Modules Structure

**CLI Layer** (`src/cli/`):
- `commands/` - Command implementations (init, migrate, tree)
- `context.rs` - CLI context and R command lookup
- Main CLI parsing in `src/main.rs`

**Core Library** (`src/lib.rs`):
- **Resolver** (`src/resolver/`) - Dependency resolution engine with SAT solver
- **Sync** (`src/sync/`) - Package installation and synchronization
- **Cache** (`src/cache/`) - Disk-based caching system
- **Config** (`src/config.rs`) - Configuration file parsing
- **Package** (`src/package/`) - R package metadata handling
- **Repository** (`src/repository.rs`) - Repository database management
- **Git** (`src/git/`) - Git dependency handling
- **Lockfile** (`src/lockfile.rs`) - Lock file management

### Key Components

**Dependency Resolution**:
- Multi-source resolution (repositories, git, local, URL)
- SAT-based conflict resolution
- Version requirement satisfaction
- Lockfile-based caching

**Package Sources**:
- Repository packages (CRAN, R-Universe, private repos)
- Git dependencies (branch/tag/commit)
- Local packages (directory or tarball)
- URL dependencies
- Built-in R packages

**Configuration**:
- TOML-based project configuration (`rproject.toml`)
- Repository definitions with priority ordering
- Package-specific options (force_source, install_suggestions, dependencies_only)
- Environment variable support

## Key Files and Their Purposes

- `src/main.rs` - CLI entry point with command parsing
- `src/resolver/mod.rs` - Core dependency resolution logic
- `src/sync/mod.rs` - Package installation orchestration
- `src/config.rs` - Configuration file structure and parsing
- `src/cli/context.rs` - CLI context setup and R version detection
- `example_projects/` - Example configurations for different use cases

## Configuration File Format

Projects use `rproject.toml` files to define:
- R version requirements
- Repository URLs and priorities
- Dependencies with various source types
- Package-specific installation options

## Testing Strategy

- Unit tests throughout modules
- Snapshot testing for resolver outputs
- Integration tests in `src/tests/`
- Test data in `src/tests/` subdirectories (descriptions, package files, resolution scenarios)

## Common Development Workflows

When adding new dependency sources, implement in the resolver's lookup methods. When modifying package installation, focus on the sync module. Configuration changes require updates to the config parser and potentially migration logic.

The codebase follows Rust conventions with comprehensive error handling using `thiserror` and structured logging. The CLI provides both human-readable and JSON output formats for automation.