//! A review's worktree starts at the pull request's head commit, the one
//! its prompt names, so the agent reads and runs the code under review
//! rather than the main checkout's `HEAD`. The remote's
//! `refs/pull/<number>/head` (which also holds a fork's head) must still be
//! that commit, the commit is fetched from it when the repository lacks it,
//! and the new checkout is checked to be at it, since Herdr checks out a
//! branch that already exists as it is.

use super::{Host, Result, Site, script};
use std::{sync::atomic::AtomicBool, time::Duration};

/// How long a fetch may go without output: a large pull request can be
/// quiet for a while, as Teleport's fetches are.
const FETCH_IDLE: Duration = Duration::from_secs(300);
const READING: &str = "Reading the pull request's head";
const FETCHING: &str = "Fetching the pull request";
const CHECKING: &str = "Checking the review's checkout";

/// The pull request whose head a review's worktree starts at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullHead {
    pub number: u64,
    /// The head commit the review prompt names.
    pub sha: String,
    /// The repository's remote the pull request was listed from.
    pub remote: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Pull request #{number}'s head {sha:?} is not a commit ID")]
    Sha { number: u64, sha: String },
    #[error("Remote {0:?} is not one Git can fetch from")]
    Remote(String),
    #[error("Pull request #{number} changed since it was read; refresh it and review it again")]
    Moved { number: u64 },
    #[error(
        "Branch {branch} is not at pull request #{number}'s head; give the review a branch name not in use"
    )]
    Branch { branch: String, number: u64 },
}

impl PullHead {
    fn check(&self) -> Result<(), Error> {
        let hex = self.sha.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !(hex && matches!(self.sha.len(), 40 | 64)) {
            return Err(Error::Sha {
                number: self.number,
                sha: self.sha.clone(),
            });
        }
        if self.remote.is_empty()
            || self.remote.starts_with('-')
            || self.remote.chars().any(char::is_control)
        {
            return Err(Error::Remote(self.remote.clone()));
        }
        Ok(())
    }

    /// Makes sure the pull request's head ref on the remote is still the
    /// head commit, and that the repository at `site.main_root` has it,
    /// fetching the ref when it does not. The fetch writes only
    /// `FETCH_HEAD`; the review's branch keeps the commit.
    pub(super) fn fetch(&self, site: &Site, cancelled: &AtomicBool) -> Result<()> {
        self.check()?;
        let root = site.main_root.as_str();
        let reference = format!("refs/pull/{}/head", self.number);
        // Whether the repository has the commit says nothing of whether it is
        // still the head: a push keeps the old head as the new one's parent.
        let listed = site
            .host
            .exec(
                "git",
                &["-C", root, "ls-remote", "--", &self.remote, &reference],
                Host::QUERY_IDLE,
                cancelled,
            )
            .map_err(script(READING))?;
        // A pattern matches any ref ending in it, so the name must be whole.
        let current = String::from_utf8_lossy(&listed).lines().find_map(|line| {
            line.split_once('\t')
                .filter(|(_, name)| *name == reference)
                .map(|(sha, _)| sha.to_owned())
        });
        if !current.is_some_and(|sha| sha.eq_ignore_ascii_case(&self.sha)) {
            return Err(Error::Moved {
                number: self.number,
            }
            .into());
        }
        let commit = format!("{}^{{commit}}", self.sha);
        let has = || {
            site.host
                .exec(
                    "git",
                    &["-C", root, "cat-file", "-e", &commit],
                    Host::QUERY_IDLE,
                    cancelled,
                )
                .is_ok()
        };
        if has() {
            return Ok(());
        }
        site.host
            .exec(
                "git",
                &[
                    "-C",
                    root,
                    "fetch",
                    "--quiet",
                    "--no-tags",
                    "--",
                    &self.remote,
                    &reference,
                ],
                FETCH_IDLE,
                cancelled,
            )
            .map_err(script(FETCHING))?;
        // The ref moved past the commit the prompt names since it was read.
        if !has() {
            return Err(Error::Moved {
                number: self.number,
            }
            .into());
        }
        Ok(())
    }

    /// Refuses a new checkout of `branch` at `path` on `host` unless it is at
    /// the head: an existing branch is checked out where it is, not at the
    /// requested base.
    pub(super) fn verify(
        &self,
        host: &Host,
        path: &str,
        branch: &str,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        let output = host
            .exec(
                "git",
                &["-C", path, "rev-parse", "--verify", "HEAD^{commit}"],
                Host::QUERY_IDLE,
                cancelled,
            )
            .map_err(script(CHECKING))?;
        if !String::from_utf8_lossy(&output)
            .trim()
            .eq_ignore_ascii_case(&self.sha)
        {
            return Err(Error::Branch {
                branch: branch.to_owned(),
                number: self.number,
            }
            .into());
        }
        Ok(())
    }
}
