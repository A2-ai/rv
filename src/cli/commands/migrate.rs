use std::path::Path;

use crate::{
    cli::{self, DiskCache},
    package::{Package, PackageType},
    renv_lock::{PackageInfo, RenvLock},
    Repository, SystemInfo, Version,
};

pub fn migrate() {}

fn migrate_renv<P: AsRef<Path>>(path: P) {
    let renv_lock = RenvLock::parse_renv_lock(path)?;
}

enum PackageSource<'a> {
    Source(&'a Repository),
    Other(String),
}

struct ResolvedLock<'a> {
    package: String,
    version: Version,
    source: PackageSource<'a>,
    pkginfo: PackageInfo,
}

impl<'a> ResolvedLock<'a> {
    fn resolve(lock: RenvLock) -> Result<Vec<Self>> {
        let cache = DiskCache::new(lock.r_version(), SystemInfo::from_os_info())?;
        let db = cli::context::load_databases(lock.repositories(), &cache)?;
        let mut resolved_dep = Vec::new();
        for (pkg_name, pkg_info) in lock.packages.into_iter() {
            let (matching_db, other_db): (Vec<_>, Vec<_>) = db
                .into_iter()
                .filter(|(repo, force_source)| repo.find_package(&pkg_name, None, lock.r_version(), *force_source).is_some())
                .partition(|(repo, _)| repo.name == pkg_info.repository);

            let tmp = matching_db
                .iter()
                .filter(|(repo, force_source)| {
                    repo.find_package(&pkg_name, None, lock.r_version(), *force_source)
                        .is_some()
                })


            resolved_dep.push(
                matching_db
                    .iter()
                    .filter(|(repo, force_source)| {
                        repo.find_package(&pkg_name, None, lock.r_version(), *force_source)
                            .is_some()
                    })
                    .map(|(repo, _)| ResolvedLock {
                        package: pkg_name,
                        version: pkg_info.version,
                        source: PackageSource::Source(
                            lock.repositories()
                                .into_iter()
                                .filter(|x| x.alias == pkg_info.repository)
                                .collect::<Vec<&Repository>>()
                                .first()
                                .unwrap(), //need to convert option to error
                        ),
                        pkginfo: pkg_info, //returning for now
                    }),
            );
        }
    }
}
