//! Resolves the `--r-bin`/`--r-version` flags (and their `RV_R_BIN`/`RV_R_VERSION`
//! env var equivalents, which clap reads and parses via `#[arg(env = ...)]`) into an
//! [`RCommandLookup`].
//!
//! If the R version in use does not match the config or the `--r-version` given,
//! this will error.

use std::path::PathBuf;

use crate::consts::R_VERSION_ENV_VAR_NAME;
use crate::r_finder::RInstall;
use crate::{RCommandLookup, Version};

#[derive(Debug, thiserror::Error)]
pub enum RSelectError {
    #[error(
        "Could not find R version for the given R path: {0:?}. Check that it points to an R executable."
    )]
    RBinNotUsable(PathBuf),
    #[error(
        "The R at {path:?} reports version {actual}, not {requested} as requested via --r-version/${R_VERSION_ENV_VAR_NAME}"
    )]
    RBinVersionMismatch {
        path: PathBuf,
        actual: Version,
        requested: Version,
    },
    #[error(
        "The R at {path:?} is {actual} but the config wants {config}. Pass --r-version {}.{} to {}",
        .actual.major_minor()[0],
        .actual.major_minor()[1],
        if *.needs_toolchain { "actually run, skipping the lockfile." } else { "run with this R." }
    )]
    RBinConfigMismatch {
        path: PathBuf,
        actual: Version,
        config: Version,
        /// Whether the command uses the lockfile
        needs_toolchain: bool,
    },
    #[error(
        "--r-version {requested} does not match the config ({config}). Use --r-bin to run with an incompatible R."
    )]
    RVersionNeedsBin { requested: Version, config: Version },
}

/// Try to match the given flags/env vars for R binary/version against the version in the config.
/// For some commands, we don't actually need the toolchain installed so in those cases we just
/// accept `--r-version` alone even if it doesn't match the config.
///
/// Return None if no flag/env vars are set and let the caller deal wit it.
#[allow(clippy::result_large_err)]
pub fn resolve_r_lookup(
    r_bin: Option<PathBuf>,
    r_version: Option<Version>,
    config_r_version: &Version,
    needs_toolchain: bool,
) -> Result<Option<RCommandLookup>, RSelectError> {
    let install = match r_bin {
        Some(path) => Some(
            RInstall::default_from_given_path(&path).ok_or(RSelectError::RBinNotUsable(path))?,
        ),
        None => None,
    };

    r_lookup(install, r_version, config_r_version, needs_toolchain)
}

/// Returns the right RCommandLookup based on optional overrides.
/// If there are no overrides, returns None.
/// Split from `resolve_r_lookup` so it doesn't shell to R and
#[allow(clippy::result_large_err)]
fn r_lookup(
    r_install: Option<RInstall>,
    r_version: Option<Version>,
    config_r_version: &Version,
    needs_toolchain: bool,
) -> Result<Option<RCommandLookup>, RSelectError> {
    match (r_install, r_version) {
        // The code will find the right lookup later
        (None, None) => Ok(None),
        (None, Some(requested_r_version)) => {
            // If the command doesn't require actual R, let's just go with it
            if !needs_toolchain {
                Ok(Some(RCommandLookup::Soft(requested_r_version)))
            } else if config_r_version.hazy_match(&requested_r_version) {
                // Same R as the config so we ignore the flag
                Ok(Some(RCommandLookup::Strict))
            } else {
                Err(RSelectError::RVersionNeedsBin {
                    requested: requested_r_version,
                    config: config_r_version.clone(),
                })
            }
        }
        (Some(install), None) => {
            if config_r_version.hazy_match(&install.version) {
                Ok(Some(RCommandLookup::Explicit(install)))
            } else {
                Err(RSelectError::RBinConfigMismatch {
                    config: config_r_version.clone(),
                    actual: install.version,
                    path: install.bin_path,
                    needs_toolchain,
                })
            }
        }
        (Some(install), Some(requested)) => {
            if requested.hazy_match(&install.version) {
                Ok(Some(RCommandLookup::Explicit(install)))
            } else {
                Err(RSelectError::RBinVersionMismatch {
                    actual: install.version,
                    path: install.bin_path,
                    requested,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    fn install(version: &str) -> RInstall {
        RInstall {
            bin_path: PathBuf::from("/opt/R/bin/R"),
            version: v(version),
            is_devel: false,
        }
    }

    /// Expected outcome for a matrix row.
    #[derive(Debug)]
    enum Want {
        None,
        Soft(&'static str),
        Strict,
        Explicit(&'static str),
        VersionNeedsBin,
        BinConfigMismatch,
        BinVersionMismatch,
    }

    #[test]
    fn r_lookup_matrix() {
        use RSelectError as E;
        // (bin version, --r-version, config, needs_toolchain, expected)
        #[allow(clippy::type_complexity)]
        let cases: &[(Option<&str>, Option<&str>, &str, bool, Want)] = &[
            (None, None, "4.4", true, Want::None),
            // --r-version alone: Soft on compute commands, no-op if it matches the
            // (major.minor) config on toolchain commands, else needs a binary
            (None, Some("4.6"), "4.4", false, Want::Soft("4.6")),
            (None, Some("4.4"), "4.4", true, Want::Strict),
            (None, Some("4.4.1"), "4.4", true, Want::Strict),
            (None, Some("4.5"), "4.4", true, Want::VersionNeedsBin),
            // --r-bin alone must match the config so the lockfile stays in play
            (Some("4.4.1"), None, "4.4", true, Want::Explicit("4.4.1")),
            (Some("4.5.1"), None, "4.4", true, Want::BinConfigMismatch),
            (Some("4.5.1"), None, "4.4", false, Want::BinConfigMismatch),
            // combo: --r-version asserts the binary and overrides the config
            (
                Some("4.5.1"),
                Some("4.5"),
                "4.4",
                true,
                Want::Explicit("4.5.1"),
            ),
            (
                Some("4.5.1"),
                Some("4.4"),
                "4.4",
                true,
                Want::BinVersionMismatch,
            ),
        ];

        for (bin, requested, config, needs_toolchain, want) in cases {
            let got = r_lookup(
                bin.map(install),
                requested.map(v),
                &v(config),
                *needs_toolchain,
            );
            let label = format!("bin={bin:?} req={requested:?} config={config}");
            match want {
                Want::None => assert_eq!(got.unwrap(), None, "{label}"),
                Want::Soft(x) => {
                    assert_eq!(got.unwrap(), Some(RCommandLookup::Soft(v(x))), "{label}")
                }
                Want::Strict => assert_eq!(got.unwrap(), Some(RCommandLookup::Strict), "{label}"),
                Want::Explicit(x) => {
                    assert_eq!(
                        got.unwrap(),
                        Some(RCommandLookup::Explicit(install(x))),
                        "{label}"
                    )
                }
                Want::VersionNeedsBin => {
                    assert!(matches!(got, Err(E::RVersionNeedsBin { .. })), "{label}")
                }
                Want::BinConfigMismatch => {
                    let err = got.unwrap_err();
                    assert!(matches!(err, E::RBinConfigMismatch { .. }), "{label}");
                    // the error should point the user at the escape hatch
                    assert!(err.to_string().contains("--r-version 4.5"), "{label}");
                    // only lockfile commands mention the lockfile in the hint
                    if *needs_toolchain {
                        assert!(err.to_string().contains("skipping the lockfile"), "{label}");
                    } else {
                        assert!(!err.to_string().contains("lockfile"), "{label}");
                    }
                }
                Want::BinVersionMismatch => {
                    assert!(matches!(got, Err(E::RBinVersionMismatch { .. })), "{label}")
                }
            }
        }
    }
}
