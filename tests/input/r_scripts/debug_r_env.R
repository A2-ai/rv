# Debug R environment in CI
cat("=== R ENVIRONMENT DEBUG ===\n")
cat("R Version:", R.version.string, "\n")
cat("Working Directory:", getwd(), "\n")
cat("Library Paths:\n")
for(i in seq_along(.libPaths())) {
  cat("  ", i, ":", .libPaths()[i], "\n")
}

cat("\n=== CHECKING FOR R6 ===\n")
r6_paths <- find.package("R6", quiet = TRUE)
if(length(r6_paths) > 0) {
  cat("R6 found at:", r6_paths, "\n")
  
  # Try to load R6
  tryCatch({
    library(R6, quietly = TRUE)
    cat("R6 loaded successfully\n")
    
    # Get version
    version <- packageVersion("R6")
    cat("R6 version object:", class(version), "\n")
    cat("R6 version string:", as.character(version), "\n")
    cat("R6_VERSION:", as.character(version), "\n")
    
  }, error = function(e) {
    cat("Error loading R6:", e$message, "\n")
  })
} else {
  cat("R6 not found in library paths\n")
}

cat("\n=== TESTING OUTPUT ===\n")
cat("Direct cat test\n")
print("Direct print test")
message("Direct message test")

cat("=== END DEBUG ===\n")