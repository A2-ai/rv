use crate::package::PackageType;
use crate::version::Version;
use crate::OsType;
use crate::SystemInfo;
use flate2::read::GzDecoder;
use log::{debug, error, info, trace};
use shellexpand;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::fs::File;
use std::path::Path;
use std::process::Command;
use tar::Archive;
use tempfile::tempdir;
use url::Url;
use rayon::prelude::*;

/// Extracts a `.tar.gz` archive to the specified destination directory.
/// If the destination directory does not exist, it is created.
///
/// # Arguments
///
/// * `archive_path` - A string slice that holds the path to the `.tar.gz` file.
/// * `destination` - A string slice that holds the path to the destination directory.
///
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
        debug!("Created directory: {}", dest.display());
    }

    // Open the tar.gz file
    let tar_gz = File::open(&archive_path)?;
    let decompressor = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(decompressor);

    // Extract the archive into the destination directory
    archive.unpack(&dest)?;

    trace!(
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
    sysinfo: &SystemInfo,
    package_type: PackageType,
    library: D,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Parse the URL to extract the filename and set query
    let dest = install_dir.as_ref();
    // TODO: return results back for whether this was already installed vs
    // was installed then so can report back to user more clearly
    // a likely better design will be to separate out determining whats already
    // present and only request to install what needs to be installed
    if dest.join(name).exists() {
        debug!("Package '{}' already present in cache", name);
        return Ok(());
    }
    debug!("Installing package from {} to {:?}", &url, dest);
    let mut parsed_url = Url::parse(url)?;
    
    let url = if let OsType::Linux(_) = sysinfo.os_type {
        crate::repo_path::set_rversion_arch_query(&mut parsed_url, rvparts, sysinfo.arch());
        parsed_url.as_str()
    } else {
        url
    };

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
        None,
    )?;
    debug!("Downloaded '{}' in {:?}", file_name, start_time.elapsed());

    start_time = std::time::Instant::now();
    let install_result = match package_type {
        PackageType::Binary => untar_package(&temp_path, dest),
        PackageType::Source => install_src_package(&temp_path, dest, library.as_ref()),
    };

    install_result
        .map(|s| {
            info!("Installed '{}' in {:?}", name, start_time.elapsed());
            s
        })
        .map_err(|e| {
            error!("Failed to install '{}': {}", name, e);
            e.into()
        })
}

fn install_src_package(
    src_path: &Path,
    dest: &Path,
    library: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create the destination directory if it doesn't exist
    if !dest.exists() {
        fs::create_dir_all(&dest)?;
        debug!("Created cache install directory: {:?}", dest);
    }
    let filtered_env: HashMap<String, String> = env::vars()
        .filter(|(k, _)| !k.starts_with("R_LIBS"))
        .collect();
    let mut command = Command::new("R");
    command
        .arg("CMD")
        .arg("INSTALL")
        .arg(format!("--library={}", dest.as_os_str().to_str().unwrap()))
        .arg("--use-vanilla")
        .arg(src_path)
        // the library itself should be where the packages are actually installed to,
        // not the dest, which is the cache dir
        .env("R_LIBS_SITE", library)
        .env("R_LIBS_USER", library)
        // Preserve other environment variables
        .envs(&filtered_env);

    let output = command.output()?;
    debug!("R CMD INSTALL output: {:?}", output);
    if !output.status.success() {
        let error_message = String::from_utf8_lossy(&output.stderr);
        error!("R package installation failed: {}", error_message);
        return Err(format!("R package installation failed: {}", error_message).into());
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: String,
    pub version: Version,
    pub path: String,
}

pub fn get_installed_pkgs(library: &Path) -> Result<HashMap<String, InstalledPackage>, Box<dyn std::error::Error>> {
    let start_time = std::time::Instant::now();
    // iterate over the library directory, for each directory, look to see if there is a description file
    // if there is, read it in and parse it to get the package name and version
    // can use the parse_description_file function from the package crate

    let installed_pkgs = fs::read_dir(library)?
        .par_bridge()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_dir() {
                let desc_path = path.join("DESCRIPTION");
                if desc_path.exists() {
                    let package_content = std::fs::read_to_string(&desc_path).ok()?;
                    let desc = crate::package::parse_description_file(&package_content);
                    return Some((desc.name.clone(), InstalledPackage {
                        name: desc.name,
                        version: desc.version,
                        path: path.to_string_lossy().to_string(),
                    }));
                }
            }
            None
        })
        .collect();
    debug!("Getting installed packages took: {:?}", start_time.elapsed());
    Ok(installed_pkgs)
}