use std::fmt;

/// What a git URL can point to
/// If it's coming from a lockfile, it will always be a commit
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitReference<'g> {
    /// A specific branch
    Branch(&'g str),
    /// A specific tag.
    Tag(&'g str),
    /// The commit hash
    Commit(&'g str),
    /// We don't know what it is.
    /// Used for Remotes
    Unknown(&'g str),
}

impl<'g> GitReference<'g> {
    pub fn reference(&self) -> &'g str {
        match self {
            GitReference::Branch(b) => b,
            GitReference::Tag(b) => b,
            GitReference::Commit(b) => b,
            GitReference::Unknown(b) => b,
        }
    }

    /// We return multiple possible refspec because for package remotes we don't actually know what it
    /// so we will try everything
    pub fn as_refspecs(&self) -> Vec<String> {
        match self {
            GitReference::Branch(branch) => {
                vec![format!("+refs/heads/{branch}:refs/remotes/origin/{branch}")]
            }
            GitReference::Tag(tag) => {
                vec![format!("+refs/tags/{tag}:refs/remotes/origin/tags/{tag}")]
            }
            GitReference::Commit(rev) => vec![format!("+{rev}:refs/commit/{rev}")],
            GitReference::Unknown(_) => {
                // We don't know, just fetch everything
                vec![
                    String::from("+refs/heads/*:refs/remotes/origin/*"),
                    String::from("+HEAD:refs/remotes/origin/HEAD"),
                ]
            }
        }
    }
}

impl fmt::Display for GitReference<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.reference())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Oid(String);

impl Oid {
    pub fn new(s: String) -> Oid {
        Oid(s)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
