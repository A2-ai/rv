use std::{
    fs::File,
    io::Write,
    path::{Path, absolute},
};

use anyhow::{Result, anyhow};

use crate::{
    DiskCache, RenvLock, Repository, SystemInfo,
    context::load_databases,
    renv::{ResolvedRenv, UnresolvedRenv},
};

const RENV_CONFIG_TEMPLATE: &str = r#"# this config was migrated from %renv_file% on %time%
[project]
name = "%project_name%"
r_version = "%r_version%"

repositories = [
%repositories%
]

dependencies = [
%dependencies%
]
"#;

pub fn migrate_renv(
    renv_file: impl AsRef<Path>,
    config_file: impl AsRef<Path>,
    strict_r_version: bool,
) -> Result<Vec<UnresolvedRenv>> {
    // project name is the parent directory of the renv project
    let abs_renv_file = absolute(renv_file.as_ref())?;
    let project_name = abs_renv_file
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|f| f.to_str())
        .unwrap_or("renv migrated project");

    // use the repositories and r version from the renv.lock to determine the repository databases
    let renv_lock = RenvLock::parse_renv_lock(&renv_file)?;
    let cache = match DiskCache::new(renv_lock.r_version(), SystemInfo::from_os_info()) {
        Ok(c) => c,
        Err(e) => return Err(anyhow!(e)),
    };
    let databases =
        load_databases(&renv_lock.config_repositories(), &cache).map_err(|e| anyhow!("{e}"))?;

    // resolve the renv.lock file to determine the true source of packages
    let (resolved, unresolved) = renv_lock.resolve(&databases);

    // Write config out to the config file specified in the cli, even if config file is outside of the renv.lock project
    let r_version = if strict_r_version {
        &renv_lock.r_version().original
    } else {
        let [major, minor] = renv_lock.r_version().major_minor();
        &format!("{major}.{minor}")
    };

    let config = render_config(
        &renv_file.as_ref().to_string_lossy(),
        project_name,
        r_version,
        &renv_lock.config_repositories(),
        &resolved,
    );
    let mut file = File::create(&config_file)?;
    file.write_all(config.as_bytes())?;
    Ok(unresolved)
}

fn render_config(
    renv_file: &str,
    project_name: &str,
    r_version: &str,
    repositories: &[Repository],
    resolved_deps: &[ResolvedRenv],
) -> String {
    let repos = repositories
        .iter()
        .map(|r| {
            format!(
                r#"    {{ alias = "{}", url = "{}"{}}}"#,
                r.alias,
                r.url(),
                if r.force_source {
                    r#", force_source = true"#.to_string()
                } else {
                    String::new()
                }
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    // print alphabetically to match with plan/sync output
    let deps = resolved_deps
        .iter()
        .map(|d| format!("    {d}"))
        .collect::<Vec<_>>()
        .join(",\n");
    // get time. Try to round to seconds, but if error, leave as unrounded
    let time = jiff::Zoned::now();
    // Format the time as just the date (YYYY-MM-DD)
    let time = time.date().to_string();

    RENV_CONFIG_TEMPLATE
        .replace("%renv_file%", renv_file)
        .replace("%time%", &time.to_string())
        .replace("%project_name%", project_name)
        .replace("%r_version%", r_version)
        .replace("%repositories%", &repos)
        .replace("%dependencies%", &deps)
}
#[cfg(test)]
mod tests {
    use super::render_config;
    use crate::{Config, RenvLock, Repository, RepositoryDatabase};
    use url::Url;

    const REPO: &str = "https://cran-binary/";

    const R6: &str = r#""R6": {"Package": "R6", "Version": "2.5.0", "Source": "Repository", "Repository": "cran-binary"}"#;

    fn load(rendered: String) -> Config {
        rendered
            .parse()
            .unwrap_or_else(|e| panic!("{e}\n--- rendered ---\n{rendered}"))
    }

    fn migrate(r_version: &str, packages: &str) -> Config {
        let lock: RenvLock = serde_json::from_str(&format!(
            r#"{{"R": {{"Version": "4.4.1",
                 "Repositories": [{{"Name": "cran-binary", "URL": "{REPO}"}}]}},
                "Packages": {{{packages}}}}}"#
        ))
        .expect("valid renv.lock fixture");

        // Serves R6 so a Repository-sourced entry resolves.
        let mut db = RepositoryDatabase::new(REPO);
        db.parse_source("Package: R6\nVersion: 2.5.0\n\n");
        let (resolved, _) = lock.resolve(&[(db, false)]);

        let repos = [Repository::new(
            "cran-binary".into(),
            Url::parse(REPO).unwrap(),
            false,
        )];
        load(render_config(
            "renv.lock",
            "migrated",
            r_version,
            &repos,
            &resolved,
        ))
    }

    #[test]
    fn renders_a_loadable_config() {
        let config = migrate("4.4", R6);
        assert_eq!(config.r_version().original, "4.4");
        assert_eq!(config.dependencies().len(), 1);
    }

    #[test]
    fn keeps_a_strict_r_version() {
        assert_eq!(migrate("4.4.1", R6).r_version().original, "4.4.1");
    }

    #[test]
    fn renders_a_loadable_config_when_nothing_resolves() {
        let config = migrate(
            "4.4",
            r#""nope": {"Package": "nope", "Version": "0.1.1", "Source": "unknown"}"#,
        );
        assert!(config.dependencies().is_empty());
    }

    #[test]
    fn renders_a_loadable_config_with_no_repositories_or_dependencies() {
        load(render_config("renv.lock", "migrated", "4.4", &[], &[]));
    }

    #[test]
    fn renders_a_loadable_config_for_a_git_package_in_a_subdirectory() {
        let config = migrate(
            "4.4",
            r#""pkg": {"Package": "pkg", "Version": "0.1.0",
                "Source": "GitHub", "RemoteType": "github", "RemoteHost": "api.github.com",
                "RemoteRepo": "git.monorepo.pkg", "RemoteUsername": "a2-ai",
                "RemoteSha": "0123456789abcdef0123456789abcdef01234567",
                "RemoteSubdir": "R/pkg"}"#,
        );
        assert_eq!(config.dependencies().len(), 1);
    }
}
