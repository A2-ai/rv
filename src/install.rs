use flate2::read::GzDecoder;
use shellexpand;
use std::fs;
use std::fs::File;
use std::path::Path;
use tar::Archive;
use tempfile::tempdir;
use url::Url;

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


// Overload this function for quick purposes - should be separate activities
pub fn dl_and_install_pkg<D: AsRef<Path>>(
    name: &str,
    url: &str,
    install_dir: D,
    rvparts: &[u32; 2],
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Parse the URL to extract the filename
    let dest = install_dir.as_ref();
    if dest.join(name).exists() {
        println!("Package '{}' already installed", name);
        return Ok(());
    }
    println!("Installing package from {} to {:?}", &url, dest);
    let parsed_url = Url::parse(url)?;
    let file_name = parsed_url
        .path_segments()
        .and_then(|segments| segments.last())
        .ok_or("Cannot extract filename from URL")?;

    // 2. Create a temporary directory
    let temp_dir = tempdir()?;
    let temp_path = temp_dir.path().join(file_name);

    // 3. Download the file to the temporary path
    let mut start_time = std::time::Instant::now();
    crate::cli::http::download(
        url,
        &mut File::create(&temp_path)?,
        Some((
            "user-agent",
            format!("R/{}.{}", rvparts[0], rvparts[1]).into(),
        )),
    )?;
    println!(
        "Downloaded '{}' in {:?}",
        file_name,
        start_time.elapsed()
    );

    start_time = std::time::Instant::now();
    let result = untar_package(&temp_path, dest)
        .map(|_| {
            println!(
                "Installed '{}' in {:?}",
                name,
                start_time.elapsed()
            )
        })
        .map_err(|e| {
            eprintln!("Failed to install '{}': {}", name, e);
            e.into()
        });

    result
}