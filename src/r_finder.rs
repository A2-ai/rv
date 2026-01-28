//! We want to do 2 things in this module.
//! 1. Find what the `R` in the $PATH is (if there is one)
//! 2. Find all the various `R` installations on the system and see if we can find
//!    a hazy match for the version defined in `rproject.toml`
//!
//! For 1, a subtlety is that the R command might not have a version if it's a devel version
//! (it will have "R Under development" instead).
//!
//! For the version in path, we can do `R --version` and extract the version number from it if present
//! but for the others (eg found via rig install paths or in /opt/ on Linux) we can read a header called `Rversion.h`
//! which contains all the necessary info. Depending on how R is installed we might not be able
//! to find the header easily since location will depend on distro etc.

use std::fmt::{Debug, Display, Formatter};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::LazyLock;

use regex::Regex;

use crate::Version;
use crate::r_cmd::RCmd;

static R_MAJOR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"#define R_MAJOR\s+"(\d+)""#).unwrap());
static R_MINOR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"#define R_MINOR\s+"(\d+\.\d+)""#).unwrap());
static R_STATUS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"#define R_STATUS\s+"([^"]*)""#).unwrap());

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct RInstall {
    pub bin_path: PathBuf,
    pub version: Version,
    pub is_devel: bool,
}

impl RInstall {
    pub fn default_from_path() -> Self {
        #[cfg(windows)]
        let bin_path = if which::which("R.bat").is_ok() {
            PathBuf::from("R.bat")
        } else {
            PathBuf::from("R")
        };

        #[cfg(not(windows))]
        let bin_path = PathBuf::from("R");

        Self {
            bin_path,
            version: Version::default(),
            is_devel: false,
        }
    }
}

impl Display for RInstall {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?}, version={}, is_devel={}",
            self.bin_path, self.version.original, self.is_devel
        )
    }
}

/// Read version and is_devel from Rversion.h header file
fn read_version_from_header(header_path: &Path) -> Option<(Version, bool)> {
    let content = std::fs::read_to_string(header_path).ok()?;

    let major = R_MAJOR_RE.captures(&content)?.get(1)?.as_str();
    let minor = R_MINOR_RE.captures(&content)?.get(1)?.as_str();
    let status = R_STATUS_RE
        .captures(&content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .unwrap_or("");

    let version = Version::from_str(&format!("{major}.{minor}")).ok()?;
    let is_devel = !status.is_empty();

    Some((version, is_devel))
}

/// Get R from PATH - try R --version first, fallback to header for devel
pub fn get_r_from_path() -> Option<RInstall> {
    #[cfg(windows)]
    let bin_path = if which::which("R.bat").is_ok() {
        PathBuf::from("R.bat")
    } else {
        PathBuf::from("R")
    };

    #[cfg(not(windows))]
    let bin_path = PathBuf::from("R");
    let mut r_cmd = RInstall {
        bin_path,
        is_devel: false,
        version: Version::default(),
    };

    match r_cmd.version() {
        Ok(Some(version)) => {
            r_cmd.version = version;
            Some(r_cmd)
        }
        Ok(None) => {
            // Devel - need header for version
            // get_r_library() returns {RHOME}/library, so we get parent to get RHOME
            let library_path = r_cmd.get_r_library().ok()?;
            let rhome = library_path.parent()?;
            let header = rhome.join("include").join("Rversion.h");
            let (version, is_devel) = read_version_from_header(&header)?;
            r_cmd.version = version;
            r_cmd.is_devel = is_devel;
            Some(r_cmd)
        }
        Err(_) => None,
    }
}

/// Get rig/homebrew installed R versions by looking at where they are installed and looking up
/// the header
fn scan_known_r_locations() -> Vec<RInstall> {
    let mut installs = Vec::new();

    #[cfg(target_os = "macos")]
    {
        // rig on macOS uses /Library/Frameworks/R.framework/Versions/
        let root = PathBuf::from("/Library/Frameworks/R.framework/Versions");
        if root.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&root) {
                for entry in entries.filter_map(Result::ok) {
                    let path = entry.path();
                    // Skip "Current" symlink
                    if path.is_symlink() {
                        continue;
                    }
                    let header = path.join("Resources").join("include").join("Rversion.h");
                    if header.exists() {
                        if let Some((version, is_devel)) = read_version_from_header(&header) {
                            let bin_path = path.join("Resources").join("bin").join("R");
                            if bin_path.exists() {
                                installs.push(RInstall {
                                    bin_path,
                                    version,
                                    is_devel,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Homebrew on Apple Silicon uses /opt/homebrew/Cellar/r/
        let homebrew_root = PathBuf::from("/opt/homebrew/Cellar/r");
        if homebrew_root.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&homebrew_root) {
                for entry in entries.filter_map(Result::ok) {
                    let path = entry.path();
                    let header = path
                        .join("lib")
                        .join("R")
                        .join("include")
                        .join("Rversion.h");
                    if header.exists() {
                        if let Some((version, is_devel)) = read_version_from_header(&header) {
                            let bin_path = path.join("lib").join("R").join("bin").join("R");
                            if bin_path.exists() {
                                installs.push(RInstall {
                                    bin_path,
                                    version,
                                    is_devel,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // rig on Linux uses /opt/R/{version}/
        let root = PathBuf::from("/opt/R");
        if root.is_dir()
            && let Ok(entries) = std::fs::read_dir(&root)
        {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                let header = path
                    .join("lib")
                    .join("R")
                    .join("include")
                    .join("Rversion.h");
                if header.exists()
                    && let Some((version, is_devel)) = read_version_from_header(&header)
                {
                    let bin_path = path.join("bin").join("R");
                    if bin_path.exists() {
                        installs.push(RInstall {
                            bin_path,
                            version,
                            is_devel,
                        });
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // rig on Windows uses C:\Program Files\R\R-{version}\
        let root = PathBuf::from(r"C:\Program Files\R");
        if root.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&root) {
                for entry in entries.filter_map(Result::ok) {
                    let path = entry.path();
                    let header = path.join("include").join("Rversion.h");
                    if header.exists() {
                        if let Some((version, is_devel)) = read_version_from_header(&header) {
                            let bin_path = path.join("bin").join("R.exe");
                            if bin_path.exists() {
                                installs.push(RInstall {
                                    bin_path,
                                    version,
                                    is_devel,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    installs
}

/// Find the R installation that matches the given parameters. Return None if nothing matches.
pub fn find_r_install(version: &Version, use_devel: bool) -> Option<RInstall> {
    // First check the R on PATH to see if it matches what we have in the config.
    if let Some(r) = get_r_from_path()
        && version.hazy_match(&r.version)
        && use_devel == r.is_devel
    {
        log::debug!(
            "R in PATH matches: {} (use_devel={use_devel})",
            r.version.original
        );
        return Some(r);
    }

    // Otherwise use known installation location to figure it out and return the first one that
    // kinda matches
    let r_installs = scan_known_r_locations();

    for r in &r_installs {
        if version.hazy_match(&r.version) && use_devel == r.is_devel {
            log::debug!(
                "R in {:?} matches: {} (use_devel={use_devel})",
                r.bin_path,
                r.version.original
            );
            return Some(r.clone());
        }
    }

    log::debug!(
        "No R version found matching {}. Found {}",
        version.original,
        r_installs
            .into_iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    None
}
