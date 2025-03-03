To test a project:

```
cargo run --features=cli -- --config-file=example_projects/simple/rproject.toml plan
```

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