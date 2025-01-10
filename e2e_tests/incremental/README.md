# Incremental Installation
The purpose of this test is to install an R package and all of its dependencies from a given repository, testing that the package can be used as intended. Then ensure an additional package can be added to the rproject.toml and the additional package and its dependencies installed and functional alongside the previous package

## Steps:

1. Install `dplyr` (listed in rproject.toml)
2. Using R, test dplyr works (do we need to run a fxn or just `library(dplyr)`?)
3. Edit the rproject.toml to include `ggplot2`
4. Using R, test ggplot2 and dplyr, to ensure installation has not corrupted dplyr
