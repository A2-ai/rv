use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum GitUrl {
    Http(Url),
    Ssh(String),
}

impl TryFrom<&str> for GitUrl {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        let s2 = s.trim();
        if s2.is_empty() {
            return Err("URL for git cannot be empty".to_string());
        }

        // Should we support user@ syntax?
        if s2.starts_with("git@") || s2.starts_with("ssh@") {
            return Ok(GitUrl::Ssh(s.to_string()));
        }

        // Try to parse as a standard URL
        if (s2.starts_with("http://") || s2.starts_with("https://"))
            && let Ok(url) = Url::parse(s2)
        {
            return Ok(GitUrl::Http(url));
        }

        Err(format!("Invalid URL format for git: {s}"))
    }
}

impl<'de> Deserialize<'de> for GitUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match Self::try_from(s.as_str()) {
            Ok(url) => Ok(url),
            Err(e) => Err(serde::de::Error::custom(e)),
        }
    }
}

impl GitUrl {
    pub fn url(&self) -> &str {
        match self {
            Self::Http(url) => url.as_str(),
            Self::Ssh(url) => url.as_str(),
        }
    }
}

impl fmt::Display for GitUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.url())
    }
}

impl fmt::Debug for GitUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{}\"", self.url())
    }
}
