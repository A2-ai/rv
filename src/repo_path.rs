use crate::version::Version;

#[cfg(target_os = "linux")]
fn repo_path(url: String, r_version: Version) -> String {
    drop(r_version);
    format!("{}/src/contrib", url)
}

#[cfg(target_os = "windows")]
fn repo_path(url: String, r_version: Version) -> String {
    let [major, minor] = r_version.major_minor();
    format!("{url}/bin/windows/contrib/{}.{}", major, minor)
}

#[cfg(target_os = "macos")]
fn repo_path(url: String, r_version: Version) -> String {
    let [major, minor] = r_version.major_minor();
    if major < 4 { panic!("TODO: handle r_version not macos supported") }
    if minor > 2 {
        format!("{url}/bin/macosx/big-sur-{}/contrib/{}.{}", std::env::consts::ARCH, major, minor)
    } else {
        format!("{url}/bin/macosx/contrib/{}.{}", major, minor)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn repo_path(url: String, r_version: Version) -> String {
    panic!("TODO: Running on an unsupported os");
    String::new()
}

mod tests {
    use super::*;

    fn url() -> String { 
        "https://cran.rstudio.com".to_string() 
    }
    fn r_version() -> Version {
        "4.4.1".parse::<Version>().unwrap()
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
        let [major, minor] = r_version().major_minor();
        assert_eq!(repo_path(url(), r_version()), 
            format!("{}/bin/windows/contrib/{}.{}", url(), major, minor))
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_repo_path_macos_r4() {
        let [major, minor] = r_version().major_minor();
        assert_eq!(repo_path(url(), r_version()), 
            format!("{}/bin/macosx/big-sur-{}/contrib/{}.{}", url(), std::env::consts::ARCH, major, minor))
    }

    #[test]
    #[cfg(target_os = "macos")]
    #[should_panic(expected = "TODO: handle r_version not macos supported")]
    fn test_repo_macos_r3() {
        repo_path(url(), "3.3.3".parse::<Version>().unwrap());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_repo_macos_r42() {
        let r_version = "4.2.1".parse::<Version>().unwrap();
        let [major, minor] = r_version.major_minor();
        assert_eq!(repo_path(url(), r_version),
        format!("{}/bin/macosx/contrib/{}.{}", url(), major, minor))
    }
}