# rv configure repository - Command Examples

## Overview

The `rv configure repository` command allows you to:
- Add repositories with precise positioning (`--first`, `--last`, `--before`, `--after`)
- Replace existing repositories (`--replace`)
- Remove specific repositories (`--remove`)
- Clear all repositories (`--clear`)
- Enable force source compilation (`--force-source`)
- Output results in JSON format (`--json`) or detailed text format

Both output formats provide comprehensive information including operation type, repository alias, and URL details.

### Initial Configuration
```toml
[project]
name = "demo-project"
r_version = "4.4"
repositories = [
    {alias = "cran", url = "https://cran.r-project.org"},
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"}
]
dependencies = [
    "dplyr",
    "ggplot2",
]```

## Example 1: Add repository as first entry

**Command:**

```bash
./target/release/rv configure repository --config-file test-example/rproject.toml --alias "ppm-latest" --url "https://packagemanager.posit.co/cran/latest" --first
```

**Output:**
```
Repository 'ppm-latest' added successfully with URL: https://packagemanager.posit.co/cran/latest
```

**Resulting rproject.toml:**
```toml
[project]
name = "demo-project"
r_version = "4.4"
repositories = [
    { alias = "ppm-latest", url = "https://packagemanager.posit.co/cran/latest" },
    {alias = "cran", url = "https://cran.r-project.org"},
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"},
]
dependencies = [
    "dplyr",
    "ggplot2",
]
```

## Example 2: Add repository as last entry

**Command:**
```bash
./target/release/rv configure repository --config-file test-example/rproject.toml --alias "bioconductor" --url "https://bioconductor.org/packages/3.18/bioc" --last
```

**Output:**
```
Repository 'bioconductor' added successfully with URL: https://bioconductor.org/packages/3.18/bioc
```

**Resulting rproject.toml:**
```toml
[project]
name = "demo-project"
r_version = "4.4"
repositories = [
    { alias = "ppm-latest", url = "https://packagemanager.posit.co/cran/latest" },
    {alias = "cran", url = "https://cran.r-project.org"},
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"},
    { alias = "bioconductor", url = "https://bioconductor.org/packages/3.18/bioc" },
]
dependencies = [
    "dplyr",
    "ggplot2",
]
```

## Example 3: Add repository before an existing one

**Command:**
```bash
./target/release/rv configure repository --config-file test-example/rproject.toml --alias "ppm-2024-06" --url "https://packagemanager.posit.co/cran/2024-06-01" --before "posit"
```

**Output:**
```
Repository 'ppm-2024-06' added successfully with URL: https://packagemanager.posit.co/cran/2024-06-01
```

**Resulting rproject.toml:**
```toml
[project]
name = "demo-project"
r_version = "4.4"
repositories = [
    { alias = "ppm-latest", url = "https://packagemanager.posit.co/cran/latest" },
    {alias = "cran", url = "https://cran.r-project.org"},
    { alias = "ppm-2024-06", url = "https://packagemanager.posit.co/cran/2024-06-01" },
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"},
    { alias = "bioconductor", url = "https://bioconductor.org/packages/3.18/bioc" },
]
dependencies = [
    "dplyr",
    "ggplot2",
]
```

## Example 4: Add repository after an existing one

**Command:**
```bash
./target/release/rv configure repository --config-file test-example/rproject.toml --alias "ppm-2024-01" --url "https://packagemanager.posit.co/cran/2024-01-01" --after "cran"
```

**Output:**
```
Repository 'ppm-2024-01' added successfully with URL: https://packagemanager.posit.co/cran/2024-01-01
```

**Resulting rproject.toml:**
```toml
[project]
name = "demo-project"
r_version = "4.4"
repositories = [
    { alias = "ppm-latest", url = "https://packagemanager.posit.co/cran/latest" },
    {alias = "cran", url = "https://cran.r-project.org"},
    { alias = "ppm-2024-01", url = "https://packagemanager.posit.co/cran/2024-01-01" },
    { alias = "ppm-2024-06", url = "https://packagemanager.posit.co/cran/2024-06-01" },
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"},
    { alias = "bioconductor", url = "https://bioconductor.org/packages/3.18/bioc" },
]
dependencies = [
    "dplyr",
    "ggplot2",
]
```

## Example 5: Replace an existing repository

**Command:**
```bash
./target/release/rv configure repository --config-file test-example/rproject.toml --alias "posit-latest" --url "https://packagemanager.posit.co/cran/latest" --replace "posit"
```

**Output:**
```
Repository replaced successfully - new alias: 'posit-latest', URL: https://packagemanager.posit.co/cran/latest
```

**Resulting rproject.toml:**
```toml
[project]
name = "demo-project"
r_version = "4.4"
repositories = [
    { alias = "ppm-latest", url = "https://packagemanager.posit.co/cran/latest" },
    {alias = "cran", url = "https://cran.r-project.org"},
    { alias = "ppm-2024-01", url = "https://packagemanager.posit.co/cran/2024-01-01" },
    { alias = "ppm-2024-06", url = "https://packagemanager.posit.co/cran/2024-06-01" },
    { alias = "posit-latest", url = "https://packagemanager.posit.co/cran/latest" },
    { alias = "bioconductor", url = "https://bioconductor.org/packages/3.18/bioc" },
]
dependencies = [
    "dplyr",
    "ggplot2",
]
```

## Example 6: Remove a repository

**Command:**
```bash
./target/release/rv configure repository --config-file test-example/rproject.toml --remove "cran"
```

**Output:**
```
Repository 'cran' removed successfully
```

**Resulting rproject.toml:**
```toml
[project]
name = "demo-project"
r_version = "4.4"
repositories = [
    { alias = "ppm-latest", url = "https://packagemanager.posit.co/cran/latest" },
    { alias = "ppm-2024-01", url = "https://packagemanager.posit.co/cran/2024-01-01" },
    { alias = "ppm-2024-06", url = "https://packagemanager.posit.co/cran/2024-06-01" },
    { alias = "posit-latest", url = "https://packagemanager.posit.co/cran/latest" },
    { alias = "bioconductor", url = "https://bioconductor.org/packages/3.18/bioc" },
]
dependencies = [
    "dplyr",
    "ggplot2",
]
```

## Example 7: Clear all repositories

**Command:**
```bash
./target/release/rv configure repository --config-file test-example/rproject.toml --clear
```

**Output:**
```
All repositories cleared successfully
```

**Resulting rproject.toml:**
```toml
[project]
name = "demo-project"
r_version = "4.4"
repositories = [
]
dependencies = [
    "dplyr",
    "ggplot2",
]
```

## Example 8: Add repository with force_source flag

**Command:**
```bash
./target/release/rv configure repository --config-file test-example/rproject.toml --alias "bioc-data" --url "https://bioconductor.org/packages/3.18/data/annotation" --force-source
```

**Output:**
```
Repository 'bioc-data' added successfully with URL: https://bioconductor.org/packages/3.18/data/annotation
```

**Resulting rproject.toml:**
```toml
[project]
name = "demo-project"
r_version = "4.4"
repositories = [
    { alias = "bioc-data", url = "https://bioconductor.org/packages/3.18/data/annotation", force_source = true },
]
dependencies = [
    "dplyr",
    "ggplot2",
]
```

## Example 9: JSON output

**Command:**
```bash
./target/release/rv --json configure repository --config-file test-example/rproject.toml --alias "cran" --url "https://cran.r-project.org" --first
```

**Output:**
```json
{
  "operation": "add",
  "alias": "cran",
  "url": "https://cran.r-project.org/",
  "success": true,
  "message": "Repository configured successfully"
}
```

**Resulting rproject.toml:**
```toml
[project]
name = "demo-project"
r_version = "4.4"
repositories = [
    { alias = "cran", url = "https://cran.r-project.org/" },
    { alias = "bioc-data", url = "https://bioconductor.org/packages/3.18/data/annotation", force_source = true },
]
dependencies = [
    "dplyr",
    "ggplot2",
]
```

## Example 10: Error scenarios

### Duplicate alias error

**Command:**
```bash
./target/release/rv configure repository --config-file test-example/rproject.toml --alias "cran" --url "https://packagemanager.posit.co/cran/2024-11-01" --first
```

**Output:**
```
Repository with alias 'cran' already exists
```

### Invalid URL error

**Command:**
```bash
./target/release/rv configure repository --config-file test-example/rproject.toml --alias "invalid" --url "not-a-valid-url" --first
```

**Output:**
```
relative URL without a base
```

## Additional JSON Examples

### JSON output for remove operation
**Command:**
```bash
./target/release/rv --json configure repository --config-file test-example/rproject.toml --remove "cran"
```

**Output:**
```json
{
  "operation": "remove",
  "alias": "cran",
  "url": null,
  "success": true,
  "message": "Repository removed successfully"
}
```

### JSON output for replace operation
**Command:**
```bash
./target/release/rv --json configure repository --config-file test-example/rproject.toml --alias "cran-updated" --url "https://packagemanager.posit.co/cran/latest" --replace "cran"
```

**Output:**
```json
{
  "operation": "replace",
  "alias": "cran-updated",
  "url": "https://packagemanager.posit.co/cran/latest",
  "success": true,
  "message": "Repository replaced successfully"
}
```

### JSON output for clear operation
**Command:**
```bash
./target/release/rv --json configure repository --config-file test-example/rproject.toml --clear
```

**Output:**
```json
{
  "operation": "clear",
  "alias": null,
  "url": null,
  "success": true,
  "message": "All repositories cleared"
}
```

## Summary

The `rv configure repository` command provides comprehensive repository management capabilities:

1. **Positioning Control**: Add repositories exactly where needed with `--first`, `--last`, `--before`, and `--after` flags
2. **Repository Management**: Replace existing repositories with `--replace`, remove specific ones with `--remove`, or clear all with `--clear`
3. **Advanced Options**: Enable source compilation with `--force-source` flag
4. **JSON Integration**: Comprehensive JSON output support for programmatic usage with `--json` flag, including operation type, alias, URL, success status, and descriptive messages
5. **Error Handling**: Clear error messages for duplicate aliases, invalid URLs, and missing references
6. **TOML Preservation**: Maintains file formatting and structure using `toml_edit`

All commands preserve existing project structure and dependencies while only modifying the repositories section as requested.
