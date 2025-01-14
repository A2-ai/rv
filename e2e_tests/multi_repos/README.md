# Multiple Repositories
The purpose of this test is to verify this tool will install packages from the specified repositories, when multiple are available.

## Steps

1. Install `ggplot2` v3.5.1 from "https://packagemanager.posit.co/cran/2025-01-13" and `usethis` v2.2.3 from "https://packagemanager.posit.co/cran/2024-03-1"
2. Verify `ggplot2` v3.5.1 is installed and `usethis` v2.2.3 is installed
3. Verify the version limits on the following packages. These are shared dependencies which have different version restraints
* glue: >= 1.3.0
* lifecycle: > 1.0.1
* withr: >= 2.5.0
3. Verify both functions work as expected using R
