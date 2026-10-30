use std::path::Path;

use anyhow::{bail, Context, Result};
use futures::future::BoxFuture;
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub repo: String,
    pub pr_number: u64,
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.repo, self.pr_number)
    }
}

pub const USAGE: &str = "Usage: /review #123 (in a GitHub checkout) or /review owner/repo#123";

/// `#123` and `123` name a PR of the GitHub repository checked out in `cwd`;
/// `owner/repo#123` names one anywhere.
pub async fn resolve(argument: &str, cwd: &Path) -> Result<Target> {
    let argument = argument.trim();
    let (repo, number) = match argument.rsplit_once('#') {
        Some((repo, number)) => (repo, number),
        None => ("", argument),
    };
    let pr_number = number
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .with_context(|| format!("`{argument}` is not a pull request. {USAGE}"))?;
    let repo = if repo.is_empty() {
        checkout_repo(cwd).await?
    } else {
        normalize_repo(repo).with_context(|| format!("`{repo}` is not owner/repo. {USAGE}"))?
    };
    Ok(Target { repo, pr_number })
}

fn normalize_repo(repo: &str) -> Option<String> {
    let repo = repo.trim().trim_end_matches(".git");
    let (owner, name) = repo.split_once('/')?;
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (valid(owner) && valid(name)).then(|| format!("{owner}/{name}"))
}

/// The `owner/repo` of a github.com remote URL in https, ssh, or scp-like form.
pub fn github_repo(remote_url: &str) -> Option<String> {
    let url = remote_url.trim();
    let path = if let Some(rest) = url.strip_prefix("git@github.com:") {
        rest
    } else {
        let without_scheme = url.split_once("://")?.1;
        let (authority, path) = without_scheme.split_once('/')?;
        let host = authority.rsplit('@').next()?;
        let host = host.split(':').next()?;
        if !host.eq_ignore_ascii_case("github.com") {
            return None;
        }
        path
    };
    normalize_repo(path.trim_end_matches('/'))
}

async fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .await
        .context("could not run git")?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// `origin` when it is on GitHub, else the only GitHub remote.
async fn checkout_repo(cwd: &Path) -> Result<String> {
    let outside = || format!("{} is not inside a git checkout. {USAGE}", cwd.display());
    git(cwd, &["rev-parse", "--git-dir"])
        .await
        .with_context(outside)?;
    let mut github = Vec::new();
    for remote in git(cwd, &["remote"]).await?.lines() {
        let url = git(cwd, &["remote", "get-url", remote]).await?;
        if let Some(repo) = github_repo(&url) {
            if remote == "origin" {
                return Ok(repo);
            }
            github.push(repo);
        }
    }
    github.dedup();
    match github.as_slice() {
        [repo] => Ok(repo.clone()),
        [] => bail!("This checkout has no GitHub remote. {USAGE}"),
        _ => bail!(
            "This checkout has several GitHub remotes ({}); name one. {USAGE}",
            github.join(", ")
        ),
    }
}

/// Reads a pull request's current head commit. Read-only: `/review` never writes to GitHub.
pub trait PrHeads: Send + Sync {
    fn head_sha<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Result<String>>;
}

/// Asks GitHub through the engineer's own `gh` login.
pub struct GhPrHeads;

impl PrHeads for GhPrHeads {
    fn head_sha<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let output = Command::new("gh")
                .args([
                    "api",
                    "--method",
                    "GET",
                    &format!("repos/{}/pulls/{}", target.repo, target.pr_number),
                    "--jq",
                    ".head.sha",
                ])
                .output()
                .await
                .context("could not run gh, which /review uses to read the PR head")?;
            if !output.status.success() {
                bail!(
                    "gh could not read {target}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            let sha = String::from_utf8_lossy(&output.stdout)
                .trim()
                .to_lowercase();
            if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                bail!("gh returned no head commit for {target}");
            }
            Ok(sha)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_remotes_in_every_form_name_their_repo() {
        for url in [
            "git@github.com:hickoryai/goose.git",
            "https://github.com/hickoryai/goose.git",
            "https://github.com/hickoryai/goose",
            "https://x-access-token:abc@github.com/hickoryai/goose.git",
            "ssh://git@github.com/hickoryai/goose.git",
            "ssh://git@github.com:22/hickoryai/goose/",
        ] {
            assert_eq!(
                github_repo(url).as_deref(),
                Some("hickoryai/goose"),
                "{url}"
            );
        }
        assert_eq!(github_repo("https://gitlab.com/hickoryai/goose.git"), None);
        assert_eq!(github_repo("/srv/git/goose.git"), None);
    }

    #[tokio::test]
    async fn an_explicit_repo_resolves_outside_a_checkout() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve("hickoryai/kory-source#42", dir.path())
                .await
                .unwrap(),
            Target {
                repo: "hickoryai/kory-source".into(),
                pr_number: 42
            }
        );
        let outside = resolve("#42", dir.path()).await.unwrap_err().to_string();
        assert!(outside.contains("not inside a git checkout"), "{outside}");
        for bad in ["", "#", "#0", "#abc", "nope#4", "a/b/c#4"] {
            assert!(resolve(bad, dir.path()).await.is_err(), "{bad}");
        }
    }
}
