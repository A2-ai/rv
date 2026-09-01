#!/usr/bin/env -S rv run
# /// rv
# dependencies = ["cli"]
# repositories = [
#     { alias = "posit", url = "https://packagemanager.posit.co/cran/2025-05-12/" }
# ]
# ///

# `cli` must resolve from the ephemeral, script-scoped library — not the
# ambient project or the user library.
stopifnot(requireNamespace("cli", quietly = TRUE))

lib <- dirname(system.file(package = "cli"))
cli::cli_alert_success("cli {packageVersion('cli')} loaded from {lib}")

# Stable marker for the integration test to assert on stdout.
cat("SELF_CONTAINED_OK\n")
