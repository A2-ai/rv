use core::fmt;
use std::path::Path;

use git2::{AutotagOption, FetchOptions, RemoteUpdateFlags, Repository};

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
            GitReference::Unknown(b) => b,
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

fn get_fetch_options() -> FetchOptions<'static> {
    // TODO: can we use a progress bar there with remote callbacks?
    let mut fetch_opts = FetchOptions::new();
    fetch_opts.download_tags(AutotagOption::All);
    fetch_opts
}

fn git_fetch(repo: &Repository) -> Result<(), git2::Error> {
    let mut remote = repo
        .find_remote("origin")
        .or_else(|_| repo.remote_anonymous("origin"))?;

    remote.download(
        &["refs/heads/*:refs/heads/*"],
        Some(&mut get_fetch_options()),
    )?;

    remote.disconnect()?;
    remote.update_tips(
        None,
        RemoteUpdateFlags::UPDATE_FETCHHEAD,
        AutotagOption::Unspecified,
        None,
    )?;

    Ok(())
}

fn clone_repository(
    url: &str,
    git_ref: Option<GitReference<'_>>,
    destination: impl AsRef<Path>,
) -> Result<String, git2::Error> {
    let destination = destination.as_ref();

    // If the destination exists, open the repo and fetch instead but only if we can't find the ref
    let repo = if destination.exists() {
        log::debug!("Repo {url} found in cache.");

        Repository::open(destination)?
    } else {
        log::debug!("Repo {url} not found in cache. Cloning.");
        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(get_fetch_options());

        if let Some(GitReference::Branch(b)) = &git_ref {
            builder.branch(b);
        }

        builder.clone(url, destination)?
    };

    // Only fetch if we can't find the reference
    if let Some(reference) = &git_ref {
        if !can_find_reference(&repo, reference.reference()) {
            log::debug!("Reference {reference:?} not fond in cache for repo {url}, fetching.");
            git_fetch(&repo)?;
        }
    }

    // For commits/tags, we need to checkout the ref specifically
    // This will be a no-op for branches
    if let Some(reference) = &git_ref {
        checkout(&repo, reference.reference())?;
    }

    get_sha(&repo)
}

pub trait GitOperations {
    /// Clones a repository at the given url and checkouts the given ref
    /// Returns the sha associated with that reference.
    /// If the repository already exists on disk, only fetch from origin if we can't find the
    /// reference.
    fn clone_and_checkout(
        &self,
        url: &str,
        git_ref: Option<GitReference<'_>>,
        destination: impl AsRef<Path>,
    ) -> Result<String, git2::Error>;
}

pub struct Git;

impl GitOperations for Git {
    fn clone_and_checkout(
        &self,
        url: &str,
        git_ref: Option<GitReference<'_>>,
        destination: impl AsRef<Path>,
    ) -> Result<String, git2::Error> {
        clone_repository(url, git_ref, destination)
    }
}
