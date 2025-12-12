use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::Result;
use fs_err::{self as fs};
use serde::Serialize;

use crate::cli::{Context, OutputFormat, ResolveMode, resolve_dependencies};
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
                    &context.cache.system_info,
                    &all_sys_deps,
                );

                for change in changes.iter_mut() {
                    change.update_sys_deps_status(&sysdeps_status);
                }

                if let Some(log_folder) = &self.save_install_logs_in {
                    fs::create_dir_all(log_folder)?;
                    for change in changes.iter().filter(|x| x.installed) {
                        let log_path = change.log_path(&context.cache);
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
                    } else if changes.is_empty() {
                        println!("Nothing to do");
                    } else {
                        for c in changes {
                            println!("{}", c.print(!self.dry_run, !sysdeps_status.is_empty()));
                        }
                    }

                    if !self.dry_run && !format.is_json() {
                        println!("sync completed in {} ms", sync_start.elapsed().as_millis());
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
