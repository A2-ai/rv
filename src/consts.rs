pub const PACKAGE_FILENAME: &str = "PACKAGES";
pub const DESCRIPTION_FILENAME: &str = "DESCRIPTION";
pub const SOURCE_PACKAGES_PATH: &str = "/src/contrib/PACKAGES";
pub const RUNIVERSE_PACKAGES_API_PATH: &str = "api/packages";
pub const LOCKFILE_NAME: &str = "rv.lock";

pub const RV_DIR_NAME: &str = "rv";
pub const LIBRARY_ROOT_DIR_NAME: &str = "library";
pub const STAGING_DIR_NAME: &str = "__rv__staging";
pub(crate) const LIBRARY_METADATA_FILENAME: &str = ".rv.metadata";
pub const BUILD_LOG_FILENAME: &str = "__rv_build.log";
pub const BUILT_FROM_SOURCE_FILENAME: &str = ".__rv_source";

/// How long are the package databases cached for
/// Same default value as PKGCACHE_TIMEOUT:
/// https://github.com/r-lib/pkgcache?tab=readme-ov-file#package-environment-variables
pub const PACKAGE_TIMEOUT: u64 = 60 * 60;
pub const PACKAGE_TIMEOUT_ENV_VAR_NAME: &str = "PKGCACHE_TIMEOUT";
pub const PACKAGE_DB_FILENAME: &str = "packages.mp";

pub const NUM_CPUS_ENV_VAR_NAME: &str = "RV_NUM_CPUS";
pub const SYS_REQ_URL_ENV_VAR_NAME: &str = "RV_SYS_REQ_URL";
pub const NO_CHECK_OPEN_FILE_ENV_VAR_NAME: &str = "RV_NO_CHECK_OPEN_FILE";
pub const SYS_DEPS_CHECK_IN_PATH_ENV_VAR_NAME: &str = "RV_SYS_DEPS_CHECK_IN_PATH";
pub const SUBMODULE_UPDATE_DISABLE_ENV_VAR_NAME: &str = "RV_SUBMODULE_UPDATE_DISABLE";
pub const CACHE_DIR_ENV_VAR_NAME: &str = "RV_CACHE_DIR";
pub const COPY_THREADS_ENV_VAR_NAME: &str = "RV_COPY_THREADS";
pub const GLOBAL_CACHE_DIR_ENV_VAR_NAME: &str = "RV_GLOBAL_CACHE_DIR";
pub const INSECURE_TLS_ENV_VAR_NAME: &str = "RV_INSECURE";
pub const LIBRARY_DIR_ENV_VAR_NAME: &str = "RV_LIBRARY_DIR";
pub const R_BIN_ENV_VAR_NAME: &str = "RV_R_BIN";
pub const R_VERSION_ENV_VAR_NAME: &str = "RV_R_VERSION";
pub const USE_SANDBOX_ENV_VAR_NAME: &str = "RV_USE_SANDBOX";

// List obtained from the REPL: `rownames(installed.packages(priority="base"))`
// Those will have the same version as R
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
// Those are versioned separately from R and some packages might have version requirements on them
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
	if (!nzchar(Sys.which("%rv command%"))) {
		warning(
			"rv is not installed! Install rv, then restart your R session",
			call. = FALSE
		)
		return()
	}
	rv_info <- system2(
		"%rv command%",
		c("info", "--library", "--r-version", "--repositories", "--sandbox"),
		stdout = TRUE
	)
	if (!is.null(attr(rv_info, "status"))) {
		# if system2 fails it'll add a status attribute with the error code
		warning("failed to run rv info, check your console for messages")
		return()
	}
	get_val <- function(prefix) {
		line <- grep(paste0("^", prefix, ":"), rv_info, value = TRUE)
		sub(paste0("^", prefix, ":\\s*"), "", line)
	}

	# Set repos option
	repo_str <- get_val("repositories")

	repo_parts <- strsplit(repo_str, "), ", fixed = TRUE)[[1]]
	repo_parts <- gsub("[()]", "", repo_parts)

	repo_urls <- character(length(repo_parts))
	repo_names <- character(length(repo_parts))

	for (i in seq_along(repo_parts)) {
		parts <- strsplit(repo_parts[i], ",", fixed = TRUE)[[1]]
		repo_names[i] <- trimws(parts[1])
		repo_urls[i] <- trimws(parts[2])
	}
	names(repo_urls) <- repo_names
	options(repos = repo_urls)

	# Check R version and set library
	rv_r_ver <- get_val("r-version")
	sys_r <- sprintf("%s.%s", R.version$major, R.version$minor)
	r_match <- grepl(paste0("^", rv_r_ver), sys_r)

	rv_lib <- if (r_match) {
		normalizePath(get_val("library"), mustWork = FALSE)
	} else {
		message(sprintf(
			"WARNING: R version specified in config (%s) does not match session version (%s).
rv library will not be activated until the issue is resolved. Entering safe mode...
			",
			rv_r_ver,
			sys_r
		))
		file.path(tempdir(), "__rv_R_mismatch")
	}

	if (!dir.exists(rv_lib)) {
		if (r_match) {
			message("creating rv library: ", rv_lib, "\n")
		} else {
			message("creating temporary library: ", rv_lib, "\n")
		}
		dir.create(rv_lib, recursive = TRUE)
	}

	# System library sandbox: when the project enables it, rv info returns a path
	# to a library containing only base + recommended packages. Repoint `.Library`
	# (and empty `.Library.site`) so packages installed in the system library
	# cannot leak into the project.
	sandbox <- if (r_match) get_val("sandbox") else character()
	sandbox_active <- length(sandbox) == 1 && nzchar(sandbox)
	if (sandbox_active) {
		sandbox_active <- isTRUE(tryCatch({
			env <- baseenv()
			if (bindingIsLocked(".Library", env)) unlockBinding(".Library", env)
			assign(".Library", sandbox, envir = env)
			lockBinding(".Library", env)
			if (bindingIsLocked(".Library.site", env)) unlockBinding(".Library.site", env)
			assign(".Library.site", character(), envir = env)
			lockBinding(".Library.site", env)
			TRUE
		}, error = function(e) FALSE))
	}

	.libPaths(rv_lib, include.site = FALSE)
	Sys.setenv("R_LIBS_USER" = rv_lib)
	Sys.setenv("R_LIBS_SITE" = if (sandbox_active) {
		paste(rv_lib, sandbox, sep = .Platform$path.sep)
	} else {
		rv_lib
	})

	# Results
	if (interactive()) {
		message(
			"rv repositories active!\nrepositories: \n",
			paste0(
				"  ",
				names(getOption("repos")),
				": ",
				getOption("repos"),
				collapse = "\n"
			),
			"\n"
		)
		if (sandbox_active) {
			message("rv system library sandbox active:\n  ", sandbox, "\n")
		}
		message(
			if (r_match) {
				"rv libpaths active!\nlibrary paths: \n"
			} else {
				"rv libpaths are not active due to R version mismatch. Using temp directory: \n"
			},
			paste0("  ", .libPaths(), collapse = "\n")
		)
	}
})
"#;

/// Site profile for the R subprocesses rv spawns while a project has the sandbox enabled.
pub(crate) const SANDBOX_INSTALL_PROFILE_TEMPLATE: &str = r#"local({
	# `.libPaths()` is computed before this profile runs, so it still holds the real
	# system library. Rebinding `.Library` alone does not retroactively remove it —
	# the search path has to be rebuilt afterwards, which is what drops it.
	sys_lib <- normalizePath(file.path(R.home(), "library"), mustWork = FALSE)
	keep <- setdiff(normalizePath(.libPaths(), mustWork = FALSE), sys_lib)

	env <- baseenv()
	if (bindingIsLocked(".Library", env)) unlockBinding(".Library", env)
	assign(".Library", "%sandbox path%", envir = env)
	lockBinding(".Library", env)
	if (bindingIsLocked(".Library.site", env)) unlockBinding(".Library.site", env)
	assign(".Library.site", character(), envir = env)
	lockBinding(".Library.site", env)

	# Appends the freshly bound `.Library`, ie the sandbox, in place of what we dropped.
	.libPaths(keep, include.site = FALSE)
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
