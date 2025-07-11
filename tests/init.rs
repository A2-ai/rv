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
    assertion_passed: Option<bool>,
    assertion_error: Option<String>,
}

#[derive(Debug)]
struct ThreadOutput {
    thread_name: String,
    step_results: Vec<StepResult>,
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
            let mut r_process: Option<std::process::Child> = None;
            let mut r_stdin: Option<std::process::ChildStdin> = None;
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
                
                let (output, assertion_passed, assertion_error) = match thread_name_clone.as_str() {
                    "rv" => {
                        // Handle rv commands
                        let result = execute_rv_command(&step.run, &thread_test_dir, &thread_config_path)?;
                        if !result.trim().is_empty() {
                            println!("   ├─ Output: {}", result.trim());
                        }
                        
                        // Store assertion for checking at the end
                        let (assertion_passed, assertion_error) = (None, None);
                        
                        (result, assertion_passed, assertion_error)
                    },
                    "r" => {
                        // Handle R commands
                        if step.run == "R" {
                            // Check if this is a restart
                            if step.restart && r_process.is_some() {
                                // Restart: capture existing output first, then start new process
                                if let (Some(mut stdin), Some(process)) = (r_stdin.take(), r_process.take()) {
                                    writeln!(stdin, "quit(save = 'no')").ok();
                                    let final_output = process.wait_with_output()
                                        .map_err(|e| anyhow::anyhow!("Failed to wait for R process during restart: {}", e))?;
                                    
                                    // Accumulate the output from the previous session (both stdout and stderr)
                                    accumulated_r_output.push_str(&String::from_utf8_lossy(&final_output.stdout));
                                    
                                    let prev_stderr = String::from_utf8_lossy(&final_output.stderr);
                                    if !prev_stderr.is_empty() {
                                        accumulated_r_output.push_str("\n# === STDERR OUTPUT ===\n");
                                        accumulated_r_output.push_str(&prev_stderr);
                                    }
                                    
                                    accumulated_r_output.push_str("\n# === R PROCESS RESTARTED ===\n");
                                }
                            }
                            
                            // Start (or restart) R process
                            let mut process = std::process::Command::new("R")
                                .args(["--interactive"])
                                .current_dir(&thread_test_dir)
                                .stdin(std::process::Stdio::piped())
                                .stdout(std::process::Stdio::piped())
                                .stderr(std::process::Stdio::piped())
                                .spawn()
                                .expect("Failed to start R process");
                            
                            let stdin = process.stdin.take().expect("Failed to get R stdin");
                            
                            r_stdin = Some(stdin);
                            r_process = Some(process);
                            
                            // If this is a restart, we need to add a step end marker
                            // after the new R process starts up, so the parser can capture the startup output
                            if step.restart {
                                // Add step end marker for the restart step to capture R startup output
                                if let Some(stdin) = &mut r_stdin {
                                    writeln!(stdin, "cat('# STEP_END: {}\\n')", step.name)
                                        .map_err(|e| anyhow::anyhow!("Failed to write restart step end marker: {}", e))?;
                                    
                                    use std::io::Write;
                                    stdin.flush().map_err(|e| anyhow::anyhow!("Failed to flush R stdin after restart: {}", e))?;
                                }
                                ("R process restarted".to_string(), None, None)
                            } else {
                                ("R process started".to_string(), None, None)
                            }
                        } else {
                            // Execute R script or command
                            if let (Some(stdin), Some(process)) = (&mut r_stdin, &mut r_process) {
                                // Check if R process is still alive
                                match process.try_wait() {
                                    Ok(Some(exit_status)) => {
                                        return Err(anyhow::anyhow!("R process exited with status: {}", exit_status));
                                    },
                                    Ok(None) => {
                                        // Process is still running, continue
                                    },
                                    Err(e) => {
                                        return Err(anyhow::anyhow!("Failed to check R process status: {}", e));
                                    }
                                }
                                
                                // First, add a marker for the startup step if this is the first command
                                // (We can tell by checking if there are any existing step results for R thread)
                                let r_steps_so_far = step_results.len();
                                
                                if r_steps_so_far == 1 {
                                    // This is the first command after R startup, add startup marker
                                    writeln!(stdin, "cat('# STEP_END: start R\\n')")
                                        .map_err(|e| anyhow::anyhow!("Failed to write startup marker: {}", e))?;
                                }
                                
                                // Execute the step
                                if step.run.ends_with(".R") {
                                    let script_content = load_r_script(&step.run)?;
                                    writeln!(stdin, "{}", script_content)
                                        .map_err(|e| anyhow::anyhow!("Failed to write R script: {}", e))?;
                                } else {
                                    writeln!(stdin, "{}", step.run)
                                        .map_err(|e| anyhow::anyhow!("Failed to write R command: {}", e))?;
                                }
                                
                                // Add step end marker after the command
                                writeln!(stdin, "cat('# STEP_END: {}\\n')", step.name)
                                    .map_err(|e| anyhow::anyhow!("Failed to write step end marker: {}", e))?;
                                
                                // Force flush of stdin to ensure R gets the commands
                                use std::io::Write;
                                stdin.flush().map_err(|e| anyhow::anyhow!("Failed to flush R stdin: {}", e))?;
                                
                                println!("   ├─ Command sent (will wait at completion barrier)");
                                
                                // We'll check assertions after capturing all output at the end
                                ("Command executed".to_string(), None, None)
                            } else {
                                return Err(anyhow::anyhow!("R process not started"));
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
                    assertion_passed,
                    assertion_error: assertion_error.clone(),
                };
                step_results.push(step_result);
                
                // Don't fail fast - collect all results first
                
                // Wait for all threads to complete their commands before moving to next step
                thread_completion_barriers[step_idx].wait();
            }
            
            // Clean up R process if it exists and capture all output
            if thread_name_clone == "r" {
                if let (Some(mut stdin), Some(process)) = (r_stdin, r_process) {
                    writeln!(stdin, "quit(save = 'no')").ok();
                    let final_output = process.wait_with_output()
                        .map_err(|e| anyhow::anyhow!("Failed to wait for R process: {}", e))?;
                    
                    // Combine accumulated output with final output (both stdout and stderr)
                    accumulated_r_output.push_str(&String::from_utf8_lossy(&final_output.stdout));
                    
                    let final_stderr = String::from_utf8_lossy(&final_output.stderr);
                    if !final_stderr.is_empty() {
                        accumulated_r_output.push_str("\n# === STDERR OUTPUT ===\n");
                        accumulated_r_output.push_str(&final_stderr);
                    }
                    
                    let full_r_stderr = final_stderr;
                    
                    println!("{}", "=".repeat(80));
                    println!("COMPLETE R STDOUT ({} bytes total):", accumulated_r_output.len());
                    println!("{}", accumulated_r_output);
                    println!("{}", "=".repeat(80));
                    println!("COMPLETE R STDERR ({} bytes):", final_output.stderr.len());
                    println!("{}", full_r_stderr);
                    println!("{}", "=".repeat(80));
                    
                    // Extract R step names from our step results
                    let r_step_names: Vec<String> = step_results.iter()
                        .map(|sr| sr.name.clone())
                        .collect();
                    
                    // Parse the complete output to extract per-step outputs
                    let parsed_outputs = parse_r_step_outputs(&accumulated_r_output, &r_step_names);
                    
                    println!("🔍 Parsing R session output into {} steps", r_step_names.len());
                    
                    // Update step results with actual outputs (assertions checked at end)
                    for step_result in &mut step_results {
                        if let Some(step_output) = parsed_outputs.get(&step_result.name) {
                            step_result.output = step_output.clone();
                            println!("   ├─ Captured '{}' output: {} chars", step_result.name, step_output.len());
                        } else {
                            println!("   ⚠️ No output found for step '{}'", step_result.name);
                        }
                    }
                }
            }
            
            // Send results through channel
            let thread_output = ThreadOutput {
                thread_name: thread_name_clone.clone(),
                step_results,
            };
            thread_tx.send(thread_output).unwrap();
            Ok(())
        });
        
        thread_handles.insert(thread_name, handle);
    }
    
    // Wait for all threads to complete
    for (thread_name, handle) in thread_handles {
        handle.join().map_err(|_| anyhow::anyhow!("Thread {} panicked", thread_name))??;
    }
    
    // Collect step results from all threads
    let mut all_thread_outputs = Vec::new();
    for (thread_name, rx) in rx_map {
        let thread_output = rx.recv().map_err(|e| anyhow::anyhow!("Failed to receive output from {}: {}", thread_name, e))?;
        println!("✅ {} thread completed successfully", thread_name.to_uppercase());
        all_thread_outputs.push(thread_output);
    }
    
    // Now check all assertions after we have all outputs
    println!("\n🔍 Checking all assertions...");
    let mut assertion_failures = Vec::new();
    
    for thread_output in &all_thread_outputs {
        for step_result in &thread_output.step_results {
            // Find the original step by index to get its assertion
            if let Some(original_step) = workflow.test.steps.get(step_result.step_index) {
                if let Some(assertion) = &original_step.assert {
                    match assertion {
                        TestAssertion::Single(s) => {
                            print!("   ├─ Checking '{}' single assertion... ", step_result.name);
                            println!("      Content: '{}'", s);
                        },
                        TestAssertion::Multiple(list) => {
                            print!("   ├─ Checking '{}' multiple assertion ({} items)... ", step_result.name, list.len());
                            for (i, item) in list.iter().enumerate() {
                                println!("      {}: '{}'", i + 1, item);
                            }
                        },
                    }
                    
                    match check_assertion(assertion, &step_result.output) {
                        Ok(()) => {
                            println!("✅ passed");
                        },
                        Err(e) => {
                            println!("❌ failed");
                            assertion_failures.push((step_result.name.clone(), e.to_string(), step_result.output.clone()));
                        }
                    }
                } else {
                    println!("   ├─ '{}' - ⏭️ no assertion", step_result.name);
                }
            }
        }
    }
    
    // Print step results summary
    println!("\n📊 Final step results:");
    for thread_output in &all_thread_outputs {
        println!("  {} thread:", thread_output.thread_name.to_uppercase());
        for step_result in &thread_output.step_results {
            let has_assertion = workflow.test.steps.get(step_result.step_index)
                .map(|s| s.assert.is_some())
                .unwrap_or(false);
            
            if has_assertion {
                let failed = assertion_failures.iter().any(|(name, _, _)| name == &step_result.name);
                let status = if failed { "❌ FAIL" } else { "✅ PASS" };
                println!("   • {} - {} (output: {} chars)", step_result.name, status, step_result.output.len());
            } else {
                println!("   • {} - ⏭️ NO ASSERTION (output: {} chars)", step_result.name, step_result.output.len());
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
    match command {
        "rv init" => {
            let output = Command::cargo_bin(RV)?
                .arg("init")
                .current_dir(test_dir)
                .output()?;
            if !output.status.success() {
                return Err(anyhow::anyhow!("rv init failed"));
            }
            
            // Copy config if needed
            if config_path.exists() {
                fs::copy(config_path, test_dir.join("rproject.toml"))?;
            }
            
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        },
        "rv sync" => {
            let output = Command::cargo_bin(RV)?
                .arg("sync")
                .current_dir(test_dir)
                .output()?;
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        },
        "rv plan" => {
            let output = Command::cargo_bin(RV)?
                .arg("plan")
                .current_dir(test_dir)
                .output()?;
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        },
        _ => Err(anyhow::anyhow!("Unknown rv command: {}", command))?,
    }
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
            println!("      Checking {} assertions:", expected_list.len());
            for (i, expected) in expected_list.iter().enumerate() {
                println!("        {} - Checking for: '{}'", i + 1, expected);
                if !output.contains(expected) {
                    println!("        ❌ NOT FOUND");
                    return Err(anyhow::anyhow!(
                        "Assertion failed: expected '{}' in output.\n\nFull output ({} chars):\n{}", 
                        expected, 
                        output.len(),
                        output
                    ));
                } else {
                    println!("        ✅ FOUND");
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


