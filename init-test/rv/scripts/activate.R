local({
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
  rv_info <- system2("rv", "info", stdout = TRUE)
  r_version_line <- grep("^R version:", rv_info, value = TRUE)
  r_version <- gsub("R version: ", "", r_version_line)
  rv_r_version <- strsplit(r_version, ".")[[1]]

  r_version <- strsplit(R.version.string, " ")[[1]]
  rv_r_version_parts <- strsplit(rv_r_version, " ")[[1]]
  r_version <- r_version[seq_along(rv_r_version_parts)]
})
if (interactive()) {
  message("rv libpaths active!\nlibrary paths: \n", 
    paste0("  ", .libPaths(), collapse = "\n")
  )
}

