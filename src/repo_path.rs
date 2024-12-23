use crate::version::Version;

fn repo_path(url: String, r_version: Version, os_type: String) -> String{
    match os_type.to_lowercase().as_str() {
        "linux" => { linux_url(url) }
        "windows" => { windows_url(url, r_version) }
        "macos" => { mac_url(url, r_version) }
        _ => { panic!("TODO: OS not supported") }
    }
}

fn linux_url(url: String) -> String {
    format!("{}/src/contrib", url)
}

fn windows_url(url: String, r_version: Version) -> String {
    let [major, minor] = r_version.major_minor();
    format!("{url}/bin/windows/contrib/{}.{}", major, minor)
}

fn mac_url(url: String, r_version: Version) -> String {
    let [major, minor] = r_version.major_minor();
    if major < 4 { panic!("TODO: handle r_version not macos supported") }
    if minor > 2 {
        format!("{url}/bin/macosx/big-sur-{}/contrib/{}.{}", std::env::consts::ARCH, major, minor)
    } else {
        format!("{url}/bin/macosx/contrib/{}.{}", major, minor)
    }
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
    fn test_repo_path_linux() {
        assert_eq!(repo_path(url(), r_version(), String::from("linux")), 
            format!("{}/src/contrib", url()))
    }

    #[test]
    fn test_repo_path_windows() {
        let [major, minor] = r_version().major_minor();
        assert_eq!(repo_path(url(), r_version(), String::from("windows")), 
            format!("{}/bin/windows/contrib/{}.{}", url(), major, minor))
    }

    #[test]
    fn test_repo_path_macos_r4() {
        let [major, minor] = r_version().major_minor();
        assert_eq!(repo_path(url(), r_version(), String::from("macos")), 
            format!("{}/bin/macosx/big-sur-{}/contrib/{}.{}", url(), std::env::consts::ARCH, major, minor))
    }

    #[test]
    #[should_panic(expected = "TODO: handle r_version not macos supported")]
    fn test_repo_macos_r3() {
        repo_path(url(), "3.3.3".parse::<Version>().unwrap(), String::from("macos"));
    }

    #[test]
    fn test_repo_macos_r42() {
        let r_version = "4.2.1".parse::<Version>().unwrap();
        let [major, minor] = r_version.major_minor();
        assert_eq!(repo_path(url(), r_version, String::from("macos")),
            format!("{}/bin/macosx/contrib/{}.{}", url(), major, minor))
    }

    #[test]
    #[should_panic]
    fn test_repo_not_valid_os() {
        repo_path(url(), r_version(), String::from("not valid os"));
    }
}