//! Integration test suite for rv (R package manager) with multi-threaded R and rv command execution.
//!
//! This test suite provides a declarative YAML-based workflow testing framework that can execute
//! rv commands and R scripts in coordinated threads. Key features:
//!
//! - **Multi-threaded execution**: Coordinates `rv` and `r` threads with step-by-step synchronization
//! - **Flexible assertions**: Supports both simple string matching and structured contains/not-contains assertions
//! - **Snapshot testing**: Integration with `insta` for reproducible output verification
//! - **R process management**: Long-running R sessions with proper cleanup and restart capabilities
//! - **Debug support**: Comprehensive debugging output via `RV_TEST_DEBUG` environment variable
//! - **Test filtering**: Selective test execution via `RV_TEST_FILTER` environment variable
//!
//! ## Architecture
//!
//! Tests are defined in YAML workflow files that specify:
//! - Steps to execute in `rv` and `r` threads
//! - Assertions to validate output (stdout/stderr separation)
//! - Snapshot comparisons for deterministic output verification  
//! - Timeout and restart behavior for robust testing
//!
//! ## Thread Coordination
//!
//! The test framework uses a step coordinator to ensure deterministic execution order
//! across multiple threads, enabling complex integration scenarios while maintaining
//! test reliability and reproducibility.

use anyhow::Result;
use assert_cmd::Command;
use fs_err as fs;
use serde::Deserialize;
use std::{
    collections::HashMap,
    io::Write,
    path::Path,
    str::FromStr,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

// Constants for test configuration
const RV: &str = "rv";

// Timeout and timing constants
const DEBUG_PAUSE_MS: u64 = 100;
const OUTPUT_TRUNCATE_LONG: usize = 1000;
const OUTPUT_TRUNCATE_SHOW: usize = 400;

// Test environment variables
const ENV_TEST_DEBUG: &str = "RV_TEST_DEBUG";
const ENV_TEST_FILTER: &str = "RV_TEST_FILTER";
const ENV_R_EXECUTABLE: &str = "RV_R_EXECUTABLE";
const ENV_R_EXECUTABLE_ALT: &str = "R_EXECUTABLE";

// R process constants
const R_QUIT_COMMAND: &str = "quit(save = 'no')";
const R_STEP_END_PREFIX: &str = "# STEP_END: ";
const R_STARTUP_MARKER: &str = "start R";

// Test result display constants  
const RESULT_SEPARATOR_MAIN: &str = "═";
const RESULT_SEPARATOR_SUB: &str = "─";
const SEPARATOR_WIDTH: usize = 80;

/// Represents the supported thread types in workflow tests.
/// 
/// Thread types are defined in YAML workflow files and determine
/// how commands are executed (rv commands vs R script commands).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ThreadType {
    /// Executes rv commands (sync, plan, add, etc.)
    Rv,
    /// Executes R commands and scripts in a long-running R process
    R,
}

impl FromStr for ThreadType {
    type Err = anyhow::Error;
    
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "rv" => Ok(ThreadType::Rv),
            "r" => Ok(ThreadType::R),
            _ => Err(anyhow::anyhow!("Unknown thread type: '{}' (supported: 'rv', 'r')", s)),
        }
    }
}

impl std::fmt::Display for ThreadType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThreadType::Rv => write!(f, "rv"),
            ThreadType::R => write!(f, "r"),
        }
    }
}

fn debug_print(msg: &str) {
    if std::env::var(ENV_TEST_DEBUG).is_ok() {
        println!("🐛 DEBUG: {}", msg);
    }
}

// New timeout execution for R commands that can actually interrupt the process
/// Executes R commands with optional timeout and process management.
///
/// ## Critical Behavior:
/// - Sends command to R process via stdin
/// - Does NOT wait for output - R parsing happens later via step markers
/// - Timeout kills the entire R process (destructive operation)
/// - After timeout, captures whatever output is available before killing
///
/// ## Assumptions:
/// - R process writes step end markers for output parsing
/// - Timeout duration accounts for R startup time and command execution
/// - R process death requires full restart and state loss
fn execute_r_command_with_timeout<F>(
    step_name: &str,
    timeout_secs: Option<u64>,
    r_manager: &mut Option<RProcessManager>,
    operation: F,
) -> Result<String>
where
    F: FnOnce(&mut RProcessManager) -> Result<String>,
{
    if let Some(timeout) = timeout_secs {
        debug_print(&format!("Executing R step '{}' with timeout: {}s", step_name, timeout));
    } else {
        debug_print(&format!("Executing R step '{}' with no timeout", step_name));
    }
    
    if let Some(manager) = r_manager {
        if let Some(timeout_duration) = timeout_secs.map(Duration::from_secs) {
            debug_print(&format!("Starting timeout monitor for step '{}'", step_name));
            
            // Send the command first  
            let operation_result = operation(manager);
            if operation_result.is_err() {
                return operation_result;
            }
            
            debug_print(&format!("Command sent, now waiting {}s for R to execute it", timeout_duration.as_secs()));
            
            // Now wait for the timeout duration to see if R completes the command
            std::thread::sleep(timeout_duration);
            
            // After timeout, capture whatever output we can before killing the process
            debug_print(&format!("Timeout expired for step '{}', capturing output before killing R process", step_name));
            
            // Try to capture output before clearing the manager
            let captured_output = match r_manager.take() {
                Some(dying_manager) => {
                    debug_print("Attempting to capture R output before timeout failure");
                    match dying_manager.shutdown_and_capture_output() {
                        Ok((stdout, stderr)) => {
                            format!("R output captured after timeout:\n\nSTDOUT ({} chars):\n{}\n\nSTDERR ({} chars):\n{}", 
                                   stdout.len(), stdout,
                                   stderr.len(), String::from_utf8_lossy(&stderr))
                        }
                        Err(e) => {
                            format!("Failed to capture R output after timeout: {}", e)
                        }
                    }
                }
                None => {
                    "No R process available for output capture".to_string()
                }
            };
            
            return Err(anyhow::anyhow!("Step '{}' timed out after {}s\n\nCaptured output:\n{}", 
                                     step_name, timeout_duration.as_secs(), captured_output));
        } else {
            // No timeout, execute normally
            operation(manager)
        }
    } else {
        Err(anyhow::anyhow!("R process not available for step '{}'", step_name))
    }
}

// Keep the old function for non-R commands
fn execute_with_timeout<F, T>(
    step_name: &str, 
    timeout_secs: Option<u64>, 
    operation: F
) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    if let Some(timeout) = timeout_secs {
        debug_print(&format!("Executing step '{}' with timeout: {}s", step_name, timeout));
    } else {
        debug_print(&format!("Executing step '{}' with no timeout", step_name));
    }
    
    let start = Instant::now();
    let result = operation();
    let elapsed = start.elapsed();
    
    match result {
        Ok(value) => {
            debug_print(&format!("Step '{}' completed in {}s", step_name, elapsed.as_secs()));
            Ok(value)
        }
        Err(e) => {
            debug_print(&format!("Step '{}' failed after {}s: {}", step_name, elapsed.as_secs(), e));
            Err(e)
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
struct WorkflowTest {
    #[serde(rename = "project-dir")]
    project_dir: String,
    config: String,
    test: TestSpec,
}

#[derive(Debug, Deserialize, Clone)]
struct TestSpec {
    steps: Vec<TestStep>,
}

#[derive(Debug, Deserialize, Clone)]
struct TestStep {
    name: String,
    run: String,
    thread: String,
    #[serde(default)]
    assert: Option<TestAssertion>,
    #[serde(default)]
    insta: Option<String>, // snapshot file path
    #[serde(default)]
    restart: bool,
    #[serde(default)]
    timeout: Option<u64>, // timeout in seconds
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum TestAssertion {
    Single(String),
    Multiple(Vec<String>),
    Structured(StructuredAssertion),
}

#[derive(Debug, Deserialize, Clone)]
struct StructuredAssertion {
    #[serde(default)]
    contains: Option<StringOrList>,
    #[serde(default, rename = "not-contains")]
    not_contains: Option<StringOrList>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum StringOrList {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone)]
struct StepResult {
    name: String,
    step_index: usize,
    stdout: String,
    stderr: String,
}

#[derive(Debug)]
struct ThreadOutput {
    thread_name: String,
    step_results: Vec<StepResult>,
}


#[derive(Debug, Clone)]
enum StepStatus {
    Pending,
    Running,
    Completed,
}

/// Coordinates step execution across multiple threads to ensure deterministic test execution.
/// 
/// ## Threading Model:
/// - Each workflow defines steps that belong to specific threads (usually 'rv' and 'r')
/// - All threads must reach a step before any thread can proceed to the next step
/// - Uses condition variables and mutexes for synchronization
/// - Step completion is tracked per-thread using a 2D status matrix
/// 
/// ## Critical Assumptions:
/// - Thread names are defined in YAML workflow files (commonly 'rv' and 'r')
/// - Steps execute sequentially within each thread
/// - All threads must participate in each step (even if they don't execute)
/// - Coordinator ensures deterministic execution order across runs
struct StepCoordinator {
    step_status: Arc<Mutex<Vec<Vec<StepStatus>>>>, // [step_index][thread_index] 
    thread_names: Vec<String>,
    step_waiters: Arc<(Mutex<Vec<bool>>, Condvar)>, // One bool per step for coordination
}

/// Manages a long-running R process for executing R commands in integration tests.
///
/// ## Critical Assumptions:
/// - R process remains interactive and responsive to stdin commands
/// - R outputs step end markers (`# STEP_END: <step_name>`) to stdout for parsing
/// - R process can be gracefully terminated with `quit(save = 'no')`
/// - Platform differences: Windows uses `R.exe`, Unix uses `R`
/// - R startup includes .Rprofile loading (no --no-restore flag)
///
/// ## Process Lifecycle:
/// 1. Start R with platform-specific arguments (--interactive on Unix, --no-save on Windows)
/// 2. Execute commands by writing to stdin and flushing
/// 3. Parse output by looking for step end markers
/// 4. Health check by monitoring process status
/// 5. Graceful shutdown by sending quit command and capturing final output
struct RProcessManager {
    process: Option<std::process::Child>,
    stdin: Option<std::process::ChildStdin>,
    last_health_check: Instant,
    process_id: Option<u32>,
}

impl RProcessManager {
    fn find_r_executable() -> Result<String> {
        debug_print("Starting R executable detection");
        
        // Check for explicit configuration first
        if let Ok(r_path) = std::env::var(ENV_R_EXECUTABLE) {
            debug_print(&format!("Using {}: {}", ENV_R_EXECUTABLE, r_path));
            return Ok(r_path);
        }
        if let Ok(r_path) = std::env::var(ENV_R_EXECUTABLE_ALT) {
            debug_print(&format!("Using {}: {}", ENV_R_EXECUTABLE_ALT, r_path));
            return Ok(r_path);
        }
        
        // Auto-detect based on platform
        let candidates = if cfg!(windows) {
            vec!["R.exe", "R"]
        } else {
            vec!["R"]
        };
        
        debug_print(&format!("Trying candidates: {:?}", candidates));
        
        for candidate in candidates {
            debug_print(&format!("Testing candidate: {}", candidate));
            if std::process::Command::new(candidate)
                .arg("--version")
                .output()
                .is_ok() 
            {
                debug_print(&format!("Found working R executable: {}", candidate));
                return Ok(candidate.to_string());
            }
        }
        
        // Fallback - let the system try to find it
        debug_print("Using fallback R executable");
        Ok("R".to_string())
    }
    
    fn start_r_process(test_dir: &Path) -> Result<Self> {
        let r_executable = Self::find_r_executable()?;
        debug_print(&format!("Starting R process '{}' in directory: {}", r_executable, test_dir.display()));
        
        let args = if cfg!(windows) {
            // Windows R.exe requires --no-save instead of --interactive
            // Don't use --no-restore so .Rprofile gets sourced
            vec!["--no-save"]
        } else {
            // Unix R supports --interactive, but also don't use --no-restore so .Rprofile gets sourced
            vec!["--interactive"]
        };
        
        debug_print(&format!("Using R args: {:?}", args));
        
        let mut process = std::process::Command::new(&r_executable)
            .args(args)
            .current_dir(test_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!(
                "Failed to start R process with executable '{}': {}\n\n\
                Troubleshooting:\n\
                - Ensure R is installed and accessible\n\
                - Check that '{}' is in your PATH\n\
                - Verify R version compatibility (4.4.x recommended)\n\
                - On Windows, try setting RV_R_EXECUTABLE=R.exe", 
                r_executable, e, r_executable
            ))?;
        
        debug_print("R process started successfully");
        
        let process_id = process.id();
        let stdin = process.stdin.take()
            .ok_or_else(|| anyhow::anyhow!(
                "Failed to get R stdin pipe. This indicates R process failed to start properly.\n\
                The R process may have crashed immediately after startup."
            ))?;
        
        Ok(Self {
            process: Some(process),
            stdin: Some(stdin),
            last_health_check: Instant::now(),
            process_id: Some(process_id),
        })
    }
    
    fn is_alive(&mut self) -> Result<bool> {
        if let Some(process) = &mut self.process {
            match process.try_wait()
                .map_err(|e| anyhow::anyhow!(
                    "Unable to check R process health status: {}\n\
                    This may indicate system resource issues.", e
                ))? 
            {
                Some(exit_status) => {
                    println!("⚠️ R process exited with status: {}", exit_status);
                    
                    // Try to capture output from the dead process
                    if let Some(dead_process) = self.process.take() {
                        match dead_process.wait_with_output() {
                            Ok(output) => {
                                let stdout = String::from_utf8_lossy(&output.stdout);
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                
                                if !stdout.trim().is_empty() {
                                    println!("📤 R STDOUT ({} chars):\n{}", stdout.len(), stdout);
                                }
                                if !stderr.trim().is_empty() {
                                    println!("📤 R STDERR ({} chars):\n{}", stderr.len(), stderr);
                                }
                            }
                            Err(e) => {
                                debug_print(&format!("Failed to capture output from dead R process: {}", e));
                            }
                        }
                    }
                    
                    Ok(false)
                }
                None => {
                    self.last_health_check = Instant::now();
                    Ok(true)
                }
            }
        } else {
            Ok(false)
        }
    }
    
    fn send_command(&mut self, command: &str) -> Result<()> {
        debug_print(&format!("Sending R command: {}", command));
        
        if !self.is_alive()? {
            return Err(anyhow::anyhow!("Cannot send command to R: process not running (may have crashed)"));
        }
        
        if let Some(stdin) = &mut self.stdin {
            writeln!(stdin, "{}", command)
                .map_err(|e| anyhow::anyhow!("Failed to write '{}' to R stdin: {} (broken pipe?)", command, e))?;
            stdin.flush()
                .map_err(|e| anyhow::anyhow!("Failed to flush R stdin after '{}': {} (R process died?)", command, e))?;
            debug_print(&format!("Successfully sent R command: {}", command));
        } else {
            return Err(anyhow::anyhow!("R stdin not available (process not initialized or terminated)"));
        }
        
        Ok(())
    }
    
    #[allow(dead_code)]
    fn kill_process(&mut self) -> Result<()> {
        debug_print("Attempting to kill R process due to timeout");
        
        if let Some(mut process) = self.process.take() {
            // Try to kill the process
            match process.kill() {
                Ok(()) => {
                    debug_print("Successfully sent kill signal to R process");
                    // Wait a bit for the process to die
                    match process.wait() {
                        Ok(status) => {
                            debug_print(&format!("R process terminated with status: {:?}", status));
                        }
                        Err(e) => {
                            debug_print(&format!("Error waiting for R process to die: {}", e));
                        }
                    }
                }
                Err(e) => {
                    debug_print(&format!("Failed to kill R process: {}", e));
                    return Err(anyhow::anyhow!("Failed to kill R process: {}", e));
                }
            }
        }
        
        // Clean up stdin
        self.stdin = None;
        self.process_id = None;
        
        Ok(())
    }
    
    fn debug_pause_after_command(&self) {
        if std::env::var(ENV_TEST_DEBUG).is_ok() {
            debug_print("Pausing briefly to let R process command");
            std::thread::sleep(std::time::Duration::from_millis(DEBUG_PAUSE_MS));
        }
    }
    
    fn shutdown_and_capture_output(mut self) -> Result<(String, Vec<u8>)> {
        debug_print("Shutting down R process and capturing output");
        let accumulated_output = String::new();
        
        if let (Some(mut stdin), Some(process)) = (self.stdin.take(), self.process.take()) {
            debug_print("Sending quit command to R");
            if let Err(e) = writeln!(stdin, "{}", R_QUIT_COMMAND) {
                debug_print(&format!("Failed to send quit command to R: {}", e));
            }
            drop(stdin); // Close stdin to signal R to exit
            
            debug_print("Waiting for R process to complete");
            let final_output = process.wait_with_output()
                .map_err(|e| anyhow::anyhow!("Failed to wait for R process termination: {}", e))?;
            
            debug_print(&format!("R process completed with status: {:?}", final_output.status));
            if !final_output.status.success() {
                debug_print(&format!("R process exited with non-zero status: {:?}", final_output.status));
            }
            
            let stdout = final_output.stdout;
            let stderr = final_output.stderr;
            
            debug_print(&format!("Captured R stdout: {} bytes, stderr: {} bytes", stdout.len(), stderr.len()));
            
            Ok((String::from_utf8_lossy(&stdout).to_string(), stderr))
        } else {
            debug_print("No R process to shutdown");
            Ok((accumulated_output, Vec::new()))
        }
    }
}

impl StepCoordinator {
    fn new(thread_names: Vec<String>, num_steps: usize) -> Self {
        let num_threads = thread_names.len();
        
        // Initialize step status - all steps start as Pending for all threads
        let step_status = Arc::new(Mutex::new(
            (0..num_steps)
                .map(|_| vec![StepStatus::Pending; num_threads])
                .collect()
        ));
        
        let step_waiters = Arc::new((Mutex::new(vec![false; num_steps]), Condvar::new()));
        
        Self {
            step_status,
            thread_names,
            step_waiters,
        }
    }
    
    
    fn get_thread_index(&self, thread_name: &str) -> Option<usize> {
        self.thread_names.iter().position(|name| name == thread_name)
    }
    
    fn wait_for_step_start(&self, step_index: usize, thread_name: &str, timeout: Option<Duration>) -> Result<()> {
        let thread_index = self.get_thread_index(thread_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown thread '{}' (expected 'rv' or 'r')", thread_name))?;
            
        debug_print(&format!("Thread {} waiting for step {} to start", thread_name, step_index));
        
        // Mark this thread as ready for this step
        {
            let mut status = self.step_status.lock().unwrap();
            status[step_index][thread_index] = StepStatus::Running;
        }
        
        // Check if all threads are ready for this step
        let all_ready = {
            let status = self.step_status.lock().unwrap();
            status[step_index].iter().all(|s| matches!(s, StepStatus::Running | StepStatus::Completed))
        };
        
        if all_ready {
            debug_print(&format!("All threads ready for step {}, proceeding", step_index));
            let (lock, cvar) = &*self.step_waiters;
            let mut step_ready = lock.lock().unwrap();
            step_ready[step_index] = true;
            cvar.notify_all();
            return Ok(());
        }
        
        // Wait for other threads to be ready
        let (lock, cvar) = &*self.step_waiters;
        let mut step_ready = lock.lock().unwrap();
        
        let wait_result = if let Some(timeout_duration) = timeout {
            let start_time = Instant::now();
            loop {
                if step_ready[step_index] {
                    break Ok(());
                }
                
                let elapsed = start_time.elapsed();
                if elapsed >= timeout_duration {
                    break Err(anyhow::anyhow!("Timeout waiting for step {} start", step_index));
                }
                
                let remaining = timeout_duration - elapsed;
                let (new_lock, timeout_result) = cvar.wait_timeout(step_ready, remaining).unwrap();
                step_ready = new_lock;
                
                if timeout_result.timed_out() {
                    break Err(anyhow::anyhow!("Timeout waiting for step {} start", step_index));
                }
            }
        } else {
            while !step_ready[step_index] {
                step_ready = cvar.wait(step_ready).unwrap();
            }
            Ok(())
        };
        
        wait_result
    }
    
    fn notify_step_completed(&self, step_index: usize, thread_name: &str) -> Result<()> {
        let thread_index = self.get_thread_index(thread_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown thread '{}' (expected 'rv' or 'r')", thread_name))?;
            
        debug_print(&format!("Thread {} completed step {}", thread_name, step_index));
        
        {
            let mut status = self.step_status.lock().unwrap();
            status[step_index][thread_index] = StepStatus::Completed;
        }
        
        Ok(())
    }
    
    
    #[allow(dead_code)]
    fn should_continue(&self, step_index: usize) -> bool {
        let status = self.step_status.lock().unwrap();
        // Continue if at least one thread is still running or completed
        status[step_index].iter().any(|s| matches!(s, StepStatus::Running | StepStatus::Completed))
    }
}

fn load_r_script(script_name: &str) -> Result<String> {
    let script_path = format!("tests/input/r_scripts/{}", script_name);
    fs::read_to_string(&script_path)
        .map_err(|e| anyhow::anyhow!("Failed to load R script '{}': {} (check file exists and is readable)", script_path, e))
}

/// Parses R process output into per-step results using step end markers.
///
/// ## Critical Assumptions:
/// - R process outputs `# STEP_END: <step_name>` markers after each command
/// - First step is R startup (ends with `# STEP_END: start R`)
/// - Output between markers belongs to the preceding step
/// - All step names are known in advance from the workflow definition
/// - Markers appear in the same order as step execution
fn parse_r_step_outputs(full_output: &str, step_names: &[String]) -> HashMap<String, String> {
    let mut step_outputs = HashMap::new();
    
    // Find all step end markers with their positions
    let mut markers = Vec::new();
    for (i, line) in full_output.lines().enumerate() {
        if line.starts_with(R_STEP_END_PREFIX) {
            if let Some(step_name) = line.strip_prefix(R_STEP_END_PREFIX) {
                markers.push((i, step_name.to_string()));
            }
        }
    }
    
    let lines: Vec<&str> = full_output.lines().collect();
    
    // Handle the first step (R startup) if it exists
    if !step_names.is_empty() && !markers.is_empty() {
        let first_step_name = &step_names[0];
        let first_marker_line = markers[0].0;
        
        // Everything from start to first marker belongs to first step
        let first_step_output = lines[0..first_marker_line].join("\n");
        step_outputs.insert(first_step_name.clone(), first_step_output);
    }
    
    // Handle subsequent steps
    for (marker_idx, (marker_line, step_name)) in markers.iter().enumerate() {
        if marker_idx == 0 {
            continue; // First marker already handled above
        }
        
        // Find the previous marker
        let prev_marker_line = markers[marker_idx - 1].0;
        
        // Output is from after previous marker to before current marker
        let step_output = lines[(prev_marker_line + 1)..*marker_line].join("\n");
        step_outputs.insert(step_name.clone(), step_output);
    }
    
    step_outputs
}

/// Test execution context containing all setup data for a workflow test.
struct TestContext {
    workflow: WorkflowTest,
    test_dir: std::path::PathBuf,
    config_path: std::path::PathBuf,
    thread_steps: HashMap<String, Vec<usize>>,
    coordinator: Arc<StepCoordinator>,
    tx_map: HashMap<String, std::sync::mpsc::Sender<ThreadOutput>>,
    rx_map: HashMap<String, std::sync::mpsc::Receiver<ThreadOutput>>,
    #[allow(dead_code)] // Keep temp_dir alive for the duration of the test
    _temp_dir: TempDir,
}

/// Sets up the test environment and coordination structures for a workflow test.
fn setup_test_context(workflow_yaml: &str) -> Result<TestContext> {
    let workflow: WorkflowTest = serde_yaml::from_str(workflow_yaml)?;
    
    let temp_dir = TempDir::new()?;
    let project_path = temp_dir.path();
    let test_dir = project_path.join(&workflow.project_dir);
    
    // Create test directory
    fs::create_dir(&test_dir)?;
    
    // Get absolute path to config file
    let config_path = std::env::current_dir()?.join("tests/input").join(&workflow.config);
    
    // Count unique threads to set up coordination
    let mut thread_steps: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, step) in workflow.test.steps.iter().enumerate() {
        thread_steps.entry(step.thread.clone()).or_default().push(i);
    }
    
    // Create StepCoordinator
    let thread_names: Vec<String> = thread_steps.keys().cloned().collect();
    let coordinator = Arc::new(StepCoordinator::new(thread_names.clone(), workflow.test.steps.len()));
    
    // Channels for collecting step results from each thread
    let (tx_map, rx_map): (HashMap<String, _>, HashMap<String, _>) = thread_steps.keys()
        .map(|thread_name| {
            let (tx, rx) = std::sync::mpsc::channel::<ThreadOutput>();
            ((thread_name.clone(), tx), (thread_name.clone(), rx))
        })
        .unzip();
    
    Ok(TestContext {
        workflow,
        test_dir,
        config_path,
        thread_steps,
        coordinator,
        tx_map,
        rx_map,
        _temp_dir: temp_dir,
    })
}

/// Executes a complete workflow test from YAML definition.
///
/// ## Execution Flow:
/// 1. Parse YAML workflow into test steps
/// 2. Create temporary directory and copy config files  
/// 3. Group steps by thread and create coordination mechanism
/// 4. Spawn threads for each thread type (rv, r, etc.)
/// 5. Execute steps in synchronized order across threads
/// 6. Collect all outputs and run assertions/snapshots
/// 7. Clean up processes and temporary resources
///
/// ## Critical Thread Coordination:
/// - All threads wait at each step boundary before proceeding
/// - R thread maintains long-running process across steps
/// - rv thread executes individual commands synchronously
/// - Step completion notifications ensure deterministic execution
/// - Output parsing happens after all execution completes
///
/// ## Error Handling Strategy:
/// - Individual step failures fail the entire test
/// - R process crashes trigger restart or test failure
/// - Assertion failures are collected and reported together
/// - Resource cleanup happens regardless of test outcome
fn run_workflow_test(workflow_yaml: &str) -> Result<()> {
    let context = setup_test_context(workflow_yaml)?;
    
    // Spawn threads
    let mut thread_handles = HashMap::new();
    
    for (thread_name, step_indices) in context.thread_steps {
        let thread_coordinator = context.coordinator.clone();
        let thread_steps = context.workflow.test.steps.clone();
        let thread_test_dir = context.test_dir.clone();
        let thread_config_path = context.config_path.clone();
        let thread_tx = context.tx_map[&thread_name].clone();
        let thread_name_clone = thread_name.clone();
        
        let handle = thread::spawn(move || -> Result<()> {
            let mut step_results = Vec::new();
            let mut r_manager: Option<RProcessManager> = None;
            let mut accumulated_r_output = String::new();
            
            // Process all steps in order, coordinating with other threads
            for step_idx in 0..thread_steps.len() {
                // Wait for this step to start (simple coordination without timeout)
                thread_coordinator.wait_for_step_start(step_idx, &thread_name_clone, None)?;
                
                // Only execute if this step belongs to our thread
                if !step_indices.contains(&step_idx) {
                    // Even if we don't execute, we must notify completion
                    thread_coordinator.notify_step_completed(step_idx, &thread_name_clone)
                        .unwrap_or_else(|e| debug_print(&format!("Failed to notify completion: {}", e)));
                    continue;
                }
                
                let step = &thread_steps[step_idx];
                
                println!("🟡 {}: {}", thread_name_clone.to_uppercase(), step.name);
                println!("   └─ Running: {}", step.run);
                if let Some(timeout) = step.timeout {
                    println!("   └─ Timeout: {}s", timeout);
                }
                
                let output = match thread_name_clone.as_str() {
                    "rv" => {
                        // Handle rv commands with original timeout mechanism
                        execute_with_timeout(&step.name, step.timeout, || {
                            let (stdout, stderr) = execute_rv_command(&step.run, &thread_test_dir, &thread_config_path)?;
                            if !stdout.trim().is_empty() {
                                println!("   ├─ Output: {}", stdout.trim());
                            }
                            if !stderr.trim().is_empty() {
                                println!("   ├─ Stderr: {}", stderr.trim());
                            }
                            Ok((stdout, stderr))
                        })?
                    },
                    "r" => {
                        // Handle R commands - wrap strings in tuples for consistency
                        let r_output = if step.run == "R" {
                            // Check if this is a restart
                            if step.restart {
                                if let Some(manager) = r_manager.take() {
                                    // Capture output from previous session
                                    let (prev_stdout, prev_stderr) = manager.shutdown_and_capture_output()
                                        .map_err(|e| anyhow::anyhow!("Failed to shutdown R process during restart: {}", e))?;
                                    
                                    // Accumulate the output from the previous session
                                    accumulated_r_output.push_str(&prev_stdout);
                                    
                                    if !prev_stderr.is_empty() {
                                        accumulated_r_output.push_str("\n# === STDERR OUTPUT ===\n");
                                        accumulated_r_output.push_str(&String::from_utf8_lossy(&prev_stderr));
                                    }
                                    
                                    accumulated_r_output.push_str("\n# === R PROCESS RESTARTED ===\n");
                                }
                            }
                            
                            // Start (or restart) R process
                            r_manager = Some(RProcessManager::start_r_process(&thread_test_dir)
                                .map_err(|e| anyhow::anyhow!("Failed to start R process for step '{}': {} (check R installation)", step.name, e))?);
                            
                            // If this is a restart, add a step end marker
                            if step.restart {
                                if let Some(manager) = &mut r_manager {
                                    manager.send_command(&format!("cat('{}{}\\n')", R_STEP_END_PREFIX, step.name))
                                        .map_err(|e| anyhow::anyhow!("Failed to write restart step end marker: {}", e))?;
                                }
                                "R process restarted".to_string()
                            } else {
                                "R process started".to_string()
                            }
                        } else {
                            // Execute R script or command with timeout
                            execute_r_command_with_timeout(&step.name, step.timeout, &mut r_manager, |manager| {
                                // Check if R process is still alive
                                if !manager.is_alive()? {
                                    debug_print(&format!("R process died during step '{}'", step.name));
                                    return Err(anyhow::anyhow!("R process died unexpectedly during step '{}' (check R logs)", step.name));
                                }
                                
                                // Debug: Pause to let R process commands
                                manager.debug_pause_after_command();
                                
                                // First, add a marker for the startup step if this is the first command
                                let r_steps_so_far = step_results.len();
                                
                                if r_steps_so_far == 1 {
                                    // This is the first command after R startup, add startup marker
                                    manager.send_command("# R startup complete")
                                        .map_err(|e| anyhow::anyhow!("Failed to write startup comment: {}", e))?;
                                    manager.send_command(&format!("cat('{}{}\\n')", R_STEP_END_PREFIX, R_STARTUP_MARKER))
                                        .map_err(|e| anyhow::anyhow!("Failed to write startup marker: {}", e))?;
                                }
                                
                                // Execute the step
                                if step.run.ends_with(".R") {
                                    let script_content = load_r_script(&step.run)
                                        .map_err(|e| anyhow::anyhow!("Failed to load R script for step '{}': {}", step.name, e))?;
                                    manager.send_command(&script_content)
                                        .map_err(|e| anyhow::anyhow!("Failed to send R script for step '{}': {}", step.name, e))?;
                                } else {
                                    manager.send_command(&step.run)
                                        .map_err(|e| anyhow::anyhow!("Failed to send R command for step '{}': {}", step.name, e))?;
                                }
                                
                                // Add step end marker after the command
                                manager.send_command(&format!("cat('{}{}\\n')", R_STEP_END_PREFIX, step.name))
                                    .map_err(|e| anyhow::anyhow!("Failed to write step end marker for '{}': {}", step.name, e))?;
                                
                                // Debug: Pause after sending commands
                                manager.debug_pause_after_command();
                                
                                println!("   ├─ Command sent");
                                
                                Ok("Command executed".to_string())
                            })?
                        };
                        // Wrap R output in tuple with empty stderr for consistency
                        (r_output, String::new())
                    },
                    _ => return Err(anyhow::anyhow!("Unknown thread type: '{}' (only 'rv' and 'r' are supported)", thread_name_clone)),
                };
                
                // Store step result  
                let (stdout, stderr) = output;
                let step_result = StepResult {
                    name: step.name.clone(),
                    step_index: step_idx,
                    stdout,
                    stderr,
                };
                step_results.push(step_result);
                
                // Notify completion to coordinator
                thread_coordinator.notify_step_completed(step_idx, &thread_name_clone)
                    .map_err(|e| anyhow::anyhow!("Failed to notify step completion: {}", e))?;
            }
            
            // Clean up R process if it exists and capture all output
            if thread_name_clone == "r" {
                if let Some(manager) = r_manager {
                    let (final_stdout, final_stderr) = manager.shutdown_and_capture_output()
                        .map_err(|e| anyhow::anyhow!("Failed to shutdown R process for thread cleanup: {}", e))?;
                    
                    // Combine accumulated output with final output
                    accumulated_r_output.push_str(&final_stdout);
                    
                    if !final_stderr.is_empty() {
                        accumulated_r_output.push_str("\n# === STDERR OUTPUT ===\n");
                        accumulated_r_output.push_str(&String::from_utf8_lossy(&final_stderr));
                    }
                    
                    
                    // Extract R step names from our step results
                    let r_step_names: Vec<String> = step_results.iter()
                        .map(|sr| sr.name.clone())
                        .collect();
                    
                    // Parse the complete output to extract per-step outputs
                    let parsed_outputs = parse_r_step_outputs(&accumulated_r_output, &r_step_names);
                    
                    // Update step results with actual outputs (assertions checked at end)
                    for step_result in &mut step_results {
                        if let Some(step_output) = parsed_outputs.get(&step_result.name) {
                            step_result.stdout = step_output.clone();
                        }
                    }
                }
            }
            
            // Send results through channel
            let thread_output = ThreadOutput {
                thread_name: thread_name_clone.clone(),
                step_results,
            };
            thread_tx.send(thread_output)
                .map_err(|e| anyhow::anyhow!("Failed to send results from {} thread: {}", thread_name_clone, e))?;
            Ok(())
        });
        
        thread_handles.insert(thread_name, handle);
    }
    
    // Wait for all threads to complete
    for (thread_name, handle) in thread_handles {
        handle.join()
            .map_err(|_| anyhow::anyhow!("Thread '{}' panicked during execution", thread_name))?
            .map_err(|e| anyhow::anyhow!("Thread '{}' failed: {}", thread_name, e))?;
    }
    
    // Collect step results from all threads
    let mut all_thread_outputs = Vec::new();
    for (thread_name, rx) in context.rx_map {
        let thread_output = rx.recv().map_err(|e| anyhow::anyhow!("Failed to receive output from {}: {}", thread_name, e))?;
        all_thread_outputs.push(thread_output);
    }
    
    // Now check all assertions after we have all outputs
    let mut assertion_failures = Vec::new();
    
    // Check assertions and collect failures
    for thread_output in &all_thread_outputs {
        for step_result in &thread_output.step_results {
            // Find the original step by index to get its assertion
            if let Some(original_step) = context.workflow.test.steps.get(step_result.step_index) {
                // Check traditional assertions
                if let Some(assertion) = &original_step.assert {
                    if let Err(e) = check_assertion(assertion, &step_result.stdout, &step_result.stderr) {
                        // Include both stdout and stderr in failure reporting
                        let combined_output = if step_result.stderr.is_empty() {
                            step_result.stdout.clone()
                        } else {
                            format!("{}\n--- STDERR ---\n{}", step_result.stdout, step_result.stderr)
                        };
                        assertion_failures.push((step_result.name.clone(), e.to_string(), combined_output));
                    }
                }
                
                // Check insta snapshots (only use stdout for clean, predictable snapshots)
                if let Some(snapshot_name) = &original_step.insta {
                    if let Err(e) = check_insta_snapshot(snapshot_name, &step_result.stdout) {
                        assertion_failures.push((step_result.name.clone(), e.to_string(), step_result.stdout.clone()));
                    }
                }
            }
        }
    }
    
    // Print final results in execution order
    println!("\n📊 Final Results:");
    
    // Collect all step results with their thread info and sort by execution order
    let mut all_steps: Vec<(&StepResult, &str)> = Vec::new();
    for thread_output in &all_thread_outputs {
        for step_result in &thread_output.step_results {
            all_steps.push((step_result, &thread_output.thread_name));
        }
    }
    
    // Sort by step_index (execution order)
    all_steps.sort_by_key(|(step_result, _)| step_result.step_index);
    
    // Display in execution order
    for (step_result, thread_name) in all_steps {
        let original_step = context.workflow.test.steps.get(step_result.step_index);
        let has_assertion = original_step.map(|s| s.assert.is_some()).unwrap_or(false);
        let has_insta = original_step.map(|s| s.insta.is_some()).unwrap_or(false);
        
        let thread_label = thread_name.to_uppercase();
        
        if has_assertion || has_insta {
            let failed = assertion_failures.iter().any(|(name, _, _)| name == &step_result.name);
            let status = if failed { "❌ FAIL" } else { "✅ PASS" };
            let test_type = match (has_assertion, has_insta) {
                (true, true) => "ASSERT+INSTA",
                (true, false) => "ASSERT",
                (false, true) => "INSTA",
                (false, false) => unreachable!(),
            };
            let total_chars = step_result.stdout.len() + step_result.stderr.len();
            let char_info = if step_result.stderr.is_empty() {
                format!("{} chars", total_chars)
            } else {
                format!("{} chars: {} stdout, {} stderr", total_chars, step_result.stdout.len(), step_result.stderr.len())
            };
            println!("   • [{}] {} - {} {} ({})", thread_label, step_result.name, status, test_type, char_info);
        } else {
            let total_chars = step_result.stdout.len() + step_result.stderr.len();
            let char_info = if step_result.stderr.is_empty() {
                format!("{} chars", total_chars)
            } else {
                format!("{} chars: {} stdout, {} stderr", total_chars, step_result.stdout.len(), step_result.stderr.len())
            };
            println!("   • [{}] {} - ⏭️ NO ASSERTION ({})", thread_label, step_result.name, char_info);
        }
    }
    
    // If there were assertion failures, report them all
    if !assertion_failures.is_empty() {
        println!("\n💥 Assertion failures:");
        for (step_name, error, output) in &assertion_failures {
            println!("\n   Step '{}' failed:", step_name);
            println!("   Error: {}", error);
            println!("   Output ({} chars): {}", output.len(), output);
        }
        
        // In debug mode, show all thread outputs for debugging
        if std::env::var(ENV_TEST_DEBUG).is_ok() {
            println!("\n🔍 All Thread Debug Output:");
            for thread_output in &all_thread_outputs {
                if !thread_output.step_results.is_empty() {
                    println!("\n   === {} THREAD ===", thread_output.thread_name.to_uppercase());
                    for step_result in &thread_output.step_results {
                        let total_chars = step_result.stdout.len() + step_result.stderr.len();
                        println!("\n   Step '{}' ({} total chars):", step_result.name, total_chars);
                        
                        // Show stdout
                        if !step_result.stdout.is_empty() {
                            println!("   STDOUT ({} chars):", step_result.stdout.len());
                            if step_result.stdout.len() > OUTPUT_TRUNCATE_LONG {
                                // Truncate very long stdout
                                let truncated = format!("{}...\n[TRUNCATED {} chars]...\n{}", 
                                    &step_result.stdout[..OUTPUT_TRUNCATE_SHOW],
                                    step_result.stdout.len() - (OUTPUT_TRUNCATE_SHOW * 2),
                                    &step_result.stdout[step_result.stdout.len()-OUTPUT_TRUNCATE_SHOW..]);
                                println!("   {}", truncated);
                            } else {
                                println!("   {}", step_result.stdout);
                            }
                        }
                        
                        // Show stderr if present
                        if !step_result.stderr.is_empty() {
                            println!("   STDERR ({} chars):", step_result.stderr.len());
                            if step_result.stderr.len() > OUTPUT_TRUNCATE_LONG {
                                // Truncate very long stderr
                                let truncated = format!("{}...\n[TRUNCATED {} chars]...\n{}", 
                                    &step_result.stderr[..OUTPUT_TRUNCATE_SHOW],
                                    step_result.stderr.len() - (OUTPUT_TRUNCATE_SHOW * 2),
                                    &step_result.stderr[step_result.stderr.len()-OUTPUT_TRUNCATE_SHOW..]);
                                println!("   {}", truncated);
                            } else {
                                println!("   {}", step_result.stderr);
                            }
                        }
                    }
                }
            }
        }
        
        return Err(anyhow::anyhow!("Test failed: {} assertion failure(s) found (see details above)", assertion_failures.len()));
    }
    
    Ok(())
}

/// Executes rv commands and returns separated stdout/stderr.
///
/// ## Return Value Contract:
/// - Returns (stdout, stderr) tuple for separate handling
/// - Snapshots use ONLY stdout for deterministic comparisons
/// - Assertions check stdout first, then stderr if not found
/// - Both streams are captured but used for different purposes
///
/// ## Critical Assumptions:
/// - rv binary is available in PATH or built with `cargo build --features=cli`
/// - Commands execute synchronously and return immediately
/// - Exit status is checked - non-zero status becomes an error
/// - Config file copying happens automatically for 'init' command
fn execute_rv_command(command: &str, test_dir: &Path, config_path: &Path) -> Result<(String, String)> {
    let (cmd, args) = match command {
        "rv init" => ("init", vec![]),
        "rv sync" => ("sync", vec![]),
        "rv plan" => ("plan", vec![]),
        cmd if cmd.starts_with("rv ") => {
            let parts: Vec<&str> = cmd.split_whitespace().skip(1).collect();
            if parts.is_empty() {
                return Err(anyhow::anyhow!("Invalid rv command: '{}' (expected 'rv <subcommand>')", command));
            }
            (parts[0], parts[1..].to_vec())
        }
        _ => return Err(anyhow::anyhow!("Unknown rv command: '{}' (supported: init, sync, plan, add, etc.)", command)),
    };
    
    let output = Command::cargo_bin(RV)
        .map_err(|e| anyhow::anyhow!("Failed to find rv binary: {} (run 'cargo build --features=cli'?)", e))?
        .arg(cmd)
        .args(args)
        .current_dir(test_dir)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to execute 'rv {}': {} (check rv is built and accessible)", cmd, e))?;
    
    // CRITICAL: Check exit status
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(anyhow::anyhow!(
            "rv {} failed with exit code: {}\nStdout: {}\nStderr: {}", 
            command, 
            output.status.code().unwrap_or(-1),
            stdout,
            stderr
        ));
    }
    
    // Handle config copying for init
    if cmd == "init" && config_path.exists() {
        fs::copy(config_path, test_dir.join("rproject.toml"))
            .map_err(|e| anyhow::anyhow!("Failed to copy config file: {} (check source file permissions)", e))?;
    }
    
    // Return separate stdout and stderr (snapshots use stdout, assertions check both)
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    
    Ok((stdout, stderr))
}

fn check_assertion(assertion: &TestAssertion, stdout: &str, stderr: &str) -> Result<()> {
    match assertion {
        TestAssertion::Single(expected) => {
            check_contains_assertion_combined(expected, stdout, stderr)
        },
        TestAssertion::Multiple(expected_list) => {
            for expected in expected_list.iter() {
                check_contains_assertion_combined(expected, stdout, stderr)?;
            }
            Ok(())
        },
        TestAssertion::Structured(structured) => {
            // Check positive assertions (contains)
            if let Some(contains) = &structured.contains {
                check_string_or_list_contains_combined(contains, stdout, stderr)?;
            }
            
            // Check negative assertions (not-contains)
            if let Some(not_contains) = &structured.not_contains {
                check_string_or_list_not_contains_combined(not_contains, stdout, stderr)?;
            }
            
            Ok(())
        },
    }
}


// Combined assertion functions that check stdout first, then stderr
fn check_contains_assertion_combined(expected: &str, stdout: &str, stderr: &str) -> Result<()> {
    // First try stdout
    if stdout.contains(expected) {
        return Ok(());
    }
    
    // Then try stderr
    if stderr.contains(expected) {
        return Ok(());
    }
    
    // Not found in either - provide detailed error message
    return Err(anyhow::anyhow!(
        "Assertion failed: expected '{}' in output.\n\nSTDOUT ({} chars):\n{}\n\nSTDERR ({} chars):\n{}\n\nSearching for lines containing '{}':\nSTDOUT matches:\n{}\nSTDERR matches:\n{}", 
        expected,
        stdout.len(),
        stdout,
        stderr.len(),
        stderr,
        expected.split(':').next().unwrap_or(expected),
        stdout.lines()
            .filter(|line| line.contains(expected.split(':').next().unwrap_or(expected)))
            .collect::<Vec<_>>()
            .join("\n"),
        stderr.lines()
            .filter(|line| line.contains(expected.split(':').next().unwrap_or(expected)))
            .collect::<Vec<_>>()
            .join("\n")
    ));
}

fn check_string_or_list_contains_combined(contains: &StringOrList, stdout: &str, stderr: &str) -> Result<()> {
    match contains {
        StringOrList::Single(expected) => {
            check_contains_assertion_combined(expected, stdout, stderr)
        },
        StringOrList::Multiple(expected_list) => {
            for expected in expected_list.iter() {
                check_contains_assertion_combined(expected, stdout, stderr)?;
            }
            Ok(())
        },
    }
}

fn check_string_or_list_not_contains_combined(not_contains: &StringOrList, stdout: &str, stderr: &str) -> Result<()> {
    match not_contains {
        StringOrList::Single(expected) => {
            // For negative assertions, fail if found in either stdout OR stderr
            if stdout.contains(expected) || stderr.contains(expected) {
                return Err(anyhow::anyhow!(
                    "Negative assertion failed: found '{}' in output (expected NOT to find it).\n\nSTDOUT ({} chars):\n{}\n\nSTDERR ({} chars):\n{}", 
                    expected,
                    stdout.len(),
                    stdout,
                    stderr.len(),
                    stderr
                ));
            }
            Ok(())
        },
        StringOrList::Multiple(expected_list) => {
            for expected in expected_list.iter() {
                if stdout.contains(expected) || stderr.contains(expected) {
                    return Err(anyhow::anyhow!(
                        "Negative assertion failed: found '{}' in output (expected NOT to find it).\n\nSTDOUT ({} chars):\n{}\n\nSTDERR ({} chars):\n{}", 
                        expected,
                        stdout.len(),
                        stdout,
                        stderr.len(),
                        stderr
                    ));
                }
            }
            Ok(())
        },
    }
}

fn check_insta_snapshot(snapshot_name: &str, output: &str) -> Result<()> {
    // Filter out timing information that varies between runs
    let filtered_output = filter_timing_from_output(output);
    
    // Use insta to assert the snapshot
    insta::assert_snapshot!(snapshot_name, filtered_output);
    Ok(())
}

fn filter_timing_from_output(output: &str) -> String {
    // Replace timing patterns like "in 0ms", "in 1ms", etc. with "in Xms"
    let re = regex::Regex::new(r" in \d+ms").unwrap();
    re.replace_all(output, " in Xms").to_string()
}


#[test]
fn test_all_workflow_files() -> Result<()> {
    let filter = std::env::var(ENV_TEST_FILTER).ok();
    run_workflow_tests(filter.as_deref())
}

fn run_workflow_tests(filter: Option<&str>) -> Result<()> {
    let workflow_dir = std::env::current_dir()?.join("tests/input/workflows");
    
    if !workflow_dir.exists() {
        println!("⚠️  Workflow directory doesn't exist: {}", workflow_dir.display());
        return Ok(());
    }
    
    let entries = fs::read_dir(&workflow_dir)?;
    let mut workflow_files = Vec::new();
    
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "yml" || ext == "yaml") {
            // Apply filter if specified
            if let Some(filter_str) = filter {
                let file_stem = path.file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                let file_name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                
                // Match against either the full filename (with extension) or just the stem
                if !file_name.contains(filter_str) && !file_stem.contains(filter_str) {
                    continue;
                }
            }
            workflow_files.push(path);
        }
    }
    
    if workflow_files.is_empty() {
        if let Some(filter_str) = filter {
            println!("⚠️  No workflow files found matching filter '{}' in {}", filter_str, workflow_dir.display());
        } else {
            println!("⚠️  No YAML workflow files found in {}", workflow_dir.display());
        }
        return Ok(());
    }
    
    workflow_files.sort();
    
    let num_workflow_files = workflow_files.len();
    if let Some(filter_str) = filter {
        println!("🚀 Found {} workflow test files matching '{}' to run", num_workflow_files, filter_str);
    } else {
        println!("🚀 Found {} workflow test files to run", num_workflow_files);
    }
    println!("{}", RESULT_SEPARATOR_MAIN.repeat(SEPARATOR_WIDTH));
    
    for workflow_file in workflow_files {
        let file_name = workflow_file.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        
        println!("\n🧪 Running workflow test: {}", file_name);
        println!("📁 Loading workflow from: {}", workflow_file.display());
        
        let workflow_content = fs::read_to_string(&workflow_file)
            .map_err(|e| anyhow::anyhow!("Failed to read workflow file '{}': {} (check file exists and permissions)", workflow_file.display(), e))?;
        
        // Skip empty workflow files or comment-only files
        let trimmed_content = workflow_content.trim();
        if trimmed_content.is_empty() || trimmed_content.starts_with('#') {
            println!("⚠️ Skipping empty/comment workflow file: {}", file_name);
            continue;
        }
        
        // Parse and show workflow summary
        if let Ok(workflow) = serde_yaml::from_str::<WorkflowTest>(&workflow_content) {
            println!("📋 Workflow has {} steps across {} threads", 
                workflow.test.steps.len(),
                workflow.test.steps.iter().map(|s| &s.thread).collect::<std::collections::HashSet<_>>().len()
            );
            for step in &workflow.test.steps {
                println!("   • [{}] {}", step.thread, step.name);
            }
            println!();
        }
        
        match run_workflow_test(&workflow_content) {
            Ok(()) => {
                println!("🎉 {} completed successfully!", file_name);
                println!("{}", RESULT_SEPARATOR_SUB.repeat(SEPARATOR_WIDTH));
            },
            Err(e) => {
                eprintln!("💥 {} failed: {}", file_name, e);
                return Err(e);
            }
        }
    }
    
    println!("\n🏁 All {} workflow tests completed successfully!", num_workflow_files);
    println!("{}", RESULT_SEPARATOR_MAIN.repeat(SEPARATOR_WIDTH));
    
    Ok(())
}


