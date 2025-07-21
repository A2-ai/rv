use serde::Deserialize;

pub mod process_manager;
pub mod coordinator;
pub mod assertions;
pub mod commands;

pub use process_manager::RProcessManager;
pub use coordinator::{StepCoordinator, StepStatus};
pub use assertions::{check_assertion, check_insta_snapshot};
pub use commands::{execute_r_command_with_timeout, execute_with_timeout, execute_rv_command, load_r_script, parse_r_step_outputs};

// Workflow data structures
#[derive(Debug, Deserialize, Clone)]
pub struct WorkflowTest {
    #[serde(rename = "project-dir")]
    pub project_dir: String,
    pub config: String,
    pub test: TestSpec,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TestSpec {
    pub steps: Vec<TestStep>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TestStep {
    pub name: String,
    pub run: String,
    pub thread: String,
    #[serde(default)]
    pub assert: Option<TestAssertion>,
    #[serde(default)]
    pub insta: Option<String>, // snapshot file path
    #[serde(default)]
    pub restart: bool,
    #[serde(default)]
    pub timeout: Option<u64>, // timeout in seconds
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum TestAssertion {
    Single(String),
    Multiple(Vec<String>),
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
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug)]
pub struct ThreadOutput {
    pub thread_name: String,
    pub step_results: Vec<StepResult>,
}