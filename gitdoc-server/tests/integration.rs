use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tempfile::TempDir;

fn default_exclusions() -> Vec<String> {
    vec![
        "node_modules/".into(),
        "target/".into(),
        ".git/".into(),
        "vendor/".into(),
        ".next/".into(),
        "dist/".into(),
        "build/".into(),
        "__pycache__/".into(),
    ]
}

/// Create a temporary git repo with test files.
fn create_test_repo() -> TempDir {
    let dir = tempfile::tempdir().unwrap();

    run_git(dir.path(), &["init", "-b", "main"]);
    run_git(dir.path(), &["config", "user.email", "test@test.com"]);
    run_git(dir.path(), &["config", "user.name", "Test"]);

    std::fs::write(dir.path().join("README.md"), "# Test Project\n\nSome docs here.\n").unwrap();
    std::fs::write(
        dir.path().join("main.rs"),
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
        dir.path().join("app.ts"),
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
    std::fs::write(dir.path().join("ignored.txt"), "this should be classified as other\n").unwrap();

    run_git(dir.path(), &["add", "."]);
    run_git(dir.path(), &["commit", "-m", "initial commit"]);

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

// --- Test database setup ---

use gitdoc_server::db::Database;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use testcontainers::ImageExt;

async fn get_test_db() -> (Option<ContainerAsync<Postgres>>, Arc<Database>) {
    if let Ok(url) = std::env::var("DATABASE_URL") {
        let db = Database::connect(&url).await.unwrap();
        (None, Arc::new(db))
    } else {
        let container = Postgres::default()
            .with_tag("17")
            .start()
            .await
            .unwrap();
        let host_port = container.get_host_port_ipv4(5432).await.unwrap();
        let url = format!("postgres://postgres:postgres@127.0.0.1:{host_port}/postgres");

        // Install pgvector extension - use a raw connection first
        let pool = sqlx::postgres::PgPool::connect(&url).await.unwrap();
        sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let db = Database::connect(&url).await.unwrap();
        (Some(container), Arc::new(db))
    }
}

// ---- git_walker tests ----

mod git_walker_tests {
    use super::*;
    use gitdoc_server::indexer::git_walker;

    #[test]
    fn resolve_head() {
        let repo = create_test_repo();
        let sha = git_walker::resolve_commit(repo.path(), "HEAD").unwrap();
        assert_eq!(sha.len(), 40);
    }

    #[test]
    fn resolve_branch() {
        let repo = create_test_repo();
        let sha = git_walker::resolve_commit(repo.path(), "main").unwrap();
        assert_eq!(sha.len(), 40);
    }

    #[test]
    fn resolve_invalid_ref_fails() {
        let repo = create_test_repo();
        let result = git_walker::resolve_commit(repo.path(), "nonexistent-ref-xyz");
        assert!(result.is_err());
    }

    #[test]
    fn walk_commit_returns_all_files() {
        let repo = create_test_repo();
        let sha = git_walker::resolve_commit(repo.path(), "HEAD").unwrap();
        let files = git_walker::walk_commit(repo.path(), &sha).unwrap();

        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"README.md"), "paths: {paths:?}");
        assert!(paths.contains(&"main.rs"), "paths: {paths:?}");
        assert!(paths.contains(&"app.ts"), "paths: {paths:?}");
        assert!(paths.contains(&"ignored.txt"), "paths: {paths:?}");

        for f in &files {
            assert!(!f.checksum.is_empty());
            assert!(!f.content.is_empty());
        }
    }

    #[test]
    fn checksums_are_deterministic() {
        let repo = create_test_repo();
        let sha = git_walker::resolve_commit(repo.path(), "HEAD").unwrap();
        let files1 = git_walker::walk_commit(repo.path(), &sha).unwrap();
        let files2 = git_walker::walk_commit(repo.path(), &sha).unwrap();

        for f1 in &files1 {
            let f2 = files2.iter().find(|f| f.path == f1.path).unwrap();
            assert_eq!(f1.checksum, f2.checksum, "checksum mismatch for {}", f1.path);
        }
    }
}

// ---- pipeline integration tests ----

mod pipeline_tests {
    use super::*;
    use gitdoc_server::search::SearchIndex;
    use gitdoc_server::indexer::pipeline;

    fn test_search() -> SearchIndex {
        SearchIndex::open_in_memory().unwrap()
    }

    #[tokio::test]
    async fn index_repo_end_to_end() {
        let repo = create_test_repo();
        let (_container, db) = get_test_db().await;
        let search = test_search();
        db.insert_repo("test", repo.path().to_str().unwrap(), "Test", None).await.unwrap();

        let result = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", Some("v1"), None, &default_exclusions()).await.unwrap();

        assert!(result.snapshot_id > 0);
        assert!(result.files_scanned >= 3, "files_scanned: {}", result.files_scanned);
        assert_eq!(result.docs_count, 1);
        assert!(result.symbols_count >= 3, "symbols_count: {}", result.symbols_count);
        assert!(result.symbols_count >= 6, "expected >=6 symbols, got {}", result.symbols_count);
        assert_eq!(result.embeddings_count, 0);

        let snaps = db.list_snapshots("test").await.unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].status, "ready");
        assert_eq!(snaps[0].label.as_deref(), Some("v1"));
    }

    #[tokio::test]
    async fn reindex_same_commit_returns_existing() {
        let repo = create_test_repo();
        let (_container, db) = get_test_db().await;
        let search = test_search();
        db.insert_repo("test", repo.path().to_str().unwrap(), "Test", None).await.unwrap();

        let r1 = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", None, None, &default_exclusions()).await.unwrap();
        let r2 = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", None, None, &default_exclusions()).await.unwrap();

        assert_eq!(r1.snapshot_id, r2.snapshot_id);
    }

    #[tokio::test]
    async fn file_deduplication_across_commits() {
        let repo = create_test_repo();
        let (_container, db) = get_test_db().await;
        let search = test_search();
        db.insert_repo("test", repo.path().to_str().unwrap(), "Test", None).await.unwrap();

        let r1 = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", Some("v1"), None, &default_exclusions()).await.unwrap();

        std::fs::write(repo.path().join("new.rs"), "pub fn added() {}\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "add new.rs"]);

        let r2 = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", Some("v2"), None, &default_exclusions()).await.unwrap();

        assert_ne!(r1.snapshot_id, r2.snapshot_id);
        assert!(r2.files_scanned >= r1.files_scanned);
        assert!(r2.symbols_count >= r1.symbols_count);
    }

    #[tokio::test]
    async fn nested_exclusion_patterns_are_skipped() {
        let repo = create_test_repo();
        let (_container, db) = get_test_db().await;
        let search = test_search();
        db.insert_repo("test", repo.path().to_str().unwrap(), "Test", None).await.unwrap();

        // Create nested node_modules (like in a monorepo)
        std::fs::create_dir_all(repo.path().join("packages/app/node_modules/pkg")).unwrap();
        std::fs::write(repo.path().join("packages/app/node_modules/pkg/index.js"), "function nested() {}").unwrap();
        // Also a root-level node_modules
        std::fs::create_dir_all(repo.path().join("node_modules/pkg")).unwrap();
        std::fs::write(repo.path().join("node_modules/pkg/index.js"), "function root() {}").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "add nested node_modules"]);

        let result = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", None, None, &default_exclusions()).await.unwrap();

        // Should only scan the 4 original files (README.md, main.rs, app.ts, ignored.txt)
        // Both root and nested node_modules should be excluded
        assert_eq!(result.files_scanned, 4, "nested node_modules should be excluded");
    }

    #[tokio::test]
    async fn excluded_prefixes_are_skipped() {
        let repo = create_test_repo();
        let (_container, db) = get_test_db().await;
        let search = test_search();
        db.insert_repo("test", repo.path().to_str().unwrap(), "Test", None).await.unwrap();

        std::fs::create_dir_all(repo.path().join("node_modules/pkg")).unwrap();
        std::fs::write(repo.path().join("node_modules/pkg/index.js"), "function x() {}").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "add node_modules"]);

        let result = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", None, None, &default_exclusions()).await.unwrap();

        assert_eq!(result.files_scanned, 4);
    }
}

// ---- search tests ----

mod search_tests {
    use super::*;
    use gitdoc_server::search::SearchIndex;
    use gitdoc_server::indexer::pipeline;

    fn test_search() -> SearchIndex {
        SearchIndex::open_in_memory().unwrap()
    }

    #[tokio::test]
    async fn search_after_indexation() {
        let repo = create_test_repo();
        let (_container, db) = get_test_db().await;
        let search = test_search();
        db.insert_repo("test", repo.path().to_str().unwrap(), "Test", None).await.unwrap();

        let result = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", Some("v1"), None, &default_exclusions()).await.unwrap();

        let file_ids = db.get_file_ids_for_snapshot(result.snapshot_id).await.unwrap();
        let doc_results = search.search_docs("Test Project", &file_ids, 10).unwrap();
        assert!(!doc_results.is_empty(), "should find README.md");
        assert_eq!(doc_results[0].file_path, "README.md");

        let sym_results = search.search_symbols("Config", &file_ids, None, None, 10).unwrap();
        assert!(!sym_results.is_empty(), "should find Config struct");
        assert!(sym_results.iter().any(|s| s.name == "Config"));

        let fn_results = search.search_symbols("main", &file_ids, Some("function"), None, 10).unwrap();
        assert!(!fn_results.is_empty(), "should find main function");
    }

    #[tokio::test]
    async fn search_respects_snapshot_isolation() {
        let repo = create_test_repo();
        let (_container, db) = get_test_db().await;
        let search = test_search();
        db.insert_repo("test", repo.path().to_str().unwrap(), "Test", None).await.unwrap();

        let r1 = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", Some("v1"), None, &default_exclusions()).await.unwrap();

        std::fs::write(repo.path().join("security.md"), "# Security\n\nSecurity guidelines for the project.\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "add security doc"]);

        let r2 = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", Some("v2"), None, &default_exclusions()).await.unwrap();

        let file_ids_v1 = db.get_file_ids_for_snapshot(r1.snapshot_id).await.unwrap();
        let results_v1 = search.search_docs("Security", &file_ids_v1, 10).unwrap();
        assert!(results_v1.is_empty(), "v1 should not contain security.md");

        let file_ids_v2 = db.get_file_ids_for_snapshot(r2.snapshot_id).await.unwrap();
        let results_v2 = search.search_docs("Security", &file_ids_v2, 10).unwrap();
        assert!(!results_v2.is_empty(), "v2 should contain security.md");
        assert_eq!(results_v2[0].file_path, "security.md");
    }
}

// ---- real repo tests ----

mod real_repo_tests {
    use super::*;
    use gitdoc_server::search::SearchIndex;
    use gitdoc_server::indexer::{git_walker, pipeline};

    fn test_search() -> SearchIndex {
        SearchIndex::open_in_memory().unwrap()
    }

    fn clone_repo(url: &str) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        let out = Command::new("git")
            .args(["clone", "--depth=1", url, dir.path().to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git clone failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        dir
    }

    #[tokio::test]
    async fn index_arcrun() {
        let repo = clone_repo("https://github.com/tinyest-org/ArcRun");
        let (_container, db) = get_test_db().await;
        let search = test_search();
        db.insert_repo("arcrun", repo.path().to_str().unwrap(), "ArcRun", None).await.unwrap();

        let sha = git_walker::resolve_commit(repo.path(), "HEAD").unwrap();
        assert_eq!(sha.len(), 40, "SHA should be 40 hex chars");

        let files = git_walker::walk_commit(repo.path(), &sha).unwrap();
        assert!(!files.is_empty(), "repo should have files");

        let has_ts = files.iter().any(|f| f.path.ends_with(".ts") || f.path.ends_with(".tsx"));
        let has_rs = files.iter().any(|f| f.path.ends_with(".rs"));
        let has_md = files.iter().any(|f| f.path.ends_with(".md"));
        eprintln!(
            "ArcRun files: total={}, ts={has_ts}, rs={has_rs}, md={has_md}",
            files.len()
        );

        let result = pipeline::run_indexation(&db, &search, "arcrun", repo.path(), "HEAD", Some("latest"), None, &default_exclusions()).await.unwrap();

        eprintln!(
            "ArcRun indexed: files_scanned={}, docs={}, symbols={}",
            result.files_scanned, result.docs_count, result.symbols_count
        );

        assert!(result.snapshot_id > 0);
        assert!(result.files_scanned > 0, "should scan at least some files");

        let snaps = db.list_snapshots("arcrun").await.unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].status, "ready");
        assert_eq!(snaps[0].label.as_deref(), Some("latest"));
        assert_eq!(snaps[0].commit_sha, sha);

        let r2 = pipeline::run_indexation(&db, &search, "arcrun", repo.path(), "HEAD", None, None, &default_exclusions()).await.unwrap();
        assert_eq!(r2.snapshot_id, result.snapshot_id);
    }
}

// ---- diff tests ----

mod diff_tests {
    use super::*;
    use gitdoc_server::db::SymbolFilters;
    use gitdoc_server::search::SearchIndex;
    use gitdoc_server::indexer::pipeline;

    fn test_search() -> SearchIndex {
        SearchIndex::open_in_memory().unwrap()
    }

    #[tokio::test]
    async fn diff_added_removed_modified() {
        let repo = create_test_repo();
        let (_container, db) = get_test_db().await;
        let search = test_search();
        db.insert_repo("test", repo.path().to_str().unwrap(), "Test", None).await.unwrap();

        // Index v1
        let r1 = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", Some("v1"), None, &default_exclusions()).await.unwrap();

        // Modify: change main's signature, remove helper, add new_func
        std::fs::write(
            repo.path().join("main.rs"),
            r#"/// The entry point
pub fn main(args: Vec<String>) {
    println!("hello");
}

pub struct Config {
    pub name: String,
    pub port: u16,
}

pub fn new_func() -> i32 {
    42
}
"#,
        )
        .unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "v2 changes"]);

        // Index v2
        let r2 = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", Some("v2"), None, &default_exclusions()).await.unwrap();

        // Now diff
        let filters = SymbolFilters {
            include_private: false,
            ..Default::default()
        };
        let from_symbols = db.list_symbols_for_snapshot(r1.snapshot_id, &filters).await.unwrap();
        let to_symbols = db.list_symbols_for_snapshot(r2.snapshot_id, &filters).await.unwrap();

        let from_names: Vec<&str> = from_symbols.iter().map(|s| s.qualified_name.as_str()).collect();
        let to_names: Vec<&str> = to_symbols.iter().map(|s| s.qualified_name.as_str()).collect();

        // new_func should be in v2 but not v1
        assert!(!from_names.contains(&"new_func"), "new_func should not be in v1");
        assert!(to_names.contains(&"new_func"), "new_func should be in v2");

        // helper should be in v1 but not v2 (it was private, so only check if include_private)
        // Since include_private is false, helper won't appear. Let's check main's signature changed.
        let from_main = from_symbols.iter().find(|s| s.qualified_name == "main");
        let to_main = to_symbols.iter().find(|s| s.qualified_name == "main");
        assert!(from_main.is_some(), "main should be in v1");
        assert!(to_main.is_some(), "main should be in v2");
        assert_ne!(
            from_main.unwrap().signature,
            to_main.unwrap().signature,
            "main signature should differ between v1 and v2"
        );
    }
}

// ---- embedding tests ----

mod embedding_tests {
    use super::*;
    use anyhow::Result;
    use gitdoc_server::embeddings::EmbeddingProvider;
    use gitdoc_server::search::SearchIndex;
    use gitdoc_server::indexer::pipeline;
    use std::future::Future;
    use std::pin::Pin;

    struct MockEmbedder {
        dims: usize,
    }

    impl MockEmbedder {
        fn new(dims: usize) -> Self {
            Self { dims }
        }
    }

    impl EmbeddingProvider for MockEmbedder {
        fn dimensions(&self) -> usize {
            self.dims
        }

        fn embed_batch(&self, texts: &[String]) -> Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>>> + Send + '_>> {
            let texts = texts.to_vec();
            let dims = self.dims;
            Box::pin(async move {
                Ok(texts
                    .iter()
                    .map(|text| {
                        let mut vec = vec![0.0f32; dims];
                        for (i, byte) in text.bytes().enumerate() {
                            vec[i % dims] += byte as f32 / 255.0;
                        }
                        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
                        if norm > 0.0 {
                            for v in &mut vec {
                                *v /= norm;
                            }
                        }
                        vec
                    })
                    .collect())
            })
        }
    }

    fn test_search() -> SearchIndex {
        SearchIndex::open_in_memory().unwrap()
    }

    #[tokio::test]
    async fn embeddings_generated_during_indexation() {
        let repo = create_test_repo();
        let (_container, db) = get_test_db().await;
        let search = test_search();
        let embedder = MockEmbedder::new(8);
        db.insert_repo("test", repo.path().to_str().unwrap(), "Test", None).await.unwrap();

        let result = pipeline::run_indexation(
            &db, &search, "test", repo.path(), "HEAD", Some("v1"), Some(Arc::new(embedder) as Arc<dyn EmbeddingProvider>), &default_exclusions(),
        ).await.unwrap();

        assert!(result.embeddings_count > 0, "should generate embeddings, got {}", result.embeddings_count);

        let file_ids = db.get_file_ids_for_snapshot(result.snapshot_id).await.unwrap();
        let all_embeddings = db.get_embeddings_for_file_ids(&file_ids).await.unwrap();

        let doc_chunks: Vec<_> = all_embeddings.iter().filter(|e| e.source_type == "doc_chunk").collect();
        let symbols: Vec<_> = all_embeddings.iter().filter(|e| e.source_type == "symbol").collect();

        assert!(!doc_chunks.is_empty(), "should have doc_chunk embeddings");
        assert!(!symbols.is_empty(), "should have symbol embeddings");

        for e in &all_embeddings {
            assert!(e.vector.is_some(), "embedding vector should not be None");
            let vec = e.vector.as_ref().unwrap();
            assert_eq!(vec.as_slice().len(), 8, "vector dimensions should match mock");
        }
    }

    #[tokio::test]
    async fn embeddings_skipped_when_no_provider() {
        let repo = create_test_repo();
        let (_container, db) = get_test_db().await;
        let search = test_search();
        db.insert_repo("test", repo.path().to_str().unwrap(), "Test", None).await.unwrap();

        let result = pipeline::run_indexation(
            &db, &search, "test", repo.path(), "HEAD", Some("v1"), None, &default_exclusions(),
        ).await.unwrap();

        assert_eq!(result.embeddings_count, 0);

        let file_ids = db.get_file_ids_for_snapshot(result.snapshot_id).await.unwrap();
        let all_embeddings = db.get_embeddings_for_file_ids(&file_ids).await.unwrap();
        assert!(all_embeddings.is_empty(), "no embeddings without provider");
    }

    #[tokio::test]
    async fn embeddings_deduplicated_across_commits() {
        let repo = create_test_repo();
        let (_container, db) = get_test_db().await;
        let search = test_search();
        let embedder: Arc<dyn EmbeddingProvider> = Arc::new(MockEmbedder::new(8));
        db.insert_repo("test", repo.path().to_str().unwrap(), "Test", None).await.unwrap();

        let r1 = pipeline::run_indexation(
            &db, &search, "test", repo.path(), "HEAD", Some("v1"), Some(Arc::clone(&embedder)), &default_exclusions(),
        ).await.unwrap();
        let count1 = r1.embeddings_count;
        assert!(count1 > 0);

        std::fs::write(repo.path().join("new.rs"), "/// A new function\npub fn added() {}\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "add new.rs"]);

        let r2 = pipeline::run_indexation(
            &db, &search, "test", repo.path(), "HEAD", Some("v2"), Some(Arc::clone(&embedder)), &default_exclusions(),
        ).await.unwrap();

        assert!(r2.embeddings_count > 0, "new file should get embeddings");
        assert!(r2.embeddings_count < count1, "should only embed the new file, not re-embed shared files: new={}, old={}", r2.embeddings_count, count1);
    }
}

// ---- gc tests ----

mod gc_tests {
    use super::*;
    use gitdoc_server::search::SearchIndex;
    use gitdoc_server::indexer::pipeline;

    fn test_search() -> SearchIndex {
        SearchIndex::open_in_memory().unwrap()
    }

    #[tokio::test]
    async fn delete_snapshot_and_gc() {
        let repo = create_test_repo();
        let (_container, db) = get_test_db().await;
        let search = test_search();
        db.insert_repo("test", repo.path().to_str().unwrap(), "Test", None).await.unwrap();

        // Index v1
        let r1 = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", Some("v1"), None, &default_exclusions()).await.unwrap();

        // Add a new file and index v2
        std::fs::write(repo.path().join("new.rs"), "pub fn added() {}\n").unwrap();
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "add new.rs"]);
        let r2 = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", Some("v2"), None, &default_exclusions()).await.unwrap();

        // Delete v1 snapshot
        let existed = db.delete_snapshot(r1.snapshot_id).await.unwrap();
        assert!(existed, "snapshot should have existed");

        // Run GC
        let gc_stats = db.gc_orphans().await.unwrap();

        // v2 should still work
        let v2_docs = db.list_docs_for_snapshot(r2.snapshot_id).await.unwrap();
        assert!(!v2_docs.is_empty(), "v2 docs should still exist");

        let v2_snap = db.get_snapshot(r2.snapshot_id).await.unwrap();
        assert!(v2_snap.is_some(), "v2 snapshot should still exist");

        // v1 snapshot should be gone
        let v1_snap = db.get_snapshot(r1.snapshot_id).await.unwrap();
        assert!(v1_snap.is_none(), "v1 snapshot should be deleted");

        // GC should have cleaned up some orphan data (the new.rs file is only in v2,
        // but shared files are still referenced by v2, so mostly files_removed should be 0
        // since all files are still referenced by v2)
        eprintln!("gc_stats: {:?}", gc_stats);
    }

    #[tokio::test]
    async fn delete_repo_cascades() {
        let repo = create_test_repo();
        let (_container, db) = get_test_db().await;
        let search = test_search();
        db.insert_repo("test", repo.path().to_str().unwrap(), "Test", None).await.unwrap();

        let r1 = pipeline::run_indexation(&db, &search, "test", repo.path(), "HEAD", Some("v1"), None, &default_exclusions()).await.unwrap();

        // Delete the entire repo
        let existed = db.delete_repo("test").await.unwrap();
        assert!(existed, "repo should have existed");

        // Snapshot should be gone (cascade)
        let snap = db.get_snapshot(r1.snapshot_id).await.unwrap();
        assert!(snap.is_none(), "snapshot should be deleted via cascade");

        // Repo should be gone
        let repo_row = db.get_repo("test").await.unwrap();
        assert!(repo_row.is_none(), "repo should be deleted");

        // GC should clean up orphaned data
        let gc_stats = db.gc_orphans().await.unwrap();
        assert!(gc_stats.files_removed > 0, "should clean up orphan files");
        assert!(gc_stats.symbols_removed > 0, "should clean up orphan symbols");
    }

    #[tokio::test]
    async fn delete_nonexistent_snapshot_returns_false() {
        let (_container, db) = get_test_db().await;
        let existed = db.delete_snapshot(999999).await.unwrap();
        assert!(!existed);
    }

    #[tokio::test]
    async fn gc_on_empty_db_is_noop() {
        let (_container, db) = get_test_db().await;
        let stats = db.gc_orphans().await.unwrap();
        assert_eq!(stats.files_removed, 0);
        assert_eq!(stats.docs_removed, 0);
        assert_eq!(stats.symbols_removed, 0);
        assert_eq!(stats.refs_removed, 0);
        assert_eq!(stats.embeddings_removed, 0);
    }
}
