# R Integration Testing System

## Overview

This document describes the comprehensive R integration testing system built for the `rv` project. The system provides sophisticated multi-threaded testing capabilities with process-level timeout handling, cross-platform compatibility, and detailed output capture for debugging.

## Architecture

### Core Components

#### 1. **Test Framework (`tests/r_integration.rs`)**
The main testing framework that orchestrates complex workflows involving both `rv` commands and R processes running concurrently.

**Key Features:**
- Multi-threaded execution with thread synchronization
- Process-level timeout handling that can kill hanging R processes
- Comprehensive output capture for debugging
- Cross-platform R process management
- Step-by-step assertion checking

#### 2. **StepCoordinator**
Manages synchronization between multiple threads executing workflow steps.

```rust
struct StepCoordinator {
    num_threads: usize,
    num_steps: usize,
    step_status: Arc<Mutex<Vec<Vec<StepStatus>>>>,
    thread_names: Vec<String>,
    message_tx: mpsc::Sender<CoordinatorMessage>,
    step_waiters: Arc<(Mutex<Vec<bool>>, Condvar)>,
}
```

**Purpose:** Ensures all threads wait at each step boundary, enabling complex multi-step workflows where rv and R operations must be coordinated.

#### 3. **RProcessManager**
Handles R process lifecycle with cross-platform compatibility.

```rust
struct RProcessManager {
    process: Option<std::process::Child>,
    stdin: Option<std::process::ChildStdin>,
    last_health_check: Instant,
    process_id: Option<u32>,
}
```

**Key Methods:**
- `start_r_process()` - Platform-aware R startup
- `send_command()` - Send R commands via stdin
- `shutdown_and_capture_output()` - Clean shutdown with output capture
- `is_alive()` - Health checking with error output capture

#### 4. **Timeout System**
Process-interrupting timeout mechanism that prevents hanging tests.

**How it works:**
1. Commands are sent to R via stdin
2. Timer starts for the specified timeout duration
3. If timeout expires, R process is killed
4. Any available output is captured before failure
5. Test fails with detailed error message including R output

## Workflow System

### Workflow Definition Format (YAML)

```yaml
project-dir: test-project-name
config: rproject-config.toml

test:
  steps:
  - name: "human readable step name"
    run: "command or script.R"
    thread: "rv" | "r"
    timeout: 30  # optional, seconds
    restart: true  # optional, for R thread only
    assert: "expected output string" | ["multiple", "expected", "strings"]  # optional
    insta: "snapshot-name"  # optional, for insta snapshot testing
```

### Thread Types

#### **`rv` Thread**
- Executes rv commands (`rv init`, `rv sync`, `rv plan`, etc.)
- Uses `cargo_bin()` to find the rv executable
- Each command runs in isolation
- Suitable for: Package management operations, project initialization

#### **`r` Thread**  
- Manages persistent R process
- Commands sent via stdin to running R session
- Supports process restart with `restart: true`
- Suitable for: R script execution, package loading, version checking

### Step Execution Flow

1. **Coordination**: All threads wait at step boundary
2. **Execution**: Thread executes its assigned step (if any)
3. **Timeout Monitoring**: Optional process-level timeout
4. **Output Capture**: Step output captured for assertions
5. **Completion**: Thread signals completion
6. **Repeat**: Move to next step

## Insta Snapshot Testing

The testing framework supports cargo insta for deterministic output testing, particularly useful for `rv` commands that produce stable, predictable output.

### When to Use Snapshots vs Assertions

**Use Insta Snapshots for:**
- `rv plan` - Shows dependency resolution (stable output)
- `rv sync` - Shows package installation (with timing normalization)
- Commands with predictable, system-independent output

**Use Traditional Assertions for:**
- `rv cache` - Contains system-specific paths
- `rv summary` - Contains OS, CPU, and path information
- Commands with variable or system-specific output

### TestStep Structure with Insta

Steps can use both `assert` and `insta` together:

```yaml
- name: "rv plan"
  run: "rv plan"
  thread: rv
  insta: "my-plan-snapshot"  # Creates snapshot for full output
  assert: "R6"  # Also check that R6 is mentioned
```

### Timing Filter Implementation

Insta snapshots automatically filter out variable timing information to ensure deterministic tests:

```rust
fn filter_timing_from_output(output: &str) -> String {
    // Replace "in 0ms", "in 15ms", etc. with "in Xms"
    let re = regex::Regex::new(r" in \d+ms").unwrap();
    re.replace_all(output, " in Xms").to_string()
}
```

**Example output transformation:**
```
+ callr (3.7.6, binary from ppm) in 1ms
+ R6 (2.6.1, binary from ppm) in 0ms
```
becomes:
```
+ callr (3.7.6, binary from ppm) in Xms
+ R6 (2.6.1, binary from ppm) in Xms
```

### Snapshot Management

**Snapshot Storage:** `tests/snapshots/r_integration__<snapshot-name>.snap`

**Accepting New Snapshots:**
```bash
cargo insta accept
```

**Reviewing Changes:**
```bash
cargo insta review
```

## Cross-Platform Considerations

### R Process Startup Arguments

**Windows:**
```rust
vec!["--no-save"]  // No --interactive support, .Rprofile sourced
```

**Unix (macOS/Linux):**
```rust
vec!["--interactive"]  // Interactive mode, .Rprofile sourced
```

**Key Point:** `--no-restore` is NOT used on any platform to ensure `.Rprofile` gets sourced, which is critical for rv's library path integration.

### R Executable Detection

**Windows:** Tries `R.exe` first, then falls back to `R`
**Unix:** Uses `R` directly

### Path Handling

The system handles Windows/Unix path differences automatically, particularly important for:
- R executable location
- Working directory setup
- Library path configuration

## Key Files and Purposes

### Test Framework Files

- **`tests/r_integration.rs`** - Main test framework
- **`tests/input/workflows/*.yml`** - Test workflow definitions
- **`tests/input/r_scripts/*.R`** - R scripts used in tests
- **`tests/input/*.toml`** - rv project configurations for tests

### Example Workflow Files

- **`full_r6_workflow.yml`** - Complex end-to-end test with package version management
- **`plan-sync-add-cache-workflow.yml`** - Multi-step workflow using both insta snapshots and assertions
- **`cache-test.yml`** - Simple workflow demonstrating insta snapshot usage
- **`simple_timeout.yml`** - Demonstrates timeout functionality

### R Scripts

- **`load_r6.R`** - Loads R6 and prints version
- **`install_old_r6.R`** - Installs older R6 version from historical snapshot
- **`wait.R`** - Sleep script for timeout testing

## Timeout System Details

### Purpose
Prevents tests from hanging indefinitely when R processes freeze or encounter infinite loops.

### Implementation
```rust
fn execute_r_command_with_timeout<F>(
    step_name: &str,
    timeout_secs: Option<u64>,
    r_manager: &mut Option<RProcessManager>,
    operation: F,
) -> Result<String>
```

### Process
1. Send R command via stdin
2. Sleep for timeout duration
3. If timeout expires, capture any available output
4. Kill R process and clear manager
5. Return error with captured output for debugging

### Configuration
Add `timeout: N` to any step in workflow YAML where N is seconds.

## Error Handling and Debugging

### Debug Output
Set `RV_TEST_DEBUG=1` to enable detailed debug output:
- R executable detection
- Command sending/receiving
- Process lifecycle events
- Step coordination timing

### Error Output Capture
When R processes fail or timeout:
- **stdout** and **stderr** are captured
- **Process exit codes** are reported
- **Step context** is provided
- **Full command history** is available

### Common Issues and Solutions

#### **"rv libpaths active!" not appearing**
- **Cause:** `.Rprofile` not being sourced
- **Solution:** Ensure R started without `--no-restore` flag
- **Check:** Verify `.Rprofile` exists in test directory

#### **"Nothing to do" from rv commands**
- **Cause:** rv not detecting package changes
- **Solution:** Check if R package installation actually occurred
- **Debug:** Examine R stderr for installation messages

#### **Tests hanging indefinitely**
- **Cause:** Missing timeout on long-running operations
- **Solution:** Add `timeout: N` to problematic steps
- **Prevention:** Use timeouts liberally for R operations

#### **Cross-platform failures**
- **Cause:** R executable not found or wrong arguments
- **Solution:** Check R executable detection logic
- **Windows specific:** Ensure `R.exe` is in PATH

## Usage Examples

### Running Tests

```bash
# Run all workflow tests
cargo test --features=cli --test r_integration -- --nocapture --test-threads=1

# With debug output
RV_TEST_DEBUG=1 cargo test --features=cli --test r_integration -- --nocapture --test-threads=1

# Run specific workflow (filter by filename)
cargo test --features=cli --test r_integration test_all_workflow_files -- --nocapture --test-threads=1
```

### Creating New Workflows

1. Create YAML file in `tests/input/workflows/`
2. Define project-dir and config
3. List steps with thread assignments
4. Choose between `assert` and `insta` for verification:
   - Use `insta` for stable, predictable output
   - Use `assert` for system-specific or variable output
5. Test incrementally with debug output
6. Accept snapshots with `cargo insta accept`

**Example workflow with mixed testing approaches:**
```yaml
project-dir: my-test
config: my-config.toml

test:
  steps:
  - name: "rv init"
    run: "rv init"
    thread: rv
    assert: "successfully initialized"
    
  - name: "rv plan"
    run: "rv plan"
    thread: rv
    insta: "my-plan-snapshot"  # Stable output suitable for snapshots
    
  - name: "rv cache"
    run: "rv cache"
    thread: rv
    assert: "https://packagemanager.posit.co"  # Path-dependent, use assertion
```

### Adding Timeout to Existing Steps

```yaml
- name: "potentially slow operation"
  run: some_script.R
  thread: r
  timeout: 30  # Kill after 30 seconds
```

## Extension Points

### Adding New Command Types
Extend `execute_rv_command()` function to handle new rv subcommands.

### Adding New R Scripts
Place `.R` files in `tests/input/r_scripts/` and reference them in workflows.

### Custom Assertions
Modify `check_assertion()` function to support new assertion types beyond string matching.

### Custom Snapshot Filters
Extend `filter_timing_from_output()` function to handle additional variable content patterns that need normalization for deterministic testing.

### Additional Thread Types
Extend the thread type system to support other external processes beyond rv and R.

## Best Practices

1. **Always use timeouts** for R operations that could hang
2. **Test cross-platform** - what works on one OS may fail on another
3. **Use debug output** liberally when developing new workflows
4. **Keep workflows focused** - test one behavior per workflow file
5. **Choose the right testing approach**:
   - **Insta snapshots** for stable, predictable output
   - **Traditional assertions** for system-specific or variable content
6. **Use descriptive step names** for clear error messages
7. **Handle R process restarts** carefully - they're expensive operations
8. **Accept snapshots promptly** - use `cargo insta accept` during development
9. **Review snapshot changes** carefully - they represent expected behavior changes

## Conclusion

This R integration testing system provides a robust foundation for testing complex interactions between rv and R. The timeout system prevents hanging tests, cross-platform support ensures reliability, and detailed error output enables effective debugging. The workflow-driven approach makes it easy to create comprehensive end-to-end tests that verify rv's behavior in realistic usage scenarios.

The integration of cargo insta snapshot testing adds deterministic output verification for stable commands while traditional assertions handle system-specific content. The timing filter ensures snapshots remain consistent across different execution environments. Together, these tools provide comprehensive testing coverage for both rv's package management functionality and its integration with live R sessions.