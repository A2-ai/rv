use std::{fs::{self, DirEntry}, str::FromStr};
use regex::Regex;

#[derive(Debug, Clone)]
struct RVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl FromStr for RVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(".").collect();
        match parts.len() {
            3 => {
                Ok(Self {
                    major: parts[0].parse::<u32>().unwrap(),
                    minor: parts[1].parse::<u32>().unwrap(),
                    patch: parts[2].parse::<u32>().unwrap(),
                })
            },
            2 => {
                Ok(Self {
                    major: parts[0].parse::<u32>().unwrap(),
                    minor: parts[1].parse::<u32>().unwrap(),
                    patch: u32::MAX, // if only two args, set to max so no patch version matches
                })
            },
            _ => { Err(format!("Invalid version string: {}", s)) }
        }
    }
}

impl RVersion {
    fn read_entry(dir: &DirEntry) -> Option<(String, Self)> {
        let file_name = dir.file_name().to_string_lossy().into_owned();
        let pattern = Regex::new(r"(\d+)\.(\d+)\.(\d+)").unwrap();
        if let Some(ver) = pattern.captures(&file_name) {
            let vers = ver.get(0).unwrap().as_str();
            return Some((file_name.clone(), vers.parse::<RVersion>().unwrap()))
        }
        None
    }
}

#[derive(Debug, Clone)]
struct RMetadata {
    version: RVersion,
    str: String,
    path: String, 
}

impl RMetadata {
    fn available_r_vers() -> Vec<RMetadata> {
        let mut r_version = Vec::new();
        let root_dir = "/opt/R";
        let content = fs::read_dir(root_dir).expect("TODO: handle error");
        for c in content {
            let c = c.expect("TODO: handle error");
            if let Some((str, version)) = RVersion::read_entry(&c) {
                r_version.push(RMetadata {
                    version,
                    str, 
                    path: c.path().as_os_str().to_string_lossy().into_owned()
                });
            }
        }
        r_version
    }

    fn find_closest_match(ver: RVersion, avail_ver: Vec<Self>) -> Self {
        let mut candidates = Vec::new();

        for v in avail_ver {
            if v.version.major == ver.major && v.version.minor == ver.minor {
                if v.version.patch == v.version.patch { return v; }
                candidates.push(v);
            }
        }

        if candidates.len() == 0 { panic!("TODO: handle no R version matches"); }

        candidates.sort_by(|a, b| a.version.patch.cmp(&b.version.patch));
        if let Some(latest_patch) = candidates.last() {
            return latest_patch.clone();
        } else {
            panic!("TODO: handle no R version matches");
        }

    }

    fn find_latest_version(mut avail_ver: Vec<Self>) -> Self{
        if avail_ver.len() == 0 { panic!("TODO: handle no R Version found"); }
        avail_ver.sort_by(|a, b| {
            if a.version.major == b.version.major {
                if a.version.minor == b.version.minor {
                    a.version.patch.cmp(&b.version.patch)
                } else {
                    a.version.minor.cmp(&b.version.minor)
                }
            } else {
                a.version.major.cmp(&b.version.major)
            }
        });

        if let Some(latest_version) = avail_ver.last() {
            return latest_version.clone();
        } else {
            panic!("TODO: handle no R Version found");
        }
    }
}

fn get_r_version(r_version: Option<&str>, avail_r: Vec<RMetadata>) -> RMetadata {
    if let Some(ver) = r_version {
        let ver = ver.parse::<RVersion>().expect("TODO: handle specified ver cannot be parsed");
        return RMetadata::find_closest_match(ver, avail_r);
    }
    return RMetadata::find_latest_version(avail_r);
}

mod tests {
    use super::*;

    fn r_metadata() -> Vec<RMetadata> {
        vec![
            RMetadata {
                version: RVersion{major: 4, minor: 4, patch: 1},
                str: String::from("4.4.1"),
                path: String::from("/opt/R/4.4.1")
            },
            RMetadata {
                version: RVersion{major: 4, minor: 3, patch: 2},
                str: String::from("4.3.1"),
                path: String::from("/opt/R/4.3.2")
            },
            RMetadata {
                version: RVersion{major: 4, minor: 2, patch: 2},
                str: String::from("4.4.1"),
                path: String::from("/opt/R/4.2.2")
            },
        ]
    }

    #[test]
    fn can_match_ver() {
        get_r_version(Some("4.4.1"), r_metadata());
    }

    #[test]
    fn can_match_major_minor() {
        get_r_version(Some("4.2"), r_metadata());
    }

    #[test]
    fn can_hazy_match_ver() {
        get_r_version(Some("4.3.8"), r_metadata());
    }

    #[test]
    #[should_panic(expected = "TODO: handle no R version matches")]
    fn can_not_find_ver() {
        get_r_version(Some("3.5.0"), r_metadata());
    }

    #[test]
    #[should_panic(expected = "TODO: handle no R version matches")]
    fn can_not_find_major_minor() {
        get_r_version(Some("3.5"), r_metadata());
    }

    #[test]
    fn can_find_latest_ver() {
        get_r_version(None, r_metadata());
    }
}
