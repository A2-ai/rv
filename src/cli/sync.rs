use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use fs_err::{self as fs};
use serde::Serialize;

use crate::cli::{Context, OutputFormat, ResolveMode, resolve_dependencies};
use crate::sync::OutputSection;
use crate::{Lockfile, Resolution, SyncChange, SyncHandler, system_req, timeit};

#[derive(Debug, Default, Serialize)]
struct SyncChanges {
    installed: Vec<SyncChange>,
    removed: Vec<SyncChange>,
}

impl SyncChanges {
    fn from_changes(changes: Vec<SyncChange>) -> Self {
        let mut installed = vec![];
        let mut removed = vec![];
        for change in changes {
            if change.installed {
                installed.push(change);
            } else {
                removed.push(change);
            }
        }
        Self { installed, removed }
    }
}

#[derive(Debug)]
pub struct SyncHelper {
    pub dry_run: bool,
    pub output_format: Option<OutputFormat>,
    pub save_install_logs_in: Option<PathBuf>,
    pub exit_on_failure: bool,
}

impl Default for SyncHelper {
    fn default() -> Self {
        Self {
            dry_run: true,
            output_format: None,
            save_install_logs_in: None,
            exit_on_failure: true,
        }
    }
}

impl SyncHelper {
    pub fn run<'a>(
        &self,
        context: &'a Context,
        resolve_mode: ResolveMode,
    ) -> Result<Resolution<'a>> {
        let sync_start = std::time::Instant::now();
        // TODO: exit on failure without println? and move that to main.rs
        // otherwise callers will think everything is fine
        let resolution = resolve_dependencies(context, resolve_mode, self.exit_on_failure);

        match timeit!(
            if self.dry_run {
                "Planned dependencies"
            } else {
                "Synced dependencies"
            },
            {
                let mut handler = SyncHandler::new(context, self.save_install_logs_in.clone());
                if self.dry_run {
                    handler.dry_run();
                }
                if context.show_progress_bar {
                    handler.show_progress_bar();
                }
                handler.set_uses_lockfile(context.config.use_lockfile());
                handler.handle(&resolution.found, &context.r_cmd)
            }
        ) {
            Ok(mut changes) => {
                if !self.dry_run && context.config.use_lockfile() {
                    if resolution.found.is_empty() {
                        // delete the lockfiles if there are no dependencies
                        let lockfile_path = context.lockfile_path();
                        if lockfile_path.exists() {
                            fs::remove_file(lockfile_path)?;
                        }
                    } else {
                        let lockfile = Lockfile::from_resolved(
                            &context.r_version.major_minor(),
                            resolution.found.clone(),
                        );
                        if let Some(existing_lockfile) = &context.lockfile {
                            if existing_lockfile != &lockfile {
                                lockfile.save(context.lockfile_path())?;
                                log::debug!("Lockfile changed, saving it.");
                            }
                        } else {
                            lockfile.save(context.lockfile_path())?;
                        }
                    }
                }
                let all_sys_deps: HashSet<_> = changes
                    .iter()
                    .flat_map(|x| x.sys_deps.iter().map(|x| x.name.as_str()))
                    .collect();
                let sysdeps_status = system_req::check_installation_status(
                    context.cache.system_info(),
                    &all_sys_deps,
                );

                for change in changes.iter_mut() {
                    change.update_sys_deps_status(&sysdeps_status);
                }

                if let Some(log_folder) = &self.save_install_logs_in {
                    fs::create_dir_all(log_folder)?;
                    for change in changes.iter().filter(|x| x.installed) {
                        let log_path = change.log_path(context.cache.local());
                        if log_path.exists() {
                            fs::copy(log_path, log_folder.join(format!("{}.log", change.name)))?;
                        }
                    }
                }

                if let Some(format) = &self.output_format {
                    if format.is_json() {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&SyncChanges::from_changes(changes))
                                .expect("valid json")
                        );
                    } else {
                        let installed_count = changes.iter().filter(|c| c.installed).count();
                        let removed_count = changes.iter().filter(|c| !c.installed).count();

                        print_grouped_changes(&changes, !self.dry_run, !sysdeps_status.is_empty());

                        if !self.dry_run {
                            println!(
                                "sync completed in {} ({} installed, {} removed)",
                                format_duration(sync_start.elapsed()),
                                installed_count,
                                removed_count
                            );
                        }
                    }
                }

                Ok(resolution)
            }
            Err(e) => {
                if context.staging_path().is_dir() {
                    fs::remove_dir_all(context.staging_path())?;
                }
                Err(e.into())
            }
        }
    }
}

/// Print changes grouped by section with aligned columns
fn print_grouped_changes(changes: &[SyncChange], include_timings: bool, supports_sysdeps: bool) {
    if changes.is_empty() {
        println!("Nothing to do");
        return;
    }

    // Group by section
    let mut sections: BTreeMap<OutputSection, Vec<&SyncChange>> = BTreeMap::new();
    for change in changes {
        sections.entry(change.section()).or_default().push(change);
    }

    // Sort each section alphabetically by package name
    for items in sections.values_mut() {
        items.sort_by(|a, b| a.name.cmp(&b.name));
    }

    // Compute max widths across ALL installed packages for consistent alignment
    let installed: Vec<_> = changes.iter().filter(|c| c.installed).collect();
    let max_name = installed.iter().map(|c| c.name.len()).max().unwrap_or(0);
    let max_ver = installed
        .iter()
        .map(|c| c.version.as_ref().map(|v| v.len()).unwrap_or(0))
        .max()
        .unwrap_or(0);
    let max_kind = 6; // "binary" or "source"
    let max_source = installed
        .iter()
        .map(|c| c.source_display().len())
        .max()
        .unwrap_or(0);

    // Section order
    let section_order: [OutputSection; 5] = [
        OutputSection::GlobalCache,
        OutputSection::LocalCache,
        OutputSection::Downloaded,
        OutputSection::LocalPath,
        OutputSection::Removed,
    ];

    for section in section_order {
        if let Some(items) = sections.get(&section) {
            println!("{} ({}):", section.header(), items.len());
            for c in items {
                if c.installed {
                    let timing_str = if include_timings {
                        format!("  {:>8}", format_duration(c.timing.unwrap()))
                    } else {
                        String::new()
                    };
                    let sys_deps_str = format_sys_deps(c, supports_sysdeps);
                    println!(
                        "  + {:<name_w$}  {:>ver_w$}  {:<kind_w$}  {:<src_w$}{}{}",
                        c.name,
                        c.version.as_ref().unwrap(),
                        c.kind.unwrap(),
                        c.source_display(),
                        timing_str,
                        sys_deps_str,
                        name_w = max_name,
                        ver_w = max_ver,
                        kind_w = max_kind,
                        src_w = max_source,
                    );
                } else {
                    println!("  - {}", c.name);
                }
            }
            println!();
        }
    }
}

/// Format sys deps for display
fn format_sys_deps(change: &SyncChange, supports_sysdeps: bool) -> String {
    if change.sys_deps.is_empty() {
        return String::new();
    }

    let deps: Vec<String> = change
        .sys_deps
        .iter()
        .map(|sys_dep| {
            if supports_sysdeps {
                let status = if sys_dep.status == system_req::SysInstallationStatus::Present {
                    "✓"
                } else {
                    "✗"
                };
                format!("{} {}", status, sys_dep.name)
            } else {
                sys_dep.name.clone()
            }
        })
        .collect();

    format!("  [sys deps: {}]", deps.join(", "))
}

/// Format duration for display (e.g., "1.2s" or "234ms")
fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms >= 1000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}ms", ms)
    }
}
