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

/// Build an `RInstall` from a binary path by querying its version.
/// Tries `R --version` first and falls back to reading `Rversion.h` for devel builds.
/// Returns `None` if the binary can't be run or its version can't be determined.
fn r_install_from_bin(bin_path: PathBuf) -> Option<RInstall> {
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

    r_install_from_bin(bin_path)
}

/// The version in a rig quick link's file name, if the name looks like one at all.
fn rig_quick_link_version(file_name: &str) -> Option<Version> {
    let captures = RIG_QUICK_LINK_RE.captures(file_name)?;
    Version::from_str(captures.get(1)?.as_str()).ok()
}

/// Look on PATH for a rig versioned quick link matching `version` and return it if it
/// runs and hazy-matches. rig writes one of these links (symlinks on macOS/Linux, `.bat`
/// files on Windows) for every version it manages, so this finds a project's pinned
/// version even when it isn't rig's global default. We scan PATH directories directly
/// rather than looking up a name because the patch/arch suffix isn't known in advance.
fn get_versioned_r_from_path(version: &Version, use_devel: bool) -> Option<RInstall> {
    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.filter_map(Result::ok) {
            // The name only decides which candidates are worth spawning a process for,
            // and only down to major.minor: on macOS the link is per-minor (`R-4.5` for
            // an install that really is 4.5.2), links can go stale when a version is
            // removed, and anyone can drop their own `R-4.5` on PATH. So the version we
            // read back out of the binary is what actually decides.
            if entry
                .file_name()
                .to_str()
                .and_then(rig_quick_link_version)
                .is_some_and(|v| v.major_minor() == version.major_minor())
                && let Some(r) = r_install_from_bin(entry.path())
                && version.hazy_match(&r.version)
                && use_devel == r.is_devel
            {
                return Some(r);
            }
        }
    }

    None
}

/// Where, inside a single R version's directory, that version's `Rversion.h` and its `R`
/// binary live. Installers nest R differently, so every root says which layouts to probe.
#[derive(Clone, Copy)]
struct Layout {
    header_dir: &'static str,
    bin_dir: &'static str,
}

/// The official Windows installer, rig's user mode, and rig everywhere it does not use
/// a framework.
const LAYOUT_PLAIN: Layout = Layout {
    header_dir: "include",
    bin_dir: "bin",
};
/// macOS frameworks, where everything sits under `Resources`.
#[allow(dead_code)] // macOS only
const LAYOUT_FRAMEWORK: Layout = Layout {
    header_dir: "Resources/include",
    bin_dir: "Resources/bin",
};
/// Homebrew kegs, which keep the whole of `R_HOME` under `lib/R`.
const LAYOUT_NESTED: Layout = Layout {
    header_dir: "lib/R/include",
    bin_dir: "lib/R/bin",
};
/// Posit's Linux builds (and rig's admin mode on Linux): `R_HOME` under `lib/R`, but
/// with the launcher hoisted to the top level.
#[allow(dead_code)] // Linux only
const LAYOUT_OPT_R: Layout = Layout {
    header_dir: "lib/R/include",
    bin_dir: "bin",
};

/// Every directory we know of that holds one subdirectory per R version, paired with the
/// layouts to probe inside those subdirectories.
fn known_r_roots() -> Vec<(PathBuf, &'static [Layout])> {
    let mut roots: Vec<(PathBuf, &'static [Layout])> = Vec::new();

    #[cfg(target_os = "macos")]
    {
        // rig and the official installer use /Library/Frameworks/R.framework/Versions/
        roots.push((
            PathBuf::from("/Library/Frameworks/R.framework/Versions"),
            &[LAYOUT_FRAMEWORK],
        ));
        // Homebrew on Apple Silicon uses /opt/homebrew/Cellar/r/
        roots.push((PathBuf::from("/opt/homebrew/Cellar/r"), &[LAYOUT_NESTED]));
    }

    #[cfg(target_os = "linux")]
    {
        // rig on Linux uses /opt/R/{version}/
        roots.push((PathBuf::from("/opt/R"), &[LAYOUT_OPT_R]));
    }

    #[cfg(target_os = "windows")]
    {
        // rig and the official installer put versions in R\R-{version}\, system-wide
        // under Program Files and per-user under %LOCALAPPDATA%\Programs.
        roots.push((PathBuf::from(r"C:\Program Files\R"), &[LAYOUT_PLAIN]));
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            roots.push((
                PathBuf::from(local_app_data).join("Programs").join("R"),
                &[LAYOUT_PLAIN],
            ));
        }
    }

    // rig user-mode installs (and any RIG_R_INSTALL_DIR override) live outside the system
    // locations, one plain-version subdirectory per version. Mirrors rig's
    // `get_r_install_dir` (rig `src/utils.rs`): the RIG_R_INSTALL_DIR override on any
    // platform, plus the user-mode default of `$HOME/.local/share/rig/r` on macOS/Linux
    // (rig uses `$HOME` directly, not `$XDG_DATA_HOME`) or `%APPDATA%\rig\data\r` on
    // Windows. macOS user mode flattens the framework, so probe the nested layout too.
    //
    // NOTE: Locations as of rig v0.10.0-alpha2. User mode is new in rig 0.10.0, which is
    // still a pre-release, so these roots do nothing on an admin-mode setup.
    const RIG_USER_LAYOUTS: &[Layout] = &[LAYOUT_PLAIN, LAYOUT_NESTED];

    if let Some(dir) = std::env::var_os("RIG_R_INSTALL_DIR") {
        roots.push((PathBuf::from(dir), RIG_USER_LAYOUTS));
    }

    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push((
            PathBuf::from(appdata).join("rig").join("data").join("r"),
            RIG_USER_LAYOUTS,
        ));
    }

    #[cfg(not(windows))]
    if let Some(home) = std::env::var_os("HOME") {
        roots.push((
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("rig")
                .join("r"),
            RIG_USER_LAYOUTS,
        ));
    }

    roots
}

/// Scan a directory whose subdirectories are individual R versions, reading each one's
/// version from its `Rversion.h`.
fn scan_versioned_root(root: &Path, layouts: &[Layout]) -> Vec<RInstall> {
    let bin_name = if cfg!(windows) { "R.exe" } else { "R" };
    let mut installs = Vec::new();

    let Ok(entries) = std::fs::read_dir(root) else {
        return installs;
    };

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        // Skip symlinked version directories, e.g. macOS' `Current`, so that we record
        // the concrete version rather than a path that moves when the default changes.
        if path.is_symlink() {
            continue;
        }

        for layout in layouts {
            let bin_path = path.join(layout.bin_dir).join(bin_name);
            if !bin_path.exists() {
                continue;
            }
            let header = path.join(layout.header_dir).join("Rversion.h");
            if let Some((version, is_devel)) = read_version_from_header(&header) {
                installs.push(RInstall {
                    bin_path,
                    version,
                    is_devel,
                });
                break;
            }
        }
    }

    installs
}

/// Get rig/homebrew installed R versions by looking at where they are installed and looking up
/// the header
fn scan_known_r_locations() -> Vec<RInstall> {
    known_r_roots()
        .iter()
        .flat_map(|(root, layouts)| scan_versioned_root(root, layouts))
        .collect()
}

/// Find the R installation that matches the given parameters. Return None if nothing matches.
pub fn find_r_install(version: &Version, use_devel: bool) -> Option<RInstall> {
    // First look for a rig versioned quick link (e.g. `R-4.5`, `R-4.5.1`, `R-4.5.1.bat`)
    // on PATH. rig keeps one for every version it manages, so this finds a project's
    // pinned version even when it isn't rig's global default (the common rig failure, #487).
    if let Some(r) = get_versioned_r_from_path(version, use_devel) {
        log::debug!(
            "Versioned R shim on PATH matches: {} (use_devel={use_devel})",
            r.version.original
        );
        return Some(r);
    }

    // Then check the plain `R` on PATH (rig's global default, or a system install).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rig_quick_link_version_parses_link_shapes() {
        // macOS (per-minor framework, optional arch suffix), Linux (full patch),
        // Windows (full patch + .bat) — all of rig's naming shapes.
        for (name, expected) in [
            ("R-4.5", "4.5"),
            ("R-4.5-arm64", "4.5"),
            ("R-4.5-x86_64", "4.5"),
            ("R-4.5.1", "4.5.1"),
            ("R-4.5.1.bat", "4.5.1"),
        ] {
            let version = rig_quick_link_version(name)
                .unwrap_or_else(|| panic!("expected {name} to be recognised"));
            assert_eq!(version.original, expected);
        }
    }

    #[test]
    fn rig_quick_link_version_rejects_other_names() {
        // rig's non-numeric aliases, other binaries and near misses must not be picked up.
        for name in [
            "R",
            "R-release",
            "R-devel",
            "Rscript-4.5",
            "R-4.5.exe",
            "R-4",
            "R-4.5.1.2",
            "xR-4.5",
        ] {
            assert!(
                rig_quick_link_version(name).is_none(),
                "expected {name} not to be recognised"
            );
        }
    }

    #[test]
    fn rig_quick_link_version_distinguishes_neighbouring_versions() {
        // The pre-filter compares major.minor, so `R-4.55` must not pass for 4.5.
        let wanted = Version::from_str("4.5.1").unwrap().major_minor();
        for name in ["R-4.55", "R-4.55.1", "R-4.6.1", "R-3.4.5"] {
            let version = rig_quick_link_version(name).unwrap();
            assert_ne!(version.major_minor(), wanted, "{name} should not match 4.5");
        }
        assert_eq!(
            rig_quick_link_version("R-4.5").unwrap().major_minor(),
            wanted
        );
    }
}
