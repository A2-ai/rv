#[derive(Debug, Clone)]
struct RVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

#[cfg(target_os = "linux")]
fn repo_path(url: String, r_version: RVersion) -> String {
    drop(r_version);
    format!("{}/src/contrib", url)
}

#[cfg(target_os = "windows")]
fn repo_path(url: String, r_version: RVersion) -> String {
    format!("{url}/bin/windows/contrib/{}.{}", r_version.major, r_version.minor)
}

#[cfg(target_os = "macos")]
fn repo_path(url: String, r_version: RVersion) -> String {
    if r_version.major < 4 { panic!("TODO: handle r_version not macos supported") }
    if r_version.minor < 3 {
        format!("{url}/bin/macosx/big-sur-{}/contrib/{}.{}", std::env::consts::ARCH, r_version.major, r_version.minor)
    } else {
        format!("{url}/bin/macosx/contrib/{}.{}", r_version.major, r_version.minor)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn repo_path(url: String, r_version: RVersion) -> String {
    panic!("TODO: Running on an unsupported os");
    String::new()
}

mod tests {
    use super::*;

    fn url() -> String { 
        "https://cran.rstudio.com".to_string() 
    }
    fn r_version() -> RVersion {
        RVersion {
            major: 4,
            minor: 4,
            patch: 1,
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_repo_path_linux() {
        assert_eq!(repo_path(url(), r_version()), 
            format!("{}/src/contrib", url()))
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn test_repo_path_windows() {
        assert_eq!(repo_path(url(), r_version()), 
            format!("{}/bin/windows/contrib/{}.{}", url(), r_version().major, r_version().minor))
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_repo_path_macos_r4() {
        assert_eq!(repo_path(url(), r_version()), 
            format!("{}/bin/macosx/big-sur-{}/contrib/{}.{}", url(), std::env::consts::ARCH, r_version().major, r_version().minor))
    }

    #[test]
    #[cfg(target_os = "macos")]
    #[should_panic(expected = "TODO: handle r_version not macos supported")]
    fn test_repo_macos_r3() {
        repo_path(url(), RVersion {
            major: 3,
            minor: 3,
            patch: 3,
        });
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_repo_macos_r43() {
        let r_version = RVersion { major: 4, minor: 3, patch: 1};
        assert_eq!(repo_path(url(), r_version),
        format!("{}/bin/macosx/contrib/{}.{}", url(), r_version.major, r_version.minor))
    }

}