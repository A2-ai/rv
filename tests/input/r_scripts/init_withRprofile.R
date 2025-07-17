# Create directory and change to it
dir.create("rv-workflow-test")
setwd("rv-workflow-test")

# Create empty .Rprofile file
file.create(".Rprofile")

# Write repository configuration to .Rprofile
writeLines('options("repos" = c("PPM" = "https://packagemanager.posit.co/cran/latest"))', ".Rprofile") # nolint: line_length_linter.
