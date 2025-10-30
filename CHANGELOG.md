## v0.16.0 - November 30, 2025

This release significantly expands the `rv add` command with comprehensive configuration options for installing packages from multiple sources and with fine-grained control over installation behavior. Additionally, custom cache directory configuration is now supported through an rv-specific environment variable `RV_CACHE_DIR`.

### 🎉 New Features

- `rv add` has been enhanced with CLI flags for all configuration options:
  - **Git repositories**: `--git`, `--commit`, `--tag`, `--branch`, and `--directory` flags for specifying commits, tags, branches, or subdirectories within repositories
  - **Local filesystem paths**: `--path` for local development or custom package locations
  - **HTTP/HTTPS URLs**: `--url` for installing from package archives
  - **Repository pinning**: `--repository <alias>` to pin packages to specific repository aliases configured in your `rproject.toml`, allowing packages come from the non-first matching repository.
  - **Package installation behavior**:
    - `--force-source`: Build packages from source instead of using pre-built binaries
    - `--install-suggestions`: Automatically install suggested packages
    - `--dependencies-only`: Install only a package's dependencies without the package itself

- **Custom cache directory**: Set the `RV_CACHE_DIR` environment variable to override the default cache location, useful for custom storage configurations, limited disk space scenarios, or CI/CD environments.

## v0.15.0 - October 25, 2025

This release adds support for RPM-based Linux distributions (AlmaLinux, CentOS, Rocky Linux, RHEL) for system dependency detection and binary package installation.

### 🎉 New Features
- **AlmaLinux and RPM distribution support**: rv now fully supports AlmaLinux, CentOS, Rocky Linux, and RHEL for both system dependency detection and binary package installation. System dependencies are automatically detected using the `rpm` package manager, and binary packages are correctly resolved from Posit Package Manager for these distributions.

### ⚡ Improvements
- **Enhanced Linux distribution reporting**: The `rv summary` command now displays the Linux distribution name used for binary package resolution, making it easier to verify that rv is correctly detecting your system and using the appropriate package repositories.

## v0.14.0 - October 13, 2025

This release improves error messaging when rv detects packages that cannot be removed because they are currently in use by running processes.

### ⚡ Improvements
- **Enhanced package in-use detection**: When rv needs to remove packages that are currently loaded by running processes,
it now provides detailed information about which processes are using which packages, including process names and PIDs.
This makes it easier to identify and close the relevant applications before retrying the operation.


## v0.13.2 - September 6, 2025

This release includes minor improvements to command documentation and fixes issues with R version detection logging.

### ⚡ Improvements
- **Enhanced command documentation**: Added missing documentation for the `--check` flag in the `rv fmt` command, providing clearer guidance on formatting verification options.
- **No xz homebrew dep on mac** - rv now requires no external dependencies (fixes #338)

### 🐛 Bug Fixes
- **Fixed R version detection logging**: Resolved inconsistent logging messages when detecting R installations across different platforms (macOS rig, Windows R.bat, and Linux /opt/R), ensuring all detection paths provide clear information about which R version was found and where.
- **Fixed macOS rig R detection**: Corrected an issue where the wrong R command path was returned when detecting R versions installed via rig on macOS, ensuring the proper rig-formatted R binary path is used.

## v0.13.1 - August 19, 2025

This release improves the R activation script with better version compatibility handling and includes several minor enhancements to cache display and system support.

### ⚡ Improvements
- **Enhanced R activation script**: The activation script now includes better R version compatibility checking.
When the R version in your config doesn't match your current R session, rv enters a safe mode using a temporary
library instead of failing, providing clearer warning messages about the version mismatch.
It also sets R_LIBS_SITE and R_LIBS_USER to improve propogation of
library settings to separate processes to help with mirai compatibility https://github.com/r-lib/mirai/issues/390
- **Improved cache information display**: The `rv cache` command now shows more accurate source paths for repositories, providing better visibility into cached package locations.
- **Expanded Linux distribution support**: Added support for Gentoo Linux in system detection.

### 🐛 Bug Fixes
- **Fixed URL dependency configuration**: Removed the non-functional `force_source` option from URL-based dependencies in configuration files, as this setting had no effect for URL packages.

## v0.13.0 - August 15, 2025

This release introduces a new formatting command for configuration files and improves dependency resolution for suggested packages.

### 🎉 New Features
- **Configuration file formatting**: You can now format your `rproject.toml` files with `rv fmt` to ensure consistent styling. Use `rv fmt --check` to verify formatting without making changes.

### 🐛 Bug Fixes
- **Fixed suggested package version requirements**: Resolved an issue where version requirements from suggested packages weren't properly considered during dependency resolution, which could lead to incorrect package versions being installed.
- **Fixed git repository error messages**: Improved error reporting when DESCRIPTION files are missing in git repositories, providing clearer feedback about what went wrong.

## v0.12.1 - August 6, 2025

### ⚡ Improvements
- **Enhanced cache information display**: The `rv cache` command now shows both source and binary paths for repositories, providing better visibility into cached package locations.
- **More reliable package installation**: Package compilation now uses file copying instead of symlinking during the build process, improving compatibility across different filesystems and build tools.

## v0.12.0

This release introduces package-specific configure arguments and improved git repository handling, along with several infrastructure improvements and bug fixes.

### 🎉 New Features
- **Package-specific configure arguments**: You can now specify custom configure arguments for R packages on a per-OS and per-architecture basis in your `rproject.toml` file. This allows you to customize package compilation flags for different platforms.
- **Programmatic repository configuration**: Added ability to configure repositories programmatically via `rv configure repository` commands, allowing you to add, update, replace, remove, and clear repositories from the command line.

### ⚡ Improvements
- **Enhanced git repository handling**: rv now properly handles git submodules. This can be disabled with the `RV_SUBMODULE_UPDATE_DISABLE` environment variable.
- **Improved activation script**: The R activation script now includes better error handling and checks for rv installation before attempting to activate a project.

### 🐛 Bug Fixes
- **Fixed library staging issues**: Resolved problems where the staging directory could interfere with library operations, particularly for custom library configurations.
- **Fixed git checkout behavior**: Fixed issues with git branch checkout operations and reference updating after fetching.
- **Fixed git reference resolution**: Improved handling of unknown git references, with better fallback logic for branches and tags that aren't immediately recognized.
- **Fixed Cross-device library support**: Fixed issues where rv did not work properly with custom library paths on different filesystems (e.g., NFS mounts).

## Version 0.11.0 - July 14, 2025

This release introduces programmatic repository configuration commands, enhanced binary package detection, and improved installation logging capabilities.

### 🎉 New Features
- **Repository configuration CLI**: You can now configure repositories programmatically via `rv configure repository` commands. Add, update, replace, remove, and clear repositories directly from the command line with support for positioning (first, last, before, after) and JSON output.

### ⚡ Improvements
- **Enhanced binary package detection**: Improved detection of binary vs source packages by checking for compiled R files in specific subdirectories (R/ and data/) and validating DESCRIPTION files and Meta directories before installation attempts.
- **Better glibc compatibility**: Enhanced installation script with more robust glibc version detection across different Linux distributions, with fallback to musl for compatibility on older systems.
- **Improved installation logging**: Install logs are now saved during the build process rather than at the end, providing better debugging capabilities for failed installations.
- **Enhanced force_source handling**: When `force_source` is enabled, rv now properly ignores cached binary packages that weren't built from source by rv itself, ensuring packages are compiled locally as requested.
- **Better error handling**: Enhanced error messages for invalid packages with clearer path and error information.
- **Improved R command detection**: Better handling of R.bat detection on Windows systems and more reliable R version finding across platforms.

### 🐛 Bug Fixes
- **Fixed library comparison logic**: Resolved issues where builtin packages weren't properly recognized as installed in project summaries.
- **Fixed database loading**: Added fallback mechanism when loading cached package databases fails, ensuring rv can recover by re-fetching the data.

## v0.10.0

This release brings incremental installs for projects without lockfiles, enhanced build compatibility, and improved system dependency detection.

### 🎉 New Features
- **Incremental installs for projects without lockfiles**: Projects that don't use lockfiles now benefit from the same incremental installation behavior introduced in v0.9. When `use_lockfile` is not specified in `rproject.toml`, rv trusts the existing library contents and only installs packages that have changed, rather than reinstalling everything through the staging directory.
- **Enhanced build compatibility**: Added musl libc support for both x86_64 and aarch64 architectures, enabling rv to run on older OS flavors than Ubuntu 22.04. Expanded the release matrix to provide pre-built binaries for more platforms.
- **Smart PATH-based detection**: rv now checks for system dependencies in PATH for tools commonly installed outside package managers (pandoc, texlive, cargo, rustc, chromium, google-chrome).
- **Customizable PATH checking**: New `RV_SYS_DEPS_CHECK_IN_PATH` environment variable allows specifying additional tools to check in PATH.

### ⚡ Improvements
- **Better package management**: More accurate detection of available system dependencies, reducing false "absent" reports.
- **Improved lockfile handling**: Better integration between lockfile usage and library state management.
- **More reliable installation**: More reliable package installation and dependency resolution.

### 🐛 Bug Fixes
- **Fixed SHA validation**: Fixed potential panic when SHA metadata is missing for git/URL-based packages. Non-repository packages now handle missing SHA metadata gracefully.
- **Improved error handling**: Enhanced error handling in library metadata operations.

## v0.9.0

### 🎉 New Features
- **Revolutionary performance**: Implemented incremental library builds that dramatically reduce sync times by only installing changed packages, with up to 80% faster sync operations for unchanged packages.
- **Smart staging**: New staging directory approach prevents unnecessary copying and linking.
- **Process safety**: Added better process detection to prevent removal of packages currently in use by R sessions.
- **Native API support**: Complete rewrite of R-Universe integration using their native API instead of PACKAGES files, with single query efficiency using one API call instead of multiple queries per package.
- **Improved git tracking**: Better handling of R-Universe packages with proper git SHA tracking and subdirectory support.
- **Comprehensive build logs**: All package builds now generate detailed logs with stdout/stderr output.
- **Log extraction**: New `--save-install-logs-in` flag for `rv sync` to save build logs to a specified directory.
- **Environment variable support**: New `RV_NO_CHECK_OPEN_FILE` environment variable for controlling file checks.

### ⚡ Improvements
- **Better error diagnosis**: Build failures now provide more actionable error information.
- **Cyclic dependency handling**: Completely resolved infinite loops when resolving cyclic dependencies.
- **Version requirement tracking**: Fixed issues where version requirements weren't properly loaded from lockfiles.
- **Smarter conflict resolution**: Improved SAT solver performance with better variable handling and timing information.
- **Improved status reporting**: More informative system dependency status in project summaries.
- **Better Ubuntu 20.04 support**: Fixed parsing issues with system requirements JSON.
- **Tree command improvements**: Added `--r-version` flag to `rv tree` command for better flexibility.
- **Dependencies-only support**: Better handling of `dependencies_only` packages in tree output.
- **Reduced network calls**: Smarter caching and fewer redundant operations.

### 🐛 Bug Fixes
- **Fixed activate/deactivate**: Commands now correctly use project directory instead of current directory.
- **Resolved tree filtering**: Fixed handling of ignored dependencies in dependency trees.
- **Repository parsing**: Better error handling for malformed repository responses.
