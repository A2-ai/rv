# Incremental Installation
The purpose of this test is to install an R package and all of its dependencies from a given repository, testing that the package can be used as intended. Then ensure an additional package can be added to the rproject.toml and the additional package and its dependencies installed and functional alongside the previous package. Lastly, remove the added package from the rproject.toml and re-sync and verify it and its additional dependencies are removed

## Steps:

1. Install `dplyr` (listed in rproject.toml)
2. Using R, test dplyr works (do we need to run a fxn or just `library(dplyr)`?)
3. Edit the rproject.toml to include `ggplot2`
4. Using R, test ggplot2 and dplyr, to ensure installation has not corrupted dplyr
5. Edit the rporject.toml to remove `ggplot2`
6. Check that `ggplot2` and its dependencies which are not used by `dplyr` are removed
