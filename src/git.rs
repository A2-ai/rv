use core::fmt;
use std::path::Path;

use git2::Repository;

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
}

impl fmt::Display for GitReference<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.reference())
    }
}

impl<'g> GitReference<'g> {
    pub fn reference(&self) -> &'g str {
        match self {
            GitReference::Branch(b) => b,
            GitReference::Tag(b) => b,
            GitReference::Commit(b) => b,
        }
    }
}

fn get_sha(repo: &Repository) -> Result<String, git2::Error> {
    let head = repo.head()?;
    let sha = head
        .target()
        .ok_or_else(|| git2::Error::from_str("HEAD has no target"))?
        .to_string();
    Ok(sha)
}

fn checkout(repo: &Repository, git_ref: &str) -> Result<(), git2::Error> {
    let obj = repo.revparse_single(git_ref)?;
    repo.checkout_tree(&obj, None)?;
    repo.set_head_detached(obj.id())?;

    Ok(())
}

fn can_find_reference(repo: &Repository, git_ref: &str) -> bool {
    repo.revparse_single(git_ref).is_ok()
}

fn clone_repository(
    url: &str,
    git_ref: GitReference<'_>,
    destination: impl AsRef<Path>,
) -> Result<String, git2::Error> {
    let destination = destination.as_ref();

    // If the destination exists, open the repo and fetch instead but only if we can't find the ref
    let repo = if destination.exists() {
        let repo = Repository::open(destination)?;
        // Only fetch if we can't find the reference
        if !can_find_reference(&repo, git_ref.reference()) {
            let remote_name = repo
                .remotes()?
                .get(0)
                .ok_or_else(|| git2::Error::from_str("No remotes found"))?
                .to_string();
            let mut remote = repo.find_remote(&remote_name)?;
            remote.fetch(&["HEAD"], None, None)?;
        }

        repo
    } else {
        let mut builder = git2::build::RepoBuilder::new();

        if let GitReference::Branch(b) = git_ref {
            builder.branch(b);
        }

        let repo = builder.clone(url, destination)?;
        repo
    };

    // For commits/tags, we need to checkout the ref specifically
    // This will be a no-op for branches
    checkout(&repo, git_ref.reference())?;
    Ok(get_sha(&repo)?)
}

pub trait GitOperations {
    /// Clones a repository at the given url and checkouts the given ref
    /// Returns the sha associated with that reference.
    /// If the repository already exists on disk, only fetch from origin if we can't find the
    /// reference.
    fn clone(
        &self,
        url: &str,
        git_ref: GitReference<'_>,
        destination: impl AsRef<Path>,
    ) -> Result<String, git2::Error>;
}

pub struct Git;

impl GitOperations for Git {
    fn clone(
        &self,
        url: &str,
        git_ref: GitReference<'_>,
        destination: impl AsRef<Path>,
    ) -> Result<String, git2::Error> {
        clone_repository(url, git_ref, destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_clone() {
        let url = "https://github.com/A2-ai/scicalc";
        let tag = "v0.1.1";
        let commit = "8fd417a477f8e1df6e4dc7923eca55c9b758df58";
        let branch = "rd2md";

        let sha_found = clone_repository(url, GitReference::Branch(branch), "from_branch").unwrap();
        println!("Branch {sha_found:?}");
        let sha_found = clone_repository(url, GitReference::Tag(tag), "from_tag").unwrap();
        println!("Tag {sha_found:?}");
        let sha_found = clone_repository(url, GitReference::Commit(commit), "from_commit").unwrap();
        println!("Commit {sha_found:?}");
        assert!(false);
    }
}
