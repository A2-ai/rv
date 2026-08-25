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

use std::env;
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

/// Matches the file names rig gives its versioned quick links, capturing the version.
/// rig names the link after the install's version, and that shape differs by platform:
/// `R-4.5` (optionally `-arm64`/`-x86_64`, from when CRAN's default macOS arch switched
/// to arm64 at 4.6) on macOS, where frameworks are per-minor; the full `R-4.5.1` on
/// Linux; and `R-4.5.1.bat` on Windows.
static RIG_QUICK_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^R-(\d+\.\d+(?:\.\d+)?)(?:-arm64|-x86_64)?(?:\.bat)?$").unwrap());

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct RInstall {
    pub bin_path: PathBuf,
    pub version: Version,
    pub is_devel: bool,
}

impl RInstall {
    pub fn default_from_env_path() -> Self {
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

    pub fn find_version(mut self) -> Option<Self> {
        match self.version() {
            Ok(Some(version)) => {
                self.version = version;
                Some(self)
            }
            Ok(None) => {
                // Devel - need header for version
                // get_r_library() returns {RHOME}/library, so we get parent to get RHOME
                let library_path = self.get_r_library().ok()?;
                let rhome = library_path.parent()?;
                let header = rhome.join("include").join("Rversion.h");
                let (version, is_devel) = read_version_from_header(&header)?;
                self.version = version;
                self.is_devel = is_devel;
                Some(self)
            }
            Err(_) => None,
        }
    }

    pub fn default_from_given_path<P: AsRef<Path>>(path: P) -> Option<Self> {
        let path = path.as_ref();
        let r_cmd = Self {
            bin_path: path.to_path_buf(),
            version: Version::default(),
            is_devel: false,
        };
        r_cmd.find_version()
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
    let r_cmd = RInstall {
        bin_path,
        is_devel: false,
        version: Version::default(),
    };

    r_cmd.find_version()
}

/// rig can put some R version in the path but the binary will be something like R-4.6
fn get_rig_versioned_r_from_path(version: &Version, use_devel: bool) -> Option<RInstall> {
    let get_version = |filename: String| -> Option<Version> {
        let captures = RIG_QUICK_LINK_RE.captures(&filename)?;
        Version::from_str(captures.get(1)?.as_str()).ok()
    };

    for dir in env::split_paths(&env::var_os("PATH")?) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.filter_map(Result::ok) {
            let Some(v) = entry.file_name().into_string().ok().and_then(get_version) else {
                continue;
            };

            if v.major_minor() == version.major_minor()
                && let Some(r) = RInstall::default_from_given_path(&entry.path())
                && version.hazy_match(&r.version)
                && use_devel == r.is_devel
            {
                return Some(r);
            }
        }
    }

    None
}

/// The last tentative if we can't find anywhere else: we call `rig list --json` if `rig` is in the
/// $PATH
fn get_r_from_rig(version: &Version, use_devel: bool) -> Option<RInstall> {
    which::which("rig").ok()?;

    let output = match std::process::Command::new("rig")
        .args(["list", "--json"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            log::warn!(
                "rig list --json failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return None;
        }
        Err(e) => {
            log::warn!("Could not run rig list --json: {e}");
            return None;
        }
    };

    let value = match serde_json::from_slice::<serde_json::Value>(&output.stdout) {
        Ok(value) => value,
        Err(e) => {
            log::error!("rig list --json didn't return JSON: {e}");
            return None;
        }
    };

    for elem in value.as_array().map(Vec::as_slice).unwrap_or_default() {
        if let Some(bin) = elem
            .get("binary")
            .and_then(|x| x.as_str())
            .and_then(RInstall::default_from_given_path)
        {
            if version.hazy_match(&bin.version) && use_devel == bin.is_devel {
                return Some(bin);
            }
        }
    }

    None
}

/// Get rig/homebrew/official installed R versions by looking at where they are installed and looking up
/// the header
fn scan_known_r_locations() -> Vec<RInstall> {
    let mut installs = Vec::new();

    #[cfg(target_os = "macos")]
    {
        let root = PathBuf::from("/Library/Frameworks/R.framework/Versions");
        if root.is_dir()
            && let Ok(entries) = std::fs::read_dir(&root)
        {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                // Skip "Current" symlink
                if path.is_symlink() {
                    continue;
                }
                let header = path.join("Resources").join("include").join("Rversion.h");
                if header.exists()
                    && let Some((version, is_devel)) = read_version_from_header(&header)
                {
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

        // Homebrew on Apple Silicon uses /opt/homebrew/Cellar/r/
        let homebrew_root = PathBuf::from("/opt/homebrew/Cellar/r");
        if homebrew_root.is_dir()
            && let Ok(entries) = std::fs::read_dir(&homebrew_root)
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

    #[cfg(target_os = "linux")]
    {
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
        let root = PathBuf::from(r"C:\Program Files\R");
        if root.is_dir()
            && let Ok(entries) = std::fs::read_dir(&root)
        {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                let header = path.join("include").join("Rversion.h");
                if header.exists()
                    && let Some((version, is_devel)) = read_version_from_header(&header)
                {
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

    // Otherwise check for rig versions in PATH
    if let Some(r) = get_rig_versioned_r_from_path(version, use_devel) {
        log::debug!(
            "Versioned R in PATH ({}) matches: {} (use_devel={use_devel})",
            r.bin_path.display(),
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

    if let Some(r) = get_r_from_rig(version, use_devel) {
        log::debug!(
            "R found from `rig list` matches: {} (use_devel={use_devel})",
            r.version.original
        );
        return Some(r);
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
