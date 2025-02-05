pub const PACKAGE_FILENAME: &str = "PACKAGES";
pub const DESCRIPTION_FILENAME: &str = "DESCRIPTION";
pub const SOURCE_PACKAGES_PATH: &str = "/src/contrib/PACKAGES";
pub const LOCKFILE_NAME: &str = "rv.lock";

pub const RV_DIR_NAME: &str = "rv";
pub const LIBRARY_ROOT_DIR_NAME: &str = "library";
pub const STAGING_DIR_NAME: &str = "staging";

// Filename where we will stick the max mtime of a local dep
pub(crate) const LOCAL_MTIME_FILENAME: &str = ".rv.mtime";

// List obtained from the REPL: `rownames(installed.packages(priority="base"))`
pub(crate) const BASE_PACKAGES: [&str; 14] = [
    "base",
    "compiler",
    "datasets",
    "grDevices",
    "graphics",
    "grid",
    "methods",
    "parallel",
    "splines",
    "stats",
    "stats4",
    "tcltk",
    "tools",
    "utils",
];
