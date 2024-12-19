use std::{fs::{self, DirEntry}, str::FromStr, u32};
use regex::Regex;

#[derive(Debug, Clone)]
struct RVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl PartialEq for RVersion {
    fn eq(&self, other: &Self) -> bool {
        self.major == other.major &&
        self.minor == other.minor &&
        self.patch == other.patch
    }
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
    fn get_r_version(r_version: Option<&str>, avail_r: Vec<Self>) -> Self {
        if let Some(ver) = r_version {
            let ver = ver.parse::<RVersion>().expect("TODO: handle specified ver cannot be parsed");
            return match ver.patch {
                u32::MAX => { RMetadata::find_closest_match(ver, avail_r) },
                _ => { RMetadata::find_exact_match(ver, avail_r) },
            }
        }
        return RMetadata::find_latest_version(avail_r);
    }

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

    fn find_closest_match(ver: RVersion, avail_r: Vec<Self>) -> Self {
        let mut candidates = Vec::new();
        let mut patch = 0;
        for v in avail_r {
            if v.version.major == ver.major && 
                v.version.minor == ver.minor && 
                v.version.patch > patch {
                    patch = v.version.patch;
                    candidates.push(v);
            }
        }

        if let Some(close_version) = candidates.last() {
            return close_version.clone();
        }

        panic!("TODO: handle no R version matches");
    }

    fn find_exact_match(ver: RVersion, avail_r: Vec<Self>) -> Self {
        avail_r
            .iter()
            .filter(|x| x.version == ver)
            .map(|x| x.clone())
            .collect::<Vec<Self>>()
            .first()
            .expect("TODO: handle no exact match found")
            .clone()
    }

    fn find_latest_version(avail_ver: Vec<Self>) -> Self {
        if avail_ver.len() == 0 { panic!("TODO: handle no R Version found"); }
        avail_ver
            .iter()
            .max_by(|a, b| {
                a.version.major
                    .cmp(&b.version.major)
                    .then_with(|| a.version.minor.cmp(&b.version.minor))
                    .then_with(|| a.version.patch.cmp(&b.version.patch))
            })
            .unwrap()
            .clone()
    }
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
                version: RVersion{major: 4, minor: 4, patch: 2},
                str: String::from("4.4.2"),
                path: String::from("/opt/R/4.4.2")
            },
            RMetadata {
                version: RVersion{major: 4, minor: 4, patch: 3},
                str: String::from("4.4.3"),
                path: String::from("/opt/R/4.4.3")
            },
            RMetadata {
                version: RVersion{major: 3, minor: 6, patch: 1},
                str: String::from("3.4.1"),
                path: String::from("/opt/R/3.4.1")
            },
        ]
    }

    #[test]
    fn can_match_ver() {
        let res = RMetadata::get_r_version(Some("4.4.1"), r_metadata());
        assert_eq!(res.path, "/opt/R/4.4.1".to_string());
    }

    #[test]
    fn can_match_major_minor() {
        let res = RMetadata::get_r_version(Some("4.4"), r_metadata());
        assert_eq!(res.path, "/opt/R/4.4.3".to_string());
    }

    #[test]
    fn can_find_latest_ver() {
        RMetadata::get_r_version(None, r_metadata());
    }

    #[test]
    #[should_panic(expected = "TODO: handle no exact match found")]
    fn can_not_find_dne_patch() {
        RMetadata::get_r_version(Some("4.4.4"), r_metadata());
    }

    #[test]
    #[should_panic(expected = "TODO: handle no R version matches")]
    fn can_not_find_ver() {
        RMetadata::get_r_version(Some("3.5"), r_metadata());
    }
}
