use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Result};
use crossbeam::{channel, thread};
use fs_err as fs;

use crate::cli::cache::PackagePaths;
use crate::cli::utils::untar_package;
use crate::cli::{http, CliContext};
use crate::git::GitReference;
use crate::link::LinkMode;
use crate::lockfile::Source;
use crate::package::PackageType;
use crate::{BuildPlan, BuildStep, RCmd, RCommandLine, RepoServer, ResolvedDependency};
use crate::{Git, GitOperations};

fn is_binary_package(path: &Path, name: &str) -> bool {
    path.join(name)
        .join("R")
        .join(format!("{name}.rdx"))
        .exists()
}

fn download_and_untar(url: &str, destination: &Path) -> Result<()> {
    fs::create_dir_all(&destination)?;
    let mut tarball = Vec::new();
    let bytes_read = http::download(&url, &mut tarball, vec![])?;

    // TODO: handle 404
    if bytes_read == 0 {
        bail!("Archive not found at {url}");
    }

    untar_package(Cursor::new(tarball), &destination)?;

    Ok(())
}

fn install_via_r(source: &Path, library_dir: &Path, binary_dir: &Path, r_cmd: &RCommandLine) -> Result<()> {
    if let Err(e) = r_cmd.install(source, library_dir, binary_dir) {
        // Do not leave empty binary dir if some install failed otherwise later install
        // would fail
        if binary_dir.is_dir() {
            fs::remove_dir_all(binary_dir)?;
        }
        bail!(e);
    }
    Ok(())
}

fn download_and_install_source(
    url: &str,
    paths: &PackagePaths,
    library_dir: &Path,
    pkg_name: &str,
    r_cmd: &RCommandLine,
) -> Result<()> {
    download_and_untar(&url, &paths.source)?;
    log::debug!("Compiling binary from {}", &paths.source.display());
    r_cmd.install(paths.source.join(pkg_name), library_dir, &paths.binary)?;
    Ok(())
}

fn download_and_install_binary(
    url: &str,
    paths: &PackagePaths,
    library_dir: &Path,
    pkg_name: &str,
    r_cmd: &RCommandLine,
) -> Result<()> {
    // If we get an error doing the binary download, fall back to source
    if let Err(e) = download_and_untar(&url, &paths.binary) {
        log::warn!("Failed to download/untar binary package: {e:?}");
        return download_and_install_source(url, paths, library_dir, pkg_name, r_cmd);
    }

    // Ok we download some tarball. We can't assume it's actually compiled though, it could be just
    // source files. We have to check first whether what we have is actually binary content.
    if !is_binary_package(&paths.binary, pkg_name) {
        log::debug!("{pkg_name} was expected as binary, found to be source. Compiling binary for {pkg_name}...");
        // Move it to the source destination if we don't have it already
        if paths.source.is_dir() {
            fs::remove_dir_all(&paths.binary)?;
        } else {
            fs::create_dir_all(&paths.source)?;
            fs::rename(&paths.binary, &paths.source)?;
        }

        // And install it to the binary path
        install_via_r(&paths.source.join(pkg_name), library_dir, &paths.binary, r_cmd)?;
    }

    Ok(())
}

fn install_package_from_repository(
    context: &CliContext,
    pkg: &ResolvedDependency,
    library_dir: &Path,
) -> Result<()> {
    let link_mode = LinkMode::new();
    let repo_server = RepoServer::from_url(pkg.source.repository_url());
    let pkg_paths =
        context
            .cache
            .get_package_paths(pkg.source.repository_url(), &pkg.name, &pkg.version);
    let binary_url = repo_server.get_binary_tarball_path(
        &pkg.name,
        &pkg.version,
        pkg.path.as_deref(),
        &context.cache.r_version,
        &context.cache.system_info,
    );

    if pkg.is_installed() {
        // If we don't have the binary, compile it
        if !pkg.installation_status.binary_available() {
            log::debug!(
                "Package {} already present in cache as source but not as binary.",
                pkg.name
            );
            install_via_r(
                &pkg_paths.source.join(pkg.name.as_ref()),
                library_dir,
                &pkg_paths.binary,
                &context.r_cmd
            )?;
        }
    } else {
        if pkg.kind == PackageType::Source || binary_url.is_none() {
            download_and_install_source(
                &repo_server.get_source_tarball_path(&pkg.name, &pkg.version, pkg.path.as_deref()),
                &pkg_paths,
                library_dir,
                &pkg.name,
                &context.r_cmd
            )?;
        } else {
            download_and_install_binary(&binary_url.unwrap(), &pkg_paths, library_dir, &pkg.name, &context.r_cmd)?;
        }
    }

    // And then we always link the binary folder into the staging library
    link_mode.link_files(&pkg.name, &pkg_paths.binary, &library_dir)?;

    Ok(())
}

fn install_package_from_git(
    context: &CliContext,
    pkg: &ResolvedDependency,
    library_dir: &Path,
) -> Result<()> {
    let link_mode = LinkMode::new();
    let repo_url = pkg.source.repository_url();
    let sha = pkg.source.git_sha();
    log::debug!("Installing {} from git", pkg.name);

    let pkg_paths = context.cache.get_git_package_paths(repo_url, sha);

    if !pkg.installation_status.binary_available() {
        let git_ops = Git {};
        // TODO: this won't work if multiple projects are trying to checkout different refs
        // on the same user at the same time
        log::debug!("Cloning repo if necessary + checkout");
        git_ops.clone_and_checkout(
            repo_url,
            Some(GitReference::Commit(&sha)),
            &pkg_paths.source,
        )?;
        // TODO: symlink file in cache directory
        log::debug!("Building the repo in {:?}", pkg_paths);
        // If we have a directory, don't forget to set it before building it
        let source_path = match &pkg.source {
            Source::Git {
                directory: Some(dir),
                ..
            } => pkg_paths.source.join(&dir),
            _ => pkg_paths.source,
        };
        install_via_r(&source_path, library_dir, &pkg_paths.binary, &context.r_cmd)?;
    }

    // And then we always link the binary folder into the staging library
    link_mode.link_files(&pkg.name, &pkg_paths.binary, &library_dir)?;

    Ok(())
}

/// Install a package and returns whether it was installed from cache or not
fn install_package(
    context: &CliContext,
    pkg: &ResolvedDependency,
    library_dir: &Path,
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        return Ok(());
    }

    match pkg.source {
        Source::Repository { .. } => install_package_from_repository(context, pkg, library_dir),
        Source::Git { .. } => install_package_from_git(context, pkg, library_dir),
        Source::Local { .. } => install_package_from_repository(context, pkg, library_dir),
    }
}

#[derive(Debug)]
pub struct SyncChange {
    pub name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub timing: Option<Duration>,
}

impl SyncChange {
    pub fn installed(name: &str, version: &str, timing: Duration) -> Self {
        Self {
            name: name.to_string(),
            installed: true,
            timing: Some(timing),
            version: Some(version.to_string()),
        }
    }

    pub fn removed(name: &str) -> Self {
        Self {
            name: name.to_string(),
            installed: false,
            timing: None,
            version: None,
        }
    }
}

/// `sync` will ensure the project library contains only exactly the dependencies from rproject.toml
/// or from the lockfile if present
/// There's 2 different paths:
/// 1. All deps are already installed, we might just need to remove some
/// 2. Some deps are missing, we need to install stuff
///
/// For option 1, we can just remove the symlinks manually without a risk of breaking anything.
/// For option 2, we want to install things but we don't want to mess the current library so
/// we install everything in a temp directory and only replace the library if everything installed
/// successfully.
///
///
/// This works the following way:
/// 2. Create a temp directory if things need to be installed
/// 2. Send all dependencies to install to worker threads in order
/// 3. Once a dep is installed, get the next step until it's over or need to wait on other deps
pub fn sync(
    context: &CliContext,
    deps: &[ResolvedDependency],
    dry_run: bool,
) -> Result<Vec<SyncChange>> {
    let mut sync_changes = Vec::new();
    let project_library = context.library_path();
    let staging_path = context.staging_path();
    let plan = BuildPlan::new(&deps);
    let num_deps_to_install = plan.num_to_install();
    let deps_to_install = plan.all_dependencies_names();
    let mut to_remove = HashSet::new();
    let mut deps_seen = 0;

    fs::create_dir_all(&project_library)?;
    for p in fs::read_dir(&project_library)? {
        let p = p?.path().canonicalize()?;
        if p.is_dir() {
            let dir_name = p.file_name().unwrap().to_string_lossy();
            if deps_to_install.contains(&*dir_name) {
                deps_seen += 1;
            } else {
                to_remove.insert(dir_name.to_string());
            }
        }
    }

    // Clean up at all times, even with a dry run
    if staging_path.is_dir() {
        fs::remove_dir_all(&staging_path)?;
    }

    for dir_name in to_remove {
        // Only actually remove the deps if we are not going to rebuild the lib folder
        if deps_seen == num_deps_to_install {
            let p = project_library.join(&dir_name);
            if !dry_run {
                log::debug!("Removing {dir_name} from library");
                fs::remove_dir_all(&p)?;
            }
        }

        sync_changes.push(SyncChange::removed(&dir_name));
    }

    // If we have all the deps we need, exit early
    if deps_seen == num_deps_to_install {
        if !dry_run {
            log::debug!("No new dependencies to install");
        }
        return Ok(sync_changes);
    }

    // Create staging only if we need to build stuff
    fs::create_dir_all(&staging_path)?;

    // We can't use references from the BuildPlan since we borrow mutably from it so we
    // create a lookup table for resolved deps by name and use those references across channels.
    let dep_by_name: HashMap<_, _> = deps.iter().map(|d| (&d.name, d)).collect();
    let plan = Arc::new(Mutex::new(plan));

    let (ready_sender, ready_receiver) = channel::unbounded();
    let (done_sender, done_receiver) = channel::unbounded();

    // Initial deps we can install immediately
    {
        let mut plan = plan.lock().unwrap();
        while let BuildStep::Install(d) = plan.get() {
            ready_sender.send(dep_by_name[&d.name]).unwrap();
        }
    }

    let installed_count = Arc::new(AtomicUsize::new(0));
    let has_errors = Arc::new(AtomicBool::new(false));
    let errors = Arc::new(Mutex::new(Vec::new()));

    thread::scope(|s| {
        let plan_clone = Arc::clone(&plan);
        let ready_sender_clone = ready_sender.clone();
        let installed_count_clone = Arc::clone(&installed_count);
        let has_errors_clone = Arc::clone(&has_errors);

        // Different thread to monitor what needs to be installed next
        s.spawn(move |_| {
            let mut seen = HashSet::new();
            while !has_errors_clone.load(Ordering::Relaxed)
                && installed_count_clone.load(Ordering::Relaxed) < num_deps_to_install
            {
                let mut plan = plan_clone.lock().unwrap();
                let mut ready = Vec::new();
                while let BuildStep::Install(d) = plan.get() {
                    ready.push(dep_by_name[&d.name]);
                }
                drop(plan); // Release lock before sending

                for p in ready {
                    if !seen.contains(&p.name) {
                        seen.insert(&p.name);
                        ready_sender_clone.send(p).unwrap();
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            drop(ready_sender_clone);
        });

        // Our worker threads that will actually perform the installation
        // TODO: make this overridable
        let num_workers = num_cpus::get();
        for _ in 0..num_workers {
            let ready_receiver = ready_receiver.clone();
            let done_sender = done_sender.clone();
            let plan = Arc::clone(&plan);
            let has_errors_clone = Arc::clone(&has_errors);
            let errors_clone = Arc::clone(&errors);
            let s_path = staging_path.as_path();

            s.spawn(move |_| {
                while let Ok(dep) = ready_receiver.recv() {
                    if has_errors_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    if !dry_run {
                        match dep.kind {
                            PackageType::Source => log::debug!("Installing {} (source)", dep.name),
                            PackageType::Binary => log::debug!("Installing {} (binary)", dep.name),
                        }
                    }
                    let start = std::time::Instant::now();
                    match install_package(&context, dep, s_path, dry_run) {
                        Ok(_) => {
                            let sync_change =
                                SyncChange::installed(&dep.name, &dep.version, start.elapsed());
                            let mut plan = plan.lock().unwrap();
                            plan.mark_installed(&dep.name);
                            drop(plan);
                            done_sender.send(sync_change).unwrap();
                        }
                        Err(e) => {
                            has_errors_clone.store(true, Ordering::Relaxed);
                            errors_clone.lock().unwrap().push((dep, e));
                            break;
                        }
                    }
                }
                drop(done_sender);
            });
        }

        // Monitor progress in the main thread
        loop {
            if has_errors.load(Ordering::Relaxed) {
                break;
            }
            // timeout is necessary to avoid deadlock
            if let Ok(change) = done_receiver.recv_timeout(Duration::from_millis(1)) {
                installed_count.fetch_add(1, Ordering::Relaxed);
                if !dry_run {
                    log::debug!(
                        "Completed installing {} ({}/{})",
                        change.name,
                        installed_count.load(Ordering::Relaxed),
                        num_deps_to_install
                    );
                }
                sync_changes.push(change);
                if installed_count.load(Ordering::Relaxed) == num_deps_to_install
                    || has_errors.load(Ordering::Relaxed)
                {
                    break;
                }
            }
        }

        // Clean up
        drop(ready_sender);
    })
    .expect("threads to not panic");

    if has_errors.load(Ordering::Relaxed) {
        let err = errors.lock().unwrap();
        let mut message = String::from("Failed to install dependencies.");
        for (dep, e) in &*err {
            message += &format!("\n    Failed to install {}:\n        {e}", dep.name);
        }
        bail!(message);
    }

    // If we are there, it means we are successful. Replace the project lib by the tmp dir
    if project_library.is_dir() {
        fs::remove_dir_all(&project_library)?;
    }

    fs::rename(&staging_path, &project_library)?;

    Ok(sync_changes)
}
