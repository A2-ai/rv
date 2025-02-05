pub const PACKAGE_FILENAME: &str = "PACKAGES";
pub const DESCRIPTION_FILENAME: &str = "DESCRIPTION";
pub const SOURCE_PACKAGES_PATH: &str = "/src/contrib/PACKAGES";
pub const LOCKFILE_NAME: &str = "rv.lock";

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

pub(crate) const DEFAULT_R_PATHS: [&str; 8] = [
    "/usr/lib/R",
    "/usr/lib64/R",
    "/usr/local/lib/R",
    "/usr/local/lib64/R",
    "/opt/local/lib/R",
    "/opt/local/lib64/R",
    "/opt/R/*",
    "/opt/local/R/*",
];

pub(crate) const ADDITIONAL_R_VERSIONS_FILENAME: &str = "/rv/r-versions";
