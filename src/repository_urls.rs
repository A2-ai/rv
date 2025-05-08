use crate::consts::PACKAGE_FILENAME;
use crate::{OsType, ResolvedDependency, SystemInfo};

/// This is based on the mapping on PPM config <https://packagemanager.posit.co/client/#/repos/cran/setup>.
fn get_distro_name(sysinfo: &SystemInfo, distro: &str) -> Option<String> {
    match distro {
        "centos" => {
            if let os_info::Version::Semantic(major, _, _) = sysinfo.version {
                if major >= 7 {
                    return Some(format!("centos{major}"));
                }
            }
            None
        }
        "rocky" => {
            // rocky linux is distributed under rhel, starting support at v9
            if let os_info::Version::Semantic(major, _, _) = sysinfo.version {
                if major >= 9 {
                    return Some(format!("rhel{major}"));
                }
            }
            None
        }
        "opensuse" | "suse" => {
            // both suse OsType's are distributed under opensuse
            if let os_info::Version::Semantic(major, minor, _) = sysinfo.version {
                if (major >= 15) && (minor >= 5) {
                    return Some(format!("opensuse{major}{minor}"));
                }
            }
            None
        }
        "redhat" => {
            // Redhat linux v7&8 are under centos. distribution changed as of v9
            if let os_info::Version::Semantic(major, _, _) = sysinfo.version {
                if major >= 9 {
                    return Some(format!("rhel{major}"));
                }
                if major >= 7 {
                    return Some(format!("centos{major}"));
                }
            }
            None
        }
        // ubuntu and debian are distributed under their codenames
        "ubuntu" | "debian" => sysinfo.codename().map(|x| x.to_string()),
        _ => None,
    }
}

/// # Get the path to the binary version of the file provided, when available.
///
/// ## Given a CRAN-type repository URL, the location of the file wanted depends on the operating system.
/// Nuances are also encoded for the few recognized repositories in RepoServer's variants.
///
/// ### Windows
/// Windows binaries are found under `/bin/windows/contrib/<R version major>.<R version minor>`
///
/// ### MacOS
/// Binaries for arm64 processors are found under `/bin/macosx/big-sur-arm64/contrib/4.<R minor version>`
///
/// Binaries for x86_64 processors are found under different paths depending on the R version
/// * For R <= 4.2, binaries are found under `/bin/macosx/contrib/4.<R minor version>`
///
/// * For R > 4.2, binaries are found under `/bin/macosx/big-sur-x86_64/contrib/4.<R minor version>`
///
/// Currently, the Mac version is hard coded to Big Sur. Earlier versions are archived for earlier versions of R,
/// but are not supported in this tooling. Later versions (sequoia) are also not yet differentiated
///
/// ### Linux
/// Linux binaries are not widely supported, but `rv` will support under the Posit Package Manager spec for the ubuntu codename.
/// See https://docs.posit.co/rspm/admin/serving-binaries.html#using-linux-binary-packages
///
/// In order to provide the correct binary for the R version and system architecture, PPM and PRISM servers use query strings or the form `r_version=<R version major>.<R version minor>` and `arch=<system arch>`
///
/// Thus the full path segment is `__linux__/<distribution codename>/<snapshot date>/src/contrib/<file name>?r_version=<R version major>.<R version minor>&arch=<system arch>`
fn get_binary_path(
    url: &str,
    file_path: &str,
    r_version: &[u32; 2],
    sysinfo: &SystemInfo,
) -> Option<String> {
    // rv does not support binaries for less than R/3.6
    if r_version < &[3, 6] {
        return None;
    }

    match sysinfo.os_type {
        OsType::Windows => Some(get_windows_url(url, file_path, r_version)),
        OsType::MacOs => get_mac_url(url, file_path, r_version, sysinfo),
        OsType::Linux(distro) => get_linux_url(url, file_path, r_version, sysinfo, distro),
        OsType::Other(_) => None,
    }
}

fn get_binary_tarball_path(
    url: &str,
    name: &str,
    version: &str,
    path: Option<&str>,
    r_version: &[u32; 2],
    sysinfo: &SystemInfo,
) -> Option<String> {
    let path = path.map(|x| format!("{x}/")).unwrap_or_default();
    let ext = sysinfo.os_type.tarball_extension();
    let file_path = format!("{path}{name}_{version}.{ext}");

    get_binary_path(url, &file_path, r_version, sysinfo)
}

fn get_source_path(url: &str, file_path: &str) -> String {
    // even if __linux__ is contained within the url, source content will be returned because no query string for PPM and PRISM
    format!("{url}/src/contrib/{file_path}")
}

fn get_source_tarball_path(url: &str, name: &str, version: &str, path: Option<&str>) -> String {
    let path = path.map(|x| format!("{x}/")).unwrap_or_default();
    let file_path = format!("{path}{name}_{version}.tar.gz");
    get_source_path(url, &file_path)
}

// Archived packages under the format <base url>/src/contrib/Archive/<pkg name>/<pkg name>_<pkg version>.tar.gz
fn get_archive_tarball_path(url: &str, name: &str, version: &str) -> String {
    let file_path = format!("Archive/{name}/{name}_{version}.tar.gz");
    get_source_path(url, &file_path)
}

fn get_windows_url(url: &str, file_path: &str, r_version: &[u32; 2]) -> String {
    let [r_major, r_minor] = r_version;
    format!("{url}/bin/windows/contrib/{r_major}.{r_minor}/{file_path}")
}

/// CRAN-type repositories have had to adapt to the introduction of the Mac arm64 processors
/// For x86_64 processors, a split in the path to the binaries occurred at R/4.2:
/// * R <= 4.2, the path is `/bin/macosx/contrib/4.<R minor version>`
/// * R > 4.2, the path is `/bin/macosx/big-sur-x86_64/contrib/4.<R minor version>`
///
/// This split occurred to mirror the new path pattern for arm64 processors.
/// The path to the binaries built for arm64 binaries is `/bin/macosx/big-sur-arm64/contrib/4.<R minor version>`
/// While CRAN itself only started supporting arm64 binaries at R/4.2, many repositories (including PPM) support binaries for older versions
fn get_mac_url(
    url: &str,
    file_path: &str,
    r_version: &[u32; 2],
    sysinfo: &SystemInfo,
) -> Option<String> {
    // If the system architecture cannot be determined, Mac binaries are not supported
    let arch = sysinfo.arch()?;
    let [r_major, r_minor] = r_version;

    // If the processor is arm64, binaries will only be found on this path
    // CRAN does not officially support arm64 binaries until R/4.2, but other repositories may (i.e. PPM does)
    if arch == "arm64" {
        return Some(format!(
            "{url}/bin/macosx/big-sur-{arch}/contrib/{r_major}.{r_minor}/{file_path}",
        ));
    }

    // For x86_64, the path in which binaries are found switches after R/4.2
    if r_version <= &[4, 2] {
        return Some(format!(
            "{url}/bin/macosx/contrib/{r_major}.{r_minor}/{file_path}",
        ));
    }

    Some(format!(
        "{url}/bin/macosx/big-sur-{arch}/contrib/{r_major}.{r_minor}/{file_path}",
    ))
}

fn get_linux_url(
    url: &str,
    file_path: &str,
    r_version: &[u32; 2],
    sysinfo: &SystemInfo,
    distro: &str,
) -> Option<String> {
    let [r_major, r_minor] = r_version;
    let arch_query = sysinfo
        .arch()
        .map(|arch| format!("&arch={arch}"))
        .unwrap_or_default();

    // if the url already contains __linux__, then we assume the user supplied the distro name purposefully
    if url.contains("__linux__") {
        return Some(format!(
            "{url}/src/contrib/{file_path}?r_version={r_major}.{r_minor}{arch_query}"
        ));
    }
    let mut parts = url.split('/').collect::<Vec<_>>();
    // split on `/`` will split "https://..." as 3 parts. Want to ensure there is at least one more path element at end of url
    if parts.len() < 4 {
        return None;
    }
    let edition = parts.pop()?;
    let base_url = parts.join("/");
    let distro_name = get_distro_name(sysinfo, distro)?;

    Some(format!(
        "{base_url}/__linux__/{distro_name}/{edition}/src/contrib/{file_path}?r_version={r_major}.{r_minor}{arch_query}"
    ))
}

pub struct TarballUrls {
    pub source: String,
    pub binary: Option<String>,
    pub archive: String,
}

pub fn get_tarball_urls(
    dep: &ResolvedDependency,
    r_version: &[u32; 2],
    sysinfo: &SystemInfo,
) -> TarballUrls {
    let url = dep.source.source_path();
    let name = &dep.name;
    let version = &dep.version.original;
    let path = dep.path.as_deref();

    TarballUrls {
        source: get_source_tarball_path(url, name, version, path),
        binary: get_binary_tarball_path(url, name, version, path, r_version, sysinfo),
        archive: get_archive_tarball_path(url, name, version),
    }
}

/// Gets the source/binary url for the given filename, usually PACKAGES
/// Use `get_tarball_urls` if you want to get the package tarballs URLs
pub fn get_package_file_urls(
    url: &str,
    r_version: &[u32; 2],
    sysinfo: &SystemInfo,
) -> (String, Option<String>) {
    (
        get_source_path(url, PACKAGE_FILENAME),
        get_binary_path(url, PACKAGE_FILENAME, r_version, sysinfo),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    static PPM_URL: &str = "https://packagemanager.posit.co/cran/latest";
    static TEST_FILE_NAME: &str = "test-file";

    #[test]
    fn test_source_url() {
        let source_url = get_source_path(PPM_URL, TEST_FILE_NAME);
        let ref_url = format!("{}/src/contrib/{}", PPM_URL, TEST_FILE_NAME);
        assert_eq!(source_url, ref_url);
    }
    #[test]
    fn test_binary_35_url() {
        let sysinfo = SystemInfo::new(
            OsType::Linux("ubuntu"),
            Some("x86_64".to_string()),
            Some("jammy".to_string()),
            "22.04",
        );
        assert_eq!(
            get_binary_path(PPM_URL, TEST_FILE_NAME, &[3, 5], &sysinfo),
            None
        );
    }

    #[test]
    fn test_windows_url() {
        let sysinfo = SystemInfo::new(OsType::Windows, Some("x86_64".to_string()), None, "");
        let source_url = get_binary_path(PPM_URL, TEST_FILE_NAME, &[4, 4], &sysinfo);
        let ref_url = format!("{}/bin/windows/contrib/4.4/{}", PPM_URL, TEST_FILE_NAME);
        assert_eq!(source_url, Some(ref_url))
    }

    #[test]
    fn test_mac_x86_64_r41_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("x86_64".to_string()), None, "");
        let source_url = get_binary_path(PPM_URL, TEST_FILE_NAME, &[4, 1], &sysinfo);
        let ref_url = format!("{}/bin/macosx/contrib/4.1/{}", PPM_URL, TEST_FILE_NAME);
        assert_eq!(source_url, Some(ref_url));
    }
    #[test]
    fn test_mac_arm64_r41_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("arm64".to_string()), None, "");
        let source_url = get_binary_path(PPM_URL, TEST_FILE_NAME, &[4, 1], &sysinfo);
        let ref_url = format!(
            "{}/bin/macosx/big-sur-arm64/contrib/4.1/{}",
            PPM_URL, TEST_FILE_NAME
        );
        assert_eq!(source_url, Some(ref_url));
    }

    #[test]
    fn test_mac_x86_64_r44_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("x86_64".to_string()), None, "");
        let source_url = get_binary_path(PPM_URL, TEST_FILE_NAME, &[4, 4], &sysinfo);
        let ref_url = format!(
            "{}/bin/macosx/big-sur-x86_64/contrib/4.4/{}",
            PPM_URL, TEST_FILE_NAME,
        );
        assert_eq!(source_url, Some(ref_url));
    }

    #[test]
    fn test_mac_arm64_r44_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("arm64".to_string()), None, "");
        let source_url = get_binary_path(PPM_URL, TEST_FILE_NAME, &[4, 4], &sysinfo);
        let ref_url = format!(
            "{}/bin/macosx/big-sur-arm64/contrib/4.4/{}",
            PPM_URL, TEST_FILE_NAME
        );
        assert_eq!(source_url, Some(ref_url));
    }

    #[test]
    fn test_linux_binaries_url() {
        let sysinfo = SystemInfo::new(
            OsType::Linux("ubuntu"),
            Some("x86_64".to_string()),
            Some("jammy".to_string()),
            "22.04",
        );
        let source_url = get_binary_path(PPM_URL, TEST_FILE_NAME, &[4, 2], &sysinfo);
        let ref_url = "https://packagemanager.posit.co/cran/__linux__/jammy/latest/src/contrib/test-file?r_version=4.2&arch=x86_64".to_string();
        assert_eq!(source_url, Some(ref_url))
    }

    #[test]
    fn test_linux_cran_url() {
        let sysinfo = SystemInfo::new(
            OsType::Linux("ubuntu"),
            Some("x86_64".to_string()),
            Some("jammy".to_string()),
            "22.04",
        );
        let source_url = get_binary_path(
            "https://cran.rstudio.com",
            TEST_FILE_NAME,
            &[4, 4],
            &sysinfo,
        );
        assert_eq!(source_url, None);
    }
}
