use serde::Deserialize;

pub mod assertions;
pub mod commands;
pub mod coordinator;
pub mod process_manager;

pub use assertions::{check_assertion, filter_timing_from_output};
pub use commands::{
    execute_r_command_with_timeout, execute_rv_command, execute_with_timeout, load_r_script,
    parse_r_step_outputs,
};
pub use coordinator::StepCoordinator;
pub use process_manager::RProcessManager;

// Workflow data structures

/// Represents a complete workflow test configuration loaded from YAML.
/// 
/// A workflow test defines a multi-threaded integration test scenario where
/// different threads (typically 'rv' and 'r') execute steps in coordination
/// with each other to test realistic usage patterns.
/// 
/// # Examples
/// 
/// ```yaml
/// project-dir: "test_project"
/// config: "rproject.toml" 
/// test:
///   steps:
///     - name: "rv init"
///       run: "rv init"
///       thread: "rv"
/// ```
#[derive(Debug, Deserialize, Clone)]
pub struct WorkflowTest {
    /// Directory name for the test project (created in temp directory)
    #[serde(rename = "project-dir")]
    pub project_dir: String,
    /// Path to the configuration file relative to tests/input/
    pub config: String,
    /// The test specification containing all steps to execute
    pub test: TestSpec,
}

/// Test specification containing the sequence of steps to execute.
/// 
/// Each test spec defines an ordered list of steps that will be executed
/// across multiple threads with proper synchronization. Steps are executed
/// in the order defined, with threads coordinating at each step boundary.
#[derive(Debug, Deserialize, Clone)]
pub struct TestSpec {
    /// Ordered list of test steps to execute across threads
    pub steps: Vec<TestStep>,
}

/// Individual test step within a workflow.
/// 
/// Each step runs on a specific thread and can include assertions,
/// snapshots, timeouts, and restart behavior for R processes.
/// Steps are executed synchronously across all threads - each step
/// waits for all previous steps to complete before starting.
/// 
/// # Examples
/// 
/// Basic rv command:
/// ```yaml
/// - name: "rv init"
///   run: "rv init" 
///   thread: "rv"
///   assert: "successfully initialized"
/// ```
/// 
/// R process restart:
/// ```yaml
/// - name: "restart R"
///   run: "R"
///   thread: "r"
///   restart: true
/// ```
#[derive(Debug, Deserialize, Clone)]
pub struct TestStep {
    /// Human-readable name for this step (used in output and assertions)
    pub name: String,
    /// Command to execute or script file to run (.R files are loaded and executed)
    pub run: String,
    /// Thread name this step should execute on ("rv" or "r" typically)
    pub thread: String,
    /// Optional assertion to validate step output
    #[serde(default)]
    pub assert: Option<TestAssertion>,
    /// Optional snapshot name for insta snapshot testing
    #[serde(default)]
    pub insta: Option<String>, // snapshot file path
    /// Whether to restart the R process before this step (R thread only)
    #[serde(default)]
    pub restart: bool,
    /// Optional timeout in seconds for this step
    #[serde(default)]
    pub timeout: Option<u64>, // timeout in seconds
}

/// Test assertion that can be applied to step output.
/// 
/// Supports simple string assertions, multiple assertions, or
/// structured assertions with contains/not-contains logic.
/// All assertions use substring matching by default.
/// 
/// # Examples
/// 
/// Simple string assertion:
/// ```yaml
/// assert: "successfully initialized"
/// ```
/// 
/// Multiple assertions (all must pass):
/// ```yaml  
/// assert:
///   - "Package installed"
///   - "No errors"
/// ```
/// 
/// Structured assertion:
/// ```yaml
/// assert:
///   contains: "success"
///   not-contains: "error"
/// ```
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum TestAssertion {
    /// Single string that must be contained in the output
    Single(String),
    /// Multiple strings that must all be contained in the output
    Multiple(Vec<String>),
    /// Structured assertion with contains/not-contains logic
    Structured(StructuredAssertion),
}

#[derive(Debug, Deserialize, Clone)]
pub struct StructuredAssertion {
    #[serde(default)]
    pub contains: Option<StringOrList>,
    #[serde(default, rename = "not-contains")]
    pub not_contains: Option<StringOrList>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum StringOrList {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct StepResult {
    pub name: String,
    pub step_index: usize,
    pub output: String,
    pub exit_status: Option<std::process::ExitStatus>,
}

#[derive(Debug)]
pub struct ThreadOutput {
    pub thread_name: String,
    pub step_results: Vec<StepResult>,
}
