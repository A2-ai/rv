use anyhow::Result;
use assert_cmd::Command;
use tempfile::TempDir;
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Barrier};
use fs_err as fs;

const RV: &str = "rv";
const R6_TEST_CONFIG: &str = "tests/input/r6-rproject.toml";

fn extract_version_from_r_output(output: &str) -> Option<String> {
    for line in output.lines() {
        if line.starts_with("VERSION:") {
            return line.strip_prefix("VERSION:")
                .map(|s| s.trim().to_string());
        }
    }
    None
}

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
fn test_rv_init_with_r_process() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let project_path = temp_dir.path().to_path_buf();
    
    // Barrier to synchronize threads
    let barrier = Arc::new(Barrier::new(2));
    let barrier_r = Arc::clone(&barrier);
    let barrier_rv = Arc::clone(&barrier);
    
    let project_path_r = project_path.clone();
    let project_path_rv = project_path.clone();
    
    // Thread 1: R process
    let r_handle = thread::spawn(move || {
        // Wait for rv init to complete
        barrier_r.wait();
        
        // Give rv init a moment to finish
        thread::sleep(Duration::from_millis(100));
        
        // R script to check if rproject.toml exists
        let r_script = r#"
            if (file.exists("rproject.toml")) {
                cat("SUCCESS: rproject.toml found\n")
                cat("Contents of rproject.toml:\n")
                cat(readLines("rproject.toml"), sep = "\n")
                quit(status = 0)
            } else {
                cat("ERROR: rproject.toml not found\n")
                quit(status = 1)
            }
        "#;
        
        let output = std::process::Command::new("R")
            .arg("--slave")
            .arg("-e")
            .arg(r_script)
            .current_dir(&project_path_r)
            .output()
            .expect("Failed to run R");
            
        (output.status.success(), String::from_utf8_lossy(&output.stdout).to_string())
    });
    
    // Thread 2: rv init
    let rv_handle = thread::spawn(move || {
        let result = Command::cargo_bin(RV).unwrap()
            .arg("init")
            .current_dir(&project_path_rv)
            .assert()
            .success();
            
        // Signal that rv init is done
        barrier_rv.wait();
        result
    });
    
    // Wait for both threads
    rv_handle.join().expect("rv thread panicked");
    let (r_success, r_output) = r_handle.join().expect("R thread panicked");
    
    println!("R output: {}", r_output);

    assert_eq!(r_success, true); 
    
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
    
    // Set up barrier for coordinating between threads (reuse same barrier)
    let barrier = Arc::new(Barrier::new(2));
    let barrier1_rv = Arc::clone(&barrier);
    let barrier2_rv = Arc::clone(&barrier);
    let barrier3_rv = Arc::clone(&barrier);
    let barrier4_rv = Arc::clone(&barrier);
    
    let barrier1_r = Arc::clone(&barrier);
    let barrier2_r = Arc::clone(&barrier);
    let barrier3_r = Arc::clone(&barrier);
    let barrier4_r = Arc::clone(&barrier);
    
    let r6_test_dir_r = r6_test_dir.clone();
    
    // R process thread  
    let r_handle = thread::spawn(move || {
        println!("R process starting...");
        
        // Wait for rv sync to complete, then test R6
        barrier1_r.wait();
        
        let r_script1 = r#"
            cat("Testing R6 after rv sync\n")
            tryCatch({
                library(R6)
                version <- as.character(packageVersion("R6"))
                cat("SUCCESS: R6 loaded from rv\n")
                cat("VERSION:", version, "\n")
            }, error = function(e) {
                cat("ERROR loading R6 from rv:", e$message, "\n")
                cat("VERSION: ERROR\n")
            })
        "#;
        
        let r_output1 = std::process::Command::new("R")
            .arg("--slave")
            .arg("-e")
            .arg(r_script1)
            .current_dir(&r6_test_dir_r)
            .output()
            .expect("Failed to run R for R6 test");
        
        let r1_output = String::from_utf8_lossy(&r_output1.stdout).to_string();
        println!("R R6 from rv: {}", r1_output);
        let version1 = extract_version_from_r_output(&r1_output).unwrap_or("ERROR".to_string());
        assert_eq!(version1, "2.6.1", "Expected R6 version 2.6.1 after rv sync");
        
        // Wait for rv plan (should be "Nothing to do")
        barrier2_r.wait();
        
        // Install R6 from CRAN and test
        let r_script2 = r#"
            cat("Installing older version R6\n")
            options(repos = c(CRAN = "https://packagemanager.posit.co/cran/2025-01-01"))
            install.packages("R6", quiet = TRUE)
            
            tryCatch({
                library(R6)
                version <- as.character(packageVersion("R6"))
                cat("SUCCESS: Old R6 loaded\n")
                cat("VERSION:", version, "\n")
            }, error = function(e) {
                cat("ERROR loading R6 from CRAN:", e$message, "\n")
                cat("VERSION: ERROR\n")
            })
        "#;
        
        let r_output2 = std::process::Command::new("R")
            .arg("--slave")
            .arg("-e")
            .arg(r_script2)
            .current_dir(&r6_test_dir_r)
            .output()
            .expect("Failed to run R for CRAN R6");
        
        let r2_output = String::from_utf8_lossy(&r_output2.stdout).to_string();
        println!("R R6 from CRAN: {}", r2_output);
        let version2 = extract_version_from_r_output(&r2_output).unwrap_or("ERROR".to_string());
        assert_eq!(version2, "2.5.1", "Expected R6 version 2.5.1 after CRAN install");
        
        barrier3_r.wait();
        
        // Wait for final rv sync
        barrier4_r.wait();
        
        // Final R script: check R6 version after rv sync
        let r_script3 = r#"
            cat("Testing R6 after final rv sync\n")
            tryCatch({
                library(R6)
                version <- as.character(packageVersion("R6"))
                cat("SUCCESS: Final R6 loaded\n")
                cat("VERSION:", version, "\n")
            }, error = function(e) {
                cat("ERROR loading R6 after final sync:", e$message, "\n")
                cat("VERSION: ERROR\n")
            })
        "#;
        
        let r_output3 = std::process::Command::new("R")
            .arg("--slave")
            .arg("-e")
            .arg(r_script3)
            .current_dir(&r6_test_dir_r)
            .output()
            .expect("Failed to run R for final R6 test");
        
        let r3_output = String::from_utf8_lossy(&r_output3.stdout).to_string();
        println!("R R6 after final sync: {}", r3_output);
        let version3 = extract_version_from_r_output(&r3_output).unwrap_or("ERROR".to_string());
        assert_eq!(version3, "2.6.1", "Expected R6 version 2.6.1 after final rv sync");
        
        println!("✅ All R6 version checks passed: 2.6.1 -> 2.5.1 -> 2.6.1");
    });
    
    // rv thread (main thread)
    
    // 1. rv init
    Command::cargo_bin(RV)?
        .arg("init")
        .current_dir(&r6_test_dir)
        .assert()
        .success();
    
    // 2. Copy config and sync
    fs::copy(&config_path, r6_test_dir.join("rproject.toml"))?;
    
    let sync_output = Command::cargo_bin(RV)?
        .arg("sync")
        .current_dir(&r6_test_dir)
        .output()?;
    
    println!("rv sync stdout: {}", String::from_utf8_lossy(&sync_output.stdout));
    assert!(sync_output.status.success(), "rv sync should succeed");
    
    barrier1_rv.wait(); // R tests R6 from rv
    
    // 3. rv plan (should be "Nothing to do")
    let plan_output1 = Command::cargo_bin(RV)?
        .arg("plan")
        .current_dir(&r6_test_dir)
        .output()?;
    
    println!("rv plan (after sync) stdout: {}", String::from_utf8_lossy(&plan_output1.stdout));
    
    barrier2_rv.wait(); // R installs R6 from CRAN
    barrier3_rv.wait(); // R completes CRAN install
    
    // 4. rv plan (after R installed from CRAN)
    let plan_output2 = Command::cargo_bin(RV)?
        .arg("plan")
        .current_dir(&r6_test_dir)
        .output()?;
    
    println!("rv plan (after CRAN install) stdout: {}", String::from_utf8_lossy(&plan_output2.stdout));
    
    // 5. Final rv sync (should restore R6 to rv-managed version)
    let final_sync_output = Command::cargo_bin(RV)?
        .arg("sync")
        .current_dir(&r6_test_dir)
        .output()?;
    
    println!("rv sync (final) stdout: {}", String::from_utf8_lossy(&final_sync_output.stdout));
    assert!(final_sync_output.status.success(), "final rv sync should succeed");
    
    barrier4_rv.wait(); // R tests final R6 version
    
    // Wait for R thread to complete
    r_handle.join().expect("R thread panicked");
    Ok(())
}

