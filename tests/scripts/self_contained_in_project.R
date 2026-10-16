#!/usr/bin/env -S rv run
# /// rv
# dependencies = ["glue"]
# repositories = [
#     { alias = "posit", url = "https://packagemanager.posit.co/cran/2023-06-01/" }
# ]
# ///

# This script is run from *inside* an rv project whose `rproject.toml` points at a
# recent repository. The embedded config must win: both the repository (hence the
# glue version) and the library it resolves into come from the block above.
stopifnot(requireNamespace("glue", quietly = TRUE))

cat("VERSION:", as.character(packageVersion("glue")), "\n")
cat("LIBRARY:", dirname(system.file(package = "glue")), "\n")
