use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Result};
use crossbeam::{channel, thread};
use fs_err as fs;

use crate::cli::cache::PackagePaths;
use crate::cli::CliContext;
use crate::consts::LOCAL_MTIME_FILENAME;
use crate::fs::mtime_recursive;
use crate::git::GitReference;
use crate::link::LinkMode;
use crate::lockfile::Source;
use crate::package::{is_binary_package, PackageType};
use crate::{
    BuildPlan, BuildStep, Http, HttpDownload, Library, RCmd, RCommandLine, RepoServer,
    ResolvedDependency,
};
use crate::{Git, GitOperations};

fn install_via_r(
    source: &Path,
    library_dir: &Path,
    binary_dir: &Path,
    r_cmd: &RCommandLine,
) -> Result<()> {
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
    Http {}.download_and_untar(&url, &paths.source, false)?;
    log::debug!("Compiling binary from {}", &paths.source.display());
    r_cmd.install(paths.source.join(pkg_name), library_dir, &paths.binary)?;
    Ok(())
}

fn download_and_install_binary(
    url: &str,
    source_url: &str,
    paths: &PackagePaths,
    library_dir: &Path,
    pkg_name: &str,
    r_cmd: &RCommandLine,
) -> Result<()> {
    let http = Http {};
    // If we get an error doing the binary download, fall back to source
    if let Err(e) = http.download_and_untar(&url, &paths.binary, false) {
        log::warn!("Failed to download/untar binary package from {url}: {e:?}, falling back to {source_url}");
        return download_and_install_source(source_url, paths, library_dir, pkg_name, r_cmd);
    }

    // Ok we download some tarball. We can't assume it's actually compiled though, it could be just
    // source files. We have to check first whether what we have is actually binary content.
    if !is_binary_package(&paths.binary.join(pkg_name), pkg_name) {
        log::debug!("{pkg_name} was expected as binary, found to be source. Compiling binary for {pkg_name}...");
        // Move it to the source destination if we don't have it already
        if paths.source.is_dir() {
            fs::remove_dir_all(&paths.binary)?;
        } else {
            fs::create_dir_all(&paths.source)?;
            fs::rename(&paths.binary, &paths.source)?;
        }

        // And install it to the binary path
        install_via_r(
            &paths.source.join(pkg_name),
            library_dir,
            &paths.binary,
            r_cmd,
        )?;
    }

    Ok(())
}

fn install_package_from_repository(
    context: &CliContext,
    pkg: &ResolvedDependency,
    library_dir: &Path,
) -> Result<()> {
    let link_mode = LinkMode::new();
    let repo_server = RepoServer::from_url(pkg.source.source_path());
    let pkg_paths =
        context
            .cache
            .get_package_paths(pkg.source.source_path(), &pkg.name, &pkg.version.original);

    let source_url =
        repo_server.get_source_tarball_path(&pkg.name, &pkg.version.original, pkg.path.as_deref());
    let binary_url = repo_server.get_binary_tarball_path(
        &pkg.name,
        &pkg.version.original,
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
                &context.r_cmd,
            )?;
        }
    } else {
        if pkg.kind == PackageType::Source || binary_url.is_none() {
            download_and_install_source(&source_url, &pkg_paths, library_dir, &pkg.name, &context.r_cmd)?;
        } else {
            download_and_install_binary(
                &binary_url.unwrap(),
                &source_url,
                &pkg_paths,
                library_dir,
                &pkg.name,
                &context.r_cmd,
            )?;
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
    let repo_url = pkg.source.source_path();
    let sha = pkg.source.git_sha();

    let pkg_paths = context.cache.get_git_package_paths(repo_url, sha);

    if !pkg.installation_status.binary_available() {
        let git_ops = Git {};
        // TODO: this won't work if multiple projects are trying to checkout different refs
        // on the same user at the same time
        git_ops.clone_and_checkout(
            repo_url,
            Some(GitReference::Commit(&sha)),
            &pkg_paths.source,
        )?;
        log::debug!("Building the repo for {}", pkg.name);
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

fn install_local_package(context: &CliContext, pkg: &ResolvedDependency, library_dir: &Path) -> Result<()> {
    // First we check if the package exists in the library and what's the mtime in it
    let local_path = Path::new(pkg.source.source_path()).canonicalize()?;
    // TODO: we actually do that twice, a bit wasteful
    let local_mtime = mtime_recursive(&local_path)?;

    // if the mtime we found locally is more recent, we build it
    log::debug!("Building the local package in {}", local_path.display());
    install_via_r(&local_path, library_dir, &library_dir, &context.r_cmd)?;

    // And just write the mtime in the output directory
    let mut file = fs::File::create(
        library_dir
            .join(pkg.name.as_ref())
            .join(LOCAL_MTIME_FILENAME),
    )?;
    file.write_all(local_mtime.unix_seconds().to_string().as_bytes())?;

    Ok(())
}

fn install_url_package(
    context: &CliContext,
    pkg: &ResolvedDependency,
    library_dir: &Path,
) -> Result<()> {
    let link_mode = LinkMode::new();
    let (url, sha) = pkg.source.url_info();

    let pkg_paths = context.cache.get_url_package_paths(url, sha);
    let download_path = pkg_paths.source.join(pkg.name.as_ref());

    // If we have a binary, copy it since we don't keep cache around for binary URL packages
    if pkg.kind == PackageType::Binary {
        log::debug!(
            "Package from URL in {} is already a binary",
            download_path.display()
        );
        if !pkg_paths.binary.is_dir() {
            LinkMode::Copy.link_files(&pkg.name, &pkg_paths.source, &pkg_paths.binary)?;
        }
    } else {
        log::debug!(
            "Building the package from URL in {}",
            download_path.display()
        );
        install_via_r(&download_path, library_dir, &pkg_paths.binary, &context.r_cmd)?;
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
        Source::Local { .. } => install_local_package(context, pkg, library_dir),
        Source::Url { .. } => install_url_package(context, pkg, library_dir),
    }
}

/// If a local package hasn't changed, we copy it from the current library to the staging dir
fn copy_package(
    context: &CliContext,
    pkg: &ResolvedDependency,
    library_dir: &Path,
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        return Ok(());
    }

    log::debug!("Copying package {} from current library", &pkg.name);
    LinkMode::Copy.link_files(
        &pkg.name,
        context.library.path().join(pkg.name.as_ref()),
        library_dir.join(pkg.name.as_ref()),
    )?;

    Ok(())
}

#[derive(Debug)]
pub struct SyncChange {
    pub name: String,
    pub installed: bool,
    pub kind: Option<PackageType>,
    pub version: Option<String>,
    pub source: Option<String>,
    pub timing: Option<Duration>,
}

impl SyncChange {
    pub fn installed(
        name: &str,
        version: &str,
        source: &str,
        kind: PackageType,
        timing: Duration,
    ) -> Self {
        Self {
            name: name.to_string(),
            installed: true,
            kind: Some(kind),
            timing: Some(timing),
            source: Some(source.to_string()),
            version: Some(version.to_string()),
        }
    }

    pub fn removed(name: &str) -> Self {
        Self {
            name: name.to_string(),
            installed: false,
            kind: None,
            timing: None,
            source: None,
            version: None,
        }
    }

    pub fn print(&self, include_timings: bool) -> String {
        if self.installed {
            let mut base = format!(
                "+ {} ({}, {} from {})",
                self.name,
                self.version.as_ref().unwrap(),
                self.kind.unwrap(),
                self.source.as_ref().unwrap(),
            );

            if include_timings {
                base += &format!(" in {}ms", self.timing.unwrap().as_millis());
                base
            } else {
                base
            }
        } else {
            format!("- {}", self.name)
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
    library: &Library,
    dry_run: bool,
) -> Result<Vec<SyncChange>> {
    let mut sync_changes = Vec::new();
    let project_library = context.library_path();
    let staging_path = context.staging_path();
    let plan = BuildPlan::new(&deps);
    let num_deps_to_install = plan.num_to_install();
    let deps_to_install = plan.all_dependencies();
    // (name, notify). We do not notify if the package is broken in some ways, otherwise we
    // do notify.
    let mut to_remove = HashSet::new();
    let mut deps_seen = HashSet::new();

    fs::create_dir_all(&project_library)?;

    for (name, version) in &library.packages {
        if deps_to_install
            .get(name.as_str())
            .map(|v| *v == version)
            .unwrap_or(false)
        {
            deps_seen.insert(name.as_str());
        } else {
            to_remove.insert((name.to_string(), true));
        }
    }

    // (name, whether to copy from current library)
    let mut local_deps = HashMap::new();
    // For local deps we also need to check whether the files from the source are newer than what
    // is installed currently, if that's the case. If the folder exists and is the same as
    // what we need to build, we will just copy it
    for dep in deps.iter().filter(|x| x.is_local()) {
        if deps_seen.contains(dep.name.as_ref()) {
            let local_path = Path::new(dep.source.source_path());
            let local_mtime = mtime_recursive(&local_path)?;
            let mtime_found = context
                .library
                .local_packages
                .get(dep.name.as_ref())
                .unwrap_or(&0);

            // if the mtime we found in the lib is lower than the source folder
            // remove it from deps_seen as we will need to install it and remove the
            // existing one from the library
            if *mtime_found < local_mtime.unix_seconds() {
                deps_seen.remove(dep.name.as_ref());
                to_remove.insert((dep.name.as_ref().to_string(), false));
                local_deps.insert(dep.name.as_ref(), false);
            } else {
                // same mtime or higher: we copy from the library
                local_deps.insert(dep.name.as_ref(), true);
            }
        } else {
            local_deps.insert(dep.name.as_ref(), false);
        }
    }
    // make it immutable
    let local_deps = Arc::new(local_deps);

    for name in &library.broken {
        log::debug!("Package {name} in library is broken");
        to_remove.insert((name.to_string(), false));
    }

    // Clean up at all times, even with a dry run
    if staging_path.is_dir() {
        fs::remove_dir_all(&staging_path)?;
    }

    for (dir_name, notify) in to_remove {
        // Only actually remove the deps if we are not going to rebuild the lib folder
        if deps_seen.len() == num_deps_to_install {
            let p = project_library.join(&dir_name);
            if !dry_run && notify {
                log::debug!("Removing {dir_name} from library");
                fs::remove_dir_all(&p)?;
            }
        }

        if notify {
            sync_changes.push(SyncChange::removed(&dir_name));
        }
    }

    // If we have all the deps we need, exit early
    if deps_seen.len() == num_deps_to_install {
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
            let local_deps_clone = Arc::clone(&local_deps);

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
                    let install_result = if local_deps_clone
                        .get(dep.name.as_ref())
                        .cloned()
                        .unwrap_or_default()
                    {
                        copy_package(&context, dep, s_path, dry_run)
                    } else {
                        install_package(&context, dep, s_path, dry_run)
                    };
                    match install_result {
                        Ok(_) => {
                            let sync_change = SyncChange::installed(
                                &dep.name,
                                &dep.version.original,
                                dep.source.source_path(),
                                dep.kind,
                                start.elapsed(),
                            );
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
                if !deps_seen.contains(change.name.as_str()) {
                    sync_changes.push(change);
                }
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

    // Sort all changes by a-z and fall back on installed status for things with the same name
    sync_changes.sort_unstable_by(
        |a, b| match a.name.to_lowercase().cmp(&b.name.to_lowercase()) {
            std::cmp::Ordering::Equal => a.installed.cmp(&b.installed),
            ordering => ordering,
        },
    );

    Ok(sync_changes)
}
