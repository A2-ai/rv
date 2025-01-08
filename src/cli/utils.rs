use std::io;
use std::path::Path;

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

pub use timeit;
