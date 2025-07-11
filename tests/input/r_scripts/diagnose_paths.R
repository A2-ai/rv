# Comprehensive path and configuration diagnosis
cat("=== DIRECTORY DIAGNOSIS ===\n")
cat("R Working Directory:", getwd(), "\n")

# Check for .Rprofile and source it if it exists
if (file.exists(".Rprofile")) {
  cat("✅ .Rprofile EXISTS - sourcing it...\n")
  source(".Rprofile")
  cat("✅ .Rprofile sourced - rv libpaths should now be active!\n")
} else {
  cat("❌ .Rprofile NOT FOUND\n")
}

# Check if rproject.toml exists
toml_path <- "rproject.toml"
if (file.exists(toml_path)) {
  cat("✅ rproject.toml EXISTS\n")
  cat("rproject.toml contents:\n")
  cat(paste(readLines(toml_path), collapse="\n"), "\n")
} else {
  cat("❌ rproject.toml NOT FOUND\n")
  cat("Files in current directory:\n")
  cat(paste(list.files(all.files=TRUE), collapse=", "), "\n")
}

cat("\n=== LIBRARY PATH DIAGNOSIS ===\n")
libpaths <- .libPaths()
cat("R Library Paths (", length(libpaths), " total):\n")
for(i in seq_along(libpaths)) {
  path <- libpaths[i]
  exists <- file.exists(path)
  cat("  ", i, ":", path, ifelse(exists, "(EXISTS)", "(MISSING)"), "\n")
  
  if (exists && grepl("rv", path, ignore.case=TRUE)) {
    cat("    ^ This looks like an rv-managed path\n")
    contents <- list.files(path)
    if (length(contents) > 0) {
      cat("    Contents:", paste(contents, collapse=", "), "\n")
    } else {
      cat("    ^ Directory is EMPTY\n")
    }
  }
}

cat("\n=== RV ENVIRONMENT CHECK ===\n")
rv_env_vars <- c("R_LIBS_USER", "R_LIBS_SITE", "R_LIBS")
for(var in rv_env_vars) {
  val <- Sys.getenv(var)
  if (val != "") {
    cat(var, "=", val, "\n")
  }
}

cat("\n=== MANUAL R6 SEARCH ===\n")
# Look for R6 in all possible locations
for(i in seq_along(libpaths)) {
  path <- libpaths[i]
  r6_path <- file.path(path, "R6")
  if (file.exists(r6_path)) {
    cat("FOUND R6 at:", r6_path, "\n")
    desc_file <- file.path(r6_path, "DESCRIPTION")
    if (file.exists(desc_file)) {
      desc_lines <- readLines(desc_file, n=10)
      version_line <- grep("Version:", desc_lines, value=TRUE)
      if(length(version_line) > 0) {
        cat("  Version info:", version_line, "\n")
      }
    }
  }
}

cat("\n=== EXPECTED RV LOCATIONS ===\n")
expected_paths <- c(
  "library",
  "rv/library", 
  file.path("rv", "library", "4.4", "x86_64-pc-linux-gnu"),
  file.path("rv", "library", "4.4", "arm64")
)

for(path in expected_paths) {
  abs_path <- file.path(getwd(), path)
  if (file.exists(abs_path)) {
    cat("✅", abs_path, "EXISTS\n")
    r6_in_path <- file.path(abs_path, "R6")
    if (file.exists(r6_in_path)) {
      cat("  ✅ R6 found in this location!\n")
    } else {
      contents <- list.files(abs_path)
      if (length(contents) > 0) {
        cat("  Contents:", paste(contents, collapse=", "), "\n")
      }
    }
  } else {
    cat("❌", abs_path, "NOT FOUND\n")
  }
}

flush.console()
