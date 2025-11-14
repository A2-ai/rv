#!/usr/bin/env Rscript

# Bootstrap script to copy shared code from monorepo into R package
# Based on tree-sitter-r pattern

files <- c("world.R")
upstream_directory <- file.path("..", "..", "src")
upstream <- file.path(upstream_directory, files)
destination <- file.path("R", files)
file.copy(upstream, destination, overwrite = TRUE)
