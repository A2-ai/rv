# Repository Configuration CLI

This document describes the command-line interface for configuring repositories in rv projects.

## Command Structure

All repository configuration commands follow this structure:
```bash
rv configure repository <operation> [arguments] [options]
```

## Operations

### Add Repository

Add a new repository to the project configuration.

```bash
rv configure repository add <alias> --url <url> [options]
```

**Arguments:**
- `<alias>` - Repository alias/name

**Options:**
- `--url <url>` - Repository URL (required)
- `--force-source` - Enable force_source for this repository
- `--first` - Add as first repository
- `--last` - Add as last repository (default)
- `--before <alias>` - Add before the specified alias
- `--after <alias>` - Add after the specified alias

**Examples:**
```bash
# Add repository at end (default)
rv configure repository add cran --url https://cran.r-project.org

# Add repository at beginning
rv configure repository add cran --url https://cran.r-project.org --first

# Add repository with positioning
rv configure repository add cran --url https://cran.r-project.org --before posit

# Add repository with force_source
rv configure repository add bioc --url https://bioconductor.org/packages/3.18/bioc --force-source
```

### Replace Repository

Replace an existing repository completely, optionally changing the alias.

```bash
rv configure repository replace <old_alias> --url <url> [options]
```

**Arguments:**
- `<old_alias>` - Alias of repository to replace

**Options:**
- `--url <url>` - New repository URL (required)
- `--alias <new_alias>` - New alias (optional, keeps original if not specified)
- `--force-source` - Enable force_source for this repository

**Examples:**
```bash
# Replace repository keeping same alias
rv configure repository replace posit --url https://packagemanager.posit.co/cran/latest

# Replace repository with new alias
rv configure repository replace posit --alias posit-new --url https://packagemanager.posit.co/cran/latest

# Replace with force_source
rv configure repository replace posit --url https://packagemanager.posit.co/cran/latest --force-source
```

### Update Repository

Update specific fields of an existing repository without replacing everything.

```bash
rv configure repository update [<alias>] [options]
rv configure repository update --match-url <url> [options]
```

**Arguments:**
- `<alias>` - Repository alias to update (optional if using --match-url)

**Options:**
- `--match-url <url>` - Match repository by URL instead of alias
- `--alias <new_alias>` - Update the repository alias
- `--url <new_url>` - Update the repository URL
- `--force-source` - Enable force_source
- `--no-force-source` - Disable force_source

**Examples:**
```bash
# Update alias only
rv configure repository update posit --alias posit-updated

# Update URL only
rv configure repository update posit --url https://packagemanager.posit.co/cran/latest

# Enable force_source
rv configure repository update posit --force-source

# Disable force_source
rv configure repository update posit --no-force-source

# Update multiple fields
rv configure repository update posit --alias posit-new --url https://packagemanager.posit.co/cran/latest --force-source

# Match by URL and update alias
rv configure repository update --match-url https://packagemanager.posit.co/cran/2024-12-16/ --alias matched-by-url
```

### Remove Repository

Remove an existing repository from the configuration.

```bash
rv configure repository remove <alias>
```

**Arguments:**
- `<alias>` - Alias of repository to remove

**Examples:**
```bash
rv configure repository remove posit
```

### Clear Repositories

Remove all repositories from the configuration.

```bash
rv configure repository clear
```

**Examples:**
```bash
rv configure repository clear
```

## Global Options

These options can be used with any operation:

- `--config-file <path>` - Specify config file path (default: rproject.toml)
- `--json` - Output results in JSON format

## URL Validation

All URLs are validated and normalized according to RFC 3986 standards:
- Invalid URLs will cause the command to fail with an error
- URLs are automatically normalized (e.g., `https://cran.r-project.org` becomes `https://cran.r-project.org/`)
- Both HTTP and HTTPS URLs are supported

## Error Handling

Common errors and their meanings:

- `Duplicate alias: <alias>` - Attempting to add/update with an existing alias
- `Alias not found: <alias>` - Specified alias doesn't exist in configuration
- `Invalid URL: <error>` - URL format is invalid
- Conflicting positioning flags - Cannot use multiple positioning options together

## Output Formats

### Plain Text Output
```
Repository 'cran' added successfully with URL: https://cran.r-project.org/
```

### JSON Output
```json
{
  "operation": "add",
  "alias": "cran", 
  "url": "https://cran.r-project.org/",
  "success": true,
  "message": "Repository configured successfully"
}
```

## Configuration File Impact

All operations modify the `rproject.toml` file (or specified config file) directly. The repositories array is automatically formatted for readability:

```toml
[project]
name = "test"
r_version = "4.4"
repositories = [
    { alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/" },
    { alias = "cran", url = "https://cran.r-project.org/" },
]
dependencies = [
    "dplyr",
]
```

## Complete Examples from Integration Tests

### Example 1: Basic Add Operation
```bash
rv configure repository add cran --url https://cran.r-project.org --config-file rproject.toml
```

**Output:**
```
Repository 'cran' added successfully with URL: https://cran.r-project.org/
```

**Result:**
```toml
repositories = [
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"},
    { alias = "cran", url = "https://cran.r-project.org/" },
]
```

### Example 2: Add with Positioning
```bash
rv configure repository add cran --url https://cran.r-project.org --first --config-file rproject.toml
```

**Result:**
```toml
repositories = [
    { alias = "cran", url = "https://cran.r-project.org/" },
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"},
]
```

### Example 3: Replace Operation
```bash
rv configure repository replace posit --url https://packagemanager.posit.co/cran/latest --config-file rproject.toml
```

**Result:**
```toml
repositories = [
    { alias = "posit", url = "https://packagemanager.posit.co/cran/latest" }
]
```

### Example 4: Replace with New Alias
```bash
rv configure repository replace posit --alias posit-new --url https://packagemanager.posit.co/cran/latest --config-file rproject.toml
```

**Result:**
```toml
repositories = [
    { alias = "posit-new", url = "https://packagemanager.posit.co/cran/latest" }
]
```

### Example 5: Update Alias
```bash
rv configure repository update posit --alias posit-updated --config-file rproject.toml
```

**Result:**
```toml
repositories = [
    { alias = "posit-updated", url = "https://packagemanager.posit.co/cran/2024-12-16/" }
]
```

### Example 6: Update URL
```bash
rv configure repository update posit --url https://packagemanager.posit.co/cran/latest --config-file rproject.toml
```

**Result:**
```toml
repositories = [
    { alias = "posit", url = "https://packagemanager.posit.co/cran/latest" }
]
```

### Example 7: Enable Force Source
```bash
rv configure repository update posit --force-source --config-file rproject.toml
```

**Result:**
```toml
repositories = [
    { alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/", force_source = true }
]
```

### Example 8: Update by URL
```bash
rv configure repository update --match-url https://packagemanager.posit.co/cran/2024-12-16/ --alias matched-by-url --config-file rproject.toml
```

**Result:**
```toml
repositories = [
    { alias = "matched-by-url", url = "https://packagemanager.posit.co/cran/2024-12-16/" }
]
```

### Example 9: Remove Repository
```bash
rv configure repository remove posit --config-file rproject.toml
```

**Result:**
```toml
repositories = []
```

### Example 10: Clear All Repositories
```bash
rv configure repository clear --config-file rproject.toml
```

**Result:**
```toml
repositories = []
```

### Example 11: JSON Output
```bash
rv --json configure repository add cran --url https://cran.r-project.org --config-file rproject.toml
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