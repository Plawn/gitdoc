use anyhow::{Context, Result};
use gix::bstr::ByteSlice;
use sha2::{Digest, Sha256};
use std::path::Path;

pub struct FileEntry {
    pub path: String,
    pub checksum: String,
    pub content: Vec<u8>,
}

/// Resolve a ref string (HEAD, branch, tag, SHA) to a full commit SHA.
pub fn resolve_commit(repo_path: &Path, ref_str: &str) -> Result<String> {
    let repo = gix::open(repo_path).context("failed to open git repo")?;
    let object = repo
        .rev_parse_single(ref_str.as_bytes())
        .context(format!("failed to resolve ref '{ref_str}'"))?;
    let commit = object
        .object()
        .context("failed to peel to object")?
        .try_into_commit()
        .map_err(|_| anyhow::anyhow!("ref '{ref_str}' does not point to a commit"))?;
    Ok(commit.id.to_string())
}

/// Walk all files at a given commit SHA and return their contents + checksums.
pub fn walk_commit(repo_path: &Path, commit_sha: &str) -> Result<Vec<FileEntry>> {
    let repo = gix::open(repo_path).context("failed to open git repo")?;
    let oid = repo
        .rev_parse_single(commit_sha.as_bytes())
        .context("failed to parse commit SHA")?;
    let commit = oid
        .object()
        .context("failed to peel to object")?
        .try_into_commit()
        .map_err(|_| anyhow::anyhow!("not a commit"))?;
    let tree = commit.tree().context("failed to get tree from commit")?;

    let mut entries = Vec::new();
    walk_tree_recursive(&repo, &tree, String::new(), &mut entries)?;
    Ok(entries)
}

fn walk_tree_recursive(
    repo: &gix::Repository,
    tree: &gix::Tree<'_>,
    prefix: String,
    entries: &mut Vec<FileEntry>,
) -> Result<()> {
    for entry_ref in tree.iter() {
        let entry = entry_ref?;
        let name = entry.filename().to_str_lossy().to_string();
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };

        let object = entry.object()?;
        match object.kind {
            gix::object::Kind::Blob => {
                let data = object.data.clone();
                let checksum = hex::encode(Sha256::digest(&data));
                entries.push(FileEntry {
                    path,
                    checksum,
                    content: data.to_vec(),
                });
            }
            gix::object::Kind::Tree => {
                let subtree = object.try_into_tree()?;
                walk_tree_recursive(repo, &subtree, path, entries)?;
            }
            _ => {}
        }
    }
    Ok(())
}
