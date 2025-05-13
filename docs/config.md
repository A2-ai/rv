# Configuration

`rv` will read a `rproject.toml` in the current directory, or you can run `rv` from another directory by setting the `--config-file` argument.

Here's a snippet detailing every field in that configuration file.


```toml
# If this is set to false, the lockfile won't be used for resolution. Defaults to true
use_lockfile = true
# By default the library will be created in the project directory in the `library` folder, and then namespaced
# by R version as well as arch
# You can however set it to any path you want and the packages will be installed directly inside that folder, without
# namespacing. `rv` will never consider a package as installed if that option is set since we can't know how it was
# installed. Useful if you want a common folder for your projects where you can do .libPaths(..) on.
# Defaults to unset
library = ""

[project]
# Which version is R is required. If we can't that find version somewhere in the system, this will error
r_version = "4.4.1"

# A list of repositories to fetch packages from. Order matters: we will try to get a package from them in order.
# The alias is only used in this file if you want to specifically require a dependency to come from a certain repository.
repositories = [
    { alias = "cran", url = "https://cran.r-project.org"},
    { alias = "prism", url = "https://prism.dev.a2-ai.cloud/rpkgs/stratus/2025-04-26"},
]

# The main element of the file! This is where you specify your dependencies, as well as some options
dependencies = [
    # A simple string will try to find a package from the repositories in order
    "dplyr",
    # You can specify a package needs to come from a specific repo by setting the `repository` field to one of the aliases
    # defined in the `repositories` array.
    { name = "some-package", repository = "prism"},
    # If you need to specify an option, you need to switch to the dict form of a dependency.
    # Options for repositories package all default to `false` and are:
    # - install_suggestions = true: install the suggested packages or not
    # - force_source = true: only get that package from source and not use binary
    # - dependencies_only = true: install only the package dependencies but not the package itself
    { name = "some-package", install_suggestions = true },
    # You can also install local dependencies if you specify a `path`.
    # Options available are `install_suggestions` and `dependencies_only`
    { name = "some-package", path = "../some"},
    # The local path can point to a directory or to an archive
    { name = "some-package", path = "../some.tar.gz"},
    # You can also use git dependencies. Set the `git` field to the git repository url as well as one of the
    # `branch`/`tag`/`commit`.
    # This requires to have the `git` CLI available.
    # Options available are `install_suggestions` and `dependencies_only`
    { name = "some-package", git = "https://github.com/A2-ai/scicalc", tag = "v0.1.1"},
    { name = "some-package", git = "https://github.com/A2-ai/scicalc", commit = "bc50e550e432c3c620714f30dd59115801f89995"},
    { name = "some-package", git = "https://github.com/A2-ai/scicalc", branch = "main"},
    # Lastly, you can point to arbitrary URLs
    # Options available are `install_suggestions` and `dependencies_only`
    {name = "dplyr", url = "https://cran.r-project.org/src/contrib/Archive/dplyr/dplyr_1.1.3.tar.gz"},
]

# By default, we will always follow the remotes defined in a DESCRIPTION file
# It is possible to override this behaviour by setting the package name in that array if
# the following conditions are met:
# 1. the package has a version requirement
# 2. we can find a package matching that version requirement in a repository
#
# If a package doesn't list a version requirement in the DESCRIPTION file, we will ALWAYS
# install from the remote.
prefer_repositories_for = []

# The fields below are reserved and not really used for anything right now
name = "project_name"
description = ""
authors = [{name = "Bob", email="hello@acme.org", maintainer = true}]
license = "MIT"
keywords = []
suggests = []
dev_dependencies = []


[project.urls]
homepage = ""
issues = ""

```