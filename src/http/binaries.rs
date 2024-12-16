use std::collections::VecDeque;

use reqwest::{blocking::get, Error};
use serde::Deserialize;
use os_info;


fn url(repo: &str) -> String {
    let base_url = get_base_url(repo);
    if base_url == repo { return repo.to_string() }; //return input if an api_url is not found/known
    println!("I made it past base_url!");
    let status = read_status_api(&base_url).expect("TODO: handle error");
    let os = std::env::consts::OS.to_string();
    if os != "linux" { return repo.to_string() }; //if not linux, then its macos/windows and no need for the work to redirect to the linux binaries
    println!("I made it past finding it being linux!");

    let repository = "cran"; //TODO: parse input repo to find repo (cran/bioc/etc)
    let date = repo.split("/").last().expect("TODO: last elem not found error"); //TODO: parse better
    let info = os_info::get();
    let distribution = info.os_type().to_string().to_lowercase();
    println!("{distribution}");
    let release = info.version().to_string();

    //release from os_info is not 0 padded on the minor release
    println!("{release}: {:#?}", status.distros.iter().filter(|x| x.distribution == distribution).map(|x| release.starts_with(x.release.as_str())).collect::<Vec<bool>>());

    if let Some(distro) = status.distros
        .iter()
        .find(|x| x.distribution == distribution && x.release.starts_with(release.as_str())) 
    {
        if !distro.binaries { return repo.to_string() }
        return format!("{}/{}/__linux__/{}/{}", base_url, repository, date, distro.binary_url);
    } else {
        println!("Could not match my stuff");
        return repo.to_string() //if can't find the distribution and release in the table, then input
    }
}

fn get_base_url(repo: &str) -> &str{
    //bigger TODO: actually parse repo to get the base

    if repo.contains("packagemanager.posit") {
        return "https://packagemanager.posit.co/";
    }
    if repo.contains("TODO: actual server url a2-ai-rv-server/<date>") {
        return "TODO: actual server url";
    }
    repo
}

fn read_status_api(base_url: &str) -> Result<Status, Error> {
    let api_url = format!("{base_url}/__api__/status");
    match get(api_url) {
        Ok(response) => {
            if response.status().is_success() {
                let content = response.text().expect("TODO: handle error");
                return Ok(Status::parse(&content))
            } else {
                // TODO: handle not successful error
                let content = response.text().expect("TODO: handle error");
                return Ok(Status::parse(&content))
            }
        }
        Err(e) => Err(e)
    }
}

impl Status {
    fn parse(content: &str) -> Status{
        let res: Status = serde_json::from_str(content)
            .expect("TODO: Failed to parse JSON");
        res
    }
}

#[derive(Debug, Deserialize)]
struct Status {
    version: String,
    build_date: String,
    metrics_enabled: bool,
    r_configured: bool,
    python_configured: bool,
    binaries_enabled: bool,
    display_ash: bool,
    custom_home: bool,
    custom_home_title: String,
    ga_id: String,
    distros: Vec<Distribution>,
}

#[derive(Debug, Deserialize)]
struct Distribution {
    name: String,
    os: String,
    #[serde(rename = "binaryDisplay")]
    binary_display: String,
    #[serde(rename = "binaryURL")]
    binary_url: String,
    display: String,
    distribution: String,
    release: String,
    build_distribution: String,
    #[serde(rename = "sysReqs")]
    sys_req: bool,
    binaries: bool,
    hidden: bool,
    official_rspm: bool,
}

mod tests {
    use super::*;

    #[test]
    fn can_parse_api_status() {
        let repo = "https://packagemanager.posit.co/cran/2024-12-04";
        println!("{}", url(repo));

        assert!(true);
    }
}