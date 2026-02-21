use std::process::Command;

fn main() {
    // Re-run if git state changes
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/");

    let pkg_version = env!("CARGO_PKG_VERSION");

    // Check if the current commit is tagged with the package version
    let is_tagged = Command::new("git")
        .args(["tag", "--points-at", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|tags| {
            tags.lines()
                .any(|t| t.trim().trim_start_matches('v') == pkg_version)
        })
        .unwrap_or(false);

    if is_tagged {
        // Tagged release: just use the package version
        println!("cargo:rustc-env=RV_LONG_VERSION={pkg_version}");
        return;
    }

    // Get short commit hash
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    if commit.is_empty() {
        println!("cargo:rustc-env=RV_LONG_VERSION={pkg_version}");
        return;
    }

    // Check if the working tree is dirty
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    let version = if dirty {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("{pkg_version}-{commit}-{timestamp}")
    } else {
        format!("{pkg_version}-{commit}")
    };

    println!("cargo:rustc-env=RV_LONG_VERSION={version}");
}
