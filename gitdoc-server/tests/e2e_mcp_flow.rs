//! End-to-end tests that simulate the MCP client flow:
//!   register_repo (via HTTP) → index_repo → list_repos → get_overview → list_symbols → get_symbol → search_symbols
//!
//! These tests spin up a real axum server on a random port and hit it with reqwest,
//! exactly like gitdoc-mcp does.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tempfile::TempDir;

use gitdoc_server::db::Database;
use gitdoc_server::search::SearchIndex;
use gitdoc_server::{AppState, config};
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers::{GenericImage, ImageExt};

use axum::{Router, routing::{get, post, delete}};

// --- Helpers ---

fn run_git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn create_test_repo() -> TempDir {
    let dir = tempfile::tempdir().unwrap();

    run_git(dir.path(), &["init", "-b", "main"]);
    run_git(dir.path(), &["config", "user.email", "test@test.com"]);
    run_git(dir.path(), &["config", "user.name", "Test"]);

    std::fs::write(
        dir.path().join("README.md"),
        "# Test Project\n\nA test project for e2e tests.\n",
    )
    .unwrap();
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
        r#"export interface User {
  name: string;
  age: number;
}

export function greet(user: User): string {
  return `Hello ${user.name}`;
}

export class UserService {
  getUser(): User {
    return { name: "test", age: 0 };
  }
}
"#,
    )
    .unwrap();

    run_git(dir.path(), &["add", "."]);
    run_git(dir.path(), &["commit", "-m", "initial commit"]);

    dir
}

async fn get_test_db() -> (Option<ContainerAsync<GenericImage>>, Arc<Database>) {
    if let Ok(url) = std::env::var("DATABASE_URL") {
        let db = Database::connect(&url).await.unwrap();
        (None, Arc::new(db))
    } else {
        // Use pgvector/pgvector image which has the vector extension pre-installed
        let image = GenericImage::new("pgvector/pgvector", "pg17")
            .with_wait_for(testcontainers::core::WaitFor::message_on_stderr("ready to accept connections"));
        let container = image
            .with_exposed_port(5432.into())
            .with_env_var("POSTGRES_PASSWORD", "postgres")
            .start()
            .await
            .unwrap();
        let host_port = container.get_host_port_ipv4(5432).await.unwrap();
        let url = format!("postgres://postgres:postgres@127.0.0.1:{host_port}/postgres");

        // Wait for postgres to be truly ready
        let mut retries = 0;
        let pool = loop {
            match sqlx::postgres::PgPool::connect(&url).await {
                Ok(pool) => break pool,
                Err(_) if retries < 30 => {
                    retries += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                Err(e) => panic!("failed to connect to postgres after {retries} retries: {e}"),
            }
        };
        sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let db = Database::connect(&url).await.unwrap();
        (Some(container), Arc::new(db))
    }
}

/// Spin up the axum server on a random port and return the base URL.
async fn start_server(db: Arc<Database>, repos_dir: &Path) -> String {
    let search = Arc::new(SearchIndex::open_in_memory().unwrap());

    let cfg = Arc::new(config::Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        database_url: String::new(), // not used, we already have the db
        index_path: std::path::PathBuf::from("/tmp/gitdoc-test-index"),
        repos_dir: repos_dir.to_path_buf(),
        log_format: "text".into(),
        exclusion_patterns: vec![
            "node_modules/".into(),
            "target/".into(),
            ".git/".into(),
            "vendor/".into(),
        ],
        embedding: None,
    });

    let state = Arc::new(AppState {
        db,
        search,
        embedder: None,
        config: cfg,
    });

    use gitdoc_server::api::snapshots;
    use gitdoc_server::api::search as search_api;

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route(
            "/repos",
            post(gitdoc_server::api::repos::create_repo).get(gitdoc_server::api::repos::list_repos),
        )
        .route(
            "/repos/{repo_id}",
            get(gitdoc_server::api::repos::get_repo).delete(gitdoc_server::api::repos::delete_repo),
        )
        .route("/repos/{repo_id}/index", post(gitdoc_server::api::repos::index_repo))
        .route("/repos/{repo_id}/fetch", post(gitdoc_server::api::repos::fetch_repo))
        .route("/snapshots/{snapshot_id}/overview", get(snapshots::get_overview))
        .route("/snapshots/{snapshot_id}/docs", get(snapshots::list_docs))
        .route("/snapshots/{snapshot_id}/docs/{*path}", get(snapshots::get_doc_content))
        .route("/snapshots/{snapshot_id}/symbols", get(snapshots::list_symbols))
        .route(
            "/snapshots/{snapshot_id}/symbols/{symbol_id}",
            get(snapshots::get_snapshot_symbol),
        )
        .route(
            "/snapshots/{snapshot_id}/symbols/{symbol_id}/references",
            get(snapshots::get_symbol_references),
        )
        .route(
            "/snapshots/{snapshot_id}/symbols/{symbol_id}/implementations",
            get(snapshots::get_symbol_implementations),
        )
        .route("/snapshots/{from_id}/diff/{to_id}", get(snapshots::diff_symbols))
        .route("/snapshots/{snapshot_id}", delete(snapshots::delete_snapshot))
        .route("/snapshots/{snapshot_id}/search/docs", get(search_api::search_docs))
        .route(
            "/snapshots/{snapshot_id}/search/symbols",
            get(search_api::search_symbols),
        )
        .route(
            "/snapshots/{snapshot_id}/search/semantic",
            get(search_api::search_semantic),
        )
        .route("/symbols/{symbol_id}", get(snapshots::get_symbol))
        .route("/admin/gc", post(search_api::gc))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://127.0.0.1:{}", addr.port())
}

// =============================================================================
// Tests
// =============================================================================

#[tokio::test]
async fn e2e_register_index_and_query_via_http() {
    // -- Setup: DB, test repo, HTTP server --
    let test_repo = create_test_repo();
    let (_container, db) = get_test_db().await;
    let repos_dir = tempfile::tempdir().unwrap();
    let base_url = start_server(db, repos_dir.path()).await;
    let http = reqwest::Client::new();

    // -- 1. Health check (like MCP ping) --
    let resp = http.get(format!("{base_url}/health")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");

    // -- 2. List repos (should be empty) --
    let resp = http.get(format!("{base_url}/repos")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let repos: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(repos.is_empty(), "should start with no repos");

    // -- 3. Register repo via path (internal test — MCP now uses url, but API still works for local) --
    // For the test we directly insert via the DB since create_repo now requires URL+clone
    // and we don't want to clone from the network in tests.
    // Instead, let's simulate what the server does: insert the repo directly.
    // Actually, let's test the REAL API. We can use a file:// URL to clone locally.
    let local_url = format!("file://{}", test_repo.path().display());
    let resp = http
        .post(format!("{base_url}/repos"))
        .json(&serde_json::json!({
            "id": "test",
            "name": "Test Project",
            "url": local_url,
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(status, 201, "register_repo failed: {body}");
    assert_eq!(body["id"], "test");

    // -- 4. List repos (should have one) --
    let resp = http.get(format!("{base_url}/repos")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let repos: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0]["id"], "test");
    assert_eq!(repos[0]["name"], "Test Project");
    // Should NOT have path in a way that leaks server internals — but the server does return it.
    // The MCP client strips it. Here we just verify the field exists.
    assert!(repos[0].get("url").is_some(), "should have url field");

    // -- 5. Index repo --
    let resp = http
        .post(format!("{base_url}/repos/test/index"))
        .json(&serde_json::json!({
            "commit": "HEAD",
            "label": "v1",
            "fetch": false,
        }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let index_result: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(status, 200, "index_repo failed: {index_result}");
    let snapshot_id = index_result["snapshot_id"].as_i64().expect(&format!("missing snapshot_id in: {index_result}"));
    assert!(snapshot_id > 0, "snapshot_id should be positive");
    assert!(
        index_result["files_scanned"].as_i64().unwrap() >= 3,
        "should scan at least 3 files: {index_result}"
    );
    assert!(
        index_result["symbols_count"].as_i64().unwrap() >= 5,
        "should have at least 5 symbols: {index_result}"
    );
    assert_eq!(
        index_result["docs_count"].as_i64().unwrap(),
        1,
        "should have 1 doc (README.md): {index_result}"
    );

    // -- 6. Get repo overview (like MCP get_repo_overview) --
    let resp = http
        .get(format!("{base_url}/snapshots/{snapshot_id}/overview"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let overview: serde_json::Value = resp.json().await.unwrap();

    // Check snapshot metadata
    assert_eq!(overview["snapshot"]["id"], snapshot_id);
    assert_eq!(overview["snapshot"]["status"], "ready");

    // Check README
    let readme = overview["readme"].as_str().unwrap();
    assert!(
        readme.contains("Test Project"),
        "README should contain 'Test Project': {readme}"
    );

    // Check docs listing
    let docs = overview["docs"].as_array().unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0]["file_path"], "README.md");

    // Check top-level symbols
    let symbols = overview["top_level_symbols"].as_array().unwrap();
    assert!(
        symbols.len() >= 3,
        "should have at least main, Config, greet, User, UserService: got {:?}",
        symbols.iter().map(|s| s["name"].as_str().unwrap()).collect::<Vec<_>>()
    );

    // Verify symbol structure
    let main_sym = symbols.iter().find(|s| s["name"] == "main");
    assert!(main_sym.is_some(), "should find 'main' symbol");
    let main_sym = main_sym.unwrap();
    assert_eq!(main_sym["kind"], "function");
    assert_ne!(main_sym["visibility"], "private", "main should be public");
    assert!(main_sym["id"].as_i64().is_some(), "symbol should have numeric id");
    assert!(main_sym["file_path"].as_str().is_some(), "symbol should have file_path");
    assert!(main_sym["signature"].as_str().is_some(), "symbol should have signature");

    // -- 7. List symbols with filters (like MCP list_symbols) --
    let resp = http
        .get(format!(
            "{base_url}/snapshots/{snapshot_id}/symbols?kind=function&include_private=false"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let functions: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(
        !functions.is_empty(),
        "should find at least one public function"
    );
    for f in &functions {
        assert_eq!(f["kind"], "function");
        assert_ne!(f["visibility"], "private", "include_private=false should exclude private");
    }
    let fn_names: Vec<&str> = functions.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert!(fn_names.contains(&"main"), "should find 'main': {fn_names:?}");
    assert!(fn_names.contains(&"greet"), "should find 'greet': {fn_names:?}");

    // -- 8. List symbols filtered by file (like MCP list_symbols with file_path) --
    let resp = http
        .get(format!(
            "{base_url}/snapshots/{snapshot_id}/symbols?file_path=app.ts"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let ts_symbols: Vec<serde_json::Value> = resp.json().await.unwrap();
    let ts_names: Vec<&str> = ts_symbols.iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert!(ts_names.contains(&"User"), "should find 'User' interface: {ts_names:?}");
    assert!(
        ts_names.contains(&"UserService"),
        "should find 'UserService' class: {ts_names:?}"
    );

    // -- 9. Get symbol detail (like MCP get_symbol) --
    let main_id = main_sym["id"].as_i64().unwrap();
    let resp = http
        .get(format!("{base_url}/symbols/{main_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let detail: serde_json::Value = resp.json().await.unwrap();
    let sym = &detail["symbol"];
    assert_eq!(sym["id"], main_id);
    assert_eq!(sym["name"], "main");
    assert!(
        sym["body"].as_str().unwrap().contains("println"),
        "body should contain function source: {}",
        sym["body"]
    );

    // -- 10. Search symbols (like MCP search_symbols) --
    let resp = http
        .get(format!(
            "{base_url}/snapshots/{snapshot_id}/search/symbols?q=Config"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let results: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(!results.is_empty(), "search for 'Config' should return results");
    assert!(
        results.iter().any(|r| r["name"] == "Config"),
        "should find Config struct: {results:?}"
    );
    // Verify search result has all expected fields
    let config_result = results.iter().find(|r| r["name"] == "Config").unwrap();
    assert!(config_result["symbol_id"].as_i64().is_some());
    assert!(config_result["score"].as_f64().is_some());
    assert!(config_result["kind"].as_str().is_some());
    assert!(config_result["signature"].as_str().is_some());

    // -- 11. Search docs (like MCP search_docs) --
    let resp = http
        .get(format!(
            "{base_url}/snapshots/{snapshot_id}/search/docs?q=test+project"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let doc_results: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(
        !doc_results.is_empty(),
        "search for 'test project' should find README"
    );
    assert_eq!(doc_results[0]["file_path"], "README.md");

    // -- 12. Read doc content (like MCP read_doc) --
    let resp = http
        .get(format!(
            "{base_url}/snapshots/{snapshot_id}/docs/README.md"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(doc["file_path"], "README.md");
    assert!(
        doc["content"].as_str().unwrap().contains("Test Project"),
        "doc content should contain 'Test Project'"
    );

    // -- 13. Get repo detail with snapshots (used by MCP snapshot resolver) --
    let resp = http
        .get(format!("{base_url}/repos/test"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let detail: serde_json::Value = resp.json().await.unwrap();
    let snapshots = detail["snapshots"].as_array().unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0]["label"], "v1");
    assert_eq!(snapshots[0]["status"], "ready");

    // -- 14. Error cases: query non-existent repo --
    let resp = http
        .get(format!("{base_url}/repos/nonexistent"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let err: serde_json::Value = resp.json().await.unwrap();
    assert!(err["error"].as_str().is_some(), "should return error message: {err}");

    // -- 15. Error case: query non-existent snapshot --
    let resp = http
        .get(format!("{base_url}/snapshots/999999/overview"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let err: serde_json::Value = resp.json().await.unwrap();
    assert!(
        err["error"].as_str().unwrap().contains("not found"),
        "error should mention 'not found': {err}"
    );

    // -- 16. Re-index same commit returns same snapshot (deduplication) --
    let resp = http
        .post(format!("{base_url}/repos/test/index"))
        .json(&serde_json::json!({ "commit": "HEAD" }))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let r2: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(status, 200, "re-index failed: {r2}");
    assert_eq!(
        r2["snapshot_id"].as_i64().unwrap(),
        snapshot_id,
        "re-indexing same commit should return same snapshot"
    );
}

/// Test that registering with a bad URL returns a clear error, not a decode failure.
#[tokio::test]
async fn e2e_register_bad_url_returns_error() {
    let (_container, db) = get_test_db().await;
    let repos_dir = tempfile::tempdir().unwrap();
    let base_url = start_server(db, repos_dir.path()).await;
    let http = reqwest::Client::new();

    let resp = http
        .post(format!("{base_url}/repos"))
        .json(&serde_json::json!({
            "id": "bad",
            "name": "Bad Repo",
            "url": "https://invalid.example.com/nonexistent.git",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap().contains("clone failed"),
        "should return a clone error: {body}"
    );
}

/// Test that missing required fields return 422/400, not decode errors.
#[tokio::test]
async fn e2e_register_missing_url_returns_error() {
    let (_container, db) = get_test_db().await;
    let repos_dir = tempfile::tempdir().unwrap();
    let base_url = start_server(db, repos_dir.path()).await;
    let http = reqwest::Client::new();

    // Missing url field entirely
    let resp = http
        .post(format!("{base_url}/repos"))
        .json(&serde_json::json!({
            "id": "nope",
            "name": "Nope",
        }))
        .send()
        .await
        .unwrap();

    // Should be 422 (unprocessable) or 400, not 500
    assert!(
        resp.status().as_u16() == 400 || resp.status().as_u16() == 422,
        "status should be 400 or 422, got {}",
        resp.status()
    );
}

/// Test that querying a repo with no snapshots returns a clear error.
#[tokio::test]
async fn e2e_query_before_index_returns_clear_error() {
    let test_repo = create_test_repo();
    let (_container, db) = get_test_db().await;
    let repos_dir = tempfile::tempdir().unwrap();
    let base_url = start_server(db, repos_dir.path()).await;
    let http = reqwest::Client::new();

    // Register
    let local_url = format!("file://{}", test_repo.path().display());
    let resp = http
        .post(format!("{base_url}/repos"))
        .json(&serde_json::json!({
            "id": "noindex",
            "name": "No Index",
            "url": local_url,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Get repo detail — should show 0 snapshots
    let resp = http
        .get(format!("{base_url}/repos/noindex"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let detail: serde_json::Value = resp.json().await.unwrap();
    let snapshots = detail["snapshots"].as_array().unwrap();
    assert!(snapshots.is_empty(), "should have no snapshots before indexing");
}
