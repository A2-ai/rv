use std::path::Path;

use anyhow::{Ok, Result};

use crate::{cli::renv, renv_lock::RenvLock};

fn migrate_renv<P: AsRef<Path>>(path: P) -> Result<()> {
    let renv_lock = RenvLock::parse_renv_lock(path)?;
    let (resolved_renv, _unresolved_renv) = renv::ResolvedRenv::resolve_renv(renv_lock)?;
    Ok(())
}

mod tests {
    use super::migrate_renv;

    #[test]
    fn test_renv_resolution() {
        migrate_renv("src/tests/renv/").unwrap();
    }
}
