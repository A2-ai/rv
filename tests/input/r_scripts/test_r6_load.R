# Test R6 loading step by step
cat("=== Testing R6 Loading ===\n")

# Check if R6 is available
if ("R6" %in% rownames(installed.packages())) {
  cat("R6 is installed\n")
} else {
  cat("R6 is NOT installed\n")
}

# Try to load R6
tryCatch({
  library(R6)
  cat("R6 loaded successfully\n")
  
  # Get version
  version <- packageVersion("R6")
  cat("R6_VERSION:", as.character(version), "\n")
  
}, error = function(e) {
  cat("ERROR loading R6:", e$message, "\n")
})

flush.console()