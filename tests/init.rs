use anyhow::Result;
use assert_cmd::Command;
use tempfile::TempDir;
use std::thread;
use std::time::Duration;
use std::io::Write;
use std::sync::{Arc, Barrier};
use fs_err as fs;

const RV: &str = "rv";
const R6_TEST_CONFIG: &str = "tests/input/r6-rproject.toml";

#[test]
fn test_rv_init_basic() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let project_path = temp_dir.path();
    
    Command::cargo_bin(RV)?
        .arg("init")
        .current_dir(project_path)
        .assert()
        .success();
    
    assert!(project_path.join("rproject.toml").exists());
 
    Ok(())
}

#[test]
fn test_r6_workflow() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let project_path = temp_dir.path();
    let r6_test_dir = project_path.join("r6-test");
    
    // Create r6-test directory
    fs::create_dir(&r6_test_dir)?;
    
    // Get absolute path to config file
    let config_path = std::env::current_dir()?.join(R6_TEST_CONFIG);
    
    // Set up barriers for synchronization
    let barrier1 = Arc::new(Barrier::new(2)); // After rv init + config setup
    let barrier2 = Arc::new(Barrier::new(2)); // R process ready
    let barrier3 = Arc::new(Barrier::new(2)); // After rv sync 
    let barrier4 = Arc::new(Barrier::new(2)); // After CRAN install
    let barrier5 = Arc::new(Barrier::new(2)); // After rv restoration
    
    let barrier1_r = Arc::clone(&barrier1);
    let barrier2_r = Arc::clone(&barrier2);
    let barrier3_r = Arc::clone(&barrier3);
    let barrier4_r = Arc::clone(&barrier4);
    let barrier5_r = Arc::clone(&barrier5);
    
    let barrier1_rv = Arc::clone(&barrier1);
    let barrier2_rv = Arc::clone(&barrier2);
    let barrier3_rv = Arc::clone(&barrier3);
    let barrier4_rv = Arc::clone(&barrier4);
    let barrier5_rv = Arc::clone(&barrier5);
    
    let r6_test_dir_r = r6_test_dir.clone();
    
    // Use channels for real-time communication between threads
    let (tx, rx) = std::sync::mpsc::channel();
    
    // R thread - persistent R session
    let r_handle = thread::spawn(move || {
        // Wait for rv init and config to be ready
        barrier1_r.wait();
        
        println!("🔵 R: Starting persistent R process...");
        
        let mut r_process = std::process::Command::new("R")
            .arg("--interactive")
            .current_dir(&r6_test_dir_r)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())  
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to start R process");
        
        let mut stdin = r_process.stdin.take().expect("Failed to get R stdin");
        
        // Signal that R process is ready - rv can now sync
        barrier2_r.wait();
        
        // Wait for rv sync to complete, then start testing
        barrier3_r.wait();
        
        println!("🔵 R: Testing R6 version from rv...");
        // Test R6 version from rv
        writeln!(stdin, r#"
library(R6)
cat("R6_VERSION_1:", as.character(packageVersion("R6")), "\n")
"#).expect("Failed to write to R process");
        
        // Give R time to load and check version
        thread::sleep(Duration::from_millis(500));
        
        println!("🔵 R: Installing R6 from CRAN (this may take a moment)...");
        // Install older R6 from CRAN  
        writeln!(stdin, r#"
options(repos = c(CRAN = "https://packagemanager.posit.co/cran/2025-01-01"))
install.packages("R6", quiet = TRUE)
detach("package:R6", unload=TRUE)
library(R6)
cat("R6_VERSION_2:", as.character(packageVersion("R6")), "\n")
"#).expect("Failed to write to R process");
        
        // Give R time to install and signal completion
        thread::sleep(Duration::from_millis(3000));
        println!("🔵 R: CRAN install completed");
        
        // Signal CRAN install is complete
        barrier4_r.wait();
        
        // Wait for rv restoration to complete before testing final version
        barrier5_r.wait();
        
        println!("🔵 R: Testing final R6 version after rv restoration...");
        // Test final R6 version after rv restoration
        writeln!(stdin, r#"
detach("package:R6", unload=TRUE)
library(R6)
cat("R6_VERSION_3:", as.character(packageVersion("R6")), "\n")
quit(save = "no")
"#).expect("Failed to write to R process");
        
        // Wait for R to finish and capture final output
        let output = r_process.wait_with_output().expect("Failed to wait for R process");
        let full_output = String::from_utf8_lossy(&output.stdout).to_string();
        
        // Send the full output back to main thread for final parsing
        tx.send(full_output).unwrap();
    });
    
    // Main thread - rv commands
    
    println!("🟡 RV: Initializing new rv project...");
    // 1. rv init
    Command::cargo_bin(RV)?
        .arg("init")
        .current_dir(&r6_test_dir)
        .assert()
        .success();
    
    println!("🟡 RV: Setting up R6 dependency configuration...");
    // 2. Copy R6 config
    fs::copy(&config_path, r6_test_dir.join("rproject.toml"))?;
    
    // Signal that rv init and config are ready
    barrier1_rv.wait();
    
    // Wait for R process to start
    barrier2_rv.wait();
    
    println!("🟡 RV: Running rv sync to install R6...");
    // 3. rv sync
    let sync_output = Command::cargo_bin(RV)?
        .arg("sync")
        .current_dir(&r6_test_dir)
        .output()?;
    
    println!("🟡 RV: {}", String::from_utf8_lossy(&sync_output.stdout).trim());
    assert!(sync_output.status.success(), "rv sync should succeed");
    
    // Signal that rv sync is complete
    barrier3_rv.wait();
    
    // 4. Wait for R to install CRAN version
    barrier4_rv.wait();
    
    println!("🟡 RV: Checking if rv detects the package conflict...");
    // 5. Check rv plan - should detect R6 needs restoration
    let plan_output = Command::cargo_bin(RV)?
        .arg("plan")
        .current_dir(&r6_test_dir)
        .output()?;
    
    println!("🟡 RV: rv plan says: {}", String::from_utf8_lossy(&plan_output.stdout).trim());
    
    println!("🟡 RV: Running rv sync to restore rv-managed version...");
    // 6. rv sync - should restore R6
    let final_sync_output = Command::cargo_bin(RV)?
        .arg("sync")
        .current_dir(&r6_test_dir)
        .output()?;
    
    println!("🟡 RV: {}", String::from_utf8_lossy(&final_sync_output.stdout).trim());
    assert!(final_sync_output.status.success(), "rv sync should succeed");
    
    // Signal that rv restoration is complete - R can now test final version
    barrier5_rv.wait();
    
    // 7. Wait for R thread to complete and get output
    r_handle.join().expect("R thread panicked");
    let r_output = rx.recv().expect("Failed to receive R output");
    
    // 8. Extract and verify version progression
    let version_lines: Vec<&str> = r_output.lines()
        .filter(|line| line.starts_with("R6_VERSION_"))
        .collect();
    
    assert_eq!(version_lines.len(), 3, "Should have exactly 3 version checks");
    
    let version1 = version_lines[0].split(':').nth(1).unwrap().trim();
    let version2 = version_lines[1].split(':').nth(1).unwrap().trim(); 
    let version3 = version_lines[2].split(':').nth(1).unwrap().trim();
    
    assert_eq!(version1, "2.6.1", "Expected R6 version 2.6.1 after rv sync");
    assert_eq!(version2, "2.5.1", "Expected R6 version 2.5.1 after CRAN install");
    assert_eq!(version3, "2.6.1", "Expected R6 version 2.6.1 after rv restoration");
    
    println!("✅ All R6 version checks passed: {} -> {} -> {}", version1, version2, version3);
    //assert!(1 == 0);
    Ok(())
}

