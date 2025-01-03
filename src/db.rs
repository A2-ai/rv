use crate::{
    cli::{http, DiskCache},
    consts::{PACKAGE_FILENAME, SOURCE_PACKAGES_PATH},
    repo_path::get_binary_path,
    Cache, CacheEntry, Repository, RepositoryDatabase, Version,
};
use rayon::prelude::*;
use log::{trace, debug};
/// Loads the package databases from a list of repositories,
/// possibly persisting them if `persist == true`.
///
/// If a type or object is missing, make a best guess or note that in a comment.
pub fn load_databases(
    repositories: &[Repository],
    cache: &DiskCache,
    r_version: &Version,
    persist: bool,
) -> Vec<(RepositoryDatabase, bool)> {
    repositories
        .par_iter()
        .map(|r| {
            // 1. Generate path to add to URL to get the src PACKAGE and binary PACKAGE for current OS
            let entry = cache.get_package_db_entry(&r.url());
            match entry {
                CacheEntry::Existing(p) => {
                    trace!("Loading db from cache {p:?}");
                    let start_time = std::time::Instant::now();
                    let db = RepositoryDatabase::load(&p);
                    trace!("Loading db from cache took: {:?}", start_time.elapsed());
                    (db, r.force_source)
                }
                CacheEntry::NotFound(p) => {
                    let mut db = RepositoryDatabase::new(&r.alias);
                    db.url = r.url().to_string();

                    // Download source PACKAGES
                    let mut source_package = Vec::new();
                    let mut start_time = std::time::Instant::now();
                    http::download(
                        &format!("{}{SOURCE_PACKAGES_PATH}", r.url()),
                        &mut source_package,
                        None,
                    )
                    .expect("TODO");
                    db.source_url = format!(
                        "{}{}",
                        r.url(),
                        std::path::Path::new(SOURCE_PACKAGES_PATH)
                            .parent()
                            .unwrap()
                            .to_string_lossy()
                    );
                    debug!(
                        "Downloading source package db took: {:?}",
                        start_time.elapsed()
                    );

                    // Parse source
                    start_time = std::time::Instant::now();
                    unsafe {
                        db.parse_source(std::str::from_utf8_unchecked(&source_package));
                    }
                    debug!("Parsing source package db took: {:?}", start_time.elapsed());

                    // Download binary PACKAGES
                    let mut binary_package = Vec::new();
                    let binary_path = get_binary_path(
                        &cache.r_version,
                        &cache.system_info.os_type,
                        cache.system_info.codename(),
                    );
                    let binary_path = format!("{}{binary_path}", r.url());
                    let dl_url = format!("{}{PACKAGE_FILENAME}", binary_path);
                    debug!("Downloading binary package from {dl_url}");
                    start_time = std::time::Instant::now();
                    let rvparts = r_version.major_minor();
                    http::download(
                        &dl_url,
                        &mut binary_package,
                        Some((
                            "user-agent",
                            format!("R/{}.{}", rvparts[0], rvparts[1]).into(),
                        )),
                    )
                    .expect("TODO");
                    db.binary_url = Some(binary_path);
                    debug!(
                        "Downloading binary package db took: {:?}",
                        start_time.elapsed()
                    );

                    // Parse binary
                    start_time = std::time::Instant::now();
                    unsafe {
                        db.parse_binary(
                            std::str::from_utf8_unchecked(&source_package),
                            cache.r_version.clone(),
                        );
                    }
                    debug!("Parsing binary package db took: {:?}", start_time.elapsed());

                    // Persist if requested
                    start_time = std::time::Instant::now();
                    if persist {
                        db.persist(&p);
                    }
                    trace!("Persisting db took: {:?}", start_time.elapsed());
                    trace!("Saving db at {p:?}");
                    (db, r.force_source)
                }
            }
        })
        .collect::<Vec<_>>()
}
