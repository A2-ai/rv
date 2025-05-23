use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize, Serializer};
use std::cmp::Ordering;
use std::fmt;
use std::hash::Hash;
use std::str::FromStr;

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum Operator {
    Equal,
    Greater,
    Lower,
    GreaterOrEqual,
    LowerOrEqual,
}

impl fmt::Display for Operator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = match self {
            Self::Equal => "==",
            Self::Greater => ">",
            Self::Lower => "<",
            Self::GreaterOrEqual => ">=",
            Self::LowerOrEqual => "<=",
        };

        write!(f, "{}", c)
    }
}

impl FromStr for Operator {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "==" => Ok(Self::Equal),
            ">" => Ok(Self::Greater),
            "<" => Ok(Self::Lower),
            ">=" => Ok(Self::GreaterOrEqual),
            "<=" => Ok(Self::LowerOrEqual),
            _ => todo!("Handle error: {s}"),
        }
    }
}

#[derive(Debug, Default, Clone, Encode, Decode, Deserialize)]
pub struct Version {
    // TODO: pack versions in a u64 for faster comparison if needed
    // I don't think a package has more than 10 values in their version
    parts: [u32; 10],
    pub original: String,
}

impl Serialize for Version {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.original)
    }
}

impl Version {
    /// Returns the major/minor part of a version.
    /// Only meant to be used for R itself.
    // unlikely to be a problem but if hashing on the list is too slow but we can return a u64 instead
    // realistically R is going to be at 4.5 so we would be safe with a u8 or u16 even
    #[inline]
    pub fn major_minor(&self) -> [u32; 2] {
        [self.parts[0], self.parts[1]]
    }

    /// Determines if the called version matches in the input version based on the number of specified elements in the called version
    /// i.e. 4.4 = 4.4.1, but 4.4.2 != 4.4.1
    pub(crate) fn hazy_match(&self, version: &Version) -> bool {
        let num_specified = self.original.replace("-", ".").split('.').count();
        self.parts[..num_specified] == version.parts[..num_specified]
    }
}

impl FromStr for Version {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts: Vec<_> = s
            .trim()
            .replace('-', ".")
            .split('.')
            .map(|x| x.parse::<u32>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| format!("{s} cannot be parsed as a version"))?;

        parts.resize(10, 0);

        Ok(Self {
            parts: parts.try_into().unwrap(),
            original: s.to_string(),
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.original)
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}

impl Hash for Version {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.original.hash(state);
    }
}

impl Eq for Version {}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts.cmp(&other.parts)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn deserialize_version<'de, D>(deserializer: D) -> Result<Version, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: String = Deserialize::deserialize(deserializer)?;
    match Version::from_str(&v) {
        Ok(v) => Ok(v),
        Err(_) => Err(serde::de::Error::custom("Invalid version number")),
    }
}

/// A package can require specific version for some versions.
/// Most of the time it's using >= but there are also some
/// >, <, <= here and there and a couple of ==
#[derive(Debug, Hash, Eq, PartialEq, Clone, Encode, Decode, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct VersionRequirement {
    pub(crate) version: Version,
    op: Operator,
}

impl VersionRequirement {
    pub fn is_satisfied(&self, version: &Version) -> bool {
        match self.op {
            Operator::Equal => &self.version == version,
            Operator::Greater => version > &self.version,
            Operator::Lower => version < &self.version,
            Operator::GreaterOrEqual => version >= &self.version,
            Operator::LowerOrEqual => version <= &self.version,
        }
    }

    pub fn new(version: Version, op: Operator) -> Self {
        Self { version, op }
    }
}

impl FromStr for VersionRequirement {
    type Err = String;

    // s is for format `(>= 4.5)`
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut current = String::new();
        let mut version = None;
        let mut op = None;

        for c in s.trim().chars() {
            if c == '(' {
                continue;
            }
            if c == ' ' {
                // we should have the op in current
                // however formatting across lines can sometimes cause multiple whitespaces
                // after the op like "(>=   1.2.0)"
                // so if we hit more whitespace after setting the op we can just continue
                if op.is_none() {
                    op = Some(Operator::from_str(&current).expect("TODO"));
                    current = String::new();
                }
                continue;
            }
            if c == ')' {
                version = Some(Version::from_str(&current).expect("TODO"));
                continue;
            }
            current.push(c);
        }

        Ok(Self {
            version: version.unwrap(),
            op: op.unwrap(),
        })
    }
}

impl TryFrom<String> for VersionRequirement {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl fmt::Display for VersionRequirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({} {})", self.op, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_parse_pinning_strings() {
        let inputs = vec![
            "(> 1.0.0)",
            "(>= 1.0)",
            "(>=    1.0)", // extra whitespace
            "(== 1.7-7-1)",
            "(<= 2023.8.2.1)",
            "(< 1.0-10)",
            "(>= 1.98-1.16)",
        ];
        // Just making sure we don't panic on weird but existing versions
        for input in inputs {
            println!("{:?}", VersionRequirement::from_str(input));
        }
    }

    #[test]
    fn can_parse_cran_versions() {
        let inputs = vec![
            "1.0.0",
            "1.0",
            "1.7-7-1",
            "2023.8.2.1",
            "1.0-10",
            "0.0.0.9",
            "2024.11.29",
            "2019.10-1",
            "1.0.2.1000",
            "1.98-1.16",
            "1.0.5.2.1",
            "4041.111",
            "1.0.0-1.1.2",
            "3.7-0",
        ];
        // Just making sure we don't panic on weird but existing versions
        for input in inputs {
            println!("{:?}", Version::from_str(input).unwrap());
        }
    }

    #[test]
    fn can_parse_version_requirements() {
        assert_eq!(
            VersionRequirement::from_str("(== 1.0.0)")
                .unwrap()
                .to_string(),
            "(== 1.0.0)"
        );
    }

    #[test]
    fn can_compare_versions() {
        assert!(Version::from_str("1.0").unwrap() == Version::from_str("1.0.0").unwrap());
        assert!(Version::from_str("1.1").unwrap() > Version::from_str("1.0.0").unwrap());
    }

    #[test]
    fn can_get_minor_major() {
        assert_eq!(Version::from_str("1.0").unwrap().major_minor(), [1, 0]);
        assert_eq!(Version::from_str("1.0.0").unwrap().major_minor(), [1, 0]);
        assert_eq!(Version::from_str("4.5").unwrap().major_minor(), [4, 5]);
    }
}
