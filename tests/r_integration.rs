use anyhow::Result;
use fs_err as fs;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use tempfile::TempDir;

mod integration;
use integration::*;

fn debug_print(msg: &str) {
    if std::env::var("RV_TEST_DEBUG").is_ok() {
        println!("🐛 DEBUG: {}", msg);
    }
}

fn setup_test_environment(
    workflow: &WorkflowTest,
) -> Result<(TempDir, std::path::PathBuf, std::path::PathBuf)> {
    let temp_dir = TempDir::new()?;
    let project_path = temp_dir.path();
    let test_dir = project_path.join(&workflow.project_dir);

    // Create test directory
    fs::create_dir(&test_dir)?;

    // Get absolute path to config file
    let config_path = std::env::current_dir()?
        .join("tests/input")
        .join(&workflow.config);

    Ok((temp_dir, test_dir, config_path))
}

fn prepare_thread_coordination(
    workflow: &WorkflowTest,
) -> Result<(
    HashMap<String, Vec<usize>>,
    Arc<StepCoordinator>,
    HashMap<String, std::sync::mpsc::Sender<ThreadOutput>>,
    HashMap<String, std::sync::mpsc::Receiver<ThreadOutput>>,
)> {
    // Count unique threads to set up coordination
    let mut thread_steps: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, step) in workflow.test.steps.iter().enumerate() {
        thread_steps.entry(step.thread.clone()).or_default().push(i);
    }

    // Create StepCoordinator
    let thread_names: Vec<String> = thread_steps.keys().cloned().collect();
    let coordinator = Arc::new(StepCoordinator::new(
        thread_names.clone(),
        workflow.test.steps.len(),
    ));

    // Channels for collecting step results from each thread
    let (tx_map, rx_map): (HashMap<String, _>, HashMap<String, _>) = thread_steps
        .keys()
        .map(|thread_name| {
            let (tx, rx) = std::sync::mpsc::channel::<ThreadOutput>();
            ((thread_name.clone(), tx), (thread_name.clone(), rx))
        })
        .unzip();

    Ok((thread_steps, coordinator, tx_map, rx_map))
}

fn run_workflow_test(workflow_yaml: &str) -> Result<()> {
    let workflow: WorkflowTest = serde_yaml::from_str(workflow_yaml)?;

    let (_temp_dir, test_dir, config_path) = setup_test_environment(&workflow)?;
    let (thread_steps, coordinator, tx_map, rx_map) = prepare_thread_coordination(&workflow)?;

    let (all_thread_outputs, thread_errors) = execute_workflow_threads(
        &workflow,
        &test_dir,
        &config_path,
        thread_steps,
        coordinator,
        tx_map,
        rx_map,
    )?;

    validate_and_report_results(&workflow, all_thread_outputs, thread_errors)
}

fn execute_workflow_threads(
    workflow: &WorkflowTest,
    test_dir: &std::path::Path,
    config_path: &std::path::Path,
    thread_steps: HashMap<String, Vec<usize>>,
    coordinator: Arc<StepCoordinator>,
    tx_map: HashMap<String, std::sync::mpsc::Sender<ThreadOutput>>,
    rx_map: HashMap<String, std::sync::mpsc::Receiver<ThreadOutput>>,
) -> Result<(Vec<ThreadOutput>, Vec<String>)> {
    // Spawn threads
    let mut thread_handles = HashMap::new();

    for (thread_name, step_indices) in thread_steps {
        let thread_coordinator = coordinator.clone();
        let thread_steps = workflow.test.steps.clone();
        let thread_test_dir = test_dir.to_path_buf();
        let thread_config_path = config_path.to_path_buf();
        let thread_tx = tx_map[&thread_name].clone();
        let thread_name_clone = thread_name.clone();

        let handle = thread::spawn(move || {
            let mut step_results = Vec::new();
            let mut r_manager: Option<RProcessManager> = None;
            let mut accumulated_r_output = String::new();
            let mut r_exit_status: Option<std::process::ExitStatus> = None;
            let mut thread_failure: Option<String> = None;

            // Execute thread logic and capture any errors
            let thread_result = (|| -> Result<()> {
                // Process all steps in order, coordinating with other threads
                for step_idx in 0..thread_steps.len() {
                    // Wait for this step to start (simple coordination without timeout)
                    thread_coordinator.wait_for_step_start(step_idx, &thread_name_clone, None)?;

                    // Only execute if this step belongs to our thread
                    if !step_indices.contains(&step_idx) {
                        // Even if we don't execute, we must notify completion
                        thread_coordinator
                            .notify_step_completed(step_idx, &thread_name_clone)
                            .unwrap_or_else(|e| {
                                debug_print(&format!("Failed to notify completion: {}", e))
                            });
                        continue;
                    }

                    let step = &thread_steps[step_idx];

                    println!("🟡 {}: {}", thread_name_clone.to_uppercase(), step.name);
                    println!("   └─ Running: {}", step.run);
                    if let Some(timeout) = step.timeout {
                        println!("   └─ Timeout: {}s", timeout);
                    }

                    let output = if thread_coordinator.is_aborted() {
                        // Skip actual work if abort is signaled, just return empty output
                        ("".to_string(), None)
                    } else {
                        match thread_name_clone.as_str() {
                        "rv" => {
                            // Handle rv commands with original timeout mechanism
                            execute_with_timeout(&step.name, step.timeout, || {
                                let (output, exit_status) = execute_rv_command(
                                    &step.run,
                                    &thread_test_dir,
                                    &thread_config_path,
                                )?;
                                if !output.trim().is_empty() {
                                    println!("   ├─ Output: {}", output.trim());
                                }
                                Ok((output, Some(exit_status)))
                            })?
                        }
                        "r" => {
                            // Handle R commands - wrap strings in tuples for consistency
                            let r_output = if step.run == "R" {
                                // Check if this is a restart
                                if step.restart {
                                    if let Some(manager) = r_manager.take() {
                                        // Capture output from previous session
                                        let (prev_stdout, prev_stderr, _prev_exit_status) =
                                        manager.shutdown_and_capture_output().map_err(|e| {
                                            anyhow::anyhow!(
                                                "Failed to shutdown R process during restart: {}",
                                                e
                                            )
                                        })?;

                                        // Accumulate the output from the previous session
                                        accumulated_r_output.push_str(&prev_stdout);

                                        if !prev_stderr.is_empty() {
                                            accumulated_r_output
                                                .push_str("\n# === STDERR OUTPUT ===\n");
                                            accumulated_r_output
                                                .push_str(&String::from_utf8_lossy(&prev_stderr));
                                        }

                                        accumulated_r_output
                                            .push_str("\n# === R PROCESS RESTARTED ===\n");
                                    }
                                }

                                // Start (or restart) R process
                                r_manager = Some(
                                    RProcessManager::start_r_process(&thread_test_dir).map_err(
                                        |e| {
                                            anyhow::anyhow!(
                                                "Failed to start R process for step '{}': {}",
                                                step.name,
                                                e
                                            )
                                        },
                                    )?,
                                );

                                // If this is a restart, add a step end marker
                                if step.restart {
                                    if let Some(manager) = &mut r_manager {
                                        manager
                                            .send_command(&format!(
                                                "cat('# STEP_END: {}\\n')",
                                                step.name
                                            ))
                                            .map_err(|e| {
                                                anyhow::anyhow!(
                                                    "Failed to write restart step end marker: {}",
                                                    e
                                                )
                                            })?;
                                    }
                                    "R process restarted".to_string()
                                } else {
                                    "R process started".to_string()
                                }
                            } else {
                                // Execute R script or command with timeout
                                execute_r_command_with_timeout(
                                    &step.name,
                                    step.timeout,
                                    &mut r_manager,
                                    |manager| {
                                        // Check if R process is still alive
                                        if !manager.is_alive()? {
                                            debug_print(&format!(
                                                "R process died during step '{}'",
                                                step.name
                                            ));
                                            return Err(anyhow::anyhow!(
                                                "R process died unexpectedly during step '{}'",
                                                step.name
                                            ));
                                        }

                                        // Debug: Pause to let R process commands
                                        manager.debug_pause_after_command();

                                        // First, add a marker for the startup step if this is the first command
                                        let r_steps_so_far = step_results.len();

                                        if r_steps_so_far == 1 {
                                            // This is the first command after R startup, add startup marker
                                            manager.send_command("# R startup complete").map_err(
                                                |e| {
                                                    anyhow::anyhow!(
                                                        "Failed to write startup comment: {}",
                                                        e
                                                    )
                                                },
                                            )?;
                                            manager
                                                .send_command(&format!(
                                                    "cat('# STEP_END: start R\\n')"
                                                ))
                                                .map_err(|e| {
                                                    anyhow::anyhow!(
                                                        "Failed to write startup marker: {}",
                                                        e
                                                    )
                                                })?;
                                        }

                                        // Execute the step
                                        if step.run.ends_with(".R") {
                                            let script_content =
                                                load_r_script(&step.run).map_err(|e| {
                                                    anyhow::anyhow!(
                                                        "Failed to load R script for step '{}': {}",
                                                        step.name,
                                                        e
                                                    )
                                                })?;
                                            manager.send_command(&script_content).map_err(|e| {
                                                anyhow::anyhow!(
                                                    "Failed to send R script for step '{}': {}",
                                                    step.name,
                                                    e
                                                )
                                            })?;
                                        } else {
                                            manager.send_command(&step.run).map_err(|e| {
                                                anyhow::anyhow!(
                                                    "Failed to send R command for step '{}': {}",
                                                    step.name,
                                                    e
                                                )
                                            })?;
                                        }

                                        // Add step end marker after the command
                                        manager
                                            .send_command(&format!(
                                                "cat('# STEP_END: {}\\n')",
                                                step.name
                                            ))
                                            .map_err(|e| {
                                                anyhow::anyhow!(
                                                    "Failed to write step end marker for '{}': {}",
                                                    step.name,
                                                    e
                                                )
                                            })?;

                                        // Debug: Pause after sending commands
                                        manager.debug_pause_after_command();

                                        println!("   ├─ Command sent");

                                        Ok("Command executed".to_string())
                                    },
                                )?
                            };
                            // Wrap R output with R exit status for consistency
                            (r_output, r_exit_status)
                        }
                        _ => {
                            return Err(anyhow::anyhow!(
                                "Unknown thread type: {}",
                                thread_name_clone
                            ));
                        }
                        }
                    };

                    // Store step result
                    let (output, exit_status) = output;
                    let step_result = StepResult {
                        name: step.name.clone(),
                        step_index: step_idx,
                        output,
                        exit_status,
                    };

                    // Check exit status - signal abort but continue to avoid deadlock
                    if let Some(exit_status) = &step_result.exit_status {
                        if !exit_status.success() && thread_failure.is_none() {
                            // Signal abort to all other threads
                            thread_coordinator.signal_abort();
                            
                            // Store the failure but continue to avoid barrier deadlock
                            thread_failure = Some(format!(
                                "Step '{}' failed with non-zero exit code: {}\n\nOutput:\n{}",
                                step_result.name,
                                exit_status.code().unwrap_or(-1),
                                step_result.output
                            ));
                        }
                    }

                    step_results.push(step_result);

                    // Notify completion to coordinator
                    thread_coordinator
                        .notify_step_completed(step_idx, &thread_name_clone)
                        .map_err(|e| anyhow::anyhow!("Failed to notify step completion: {}", e))?;
                }

                // Clean up R process if it exists and capture all output
                if thread_name_clone == "r" {
                    if let Some(manager) = r_manager {
                        let (final_stdout, final_stderr, final_exit_status) =
                            manager.shutdown_and_capture_output().map_err(|e| {
                                anyhow::anyhow!(
                                    "Failed to shutdown R process for thread cleanup: {}",
                                    e
                                )
                            })?;

                        // Store the R process exit status for StepResult creation
                        r_exit_status = final_exit_status;

                        // Combine accumulated output with final output
                        accumulated_r_output.push_str(&final_stdout);

                        if !final_stderr.is_empty() {
                            accumulated_r_output.push_str("\n# === STDERR OUTPUT ===\n");
                            accumulated_r_output.push_str(&String::from_utf8_lossy(&final_stderr));
                        }

                        // Extract R step names from our step results
                        let r_step_names: Vec<String> =
                            step_results.iter().map(|sr| sr.name.clone()).collect();

                        // Parse the complete output to extract per-step outputs
                        let parsed_outputs =
                            parse_r_step_outputs(&accumulated_r_output, &r_step_names);

                        // Update step results with actual outputs (assertions checked at end)
                        for step_result in &mut step_results {
                            if let Some(step_output) = parsed_outputs.get(&step_result.name) {
                                step_result.output = step_output.clone();
                            }
                        }
                    }
                }

                // Return any stored failure
                if let Some(failure_msg) = thread_failure {
                    Err(anyhow::anyhow!(failure_msg))
                } else {
                    Ok(())
                }
            })();

            // Always send results through channel, even if there were errors
            let thread_output = ThreadOutput {
                thread_name: thread_name_clone.clone(),
                step_results,
            };

            // Send the output (ignore send errors since main thread may have exited)
            let _ = thread_tx.send(thread_output);

            // Return the result of thread execution
            thread_result
        });

        thread_handles.insert(thread_name, handle);
    }

    // Wait for all threads to complete and collect any errors
    let mut thread_errors = Vec::new();
    for (thread_name, handle) in thread_handles {
        match handle.join() {
            Ok(thread_result) => {
                if let Err(e) = thread_result {
                    thread_errors.push(format!("Thread '{}' failed: {}", thread_name, e));
                }
            }
            Err(_) => {
                thread_errors.push(format!(
                    "Thread '{}' panicked during execution",
                    thread_name
                ));
            }
        }
    }

    // Collect step results from all threads (even if some failed)
    let mut all_thread_outputs = Vec::new();
    for (thread_name, rx) in rx_map {
        match rx.recv() {
            Ok(thread_output) => {
                all_thread_outputs.push(thread_output);
            }
            Err(e) => {
                // Thread failed before sending output, but continue collecting others
                debug_print(&format!(
                    "Failed to receive output from {}: {}",
                    thread_name, e
                ));
            }
        }
    }

    // Return both outputs and any errors
    Ok((all_thread_outputs, thread_errors))
}

fn validate_and_report_results(
    workflow: &WorkflowTest,
    all_thread_outputs: Vec<ThreadOutput>,
    thread_errors: Vec<String>,
) -> Result<()> {
    // Now check all assertions after we have all outputs
    let mut assertion_failures = Vec::new();

    // Check assertions and collect failures
    for thread_output in &all_thread_outputs {
        for step_result in &thread_output.step_results {
            // Find the original step by index to get its assertion
            if let Some(original_step) = workflow.test.steps.get(step_result.step_index) {
                // Check exit status first - fail if non-zero (unless configured otherwise in future)
                if let Some(exit_status) = &step_result.exit_status {
                    if !exit_status.success() {
                        assertion_failures.push((
                            step_result.name.clone(),
                            format!(
                                "Step failed with non-zero exit code: {}",
                                exit_status.code().unwrap_or(-1)
                            ),
                            step_result.output.clone(),
                        ));
                        continue; // Skip other checks if exit code failed
                    }
                }

                // Check traditional assertions
                if let Some(assertion) = &original_step.assert {
                    if let Err(e) = check_assertion(assertion, &step_result.output, "")
                    {
                        assertion_failures.push((
                            step_result.name.clone(),
                            e.to_string(),
                            step_result.output.clone(),
                        ));
                    }
                }

                // Check insta snapshots (only use output for clean, predictable snapshots)
                if let Some(snapshot_name) = &original_step.insta {
                    // Filter out timing information that varies between runs
                    let filtered_output = filter_timing_from_output(&step_result.output);

                    // Use insta to assert the snapshot from main test file context
                    if let Err(_) = std::panic::catch_unwind(|| {
                        insta::assert_snapshot!(snapshot_name.as_str(), filtered_output);
                    }) {
                        assertion_failures.push((
                            step_result.name.clone(),
                            format!("Snapshot mismatch for '{}'", snapshot_name),
                            step_result.output.clone(),
                        ));
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
        let original_step = workflow.test.steps.get(step_result.step_index);
        let has_assertion = original_step.map(|s| s.assert.is_some()).unwrap_or(false);
        let has_insta = original_step.map(|s| s.insta.is_some()).unwrap_or(false);

        let thread_label = thread_name.to_uppercase();

        if has_assertion || has_insta {
            let failed = assertion_failures
                .iter()
                .any(|(name, _, _)| name == &step_result.name);
            let status = if failed { "❌ FAIL" } else { "✅ PASS" };
            let test_type = match (has_assertion, has_insta) {
                (true, true) => "ASSERT+INSTA",
                (true, false) => "ASSERT",
                (false, true) => "INSTA",
                (false, false) => unreachable!(),
            };
            let total_chars = step_result.output.len();
            let exit_info = if let Some(exit_status) = &step_result.exit_status {
                if exit_status.success() {
                    "exit:0".to_string()
                } else {
                    format!("exit:{}", exit_status.code().unwrap_or(-1))
                }
            } else {
                "exit:N/A".to_string()
            };
            let char_info = format!("{} chars, {}", total_chars, exit_info);
            println!(
                "   • [{}] {} - {} {} ({})",
                thread_label, step_result.name, status, test_type, char_info
            );
        } else {
            let total_chars = step_result.output.len();
            let exit_info = if let Some(exit_status) = &step_result.exit_status {
                if exit_status.success() {
                    "exit:0".to_string()
                } else {
                    format!("exit:{}", exit_status.code().unwrap_or(-1))
                }
            } else {
                "exit:N/A".to_string()
            };
            let char_info = format!("{} chars, {}", total_chars, exit_info);
            println!(
                "   • [{}] {} - ⏭️ NO ASSERTION ({})",
                thread_label, step_result.name, char_info
            );
        }
    }

    // In verbose mode, show detailed output in execution order
    if std::env::var("RV_TEST_VERBOSE").is_ok() {
        println!("\n🔍 Detailed Step Output (RV_TEST_VERBOSE detected):");
        
        // Collect all steps with their thread info and sort by execution order
        let mut all_steps_with_thread: Vec<(&StepResult, &str)> = Vec::new();
        for thread_output in &all_thread_outputs {
            for step_result in &thread_output.step_results {
                all_steps_with_thread.push((step_result, &thread_output.thread_name));
            }
        }
        
        // Sort by step_index (execution order)
        all_steps_with_thread.sort_by_key(|(step_result, _)| step_result.step_index);
        
        for (step_result, thread_name) in &all_steps_with_thread {
            if !step_result.output.is_empty() {
                println!("\n{}", "─".repeat(80));
                println!("📋 [{}] {} ({} chars)", 
                    thread_name.to_uppercase(), 
                    step_result.name, 
                    step_result.output.len()
                );
                println!("{}", "─".repeat(80));
                println!("{}", step_result.output);
            }
        }
        if !all_steps_with_thread.is_empty() {
            println!("{}", "─".repeat(80));
        }
    }

    // Report any thread execution errors
    if !thread_errors.is_empty() {
        println!("\n💥 Thread execution errors:");
        for error in &thread_errors {
            println!("   {}", error);
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
    }

    // Fail if there were any errors
    if !thread_errors.is_empty() || !assertion_failures.is_empty() {
        let total_errors = thread_errors.len() + assertion_failures.len();
        return Err(anyhow::anyhow!(
            "Test failed with {} total errors ({} thread errors, {} assertion failures)",
            total_errors,
            thread_errors.len(),
            assertion_failures.len()
        ));
    }

    Ok(())
}

fn run_workflow_tests(filter: Option<&str>) -> Result<()> {
    let workflow_dir = std::env::current_dir()?.join("tests/input/workflows");

    if !workflow_dir.exists() {
        println!(
            "⚠️  Workflow directory doesn't exist: {}",
            workflow_dir.display()
        );
        return Ok(());
    }

    let entries = fs::read_dir(&workflow_dir)?;
    let mut workflow_files = Vec::new();

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path
            .extension()
            .map_or(false, |ext| ext == "yml" || ext == "yaml")
        {
            // Apply filter if specified
            if let Some(filter_str) = filter {
                let file_stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

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
            println!(
                "⚠️  No workflow files found matching filter '{}' in {}",
                filter_str,
                workflow_dir.display()
            );
        } else {
            println!(
                "⚠️  No YAML workflow files found in {}",
                workflow_dir.display()
            );
        }
        return Ok(());
    }

    workflow_files.sort();

    let num_workflow_files = workflow_files.len();
    if let Some(filter_str) = filter {
        println!(
            "🚀 Found {} workflow test files matching '{}' to run",
            num_workflow_files, filter_str
        );
    } else {
        println!("🚀 Found {} workflow test files to run", num_workflow_files);
    }
    println!("{}", "═".repeat(80));

    for workflow_file in workflow_files {
        let file_name = workflow_file
            .file_name()
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
            println!(
                "📋 Workflow has {} steps across {} threads",
                workflow.test.steps.len(),
                workflow
                    .test
                    .steps
                    .iter()
                    .map(|s| &s.thread)
                    .collect::<std::collections::HashSet<_>>()
                    .len()
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
            }
            Err(e) => {
                eprintln!("💥 {} failed: {}", file_name, e);
                return Err(e);
            }
        }
    }

    println!(
        "\n🏁 All {} workflow tests completed successfully!",
        num_workflow_files
    );
    println!("{}", "═".repeat(80));

    Ok(())
}

#[test]
fn test_all_workflow_files() -> Result<()> {
    let filter = std::env::var("RV_TEST_FILTER").ok();
    run_workflow_tests(filter.as_deref())
}
