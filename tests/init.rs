use anyhow::Result;
use assert_cmd::Command;
use tempfile::TempDir;
use std::thread;
use std::time::Duration;
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
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum TestAssertion {
    Single(String),
    Multiple(Vec<String>),
}

fn load_r_script(script_name: &str) -> Result<String> {
    let script_path = format!("tests/input/r_scripts/{}", script_name);
    fs::read_to_string(&script_path)
        .map_err(|e| anyhow::anyhow!("Failed to load R script {}: {}", script_path, e))
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
    let num_threads = thread_steps.len();
    let barriers: Vec<Arc<Barrier>> = (0..workflow.test.steps.len())
        .map(|_| Arc::new(Barrier::new(num_threads)))
        .collect();
    
    // Channels for collecting outputs from each thread
    let (tx_map, rx_map): (HashMap<String, _>, HashMap<String, _>) = thread_steps.keys()
        .map(|thread_name| {
            let (tx, rx) = std::sync::mpsc::channel();
            ((thread_name.clone(), tx), (thread_name.clone(), rx))
        })
        .unzip();
    
    // Spawn threads
    let mut thread_handles = HashMap::new();
    
    for (thread_name, step_indices) in thread_steps {
        let thread_barriers = barriers.clone();
        let thread_steps = workflow.test.steps.clone();
        let thread_test_dir = test_dir.clone();
        let thread_config_path = config_path.clone();
        let thread_tx = tx_map[&thread_name].clone();
        let thread_name_clone = thread_name.clone();
        
        let handle = thread::spawn(move || -> Result<String> {
            let mut outputs = Vec::new();
            let mut r_process = None;
            let mut r_stdin = None;
            
            // Process all steps in order, participating in barriers even if not executing
            for step_idx in 0..thread_steps.len() {
                // Wait for this step's turn
                thread_barriers[step_idx].wait();
                
                // Only execute if this step belongs to our thread
                if !step_indices.contains(&step_idx) {
                    continue;
                }
                
                let step = &thread_steps[step_idx];
                
                println!("🟡 {}: {}", thread_name_clone.to_uppercase(), step.name);
                
                // Show what command is being executed
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
                            // Start R process - try interactive mode first, fallback to slave mode
                            let mut process = std::process::Command::new("R")
                                .args(["--interactive", "--no-restore", "--no-save"])
                                .current_dir(&thread_test_dir)
                                .stdin(std::process::Stdio::piped())
                                .stdout(std::process::Stdio::piped())
                                .stderr(std::process::Stdio::piped())
                                .spawn()
                                .expect("Failed to start R process");
                            
                            r_stdin = Some(process.stdin.take().expect("Failed to get R stdin"));
                            r_process = Some(process);
                            
                            // Give R time to start up properly
                            thread::sleep(Duration::from_millis(2000));
                            "".to_string()
                        } else if step.run.ends_with(".R") {
                            // Execute R script
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
                                
                                let script_content = load_r_script(&step.run)?;
                                writeln!(stdin, "{}", script_content)
                                    .map_err(|e| anyhow::anyhow!("Failed to write R script (process may have died): {}", e))?;
                                writeln!(stdin, "flush.console()").ok(); // Force output
                                thread::sleep(Duration::from_millis(1000));
                                "R_SCRIPT_EXECUTED".to_string() // Placeholder - real output captured at end
                            } else {
                                return Err(anyhow::anyhow!("R process not started"));
                            }
                        } else {
                            // Direct R command
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
                                
                                writeln!(stdin, "{}", step.run)
                                    .map_err(|e| anyhow::anyhow!("Failed to write R command (process may have died): {}", e))?;
                                writeln!(stdin, "flush.console()").ok(); // Force output
                                thread::sleep(Duration::from_millis(1000));
                                "R_COMMAND_EXECUTED".to_string() // Placeholder - real output captured at end
                            } else {
                                return Err(anyhow::anyhow!("R process not started"));
                            }
                        }
                    },
                    _ => return Err(anyhow::anyhow!("Unknown thread type: {}", thread_name_clone)),
                };
                
                outputs.push(format!("{}: {}", step.name, output));
                
                // For R steps, store assertions to check against final output
                // For rv steps, check assertions immediately
                if let Some(assertion) = &step.assert {
                    if thread_name_clone == "rv" {
                        println!("   ├─ Checking assertion...");
                        match check_assertion(assertion, &output) {
                            Ok(()) => println!("   └─ ✅ Assertion passed"),
                            Err(e) => {
                                println!("   └─ ❌ Assertion failed");
                                return Err(e);
                            }
                        }
                    } else {
                        println!("   └─ ⏳ Assertion will be checked after R completes");
                    }
                }
            }
            
            // If this is R thread, finish the process and get final output
            if thread_name_clone == "r" {
                if let (Some(mut stdin), Some(process)) = (r_stdin, r_process) {
                    writeln!(stdin, "quit(save = 'no')")
                        .map_err(|e| anyhow::anyhow!("Failed to quit R: {}", e))?;
                    let final_output = process.wait_with_output()
                        .map_err(|e| anyhow::anyhow!("Failed to wait for R process: {}", e))?;
                    let r_stdout = String::from_utf8_lossy(&final_output.stdout);
                    outputs.push(format!("R_FINAL_OUTPUT: {}", r_stdout));
                    
                    // Check R assertions against final output
                    println!("🔍 Checking R assertions against final output...");
                    for &step_idx in &step_indices {
                        let step = &thread_steps[step_idx];
                        if let Some(assertion) = &step.assert {
                            print!("   ├─ Checking '{}' assertion... ", step.name);
                            match check_assertion(assertion, &r_stdout) {
                                Ok(()) => println!("✅ passed"),
                                Err(e) => {
                                    println!("❌ failed");
                                    return Err(e);
                                }
                            }
                        }
                    }
                }
            }
            
            let combined_output = outputs.join("\n");
            thread_tx.send(combined_output.clone()).unwrap();
            Ok(combined_output)
        });
        
        thread_handles.insert(thread_name, handle);
    }
    
    // Wait for all threads to complete
    for (thread_name, handle) in thread_handles {
        handle.join().map_err(|_| anyhow::anyhow!("Thread {} panicked", thread_name))??;
    }
    
    // Collect outputs from all threads (but don't print them as they're already shown inline)
    for (thread_name, rx) in rx_map {
        let _output = rx.recv().map_err(|e| anyhow::anyhow!("Failed to receive output from {}: {}", thread_name, e))?;
        println!("✅ {} thread completed successfully", thread_name.to_uppercase());
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
            for expected in expected_list {
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

#[test] 
fn test_debug_workflow_only() -> Result<()> {
    run_workflow_tests(Some("debug"))
}

#[test]
fn test_simple_workflow_only() -> Result<()> {
    run_workflow_tests(Some("simple"))
}

#[test]
fn test_r6_loading_workflow() -> Result<()> {
    run_workflow_tests(Some("test_r6_loading"))
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


