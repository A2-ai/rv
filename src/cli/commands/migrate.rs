use std::path::Path;

use anyhow::{Ok, Result};

use crate::{cli::renv, renv_lock::RenvLock};

fn migrate_renv<P: AsRef<Path>>(path: P) -> Result<()> {
    let renv_lock = RenvLock::parse_renv_lock(path)?;
    let (resolved_renv, _unresolved_renv) = renv::resolve(renv_lock)?;
    Ok(())
}

mod tests {
    #[test]
    fn test_renv_resolution() {
        super::migrate_renv("src/tests/renv/").unwrap();
    }
}
