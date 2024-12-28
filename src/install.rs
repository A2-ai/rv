use flate2::read::GzDecoder;
use shellexpand;
use std::fs;
use std::fs::File;
use std::io;
use std::path::Path;
use tar::Archive;

/// Extracts a `.tar.gz` archive to the specified destination directory.
/// If the destination directory does not exist, it is created.
///
/// # Arguments
///
/// * `archive_path` - A string slice that holds the path to the `.tar.gz` file.
/// * `destination` - A string slice that holds the path to the destination directory.
///
/// # Example
///
/// ```
/// untar_package("dplyr_1.1.4.tar.gz", "~/rpkgs").expect("Extraction failed");
/// ```
pub fn untar_package<P: AsRef<Path>, D: AsRef<Path>>(
    archive_path: P,
    destination: D,
) -> Result<(), Box<dyn std::error::Error>> {
    // Expand the destination path to handle the '~'
    let dest_path = shellexpand::tilde(destination.as_ref().to_str().unwrap()).to_string();
    let dest = Path::new(&dest_path);

    // Create the destination directory if it doesn't exist
    if !dest.exists() {
        fs::create_dir_all(&dest)?;
        println!("Created directory: {}", dest.display());
    }

    // Open the tar.gz file
    let tar_gz = File::open(&archive_path)?;
    let decompressor = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(decompressor);

    // Extract the archive into the destination directory
    archive.unpack(&dest)?;

    println!(
        "Successfully extracted '{}' to '{}'",
        archive_path.as_ref().display(),
        dest.display()
    );

    Ok(())
}
