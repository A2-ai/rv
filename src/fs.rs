use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// Copy the whole content of a folder to another folder
pub(crate) fn copy_folder(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
) -> Result<(), std::io::Error> {
    let from = from.as_ref();
    let to = to.as_ref();

    for entry in WalkDir::new(from) {
        let entry = entry?;
        let path = entry.path();

        let relative = path.strip_prefix(&from).expect("walkdir starts with root");
        let out_path = to.join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        fs::copy(path, out_path)?;
    }

    Ok(())
}
