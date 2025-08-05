use anyhow::Result;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

fn debug_print(msg: &str) {
    if std::env::var("RV_TEST_DEBUG").is_ok() {
        println!("🐛 DEBUG: {}", msg);
    }
}

/// Manages the lifecycle of R subprocess for integration testing.
/// 
/// Handles starting, stopping, command execution, and output capture
/// for R processes with proper timeout and error handling. Supports
/// health checking and graceful shutdown with output collection.
/// 
/// # Examples
/// 
/// ```rust
/// let mut manager = RProcessManager::start_r_process(&test_dir)?;
/// manager.send_command("library(dplyr)")?;
/// let (stdout, stderr, exit_status) = manager.shutdown_and_capture_output()?;
/// ```
/// 
/// # Environment Variables
/// 
/// - `R_EXECUTABLE`: Alternative R executable path
/// - `RV_TEST_DEBUG`: Enable debug output when set
pub struct RProcessManager {
    /// The running R process, if active
    process: Option<std::process::Child>,
    /// Standard input pipe to send commands to R
    stdin: Option<std::process::ChildStdin>,
    /// Last time process health was checked
    last_health_check: Instant,
}

impl RProcessManager {
    fn find_r_executable() -> Result<String> {
        debug_print("Starting R executable detection");

        if let Ok(r_path) = std::env::var("R_EXECUTABLE") {
            debug_print(&format!("Using R_EXECUTABLE: {}", r_path));
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

    pub fn start_r_process(test_dir: &Path) -> Result<Self> {
        let r_executable = Self::find_r_executable()?;
        debug_print(&format!(
            "Starting R process '{}' in directory: {}",
            r_executable,
            test_dir.display()
        ));

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
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to start R process with '{}': {}. Is R installed and in PATH?",
                    r_executable,
                    e
                )
            })?;

        debug_print("R process started successfully");

        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get R stdin"))?;

        Ok(Self {
            process: Some(process),
            stdin: Some(stdin),
            last_health_check: Instant::now(),
        })
    }

    pub fn is_alive(&mut self) -> Result<bool> {
        if let Some(process) = &mut self.process {
            match process
                .try_wait()
                .map_err(|e| anyhow::anyhow!("Failed to check R process status: {}", e))?
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
                                    println!("📤 R STDOUT ({} chars):\\n{}", stdout.len(), stdout);
                                }
                                if !stderr.trim().is_empty() {
                                    println!("📤 R STDERR ({} chars):\\n{}", stderr.len(), stderr);
                                }
                            }
                            Err(e) => {
                                debug_print(&format!(
                                    "Failed to capture output from dead R process: {}",
                                    e
                                ));
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

    pub fn send_command(&mut self, command: &str) -> Result<()> {
        debug_print(&format!("Sending R command: {}", command));

        if !self.is_alive()? {
            return Err(anyhow::anyhow!("R process is not running"));
        }

        if let Some(stdin) = &mut self.stdin {
            writeln!(stdin, "{}", command)
                .map_err(|e| anyhow::anyhow!("Failed to write '{}' to R stdin: {}", command, e))?;
            stdin.flush().map_err(|e| {
                anyhow::anyhow!("Failed to flush R stdin after command '{}': {}", command, e)
            })?;
            debug_print(&format!("Successfully sent R command: {}", command));
        } else {
            return Err(anyhow::anyhow!("R stdin not available"));
        }

        Ok(())
    }

    pub fn debug_pause_after_command(&self) {
        if std::env::var("RV_TEST_DEBUG").is_ok() {
            debug_print("Pausing briefly to let R process command");
        }
        // Always pause briefly to allow R to process commands and flush output
        // This is especially important on Windows where output buffering can cause
        // commands to not be fully processed before the R process is terminated
        std::thread::sleep(Duration::from_millis(100));
    }

    pub fn try_capture_output(&self) -> Result<(String, Vec<u8>)> {
        debug_print("Attempting to capture R output without shutdown");

        if let Some(_process) = &self.process {
            // We can't capture output from a running process safely without consuming it
            // This is a non-destructive check - just return empty output with a note
            debug_print("R process is still running, cannot capture output safely");
            Ok((
                "R process still running - output not available".to_string(),
                Vec::new(),
            ))
        } else {
            debug_print("No R process available for output capture");
            Ok(("No R process available".to_string(), Vec::new()))
        }
    }

    pub fn force_shutdown(&mut self) -> Result<()> {
        debug_print("Forcing R process shutdown due to timeout");

        if let Some(mut process) = self.process.take() {
            // Try to kill the process forcefully
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

        // Clean up resources
        self.stdin = None;

        Ok(())
    }

    pub fn shutdown_and_capture_output(mut self) -> Result<(String, Vec<u8>, Option<std::process::ExitStatus>)> {
        debug_print("Shutting down R process and capturing output");
        let accumulated_output = String::new();

        if let (Some(mut stdin), Some(process)) = (self.stdin.take(), self.process.take()) {
            debug_print("Sending quit command to R");
            
            // On Windows, add a small delay to ensure output is flushed before quitting
            if cfg!(windows) {
                debug_print("Windows detected - adding flush delay");
                if let Err(e) = writeln!(stdin, "flush.console()") {
                    debug_print(&format!("Failed to send flush command to R: {}", e));
                }
                if let Err(e) = stdin.flush() {
                    debug_print(&format!("Failed to flush stdin: {}", e));
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            
            if let Err(e) = writeln!(stdin, "quit(save = 'no')") {
                debug_print(&format!("Failed to send quit command to R: {}", e));
            }
            if let Err(e) = stdin.flush() {
                debug_print(&format!("Failed to flush quit command: {}", e));
            }
            drop(stdin); // Close stdin to signal R to exit

            debug_print("Waiting for R process to complete");
            let final_output = process
                .wait_with_output()
                .map_err(|e| anyhow::anyhow!("Failed to wait for R process termination: {}", e))?;

            debug_print(&format!(
                "R process completed with status: {:?}",
                final_output.status
            ));
            if !final_output.status.success() {
                debug_print(&format!(
                    "R process exited with non-zero status: {:?}",
                    final_output.status
                ));
            }

            let stdout = final_output.stdout;
            let stderr = final_output.stderr;

            debug_print(&format!(
                "Captured R stdout: {} bytes, stderr: {} bytes",
                stdout.len(),
                stderr.len()
            ));

            Ok((String::from_utf8_lossy(&stdout).to_string(), stderr, Some(final_output.status)))
        } else {
            debug_print("No R process to shutdown");
            Ok((accumulated_output, Vec::new(), None))
        }
    }
}

impl Drop for RProcessManager {
    fn drop(&mut self) {
        debug_print("RProcessManager being dropped - ensuring R process cleanup");

        // Attempt graceful shutdown first
        if let Some(mut process) = self.process.take() {
            // Check if process is still running
            match process.try_wait() {
                Ok(Some(status)) => {
                    debug_print(&format!(
                        "R process already exited with status: {:?}",
                        status
                    ));
                }
                Ok(None) => {
                    debug_print("R process still running, attempting graceful shutdown");

                    // Try to send quit command if stdin is still available
                    if let Some(mut stdin) = self.stdin.take() {
                        if writeln!(stdin, "quit(save = 'no')").is_ok() {
                            let _ = stdin.flush();
                            debug_print("Sent quit command to R process");
                        }
                        drop(stdin);
                    }

                    // Wait a reasonable time for graceful shutdown
                    let shutdown_timeout = Duration::from_secs(2);
                    let start = Instant::now();

                    while start.elapsed() < shutdown_timeout {
                        match process.try_wait() {
                            Ok(Some(status)) => {
                                debug_print(&format!(
                                    "R process gracefully exited with status: {:?}",
                                    status
                                ));
                                return;
                            }
                            Ok(None) => {
                                std::thread::sleep(Duration::from_millis(100));
                                continue;
                            }
                            Err(e) => {
                                debug_print(&format!(
                                    "Error checking R process status during shutdown: {}",
                                    e
                                ));
                                break;
                            }
                        }
                    }

                    // If graceful shutdown failed, force kill
                    debug_print("Graceful shutdown timed out, forcing R process termination");
                    match process.kill() {
                        Ok(()) => {
                            debug_print("Successfully sent kill signal to R process");
                            // Wait for the process to actually die
                            match process.wait() {
                                Ok(status) => {
                                    debug_print(&format!(
                                        "R process terminated with status: {:?}",
                                        status
                                    ));
                                }
                                Err(e) => {
                                    debug_print(&format!(
                                        "Error waiting for R process termination: {}",
                                        e
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            debug_print(&format!("Failed to kill R process: {}", e));
                        }
                    }
                }
                Err(e) => {
                    debug_print(&format!("Error checking R process status: {}", e));
                }
            }
        }

        // Clean up stdin if it wasn't already taken
        if self.stdin.is_some() {
            debug_print("Cleaning up R process stdin");
            self.stdin = None;
        }
    }
}
