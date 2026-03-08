use anyhow::{Result, bail};
use crate::client::GitdocClient;

/// Resolve a snapshot ID for a given repo.
///
/// - `reference` is None → latest snapshot (first in DESC-ordered list)
/// - `reference` matches a label → that snapshot
/// - `reference` matches a commit SHA prefix → that snapshot
pub async fn resolve_snapshot(
    client: &GitdocClient,
    repo_id: &str,
    reference: Option<&str>,
) -> Result<i64> {
    let detail = client.get_repo(repo_id).await?;
    let snapshots = detail.snapshots;

    if snapshots.is_empty() {
        bail!("No snapshot found for repo '{repo_id}'. You must call index_repo(repo_id: \"{repo_id}\") first to create a snapshot.");
    }

    match reference {
        None => Ok(snapshots[0].id),
        Some(r) => {
            // Try label match first
            if let Some(s) = snapshots.iter().find(|s| s.label.as_deref() == Some(r)) {
                return Ok(s.id);
            }
            // Try SHA prefix match
            if let Some(s) = snapshots.iter().find(|s| s.commit_sha.starts_with(r)) {
                return Ok(s.id);
            }
            bail!("no snapshot matching ref '{r}' in repo '{repo_id}'");
        }
    }
}
