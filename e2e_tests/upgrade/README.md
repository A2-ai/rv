# Repository Upgrade
The purpose of this test is to "upgrade" a repository, i.e. move to a newer snapshot date for newer package versions, and verify the necessary dependencies change

## Steps
1. Install `ggplot2` from "https://packagemanager.posit.co/cran/2024-03-01"
2. Verify `ggplot2` v3.5.0 is installed
3. Change the repository to "https://packagemanager.posit.co/cran/2025-01-1"
4. Verify `ggplot2` v3.5.1 is installed and the dependencies meet its minimum required set
