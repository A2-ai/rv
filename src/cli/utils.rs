use std::io;
use std::path::Path;

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use tar::Archive;

pub fn write_err(err: &(dyn std::error::Error + 'static)) -> String {
    let mut out = format!("{err}");

    let mut cause = err.source();
    while let Some(e) = cause {
        out += &format!("\nReason: {e}");
        cause = e.source();
    }

    out
}

/// Same as std but indicates the path
pub fn create_dir_all(path: &Path) -> Result<()> {
    std::fs::create_dir_all(&path)
        .with_context(|| format!("Failed to created directory at {path:?}"))?;
    Ok(())
}

pub fn untar_package<R: io::Read, T: AsRef<Path>>(reader: R, destination: T) -> Result<()> {
    let destination = destination.as_ref();
    create_dir_all(destination)?;

    let tar = GzDecoder::new(reader);
    let mut archive = Archive::new(tar);
    archive.unpack(destination)?;

    log::debug!("Successfully extracted archive to {destination:?}");
    Ok(())
}
