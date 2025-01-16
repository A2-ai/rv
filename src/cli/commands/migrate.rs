use std::path::Path;

use anyhow::{Ok, Result};

use crate::{cli::renv, renv_lock::RenvLock};

fn migrate_renv<P: AsRef<Path>>(path: P) -> Result<()> {
    let renv_lock = RenvLock::parse_renv_lock(path)?;
    let (resolved_renv, unresolved_renv) = renv::ResolvedRenv::resolve_renv(renv_lock)?;
    Ok(())
}

mod tests {
    use super::migrate_renv;

    #[test]
    fn tester() {
        println!(
            "{:#?}",
            migrate_renv("/cluster-data/user-homes/wes/projects/rv/src/tests/renv/").unwrap()
        );
    }
}
