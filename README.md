# rv

`rv` is a new way to manage and install your R packages in a reproducible, fast, and declaritive way. 

> [!WARNING]
> `rv` is still in development and may not be fully documented. For additional information, issues, or feature requests, please create an issue or contact us directly.

## How it works

`rv` has several top level commands to provide the user with as much flexibility as possible. The two primary commands are:
```
rv plan # detail what will occur if sync is run
rv sync # synchronize the library, config file, and lock file
```

The subsequent actions of these commands are controlled by a configuration file that specifies a desired project state by specifying the R version, repositories, and dependencies the project uses. Additionally, specific package and repository level customizations can be specified.

For example, a simple configuration file:
```toml
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

Additional example projects with more configurations can be found in the [example_projects](example_projects)  directory of this repository.

## Installation

See the [installation documentation](docs/installation.md) for information on how to install `rv`.

## Usage

See the [usage documentation](docs/usage.md) for information on how to use `rv` and how to [configure](docs/config.md) it with
the `rproject.toml` file.

## Contributing

### Getting started

To get started with the development of `rv`, you'll need:

- [Rust](https://rustup.rs/)
- and optionally [Just](https://github.com/casey/just)

After installing Rust, you can build the project by running:

```bash
just run <args>
// or
cargo run --features=cli --release -- ...
```

e.g. `just run sync` or `just run add --dry-run`.

If you'd like to install the current version of the project as a binary, you can run:

```bash
just install
// or
cargo install --path . --features cli
```

### Unit testing

Run the unit tests with:

```bash
just test
// or
cargo test --features=cli
```

### Snapshot testing

Snapshots require R version 4.4.x.