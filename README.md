To test a project:

```
cargo run --features=cli -- --config-file=example_projects/simple/rproject.toml plan
```
# Commands and Flags to know

`rv` has many top level commands to help you easily setup your projects, install packages, and see the status of your project. For a full list of commands, please run `rv --help`

## Starting a new `rv` project

### From scratch
`rv init` will initialize a new or existing project by:
1. Setting up the project infrastructure, including the project library and an activation script to ensure the rv library is used for this project
2. Create a configuration file which is populated with the R version and repositories

You can customize this configuration file using the following flags:
* `--r-version`: The R version is set to be the version found on the path by default. This flag allows you to set any custom version
    > NOTE: For RStudio/Positron users, the R version on the path does NOT always match the version set for the session. Please use this flag to ensure correct R version is used for your project
* `--no-repositories`: The repositories are set as what is found in the current R session. This flag sets the repositories field in the configuration file to blank
* `--add`: The dependency field is blank by default. This flag can be used to add dependencies you know will be needed to the project directly to the config
    > NOTE: `rv init` will not automatically sync your project

For interactive R sessions, we recommend restarting R after initializing your project to ensure your library paths are set properly

### From a renv project
`rv migrate renv` will initilise an existing renv project by migrating your renv.lock file to a rv configuration file.

We cannot guarantee `rv` will migrate your renv project in its entirety, but any dependencies not fully migrated will be logged.

Some common reasons a dependency may not be able to be migrated:
* It could not be found in any of the repositories listed in the renv.lock. 
    * RECOMMENDEND SOLUTION: Determine the repository the dependency was installed from and add both to the configuration file
* The correct version could not be found in any of the repositories listed in the renv.lock
    * RECOMMENDED SOLUTION 1: If the exact version is required and can be found in a different repository, add both the dependency and repository to the config
    * RECOMMENDED SOLUTION 2: If the exact version is required, use the url dependency format to directly access the archive (i.e. {name = "dplyr", url = "https://cran.r-project.org/src/contrib/Archive/dplyr_1.1.3.tar.gz"})
    * RECOMMENDED SOLUTION 3: If the exact version is not required, add the dependency to the config

After migration of the renv.lock file is complete, we recommend:
    1. Removing `source("renv/activate.R")` from the projects `.Rprofile`
    2. Running `rv activate`
    3. Restarting R to set your library paths to rv



# How it works

`rv` has several top level commands to provide the user with as much flexibility as possible. The two primary commands are:
```
rv plan # detail what will occur if sync is run
rv sync # synchronize the library, config file, and lock file
```

The subsequent actions of these commands are controlled by a configuration file that specifies a desired project state by specifying the R version, repositories, and dependencies the project uses. Additionally, specific package and repository level customizations can be specified.

For example, a simple configuration file:
```
[project]
name = "my first rv project"
r_version = "4.4"

# any repositories, order matters
repositories = [
    { alias = "PPM", url = "https://packagemanager.posit.co/cran/latest" },
]

# top level packages to install
dependencies = [
    "dplyr",
    { name = "ggplot2", install_suggestions = true}
]
```

Running `rv sync` will synchronize the library, lock file, and configuration file by installing `dplyr`, `ggplot2`, any dependencies those packages require, and the suggested packages for `ggplot2`. Running `rv plan` will give you a preview of what `rv sync` will do.

Additional example projects with more configurations can be found in the `example_projects' directory of this repository