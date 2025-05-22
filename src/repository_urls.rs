use std::error::Error;

use crate::consts::PACKAGE_FILENAME;
use crate::lockfile::Source;
use crate::{OsType, ResolvedDependency, SystemInfo};
use url::Url;

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

fn get_source_path(url: &Url, file_path: &[&str]) -> Url {
    // even if __linux__ is contained within the url, source content will be returned because no query string for PPM and PRISM
    let mut new_url = url.clone();
    let mut segments = new_url.path_segments_mut().expect("Valid absolute url");
    segments.extend(["src", "contrib"].iter().chain(file_path));
    drop(segments);
    new_url
}

// Archived packages under the format <base url>/src/contrib/Archive/<pkg name>/<pkg name>_<pkg version>.tar.gz
fn get_archive_tarball_path(url: &Url, name: &str, version: &str) -> Url {
    let file_name = format!("{name}_{version}.tar.gz");
    get_source_path(url, &["Archive", name, &file_name])
}

/// # Get the path to the binary version of the file provided, when available.
///
/// ## Given a CRAN-type repository URL, the location of the file wanted depends on the operating system.
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
    url: &Url,
    // file_path must contain the file name and prepended by any additional path elements (Path arg in PACKAGES file)
    file_path: &[&str],
    r_version: &[u32; 2],
    sysinfo: &SystemInfo,
) -> Option<Url> {
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

fn get_windows_url(url: &Url, file_path: &[&str], r_version: &[u32; 2]) -> Url {
    let mut new_url = url.clone();
    {
        let mut segments = new_url.path_segments_mut().expect("Valid absolute url");
        segments.extend(["bin", "windows", "contrib", &format!("{}.{}", r_version[0], r_version[1])].iter().chain(file_path));
    }
    new_url
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
    url: &Url,
    file_path: &[&str],
    r_version: &[u32; 2],
    sysinfo: &SystemInfo,
) -> Option<Url> {
    // If the system architecture cannot be determined, Mac binaries are not supported
    let arch = sysinfo.arch()?;

    let mut new_url = url.clone();

    {
        let mut segments = new_url.path_segments_mut().ok()?;
        segments.extend(["bin", "macosx"].iter());


        // The additional path element containing the arch is officially introduced for R > 4.3.
        // Some sources (PPM for example) start to include arch earlier for arm64.
        // Therefore, we only do not include the additional path element for x86_64 with R <= 4.2
        if !(arch == "x86_64" && r_version <= &[4, 2]) {
            segments.push(&format!("big-sur-{arch}"));
        }

        segments.extend(["contrib", &format!("{}.{}", r_version[0], r_version[1])].iter().chain(file_path));
    }

    Some(new_url)
}

fn get_linux_url(
    url: &Url,
    file_path: &[&str],
    r_version: &[u32; 2],
    sysinfo: &SystemInfo,
    distro: &str,
) -> Option<Url> {
    let [r_major, r_minor] = r_version;
    let mut new_url = url.clone();

    let insert_query_strings = |url_mut: &mut Url| {
        let mut pairs = url_mut.query_pairs_mut();
        pairs.append_pair("r_version", &format!("{r_major}.{r_minor}"));
        if let Some(arch) = sysinfo.arch() {
            pairs.append_pair("arch", arch);
        }
    };

    let path_segs = url.path_segments()?.collect::<Vec<_>>();
    if path_segs.iter().any(|s| *s == "__linux__") {
        let segments = ["src", "contrib"].iter().chain(file_path);
        new_url.path_segments_mut().ok()?.extend(segments);
        insert_query_strings(&mut new_url);
        return Some(new_url)
    }

    let distro_name = get_distro_name(sysinfo, distro)?;
    let mut segments = url.path_segments()?.collect::<Vec<_>>();

    // if there is not at least one path segment, we cannot determine the linux binary url
    let edition = segments.pop()?;
    segments.push("__linux__");
    segments.push(&distro_name);
    segments.push(edition);
    segments.extend(["src", "contrib"]);
    segments.extend(file_path);


    new_url.path_segments_mut().ok()?.clear().extend(segments);

    insert_query_strings(&mut new_url);


    Some(new_url)
}

pub struct TarballUrls {
    pub source: Url,
    pub binary: Option<Url>,
    pub archive: Url,
}

pub fn get_tarball_urls(
    dep: &ResolvedDependency,
    r_version: &[u32; 2],
    sysinfo: &SystemInfo,
) -> Result<TarballUrls, Box<dyn Error>> {
    if let Source::Repository { repository } = &dep.source {
        let name = &dep.name;
        let version = &dep.version.original;
        let path = dep.path.as_deref();
        let ext = sysinfo.os_type.tarball_extension();

        let file_path = path
            .map(|p| p.split('/')
                .into_iter()
                .collect::<Vec<_>>()
            )
            .unwrap_or_default();

            let mut binary_file_path = file_path.clone();
            let binary_name = format!("{name}_{version}.{ext}");
            binary_file_path.push(&binary_name);

            let mut source_file_path = file_path.clone();
            let source_name = format!("{name}_{version}.tar.gz");
            source_file_path.push(&source_name);

        Ok(TarballUrls { 
            source: get_source_path(repository, &source_file_path), 
            binary: get_binary_path(repository, &binary_file_path, r_version, sysinfo), 
            archive: get_archive_tarball_path(repository, name, version),
        })
    } else {
        Err("Dependency does not have source Repository".into())
    }
}

/// Gets the source/binary url for the given filename, usually PACKAGES
/// Use `get_tarball_urls` if you want to get the package tarballs URLs
pub fn get_package_file_urls(
    url: &Url,
    r_version: &[u32; 2],
    sysinfo: &SystemInfo,
) -> (Url, Option<Url>) {
    (
        get_source_path(url, &[PACKAGE_FILENAME]),
        get_binary_path(url, &[PACKAGE_FILENAME], r_version, sysinfo)
    )
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;
    static PPM_URL: LazyLock<Url> = LazyLock::new(|| Url::parse("https://packagemanager.posit.co/cran/latest").unwrap());
    static TEST_FILE_NAME: [&str; 1] = ["test-file"];

    #[test]
    fn test_source_url() {
        let source_url = get_source_path(&PPM_URL, &TEST_FILE_NAME);
        let ref_url = format!("{}/src/contrib/{}", &PPM_URL.as_str(), TEST_FILE_NAME[0]);
        assert_eq!(source_url.as_str(), ref_url);
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
            get_binary_path(&PPM_URL, &TEST_FILE_NAME, &[3, 5], &sysinfo),
            None
        );
    }

    #[test]
    fn test_windows_url() {
        let sysinfo = SystemInfo::new(OsType::Windows, Some("x86_64".to_string()), None, "");
        let source_url = get_binary_path(&PPM_URL, &TEST_FILE_NAME, &[4, 4], &sysinfo).unwrap();
        let ref_url = format!("{}/bin/windows/contrib/4.4/{}", PPM_URL.as_str(), TEST_FILE_NAME[0]);
        assert_eq!(source_url.as_str(), ref_url)
    }

    #[test]
    fn test_mac_x86_64_r41_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("x86_64".to_string()), None, "");
        let source_url = get_binary_path(&PPM_URL, &TEST_FILE_NAME, &[4, 1], &sysinfo).unwrap();
        let ref_url = format!("{}/bin/macosx/contrib/4.1/{}", PPM_URL.as_str(), TEST_FILE_NAME[0]);
        assert_eq!(source_url.as_str(), ref_url);
    }
    #[test]
    fn test_mac_arm64_r41_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("arm64".to_string()), None, "");
        let source_url = get_binary_path(&PPM_URL, &TEST_FILE_NAME, &[4, 1], &sysinfo).unwrap();
        let ref_url = format!(
            "{}/bin/macosx/big-sur-arm64/contrib/4.1/{}",
            PPM_URL.as_str(), TEST_FILE_NAME[0]
        );
        assert_eq!(source_url.as_str(), ref_url);
    }

    #[test]
    fn test_mac_x86_64_r44_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("x86_64".to_string()), None, "");
        let source_url = get_binary_path(&PPM_URL, &TEST_FILE_NAME, &[4, 4], &sysinfo).unwrap();
        let ref_url = format!(
            "{}/bin/macosx/big-sur-x86_64/contrib/4.4/{}",
            PPM_URL.as_str(), TEST_FILE_NAME[0],
        );
        assert_eq!(source_url.as_str(), ref_url);
    }

    #[test]
    fn test_mac_arm64_r44_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("arm64".to_string()), None, "");
        let source_url = get_binary_path(&PPM_URL, &TEST_FILE_NAME, &[4, 4], &sysinfo).unwrap();
        let ref_url = format!(
            "{}/bin/macosx/big-sur-arm64/contrib/4.4/{}",
            PPM_URL.as_str(), TEST_FILE_NAME[0]
        );
        assert_eq!(source_url.as_str(), ref_url);
    }

    #[test]
    fn test_linux_binaries_url() {
        let sysinfo = SystemInfo::new(
            OsType::Linux("ubuntu"),
            Some("x86_64".to_string()),
            Some("jammy".to_string()),
            "22.04",
        );
        let source_url = get_binary_path(&PPM_URL, &TEST_FILE_NAME, &[4, 2], &sysinfo).unwrap();
        let ref_url = "https://packagemanager.posit.co/cran/__linux__/jammy/latest/src/contrib/test-file?r_version=4.2&arch=x86_64".to_string();
        assert_eq!(source_url.as_str(), ref_url)
    }

    #[test]
    // also test the additional path elements being handled properly
    fn test_archive_url() {
        let source_url = get_archive_tarball_path(&PPM_URL, "name", "version");
        let ref_url = format!("{}/src/contrib/Archive/name/name_version.tar.gz", PPM_URL.as_str());
        assert_eq!(source_url.as_str(), ref_url);
    }
}
