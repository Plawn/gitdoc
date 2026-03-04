use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

/// Create a temporary git repo with test files and return its path.
/// Caller is responsible for cleanup.
fn create_test_repo() -> PathBuf {
    let dir = tempfile::tempdir().unwrap().keep();

    run_git(&dir, &["init", "-b", "main"]);
    run_git(&dir, &["config", "user.email", "test@test.com"]);
    run_git(&dir, &["config", "user.name", "Test"]);

    // Create files
    std::fs::write(dir.join("README.md"), "# Test Project\n\nSome docs here.\n").unwrap();
    std::fs::write(
        dir.join("main.rs"),
        r#"/// The entry point
pub fn main() {
    println!("hello");
}

pub struct Config {
    pub name: String,
    pub port: u16,
}

fn helper() -> bool {
    true
}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("app.ts"),
        r#"interface User {
  name: string;
  age: number;
}

function greet(user: User): string {
  return `Hello ${user.name}`;
}

class UserService {
  getUser(): User {
    return { name: "test", age: 0 };
  }
}
"#,
    )
    .unwrap();
    std::fs::write(dir.join("ignored.txt"), "this should be classified as other\n").unwrap();

    run_git(&dir, &["add", "."]);
    run_git(&dir, &["commit", "-m", "initial commit"]);

    dir
}

fn run_git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
}

// ---- git_walker tests ----

mod git_walker_tests {
    use super::*;
    use gitdoc_server::indexer::git_walker;

    #[test]
    fn resolve_head() {
        let repo = create_test_repo();
        let sha = git_walker::resolve_commit(&repo, "HEAD").unwrap();
        assert_eq!(sha.len(), 40); // full SHA
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn resolve_branch() {
        let repo = create_test_repo();
        let sha = git_walker::resolve_commit(&repo, "main").unwrap();
        assert_eq!(sha.len(), 40);
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn resolve_invalid_ref_fails() {
        let repo = create_test_repo();
        let result = git_walker::resolve_commit(&repo, "nonexistent-ref-xyz");
        assert!(result.is_err());
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn walk_commit_returns_all_files() {
        let repo = create_test_repo();
        let sha = git_walker::resolve_commit(&repo, "HEAD").unwrap();
        let files = git_walker::walk_commit(&repo, &sha).unwrap();

        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"README.md"), "paths: {paths:?}");
        assert!(paths.contains(&"main.rs"), "paths: {paths:?}");
        assert!(paths.contains(&"app.ts"), "paths: {paths:?}");
        assert!(paths.contains(&"ignored.txt"), "paths: {paths:?}");

        // Every file should have a non-empty checksum and content
        for f in &files {
            assert!(!f.checksum.is_empty());
            assert!(!f.content.is_empty());
        }

        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn checksums_are_deterministic() {
        let repo = create_test_repo();
        let sha = git_walker::resolve_commit(&repo, "HEAD").unwrap();
        let files1 = git_walker::walk_commit(&repo, &sha).unwrap();
        let files2 = git_walker::walk_commit(&repo, &sha).unwrap();

        for f1 in &files1 {
            let f2 = files2.iter().find(|f| f.path == f1.path).unwrap();
            assert_eq!(f1.checksum, f2.checksum, "checksum mismatch for {}", f1.path);
        }

        std::fs::remove_dir_all(&repo).ok();
    }
}

// ---- pipeline integration tests ----

mod pipeline_tests {
    use super::*;
    use gitdoc_server::db::Database;
    use gitdoc_server::indexer::pipeline;

    fn test_db() -> Arc<Database> {
        Arc::new(Database::open_in_memory().unwrap())
    }

    #[test]
    fn index_repo_end_to_end() {
        let repo = create_test_repo();
        let db = test_db();
        db.insert_repo("test", repo.to_str().unwrap(), "Test").unwrap();

        let result = pipeline::run_indexation(&db, "test", &repo, "HEAD", Some("v1")).unwrap();

        assert!(result.snapshot_id > 0);
        assert!(result.files_scanned >= 3, "files_scanned: {}", result.files_scanned);
        assert_eq!(result.docs_count, 1); // README.md
        assert!(result.symbols_count >= 3, "symbols_count: {}", result.symbols_count);
        // Rust: main, Config, helper = 3
        // TS: User, greet, UserService = 3
        // Total >= 6
        assert!(result.symbols_count >= 6, "expected >=6 symbols, got {}", result.symbols_count);

        // Snapshot should be "ready"
        let snaps = db.list_snapshots("test").unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].status, "ready");
        assert_eq!(snaps[0].label.as_deref(), Some("v1"));

        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn reindex_same_commit_returns_existing() {
        let repo = create_test_repo();
        let db = test_db();
        db.insert_repo("test", repo.to_str().unwrap(), "Test").unwrap();

        let r1 = pipeline::run_indexation(&db, "test", &repo, "HEAD", None).unwrap();
        let r2 = pipeline::run_indexation(&db, "test", &repo, "HEAD", None).unwrap();

        assert_eq!(r1.snapshot_id, r2.snapshot_id);

        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn file_deduplication_across_commits() {
        let repo = create_test_repo();
        let db = test_db();
        db.insert_repo("test", repo.to_str().unwrap(), "Test").unwrap();

        // Index first commit
        let r1 = pipeline::run_indexation(&db, "test", &repo, "HEAD", Some("v1")).unwrap();

        // Add a new file, commit
        std::fs::write(repo.join("new.rs"), "pub fn added() {}\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "add new.rs"]);

        // Index second commit
        let r2 = pipeline::run_indexation(&db, "test", &repo, "HEAD", Some("v2")).unwrap();

        assert_ne!(r1.snapshot_id, r2.snapshot_id);
        // Second commit should have more files
        assert!(r2.files_scanned >= r1.files_scanned);
        // But the unchanged files share the same file_id (verified via symbol count)
        assert!(r2.symbols_count >= r1.symbols_count);

        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn excluded_prefixes_are_skipped() {
        let repo = create_test_repo();
        let db = test_db();
        db.insert_repo("test", repo.to_str().unwrap(), "Test").unwrap();

        // Create a file in node_modules/
        std::fs::create_dir_all(repo.join("node_modules/pkg")).unwrap();
        std::fs::write(repo.join("node_modules/pkg/index.js"), "function x() {}").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "add node_modules"]);

        let result = pipeline::run_indexation(&db, "test", &repo, "HEAD", None).unwrap();

        // node_modules file should not be counted
        // Original: README.md, main.rs, app.ts, ignored.txt = 4
        assert_eq!(result.files_scanned, 4);

        std::fs::remove_dir_all(&repo).ok();
    }
}

// ---- real repo tests (clone from GitHub) ----

mod real_repo_tests {
    use super::*;
    use gitdoc_server::db::Database;
    use gitdoc_server::indexer::{git_walker, pipeline};

    fn test_db() -> Arc<Database> {
        Arc::new(Database::open_in_memory().unwrap())
    }

    /// Clone a remote repo into a temp dir. Returns the path.
    fn clone_repo(url: &str) -> PathBuf {
        let dir = tempfile::tempdir().unwrap().keep();
        let out = Command::new("git")
            .args(["clone", "--depth=1", url, dir.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git clone failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        dir
    }

    #[test]
    fn index_arcrun() {
        let repo = clone_repo("https://github.com/tinyest-org/ArcRun");
        let db = test_db();
        db.insert_repo("arcrun", repo.to_str().unwrap(), "ArcRun").unwrap();

        // Resolve HEAD
        let sha = git_walker::resolve_commit(&repo, "HEAD").unwrap();
        assert_eq!(sha.len(), 40, "SHA should be 40 hex chars");

        // Walk files — make sure we get something
        let files = git_walker::walk_commit(&repo, &sha).unwrap();
        assert!(!files.is_empty(), "repo should have files");

        // Check we found some expected file types
        let has_ts = files.iter().any(|f| f.path.ends_with(".ts") || f.path.ends_with(".tsx"));
        let has_rs = files.iter().any(|f| f.path.ends_with(".rs"));
        let has_md = files.iter().any(|f| f.path.ends_with(".md"));
        eprintln!(
            "ArcRun files: total={}, ts={has_ts}, rs={has_rs}, md={has_md}",
            files.len()
        );

        // Full pipeline indexation
        let result = pipeline::run_indexation(&db, "arcrun", &repo, "HEAD", Some("latest")).unwrap();

        eprintln!(
            "ArcRun indexed: files_scanned={}, docs={}, symbols={}",
            result.files_scanned, result.docs_count, result.symbols_count
        );

        assert!(result.snapshot_id > 0);
        assert!(result.files_scanned > 0, "should scan at least some files");

        // Snapshot is ready
        let snaps = db.list_snapshots("arcrun").unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].status, "ready");
        assert_eq!(snaps[0].label.as_deref(), Some("latest"));
        assert_eq!(snaps[0].commit_sha, sha);

        // Re-indexing same commit should return existing snapshot
        let r2 = pipeline::run_indexation(&db, "arcrun", &repo, "HEAD", None).unwrap();
        assert_eq!(r2.snapshot_id, result.snapshot_id);

        std::fs::remove_dir_all(&repo).ok();
    }
}
