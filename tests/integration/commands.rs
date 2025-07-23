use crate::integration::process_manager::RProcessManager;
use anyhow::Result;
use assert_cmd::Command;
use fs_err as fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command as StdCommand};
use std::time::{Duration, Instant};

const RV: &str = "rv";

fn debug_print(msg: &str) {
    if std::env::var("RV_TEST_DEBUG").is_ok() {
        println!("🐛 DEBUG: {}", msg);
    }
}

// Improved timeout execution for R commands with better process monitoring
pub fn execute_r_command_with_timeout<F>(
    step_name: &str,
    timeout_secs: Option<u64>,
    r_manager: &mut Option<RProcessManager>,
    operation: F,
) -> Result<String>
where
    F: FnOnce(&mut RProcessManager) -> Result<String>,
{
    if let Some(timeout) = timeout_secs {
        debug_print(&format!(
            "Executing R step '{}' with timeout: {}s",
            step_name, timeout
        ));
    } else {
        debug_print(&format!("Executing R step '{}' with no timeout", step_name));
    }

    if let Some(manager) = r_manager {
        if let Some(timeout_duration) = timeout_secs.map(Duration::from_secs) {
            debug_print(&format!(
                "Starting timeout monitor for step '{}'",
                step_name
            ));

            // Start timing the operation
            let start_time = Instant::now();

            // Execute the operation
            let operation_result = operation(manager);

            // Check if the operation completed within the timeout
            let elapsed = start_time.elapsed();

            if elapsed > timeout_duration {
                debug_print(&format!(
                    "Step '{}' took {}s, exceeding timeout of {}s",
                    step_name,
                    elapsed.as_secs(),
                    timeout_duration.as_secs()
                ));

                // The operation completed but took too long
                // Try to capture output and kill the R process if it's still running
                let captured_output = if let Some(manager) = r_manager {
                    if manager.is_alive().unwrap_or(false) {
                        debug_print("R process is still alive after timeout, attempting cleanup");

                        // Try to capture current state before cleanup
                        match manager.try_capture_output() {
                            Ok((stdout, stderr)) => {
                                // Now attempt to kill the process
                                if let Err(e) = manager.force_shutdown() {
                                    debug_print(&format!(
                                        "Failed to force shutdown R process: {}",
                                        e
                                    ));
                                }

                                format!(
                                    "R output captured after timeout:\\n\\nSTDOUT ({} chars):\\n{}\\n\\nSTDERR ({} chars):\\n{}",
                                    stdout.len(),
                                    stdout,
                                    stderr.len(),
                                    String::from_utf8_lossy(&stderr)
                                )
                            }
                            Err(e) => {
                                format!("Failed to capture R output after timeout: {}", e)
                            }
                        }
                    } else {
                        "R process was not running after timeout".to_string()
                    }
                } else {
                    "No R process manager available".to_string()
                };

                return Err(anyhow::anyhow!(
                    "Step '{}' timed out after {}s (actual: {}s)\\n\\nCaptured output:\\n{}",
                    step_name,
                    timeout_duration.as_secs(),
                    elapsed.as_secs(),
                    captured_output
                ));
            } else {
                debug_print(&format!(
                    "Step '{}' completed in {}s (within timeout of {}s)",
                    step_name,
                    elapsed.as_secs(),
                    timeout_duration.as_secs()
                ));
                operation_result
            }
        } else {
            // No timeout, execute normally
            operation(manager)
        }
    } else {
        Err(anyhow::anyhow!(
            "R process not available for step '{}'",
            step_name
        ))
    }
}

// Keep the old function for non-R commands
pub fn execute_with_timeout<F, T>(
    step_name: &str,
    timeout_secs: Option<u64>,
    operation: F,
) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    if let Some(timeout) = timeout_secs {
        debug_print(&format!(
            "Executing step '{}' with timeout: {}s",
            step_name, timeout
        ));
    } else {
        debug_print(&format!("Executing step '{}' with no timeout", step_name));
    }

    let start = Instant::now();
    let result = operation();
    let elapsed = start.elapsed();

    match result {
        Ok(value) => {
            debug_print(&format!(
                "Step '{}' completed in {}s",
                step_name,
                elapsed.as_secs()
            ));
            Ok(value)
        }
        Err(e) => {
            debug_print(&format!(
                "Step '{}' failed after {}s: {}",
                step_name,
                elapsed.as_secs(),
                e
            ));
            Err(e)
        }
    }
}

pub fn execute_rv_command(
    command: &str,
    test_dir: &Path,
    config_path: &Path,
) -> Result<(String, std::process::ExitStatus)> {
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

    // Get the rv binary path from assert_cmd
    let rv_binary = Command::cargo_bin(RV)
        .map_err(|e| anyhow::anyhow!("Failed to find rv binary: {}", e))?
        .get_program()
        .to_owned();

    debug_print(&format!("Spawning rv command: {} {}", cmd, args.join(" ")));
    
    // Use anonymous pipe to get truly interleaved output following the exact pattern
    let (mut recv, send) = std::io::pipe()
        .map_err(|e| anyhow::anyhow!("Failed to create pipe: {}", e))?;

    // Both stdout and stderr will write to the same pipe, combining the two
    let mut child = StdCommand::new(rv_binary)
        .arg(cmd)
        .args(args)
        .current_dir(test_dir)
        .stdout(send.try_clone().map_err(|e| anyhow::anyhow!("Failed to clone pipe: {}", e))?)
        .stderr(send)
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn rv {}: {}", cmd, e))?;

    // Read all output from the combined pipe
    let mut combined_output = Vec::new();
    recv.read_to_end(&mut combined_output)
        .map_err(|e| anyhow::anyhow!("Failed to read rv {} output: {}", cmd, e))?;

    // It's important that we read from the pipe before the process exits, to avoid
    // filling the OS buffers if the program emits too much output.
    let exit_status = child.wait()
        .map_err(|e| anyhow::anyhow!("Failed to wait for rv {} completion: {}", cmd, e))?;

    let combined_str = String::from_utf8_lossy(&combined_output).to_string();

    // Handle config copying for init (only if command succeeded)
    if cmd == "init" && exit_status.success() && config_path.exists() {
        fs::copy(config_path, test_dir.join("rproject.toml"))
            .map_err(|e| anyhow::anyhow!("Failed to copy config file: {}", e))?;
    }

    // Return truly interleaved output and exit status
    // Since output is interleaved, stderr is already combined into the output
    Ok((combined_str, exit_status))
}

pub fn load_r_script(script_name: &str) -> Result<String> {
    let script_path = format!("tests/input/r_scripts/{}", script_name);
    fs::read_to_string(&script_path)
        .map_err(|e| anyhow::anyhow!("Failed to load R script {}: {}", script_path, e))
}

pub fn parse_r_step_outputs(
    full_output: &str,
    step_names: &[String],
) -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;

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
