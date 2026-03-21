# E2E test: Create a reference renv.lock using native renv
# This installs R6 from CRAN and rv.git.pkgA from GitHub (tag v0.0.6)
# then runs renv::snapshot() to produce the "gold standard" renv.lock

# Use a fixed CRAN snapshot for reproducibility
options(repos = c(CRAN = "https://packagemanager.posit.co/cran/__linux__/noble/2025-07-15/"))

# Initialize renv in this directory
renv::init(bare = TRUE, restart = FALSE)

# Install R6 from CRAN
renv::install("R6")

# Install rv.git.pkgA from GitHub at tag v0.0.6
renv::install("A2-ai/rv.git.pkgA@v0.0.6")

# Snapshot with type="all" to capture all installed packages (not just used ones)
renv::snapshot(type = "all", prompt = FALSE)

cat("renv.lock created successfully\n")

# Also dump the DESCRIPTION files for comparison
desc_dir <- file.path("captured_descriptions")
dir.create(desc_dir, showWarnings = FALSE)

lib_path <- renv::paths$library()
for (pkg in c("R6", "rv.git.pkgA")) {
  desc_file <- file.path(lib_path, pkg, "DESCRIPTION")
  if (file.exists(desc_file)) {
    file.copy(desc_file, file.path(desc_dir, paste0(pkg, ".DESCRIPTION")), overwrite = TRUE)
    cat(sprintf("Captured DESCRIPTION for %s\n", pkg))
  } else {
    cat(sprintf("WARNING: DESCRIPTION not found for %s at %s\n", pkg, desc_file))
  }
}
