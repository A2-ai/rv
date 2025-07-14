use anyhow::Result;
use assert_cmd::Command;
use tempfile::TempDir;
use std::thread;
use std::io::Write;
use std::sync::{Arc, Barrier};
use fs_err as fs;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

const RV: &str = "rv";

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
    restart: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum TestAssertion {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone)]
struct StepResult {
    name: String,
    step_index: usize,
    output: String,
}

#[derive(Debug)]
struct ThreadOutput {
    thread_name: String,
    step_results: Vec<StepResult>,
}

struct RProcessManager {
    process: Option<std::process::Child>,
    stdin: Option<std::process::ChildStdin>,
    last_health_check: Instant,
}

impl RProcessManager {
    fn start_r_process(test_dir: &Path) -> Result<Self> {
        let mut process = std::process::Command::new("R")
            .args(["--interactive", "--no-restore"])
            .current_dir(test_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to start R process: {}. Is R installed and in PATH?", e))?;
        
        let stdin = process.stdin.take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get R stdin"))?;
        
        Ok(Self {
            process: Some(process),
            stdin: Some(stdin),
            last_health_check: Instant::now(),
        })
    }
    
    fn is_alive(&mut self) -> Result<bool> {
        if let Some(process) = &mut self.process {
            match process.try_wait()
                .map_err(|e| anyhow::anyhow!("Failed to check R process status: {}", e))? 
            {
                Some(exit_status) => {
                    println!("⚠️ R process exited with status: {}", exit_status);
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
        if !self.is_alive()? {
            return Err(anyhow::anyhow!("R process is not running"));
        }
        
        if let Some(stdin) = &mut self.stdin {
            writeln!(stdin, "{}", command)
                .map_err(|e| anyhow::anyhow!("Failed to write '{}' to R stdin: {}", command, e))?;
            stdin.flush()
                .map_err(|e| anyhow::anyhow!("Failed to flush R stdin after command '{}': {}", command, e))?;
        } else {
            return Err(anyhow::anyhow!("R stdin not available"));
        }
        
        Ok(())
    }
    
    fn shutdown_and_capture_output(mut self) -> Result<(String, Vec<u8>)> {
        let accumulated_output = String::new();
        
        if let (Some(mut stdin), Some(process)) = (self.stdin.take(), self.process.take()) {
            if let Err(e) = writeln!(stdin, "quit(save = 'no')") {
                println!("⚠️ Failed to send quit command to R: {}", e);
            }
            drop(stdin); // Close stdin to signal R to exit
            
            let final_output = process.wait_with_output()
                .map_err(|e| anyhow::anyhow!("Failed to wait for R process termination: {}", e))?;
            
            if !final_output.status.success() {
                println!("⚠️ R process exited with non-zero status: {:?}", final_output.status);
            }
            
            let stdout = final_output.stdout;
            let stderr = final_output.stderr;
            
            Ok((String::from_utf8_lossy(&stdout).to_string(), stderr))
        } else {
            Ok((accumulated_output, Vec::new()))
        }
    }
}

fn load_r_script(script_name: &str) -> Result<String> {
    let script_path = format!("tests/input/r_scripts/{}", script_name);
    fs::read_to_string(&script_path)
        .map_err(|e| anyhow::anyhow!("Failed to load R script {}: {}", script_path, e))
}

fn parse_r_step_outputs(full_output: &str, step_names: &[String]) -> HashMap<String, String> {
    let mut step_outputs = HashMap::new();
    
    // Find all step end markers with their positions
    let mut markers = Vec::new();
    for (i, line) in full_output.lines().enumerate() {
        if line.starts_with("# STEP_END: ") {
            if let Some(step_name) = line.strip_prefix("# STEP_END: ") {
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

fn run_workflow_test(workflow_yaml: &str) -> Result<()> {
    let workflow: WorkflowTest = serde_yaml::from_str(workflow_yaml)?;
    
    let temp_dir = TempDir::new()?;
    let project_path = temp_dir.path();
    let test_dir = project_path.join(&workflow.project_dir);
    
    // Create test directory
    fs::create_dir(&test_dir)?;
    
    // Get absolute path to config file
    let config_path = std::env::current_dir()?.join("tests/input").join(&workflow.config);
    
    // Count unique threads to set up barriers
    let mut thread_steps: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, step) in workflow.test.steps.iter().enumerate() {
        thread_steps.entry(step.thread.clone()).or_default().push(i);
    }
    
    // Create barriers for synchronization between steps - each step needs all threads to sync
    // We need two barriers per step: one for start, one for completion
    let num_threads = thread_steps.len();
    let start_barriers: Vec<Arc<Barrier>> = (0..workflow.test.steps.len())
        .map(|_| Arc::new(Barrier::new(num_threads)))
        .collect();
    let completion_barriers: Vec<Arc<Barrier>> = (0..workflow.test.steps.len())
        .map(|_| Arc::new(Barrier::new(num_threads)))
        .collect();
    
    // Channels for collecting step results from each thread
    let (tx_map, rx_map): (HashMap<String, _>, HashMap<String, _>) = thread_steps.keys()
        .map(|thread_name| {
            let (tx, rx) = std::sync::mpsc::channel::<ThreadOutput>();
            ((thread_name.clone(), tx), (thread_name.clone(), rx))
        })
        .unzip();
    
    // Spawn threads
    let mut thread_handles = HashMap::new();
    
    for (thread_name, step_indices) in thread_steps {
        let thread_start_barriers = start_barriers.clone();
        let thread_completion_barriers = completion_barriers.clone();
        let thread_steps = workflow.test.steps.clone();
        let thread_test_dir = test_dir.clone();
        let thread_config_path = config_path.clone();
        let thread_tx = tx_map[&thread_name].clone();
        let thread_name_clone = thread_name.clone();
        
        let handle = thread::spawn(move || -> Result<()> {
            let mut step_results = Vec::new();
            let mut r_manager: Option<RProcessManager> = None;
            let mut accumulated_r_output = String::new();
            
            // Process all steps in order, participating in barriers even if not executing
            for step_idx in 0..thread_steps.len() {
                // Wait for this step's start - all threads begin this step together
                thread_start_barriers[step_idx].wait();
                
                // Only execute if this step belongs to our thread
                if !step_indices.contains(&step_idx) {
                    // Even if we don't execute, we must wait at the completion barrier
                    thread_completion_barriers[step_idx].wait();
                    continue;
                }
                
                let step = &thread_steps[step_idx];
                
                println!("🟡 {}: {}", thread_name_clone.to_uppercase(), step.name);
                println!("   └─ Running: {}", step.run);
                
                let output = match thread_name_clone.as_str() {
                    "rv" => {
                        // Handle rv commands
                        let result = execute_rv_command(&step.run, &thread_test_dir, &thread_config_path)?;
                        if !result.trim().is_empty() {
                            println!("   ├─ Output: {}", result.trim());
                        }
                        
                        result
                    },
                    "r" => {
                        // Handle R commands
                        if step.run == "R" {
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
                                .map_err(|e| anyhow::anyhow!("Failed to start R process for step '{}': {}", step.name, e))?);
                            
                            // If this is a restart, add a step end marker
                            if step.restart {
                                if let Some(manager) = &mut r_manager {
                                    manager.send_command(&format!("cat('# STEP_END: {}\\n')", step.name))
                                        .map_err(|e| anyhow::anyhow!("Failed to write restart step end marker: {}", e))?;
                                }
                                "R process restarted".to_string()
                            } else {
                                "R process started".to_string()
                            }
                        } else {
                            // Execute R script or command
                            if let Some(manager) = &mut r_manager {
                                // Check if R process is still alive
                                if !manager.is_alive()? {
                                    return Err(anyhow::anyhow!("R process died unexpectedly during step '{}'", step.name));
                                }
                                
                                // First, add a marker for the startup step if this is the first command
                                let r_steps_so_far = step_results.len();
                                
                                if r_steps_so_far == 1 {
                                    // This is the first command after R startup, add startup marker
                                    manager.send_command("# R startup complete")
                                        .map_err(|e| anyhow::anyhow!("Failed to write startup comment: {}", e))?;
                                    manager.send_command(&format!("cat('# STEP_END: start R\\n')"))
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
                                manager.send_command(&format!("cat('# STEP_END: {}\\n')", step.name))
                                    .map_err(|e| anyhow::anyhow!("Failed to write step end marker for '{}': {}", step.name, e))?;
                                
                                println!("   ├─ Command sent (will wait at completion barrier)");
                                
                                "Command executed".to_string()
                            } else {
                                return Err(anyhow::anyhow!("R process not started for step '{}'", step.name));
                            }
                        }
                    },
                    _ => return Err(anyhow::anyhow!("Unknown thread type: {}", thread_name_clone)),
                };
                
                // Store step result
                let step_result = StepResult {
                    name: step.name.clone(),
                    step_index: step_idx,
                    output,
                };
                step_results.push(step_result);
                
                // Don't fail fast - collect all results first
                
                // Wait for all threads to complete their commands before moving to next step
                thread_completion_barriers[step_idx].wait();
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
                            step_result.output = step_output.clone();
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
    for (thread_name, rx) in rx_map {
        let thread_output = rx.recv().map_err(|e| anyhow::anyhow!("Failed to receive output from {}: {}", thread_name, e))?;
        all_thread_outputs.push(thread_output);
    }
    
    // Now check all assertions after we have all outputs
    let mut assertion_failures = Vec::new();
    
    // Check assertions and collect failures
    for thread_output in &all_thread_outputs {
        for step_result in &thread_output.step_results {
            // Find the original step by index to get its assertion
            if let Some(original_step) = workflow.test.steps.get(step_result.step_index) {
                if let Some(assertion) = &original_step.assert {
                    if let Err(e) = check_assertion(assertion, &step_result.output) {
                        assertion_failures.push((step_result.name.clone(), e.to_string(), step_result.output.clone()));
                    }
                }
            }
        }
    }
    
    // Print final results organized by thread
    println!("\n📊 Final Results:");
    for thread_output in &all_thread_outputs {
        println!("  {} thread:", thread_output.thread_name.to_uppercase());
        for step_result in &thread_output.step_results {
            let has_assertion = workflow.test.steps.get(step_result.step_index)
                .map(|s| s.assert.is_some())
                .unwrap_or(false);
            
            if has_assertion {
                let failed = assertion_failures.iter().any(|(name, _, _)| name == &step_result.name);
                let status = if failed { "❌ FAIL" } else { "✅ PASS" };
                println!("   • {} - {} ({} chars)", step_result.name, status, step_result.output.len());
            } else {
                println!("   • {} - ⏭️ NO ASSERTION ({} chars)", step_result.name, step_result.output.len());
            }
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
        return Err(anyhow::anyhow!("Test failed with {} assertion failures", assertion_failures.len()));
    }
    
    Ok(())
}

fn execute_rv_command(command: &str, test_dir: &Path, config_path: &Path) -> Result<String> {
    let (cmd, args) = match command {
        "rv init" => ("init", vec![]),
        "rv sync" => ("sync", vec![]),
        "rv plan" => ("plan", vec![]),
        cmd if cmd.starts_with("rv ") => {
            let parts: Vec<&str> = cmd.split_whitespace().skip(1).collect();
            if parts.is_empty() {
                return Err(anyhow::anyhow!("Invalid rv command: {}", command));
            }
            (parts[0], parts[1..].to_vec())
        }
        _ => return Err(anyhow::anyhow!("Unknown rv command: {}", command)),
    };
    
    let output = Command::cargo_bin(RV)
        .map_err(|e| anyhow::anyhow!("Failed to find rv binary: {}", e))?
        .arg(cmd)
        .args(args)
        .current_dir(test_dir)
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to execute rv {}: {}", cmd, e))?;
    
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
            .map_err(|e| anyhow::anyhow!("Failed to copy config file: {}", e))?;
    }
    
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn check_assertion(assertion: &TestAssertion, output: &str) -> Result<()> {
    match assertion {
        TestAssertion::Single(expected) => {
            if !output.contains(expected) {
                return Err(anyhow::anyhow!(
                    "Assertion failed: expected '{}' in output.\n\nFull output ({} chars):\n{}\n\nSearching for lines containing '{}':\n{}", 
                    expected, 
                    output.len(),
                    output,
                    expected.split(':').next().unwrap_or(expected),
                    output.lines()
                        .filter(|line| line.contains(expected.split(':').next().unwrap_or(expected)))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
        },
        TestAssertion::Multiple(expected_list) => {
            for expected in expected_list.iter() {
                if !output.contains(expected) {
                    return Err(anyhow::anyhow!(
                        "Assertion failed: expected '{}' in output.\n\nFull output ({} chars):\n{}", 
                        expected, 
                        output.len(),
                        output
                    ));
                }
            }
        },
    }
    Ok(())
}


#[test]
fn test_all_workflow_files() -> Result<()> {
    run_workflow_tests(None)
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
                let file_name = path.file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if !file_name.contains(filter_str) {
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
    println!("{}", "═".repeat(80));
    
    for workflow_file in workflow_files {
        let file_name = workflow_file.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        
        println!("\n🧪 Running workflow test: {}", file_name);
        println!("📁 Loading workflow from: {}", workflow_file.display());
        
        let workflow_content = fs::read_to_string(&workflow_file)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", workflow_file.display(), e))?;
        
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
                println!("{}", "─".repeat(80));
            },
            Err(e) => {
                eprintln!("💥 {} failed: {}", file_name, e);
                return Err(e);
            }
        }
    }
    
    println!("\n🏁 All {} workflow tests completed successfully!", num_workflow_files);
    println!("{}", "═".repeat(80));
    
    Ok(())
}


