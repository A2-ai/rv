# Source and Suggestions Options
The purpose of this test is to verify that the options at both the package and repository level function as expected

1. Set repos to [alias = "old", url = "https://packagemanager.posit.co/cran/2024-01-01", force_source = true] and [alias = "new", url = "https://packagemanager.posit.co/cran/2025-01-1"]. 
2. Install `allometric` (which is only available in `old`), `ggplot2` and all of its suggested dependencies, and the source of `glue` from the "new" repo
3. Verify the packages install with `ggplot2`=v3.5.1, the `ggplot2` suggested dependencies are installed, and `allometric` and `glue` are installed from source