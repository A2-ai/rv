# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

### Build and Run
- `just run <args>` - Build and run rv with arguments (e.g., `just run sync`)  
- `cargo run --features=cli -- <args>` - Alternative build/run command (requires `--` before args)
- `cargo run --features=cli --release -- <args>` - Release build with optimizations
- `just install` - Install rv binary locally
- `cargo install --path . --features=cli` - Alternative install command

### Testing
- `just test` - Run all unit tests
- `cargo test --features=cli` - Alternative test command
- `cargo test --all-features` - Run tests with all features enabled (used in CI)
- Snapshot tests with `insta` crate require R version 4.4.x to be installed
- Integration tests in `tests/` directory use `assert_cmd` for CLI testing
- Test fixtures in `src/tests/` with mock data for different scenarios
- CI runs `cargo fmt --check` and `cargo test --all-features`

### Code Quality
- `cargo fmt` - Format code using standard Rust formatter
- `cargo fmt --check` - Check formatting without applying changes (used in CI)
- `cargo clippy` - Run Rust linter for code quality checks
- `rv fmt` - Format rproject.toml configuration files using `taplo` formatter (requires rv to be installed)

## Project Architecture

### Core Purpose
**rv** is a fast, reproducible R package manager written in Rust that manages R dependencies through configuration files (`rproject.toml`), lock files (`rv.lock`), and project-specific package libraries.

### Key Components

**Configuration System** (`src/config.rs`)
- `rproject.toml` files define project dependencies and repositories
- Supports multiple dependency types: simple strings, git repos, local paths, URLs, detailed specs with version requirements
- Repository aliases allow specific package sourcing (CRAN, R-Universe, etc.)
- Environment variables for package compilation (`configure_args`, `env`)
- TOML formatting support via `taplo` crate

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
- Primary commands: `init`, `sync`, `plan`, `add`, `upgrade`, `tree`, `configure`, `fmt`
- Additional commands: `library`, `info`, `cache`, `summary`, `migrate`, `activate`, `deactivate`, `sysdeps`
- `CliContext` manages project state across commands
- JSON output support for programmatic usage with `--json` flag
- Repository configuration via `configure repository` subcommands:
  - Operations: `add`, `remove`, `update`, `replace`, `clear`
  - Position flags: `--first`, `--last`, `--before <alias>`, `--after <alias>`
  - Force source flag: `--force-source` to always build packages from source

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
- Unit tests with `cargo test --features=cli`
- Snapshot testing with `insta` crate for resolver behavior and CLI output
- Integration tests in `tests/` directory:
  - `cli_configure_repository.rs` - Tests for repository configuration commands
  - Uses `assert_cmd` and `tempfile` for CLI testing
  - Snapshot files in `tests/snapshots/` for CLI output validation
- Test fixtures in `src/tests/`:
  - `descriptions/` - Sample DESCRIPTION files for packages
  - `package_files/` - Mock repository PACKAGE files (CRAN, R-Universe formats)
  - `resolution/` - Resolution test cases covering edge cases and conflicts
  - `formatting/` - TOML formatting test files
  - `valid_config/` and `invalid_config/` - Configuration validation tests
  - `r_universe/` - R-Universe API response fixtures
  - `sys_reqs/ubuntu_20.04.json` - System requirements mock data
- Example projects in `example_projects/` demonstrating various use cases:
  - `simple/`, `multi-repo/`, `r-universe/`, `git-dep/`, `local-deps/`, etc.
  - Each with working `rproject.toml` configurations

### Special Considerations
- Requires R to be installed and accessible via PATH
- Git CLI required for git-based dependencies
- System dependency detection currently Ubuntu/Debian only
- Windows support with R.bat fallback detection
- Uses feature flag `cli` to separate library code from CLI binary
- Parallel processing with `rayon` and `crossbeam` for efficient package operations
- HTTP requests via `ureq` with platform certificate verification

## Key Dependencies and Technologies

### Core Rust Crates (always included)
- **serde** (v1): Configuration serialization/deserialization
- **serde_json** (v1): JSON parsing and serialization
- **toml** (v0.9) / **toml_edit** (v0.23): TOML parsing and manipulation
- **url** (v2): URL handling and validation with serde support
- **regex** (v1): Pattern matching for R version parsing
- **ureq** (v3): HTTP client with platform-verifier for repository access
- **crossbeam** (v0.8.4): Concurrent data structures
- **tempfile** (v3): Temporary file management
- **fs-err** (v3): Enhanced filesystem operations with better error messages
- **etcetera** (v0.10.0) / **cachedir** (v0.3): Cross-platform cache directory management
- **os_info** (v3.9.1): OS name and version detection
- **bincode** (v2): Binary serialization for package databases
- **thiserror** (v2): Error type derivation
- **walkdir** (v2): Recursive directory traversal
- **reflink-copy** (v0.1): Copy-on-write file operations
- **filetime** (v0.2.25): File timestamp manipulation
- **flate2** (v1) / **tar** (v0.4) / **zip** (v4): Archive handling
- **sha2** (v0.10): SHA256 hashing
- **num_cpus** (v1.16.0): CPU count detection
- **indicatif** (v0.18): Progress bars (also in library code)
- **log** (v0.4): Logging facade
- **which** (v8): Executable path resolution
- **libc** (v0.2.172): System calls
- **taplo** (v0.14.0): TOML formatting

### CLI-Specific Dependencies (feature = "cli")
- **clap** (v4): Command-line argument parsing with derive
- **clap-verbosity-flag** (v3): Verbosity flag handling
- **rayon** (v1): Parallel processing
- **anyhow** (v1): Error handling for CLI
- **env_logger** (v0.11): Logging implementation
- **jiff** (v0.2): Date/time operations
- **ctrlc** (v3): Signal handling with termination support

### Development and Testing
- **insta** (v1): Snapshot testing framework
- **mockito** (v1): HTTP mocking for tests
- **assert_cmd** (v2): CLI testing utilities
- **predicates** (v3): Test assertions

## Important Code Locations

### Core Functionality
- `src/main.rs` - CLI entry point and command dispatch
- `src/config.rs` - Configuration parsing and validation
- `src/resolver/mod.rs` - Main dependency resolution logic
- `src/sync/handler.rs` - Package installation orchestration
- `src/cache/disk.rs` - Disk cache management

### CLI Commands
- `src/cli/context.rs` - CLI context and state management
- `src/cli/commands/` - Command implementations (`init.rs`, `migrate.rs`, `tree.rs`)
- `src/add.rs` - Package addition logic
- `src/configure.rs` - Repository configuration management
- `src/format.rs` - TOML formatting functionality with taplo
- `src/activate.rs` / `src/deactivate.rs` - renv activation/deactivation
- `src/library.rs` - Library path management
- `src/project_summary.rs` - Project summary generation

### Package Management
- `src/package/` - Package-related functionality
  - `description.rs` - DESCRIPTION file parsing
  - `version.rs` - Version handling
  - `remotes.rs` - Remote package specifications
  - `builtin.rs` - Built-in package detection
- `src/repository.rs` - Repository handling
- `src/lockfile.rs` - Lock file management

### Sync and Sources
- `src/sync/` - Synchronization logic
  - `build_plan.rs` - Build plan generation
  - `changes.rs` - Change detection
  - `sources/` - Package source handlers (git, local, repositories, url)

### Testing
- `tests/cli_configure_repository.rs` - Integration tests for repository configuration
- `src/tests/` - Unit test fixtures and data
- `src/snapshots/` and `tests/snapshots/` - Snapshot test baselines
- `src/*/snapshots/` - Module-specific snapshot tests

### Other Important Files
- `src/r_cmd.rs` - R command execution wrapper
- `src/renv.rs` - renv integration support
- `src/git/` - Git repository handling
- `src/http.rs` - HTTP client configuration
- `src/consts.rs` - Global constants
- `src/cancellation.rs` - Ctrl-C handling
- `src/system_info.rs` / `src/system_req.rs` - System requirements detection

## Important Reminders

### General Development Guidelines
- Do what has been asked; nothing more, nothing less
- NEVER create files unless they're absolutely necessary for achieving your goal
- ALWAYS prefer editing an existing file to creating a new one
- NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User

### rv-Specific Guidelines
- The project is dual-purpose: library crate and CLI binary (controlled by `cli` feature flag)
- Always use `--features=cli` when building/testing the CLI
- R version 4.4.x is required for snapshot tests
- Use existing example projects in `example_projects/` as references for configurations
- Repository order matters in `rproject.toml` (first repository has highest priority)
- The `rv fmt` command requires rv to be installed (it formats rproject.toml files)
- Project version is currently 0.13.0 (as defined in Cargo.toml)