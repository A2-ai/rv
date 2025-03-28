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

pub(crate) const LIBRARY_METADATA_FILENAME: &str = ".rv.metadata";

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

pub(crate) const ACTIVATE_FILE_TEMPLATE: &str = r#"local({%global wd content%
	rv_info <- system2("%rv command%", c("info", "--library", "--r-version", "--repositories"), stdout = TRUE)
	if (!is.null(attr(rv_info, "status"))) {
		# if system2 fails it'll add a status attribute with the error code
		warning("failed to run rv info, check your console for messages")
	} else {
		# extract library, r-version, and repositories from rv
		rv_lib <- sub("library: (.+)", "\\1", grep("^library:", rv_info, value = TRUE))
		rv_r_ver <- sub("r-version: (.+)", "\\1", grep("^r-version:", rv_info, value = TRUE))
		repo_str <- sub("repositories: ", "", grep("^repositories:", rv_info, value = TRUE))
		repo_entries <- gsub("[()]", "", strsplit(repo_str, "), (", fixed = TRUE)[[1]])
    repo_list <- trimws(sub(".*, ", "", repo_entries))  # Extract URL
    names(repo_list) <- trimws(sub(", .*", "", repo_entries))   # Extract Name
		# this might not yet exist, so we'll normalize it but not force it to exist
		# and we create it below as needed
		rv_lib <- normalizePath(rv_lib, mustWork = FALSE)
		if (!dir.exists(rv_lib)) {
			message("creating rv library: ", rv_lib)
			dir.create(rv_lib, recursive = TRUE)
		}
		.libPaths(rv_lib, include.site = FALSE)
		options(repos = repo_list)

		if (interactive()) {
			message("rv libpaths active!\nlibrary paths: \n", paste0("  ", .libPaths(), collapse = "\n"), "\n")
			message("rv repositories active!\nrepositories: \n", paste0("  ", names(getOption("repos")), ": ", getOption("repos"), collapse = "\n"))
			sys_r <- sprintf("%s.%s", R.version$major, R.version$minor)
			if (!grepl(paste0("^", rv_r_ver), sys_r)) {
				message(sprintf("\nWARNING: R version specified in config (%s) does not match session version (%s)", rv_r_ver, sys_r))
		}
	}
	}
})
"#;

pub(crate) const RVR_FILE_CONTENT: &str = r#".rv <- new.env()
.rv$config_path <- file.path(normalizePath(getwd()), "rproject.toml")
.rv$summary <- function(json = FALSE) {
  command <- c("summary")
  if (json) { command <- c(command, "--json") }
  .rv$command(command)
}
.rv$plan <- function() { .rv$command("plan") }
.rv$sync <- function() { .rv$command("sync") }
.rv$add <- function(..., dry_run = FALSE) {
  dots <- unlist(list(...))
  command <- c("add", dots)
  if (dry_run) { command <- c(command, "--dry-run") }
  .rv$command(command)
}

.rv$command <- function(command) {
  # underlying system calls to rv
  args <- c(command, "-c", .rv$config_path)
  res <- system2("rv", args, stdout = TRUE)
  if (!is.null(attr(res, "status"))) {
    warning(sprintf("failed to run `rv %s`, check your console for messages", paste(args, collapse = " ")))
  } else {
    message(paste(res, collapse = "\n"))
  }
}"#;
