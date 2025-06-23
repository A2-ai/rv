use std::path::Path;

use anyhow::Result;

use crate::{cli::{CliContext, RCommandLookup}, CacheInfo};

fn list_dirs(config_file: impl AsRef<Path>, log_enabled: bool) -> Result<CacheInfo> {
    let mut context = CliContext::new(config_file.as_ref(), RCommandLookup::Skip)?;
    if !log_enabled {
        context.show_progress_bar();
    }
    context.load_databases()?;

    Ok(())
}