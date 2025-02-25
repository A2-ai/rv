pub const PACKAGE_FILENAME: &str = "PACKAGES";
pub const DESCRIPTION_FILENAME: &str = "DESCRIPTION";
pub const SOURCE_PACKAGES_PATH: &str = "/src/contrib/PACKAGES";
pub const LOCKFILE_NAME: &str = "rv.lock";

pub const RV_DIR_NAME: &str = "rv";
pub const LIBRARY_ROOT_DIR_NAME: &str = "library";
pub const STAGING_DIR_NAME: &str = "staging";

/// How long are the package databases cached for
/// Same default value as PKGCACHE_TIMEOUT:
/// https://github.com/r-lib/pkgcache?tab=readme-ov-file#package-environment-variables
pub const PACKAGE_TIMEOUT: u64 = 60 * 60;
pub const PACKAGE_TIMEOUT_ENV_VAR_NAME: &str = "PKGCACHE_TIMEOUT";
pub const PACKAGE_DB_FILENAME: &str = "packages.bin";

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

// List obtained from the REPL: `rownames(installed.packages(priority="recommended"))`
pub(crate) const RECOMMENDED_PACKAGES: [&str; 15] = [
    "boot",
    "class",
    "cluster",
    "codetools",
    "foreign",
    "KernSmooth",
    "lattice",
    "MASS",
    "Matrix",
    "mgcv",
    "nlme",
    "nnet",
    "rpart",
    "spatial",
    "survival",
];

pub(crate) const GLOBAL_ACTIVATE_FILE_CONTENT: &str = r#"local({
	owd <- getwd()
	setwd("~")
	on.exit({
	   setwd(owd)
	})
	rv_lib <- system2("rv", "library", stdout = TRUE)
	# this might not yet exist, so we'll normalize it but not force it to exist
	# and we create it below as needed
	rv_lib <- normalizePath(rv_lib, mustWork = FALSE)
	if (!is.null(attr(rv_lib, "status"))) {
		# if system2 fails it'll add a status attribute with the error code
		warning("failed to run rv library, check your console for messages")
	} else {
		if (!dir.exists(rv_lib)) {
			message("creating rv library: ", rv_lib)
			dir.create(rv_lib, recursive = TRUE)
		}
		.libPaths(rv_lib, include.site = FALSE)
	}
})
if (interactive()) {
	message("rv libpaths active!\nlibrary paths: \n", paste0("  ", .libPaths(), collapse = "\n"))
}
"#;

pub(crate) const PROJECT_ACTIVATE_FILE_CONTENT: &str = r#"local({
	rv_lib <- system2("rv", "library", stdout = TRUE)
	# this might not yet exist, so we'll normalize it but not force it to exist
	# and we create it below as needed
	rv_lib <- normalizePath(rv_lib, mustWork = FALSE)
	if (!is.null(attr(rv_lib, "status"))) {
		# if system2 fails it'll add a status attribute with the error code
		warning("failed to run rv library, check your console for messages")
	} else {
		if (!dir.exists(rv_lib)) {
			message("creating rv library: ", rv_lib)
			dir.create(rv_lib, recursive = TRUE)
		}
		.libPaths(rv_lib, include.site = FALSE)
	}
})
if (interactive()) {
	message("rv libpaths active!\nlibrary paths: \n", paste0("  ", .libPaths(), collapse = "\n"))
}
"#;
