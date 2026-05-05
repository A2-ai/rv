use self_update::cargo_crate_version;

pub fn update_rv() -> anyhow::Result<()> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner("a2-ai")
        .repo_name("rv")
        .bin_name("rv")
        .show_download_progress(true)
        .current_version(cargo_crate_version!())
        .build()?
        .update()?;

    if status.uptodate() {
        println!("rv is already up to date");
        return Ok(());
    }

    println!("rv updated to {}", status.version());
    Ok(())
}
