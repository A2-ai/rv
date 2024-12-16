use std::process::Command;

pub fn new() -> String {
    // to be filled in with "properly" controlled User Agent
    r_command(r#"cat(getOption('HTTPUserAgent'))"#)
}

fn r_command(command: &str) -> String {
    println!("{command}");
    let output = Command::new("Rscript")
        .arg("-e")
        .arg(command)
        .output()
        .expect("TODO: 1. handle command not run error");

    if !output.status.success() { eprintln!("TODO: 2. handle command failed error") };
    String::from_utf8_lossy(&output.stdout).to_string()
}