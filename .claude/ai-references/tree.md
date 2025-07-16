# Tree Command Implementation

The `tree` command in rv provides a visual representation of the dependency tree for an R project, showing how packages depend on each other in a hierarchical format.

## Location
`src/cli/commands/tree.rs`

## Core Data Structures

### NodeKind
```rust
enum NodeKind {
    Normal,  // Uses "├─" prefix
    Last,    // Uses "└─" prefix  
}
```
Controls the visual tree prefixes for proper ASCII art formatting.

### TreeNode
A comprehensive structure representing each package in the dependency tree:

**Key Fields:**
- `name`: Package name
- `version`: Package version (if resolved)
- `source`: Where the package comes from (lockfile, repository, git, etc.)
- `package_type`: Binary vs source package type
- `sys_deps`: System dependencies required by the package
- `resolved`: Whether dependency resolution succeeded
- `error`: Error message if resolution failed
- `version_req`: Version requirement string if unresolved
- `children`: Child dependencies
- `ignored`: Whether package was ignored during resolution

**Methods:**
- `get_sys_deps()`: Formats system dependencies for display
- `get_details()`: Creates detailed info string for each node
- `print_recursive()`: Handles the recursive tree printing with proper indentation

### Tree
Contains the collection of root-level dependency nodes and provides the main print functionality.

## Key Functions

### `tree()`
**Location:** `src/cli/commands/tree.rs:206-249`

Main entry point that builds the complete dependency tree structure:

1. Creates lookup maps for resolved and unresolved dependencies
2. Iterates through top-level dependencies from config
3. For each dependency, calls `recursive_finder()` to build the subtree
4. Returns a `Tree` struct containing all root nodes

### `recursive_finder()`
**Location:** `src/cli/commands/tree.rs:124-177`

Core recursive function that builds individual tree nodes:

1. **Resolved Dependencies**: Creates detailed nodes with version, source, package type, and system dependencies
2. **Unresolved Dependencies**: Creates error nodes with failure information
3. **Recursion**: Processes all child dependencies by calling itself
4. **System Dependencies**: Looks up system requirements from the context

## Tree Visualization Features

### ASCII Art Formatting
- Uses Unicode box-drawing characters (`├─`, `└─`, `│`)
- Proper indentation with `│ ` for continuation lines
- `▶` symbol for root-level packages

### Information Display
For **resolved packages**:
- Version number
- Source (lockfile, repository, git, etc.)  
- Package type (binary/source)
- System dependencies (if any)
- "ignored" status for packages that were skipped

For **unresolved packages**:
- "unresolved" status
- Error message
- Version requirement that failed

### Depth Control
- Supports `max_depth` parameter to limit tree traversal
- Depth 1 = only root dependencies
- Depth 2 = root + direct dependencies
- No limit = full tree

### System Dependencies
- Shows required system packages (Ubuntu/Debian only)
- Format: `(sys: package1, package2)`
- Can be hidden with `hide_system_deps` flag

## CLI Integration

The tree command is integrated into main CLI at `src/main.rs:870-894`:

```rust
Command::Tree {
    depth,
    hide_system_deps, 
    r_version,
} => {
    // Context setup and dependency resolution
    let tree = tree(&context, &resolution.found, &resolution.failed);
    
    // Output formatting (JSON or text)
    if output_format.is_json() {
        println!("{}", serde_json::to_string_pretty(&tree)?);
    } else {
        tree.print(depth, !hide_system_deps);
    }
}
```

## Example Output
```
▶ dplyr [version: 1.1.4, source: repository, type: binary]
├─ R6 [version: 2.5.1, source: repository, type: binary]
├─ cli [version: 3.6.2, source: repository, type: binary]
│ └─ glue [version: 1.7.0, source: repository, type: binary]
├─ generics [version: 0.1.3, source: repository, type: binary]
├─ glue [version: 1.7.0, source: repository, type: binary]
├─ lifecycle [version: 1.0.4, source: repository, type: binary]
│ ├─ cli [version: 3.6.2, source: repository, type: binary]
│ │ └─ glue [version: 1.7.0, source: repository, type: binary]
│ ├─ glue [version: 1.7.0, source: repository, type: binary]
│ └─ rlang [version: 1.1.4, source: repository, type: binary]
```

## JSON Output Support
All tree structures implement `Serialize` for JSON output, enabling programmatic consumption of dependency tree data.