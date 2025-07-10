I want a new feature for rv that allows repositories to be configured from the rv cli

```
rv configure repository --alias my-repo --url https://github.com/my-org/my-repo.git
```

The flags should correspond to the fields for repositories in `rproject.toml`. 
The only two required fields are `alias` and `url`. The command should update the `rproject.toml` file with the new repository configuration.

In addition, we must be able to specify where to add the respository. To do so the following flag options should be available:
- `--before <alias>`: Add the new repository before the specified alias
- `--after <alias>`: Add the new repository after the specified alias
- `--first`: Add the new repository as the first entry
- `--last`: Add the new repository as the last entry    
- `--replace <alias>`: Replace the existing repository with the specified alias
- `--remove <alias>`: Remove the existing repository with the specified alias
- `--clear`: Clear all repositories

for example given a starting `rproject.toml` file like this:

```
[project]
name = "simple"
r_version = "4.4"
repositories = [
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"}
]
dependencies = [
    "dplyr",
]
```

```
rv configure repository --alias ppm --url https://packagemanager.posit.co/cran/latest --first
```

should result in the following `rproject.toml` file:

```
[project]
name = "simple"
r_version = "4.4"
repositories = [
    {alias = "ppm", url = "https://packagemanager.posit.co/cran/latest"},
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"}
]
dependencies = [
    "dplyr",
]
```

another example, given:

```
[project]
name = "simple"
r_version = "4.4"
repositories = [
    {alias = "ppm", url = "https://packagemanager.posit.co/cran/latest"},
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"}
]
dependencies = [
    "dplyr",
]
```

```
rv configure repository --alias ppm-old --url https://packagemanager.posit.co/cran/2024-11-16 --after ppm
```


```
[project]
name = "simple"
r_version = "4.4"
repositories = [
    {alias = "ppm", url = "https://packagemanager.posit.co/cran/latest"},
    {alias = "ppm-old", url = "https://packagemanager.posit.co/cran/2024-11-16/"},
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"}
]
dependencies = [
    "dplyr",
]
```

When making these changes, rv should insure that the resulting rproject.toml is valid and no duplicate aliases exist. If a duplicate alias is found, rv should return an error message indicating the conflict.

the tomledit crate should be used, such that comments are preserved and the file is formatted correctly.

for testing, use cargo insta to generate snapshots of representative `rproject.toml` file before and after the command is run. 
This will ensure that the changes are correctly applied and that the file remains valid. For testing, integration tests
should be used to test all permutations. Check the testing strategy in src/tests/add.rs for examples of how to write these tests,
where the tests do not shell out to rv directly, but instead test the functionality on the documentmut from tomledit then call to_string to see 
and test snapshots of resulting contents.
