This repo allows us to test a couple things


On linux the RSPM repo should be binary but cran src, so when we look at a plan

```
RV_LOG=debug cargo run -- --config-file example_projects/rspm-cran/rproject.toml plan --distribution focal ```
```

we see that binary is only reached out for RSPM but not CRAN. This comes from the heuristic
that cran or a repo with no path wouldn't be able to provide a binary compatible url spec
so we skip even trying to look.

```
[2025-01-06T13:07:31Z DEBUG reqwest::connect] starting new connection: https://cran.r-project.org/
[2025-01-06T13:07:31Z DEBUG reqwest::connect] starting new connection: https://packagemanager.posit.co/
[2025-01-06T13:07:31Z DEBUG rv::db] Downloading source package db took: 367.584334ms
[2025-01-06T13:07:31Z DEBUG rv::db] Parsing source package db took: 247.006042ms
[2025-01-06T13:07:31Z DEBUG rv::db] Downloading binary package from https://packagemanager.posit.co/cran/__linux__/focal/latest/src/contrib/PACKAGES
[2025-01-06T13:07:31Z DEBUG reqwest::connect] starting new connection: https://packagemanager.posit.co/
[2025-01-06T13:07:32Z DEBUG rv::db] Downloading binary package db took: 335.058958ms
[2025-01-06T13:07:32Z DEBUG rv::db] Downloading source package db took: 1.19237s
[2025-01-06T13:07:32Z DEBUG rv::db] Parsing binary package db took: 245.6395ms
[2025-01-06T13:07:32Z DEBUG rv::db] Parsing source package db took: 242.67675ms
[2025-01-06T13:07:32Z DEBUG rv::cli::plan] Loading databases took: 1.435939292s
[2025-01-06T13:07:32Z INFO  rv::cli::plan] Plan successful! The following packages will be installed:
[2025-01-06T13:07:32Z INFO  rv::cli::plan] Found 0 packages installed with correct version, 0 need updating, 2 missing
[2025-01-06T13:07:32Z INFO  rv::cli::plan]     R6=2.5.1 (from RSPM, type=binary)
[2025-01-06T13:07:32Z INFO  rv::cli::plan]     renv=1.0.11 (from CRAN, type=source)
[2025-01-06T13:07:32Z INFO  rv::cli::plan] Plan took: 1.44683025s
```