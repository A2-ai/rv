
# Usage - Commands, Flags, and Terminology to know

> [!TIP]
> Before diving into the commands and flags, a couple terms are used interchangably throughout the documentation, code base, and configuration file:
> * **Dependencies**: Are packages the project *depends* on
> * **Sync**: Is installing dependencies to *synchronize* the package library, config file, and lock file.

`rv` has many top level commands to help you easily setup your projects, install/sync packages, and see the status of your project. For a full list of commands, please run `rv --help`.

## Starting a new `rv` project

### From scratch
`rv init` will initialize a new or existing project by:
1. Setting up the project infrastructure, including the project library and an activation script to ensure the rv library is used for this project
2. Create a configuration file which is populated with the R version and repositories

You can customize this configuration file using the following flags:
* `--r-version`: The R version is set to be the version found on the path by default. This flag allows you to set any custom version
> [!NOTE]
> For RStudio/Positron users, the R version on the path does NOT always match the version set for the session. Please use this flag to ensure correct R version is used for your project
* `--no-repositories`: The repositories are set as what is found in the current R session. This flag sets the repositories field in the configuration file to blank
* `--add`: The dependency field is blank by default. This flag can be used to add dependencies you know will be needed to the project directly to the config
> [!NOTE]
> `rv init` will not automatically sync your project

For interactive R sessions, we recommend restarting R after initializing your project to ensure your library paths are set properly

### From a renv project
`rv migrate renv` will initialize an existing renv project by migrating your renv.lock file to a rv configuration file.

We cannot guarantee `rv` will migrate your renv project in its entirety, but any dependencies not fully migrated will be logged.

Some common reasons a dependency may not be able to be migrated:
* It could not be found in any of the repositories listed in the renv.lock. 
> [!TIP]
> Determine the repository the dependency was installed from and add both to the configuration file
* The correct version could not be found in any of the repositories listed in the renv.lock
> [!TIP]
> * If the exact version is required and can be found in a different repository, add both the dependency and repository to the config
> * If the exact version is required, use the url dependency format to directly access the archive (i.e. {name = "dplyr", url = "https://cran.r-project.org/src/contrib/Archive/dplyr_1.1.3.tar.gz"})
> * If the exact version is not required, add the dependency to the config

After migration of the renv.lock file is complete, we recommend:
    1. Removing `source("renv/activate.R")` from the projects `.Rprofile`
    2. Running `rv activate`
    3. Restarting R to set your library paths to rv

## Installing packages
`rv sync` is used to synchronize the lock file, configuration file, and library of a project. So if a new package is added to your configuration file, `rv sync` will install the package and its dependencies.

For quick editing, you can use `rv add <pkg1> <pkg2> ...` which will add these packages to the dependencies section of the config file and sync.

Additionally, you can use the following flags:
* `--no-sync` will add the listed packages to the config but will NOT sync
* `--dry-run` will not make any changes and only report what would happen if you were to install those packages

For more complex edits, including specific sources and other configuration, you can directly edit the configuration file and re-run `rv sync`.

## Upgrading packages
`rv` will default to installing packages from the source they were originally installed from. 
This means if you installed a package from a repository, but later remove that repository from the configuration file, the package will still be installed from the original repository.

To upgrade packages to be the latest versions available from the sources listed, use `rv upgrade`. If you'd like to see what will occur when you were to upgrade, run `rv upgrade --dry-run` or `rv plan --upgrade`.