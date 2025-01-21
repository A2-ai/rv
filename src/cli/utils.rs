use std::io;
use std::path::{Path, PathBuf};

use anyhow::Result;
use flate2::read::GzDecoder;
use fs_err as fs;
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

pub fn untar_package<R: io::Read, T: AsRef<Path>>(reader: R, destination: T) -> Result<()> {
    let destination = destination.as_ref();
    fs::create_dir_all(destination)?;

    let tar = GzDecoder::new(reader);
    let mut archive = Archive::new(tar);
    archive.unpack(destination)?;

    log::debug!("Successfully extracted archive to {destination:?}");
    Ok(())
}

/// Builds the path for binary in the cache and the library based on os info and R version
/// {R_Version}/{arch}/{codename}/
pub fn get_os_path(system_info: &SystemInfo, r_version: [u32; 2]) -> PathBuf {
    let mut path = PathBuf::new().join(format!("{}.{}", r_version[0], r_version[1]));

    if let Some(arch) = system_info.arch() {
        path = path.join(arch);
    }
    if let Some(codename) = system_info.codename() {
        path = path.join(codename);
    }

    path
}

#[macro_export]
macro_rules! timeit {
    ($msg:expr, $x:expr) => {{
        let start = std::time::Instant::now();
        let res = $x;
        let duration = start.elapsed();
        log::info!("{} in {}ms", $msg, duration.as_millis());
        res
    }};
}

use crate::SystemInfo;
pub use timeit;
