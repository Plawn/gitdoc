use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Clone a git repository from `url` into `dest`.
pub async fn clone_repo(url: &str, dest: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["clone", "--", url])
        .arg(dest)
        .output()
        .await
        .context("failed to spawn git clone")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git clone failed: {stderr}");
    }
    Ok(())
}

/// Fetch all remotes and reset to origin/HEAD.
pub async fn fetch_and_reset(repo_path: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["fetch", "--all", "--prune"])
        .current_dir(repo_path)
        .output()
        .await
        .context("failed to spawn git fetch")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git fetch failed: {stderr}");
    }

    let output = Command::new("git")
        .args(["reset", "--hard", "origin/HEAD"])
        .current_dir(repo_path)
        .output()
        .await
        .context("failed to spawn git reset")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git reset failed: {stderr}");
    }
    Ok(())
}

/// Compute the clone path for a repo: `{repos_dir}/{repo_id}`.
pub fn repo_clone_path(repos_dir: &Path, repo_id: &str) -> PathBuf {
    repos_dir.join(repo_id)
}
