# Serve-Web Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate `ossgallery-web` (templates) and `ossgalley-cli serve` (server entry point) so that `cargo run -p ossgalley-cli serve <host>` starts a real axum web server with CLI-configured S3 client and database path.

**Architecture:** Refactor `ossgallery-web` into a template-only lib crate providing `register_templates()`. Move all handlers, router, and AppState into `ossgalley-cli/src/web/`. Rewrite `cmd_serve.rs` to start axum server using real S3 client and templates from `ossgallery-web`.

**Tech Stack:** Rust, axum, minijinja, serde, tower-http, ossgalley-core

---

### Task 1: Refactor ossgallery-web into template-only lib crate

**Files:**
- Create: `crates/ossgallery-web/src/lib.rs`
- Modify: `crates/ossgallery-web/Cargo.toml`
- Delete: `crates/ossgallery-web/src/main.rs`
- Delete: `crates/ossgallery-web/src/state.rs`
- Delete: `crates/ossgallery-web/src/router.rs`
- Delete: `crates/ossgallery-web/src/handlers/`

- [ ] **Step 1: Simplify Cargo.toml**

Replace the current `Cargo.toml` with one that only has minijinja and ossgalley-core:

```toml
[package]
name = "ossgallery-web"
edition = "2021"
version = "0.1.0"

[dependencies]
ossgalley-core = { path = "../ossgalley-core" }
minijinja = "2"
tracing = "0.1"
```

- [ ] **Step 2: Create `src/lib.rs` with template registration**

```rust
#![forbid(unsafe_code)]
#![deny(unreachable_code)]
#![deny(unused_must_use)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]
#![deny(clippy::panic)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::let_underscore_must_use)]
#![deny(clippy::await_holding_lock)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]

/// All built-in templates, embedded at compile time via `include_str!`.
const TEMPLATES: &[(&str, &str)] = &[
    ("browse.html", include_str!("../templates/browse.html")),
    ("browse_table.html", include_str!("../templates/browse_table.html")),
    ("gallery.html", include_str!("../templates/gallery.html")),
    ("gallery_items.html", include_str!("../templates/gallery_items.html")),
    ("file_detail.html", include_str!("../templates/file_detail.html")),
    ("search.html", include_str!("../templates/search.html")),
    ("search_results.html", include_str!("../templates/search_results.html")),
    ("timeline.html", include_str!("../templates/timeline.html")),
    ("tags.html", include_str!("../templates/tags.html")),
    ("stats.html", include_str!("../templates/stats.html")),
    ("duplicates.html", include_str!("../templates/duplicates.html")),
    ("settings.html", include_str!("../templates/settings.html")),
    ("layout.html", include_str!("../templates/layout.html")),
];

/// Register all built-in templates into a minijinja `Environment`.
///
/// Returns a list of template names that failed to register (empty on success).
/// Failures are non-fatal — the caller can decide how to handle them.
pub fn register_templates(env: &mut minijinja::Environment<'static>) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, content) in TEMPLATES {
        if let Err(e) = env.add_template(name, *content) {
            tracing::warn!("ossgallery-web: failed to register template '{name}': {e}");
            errors.push(name.to_string());
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_all_templates() {
        let mut env = minijinja::Environment::new();
        let errors = register_templates(&mut env);
        assert!(errors.is_empty(), "templates failed to register: {errors:?}");
    }

    #[test]
    fn test_render_browse_template() {
        let mut env = minijinja::Environment::new();
        register_templates(&mut env);
        let tmpl = env.get_template("browse.html").unwrap();
        let result = tmpl.render(&serde_json::json!({
            "entries": [],
            "breadcrumbs": [],
            "sort_by": "name",
            "sort_order": "asc",
        }));
        assert!(result.is_ok(), "browse.html rendering failed: {:?}", result.err());
    }

    #[test]
    fn test_render_gallery_template() {
        let mut env = minijinja::Environment::new();
        register_templates(&mut env);
        let tmpl = env.get_template("gallery.html").unwrap();
        let result = tmpl.render(&serde_json::json!({
            "items": [],
            "page": 0,
            "has_more": false,
        }));
        assert!(result.is_ok(), "gallery.html rendering failed: {:?}", result.err());
    }
}
```

- [ ] **Step 3: Delete old source files**

```bash
rm crates/ossgallery-web/src/main.rs
rm crates/ossgallery-web/src/state.rs
rm crates/ossgallery-web/src/router.rs
rm -rf crates/ossgallery-web/src/handlers/
```

- [ ] **Step 4: Build and test**

Run: `cargo build -p ossgallery-web`
Expected: success

Run: `cargo test -p ossgallery-web`
Expected: 3 tests pass (two template tests, one ... wait, only the two tests defined above)

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: ossgallery-web becomes template-only lib crate

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: Add web dependencies to ossgalley-cli and create web/ module skeleton

**Files:**
- Modify: `crates/ossgalley-cli/Cargo.toml`
- Modify: `crates/ossgalley-cli/src/main.rs`
- Create: `crates/ossgalley-cli/src/web/mod.rs`

- [ ] **Step 1: Add web dependencies to CLI Cargo.toml**

Add to `crates/ossgalley-cli/Cargo.toml`:
```toml
ossgallery-web = { path = "../ossgallery-web" }
axum = "0.7"
minijinja = "2"
tower-http = { version = "0.5", features = ["cors", "trace"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1.0"
tracing = "0.1"
```

- [ ] **Step 2: Create `src/web/mod.rs`**

```rust
pub mod handlers;
pub mod router;
pub mod state;
```

- [ ] **Step 3: Add `mod web` to `src/main.rs`**

Add after `mod cmd_serve;`:
```rust
mod web;
```

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: add web dependencies and web module skeleton

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: Move handlers, state, router from ossgallery-web to ossgalley-cli

**Files:**
- Create: `crates/ossgalley-cli/src/web/state.rs`
- Create: `crates/ossgalley-cli/src/web/router.rs`
- Create: `crates/ossgalley-cli/src/web/handlers/mod.rs`
- Create: `crates/ossgalley-cli/src/web/handlers/browse.rs`
- Create: `crates/ossgalley-cli/src/web/handlers/dashboard.rs`
- Create: `crates/ossgalley-cli/src/web/handlers/download.rs`
- Create: `crates/ossgalley-cli/src/web/handlers/duplicates.rs`
- Create: `crates/ossgalley-cli/src/web/handlers/file_detail.rs`
- Create: `crates/ossgalley-cli/src/web/handlers/gallery.rs`
- Create: `crates/ossgalley-cli/src/web/handlers/search.rs`
- Create: `crates/ossgalley-cli/src/web/handlers/settings.rs`
- Create: `crates/ossgalley-cli/src/web/handlers/stats.rs`
- Create: `crates/ossgalley-cli/src/web/handlers/tags.rs`
- Create: `crates/ossgalley-cli/src/web/handlers/thumbnail.rs`
- Create: `crates/ossgalley-cli/src/web/handlers/timeline.rs`

- [ ] **Step 1: Create `src/web/state.rs`**

Copy from `ossgallery-web/src/state.rs` with the following changes:
- Remove `impl AppState::new()` — template loading will be done in `cmd_serve.rs`
- Change `#[allow(dead_code)]` to normal (no longer needed)
- Keep `Clone` derive, all fields, and the struct definition

```rust
use std::sync::Arc;

use ossgalley_core::s3::client::S3Client;
use ossgalley_core::types::BucketName;
use ossgalley_core::view::LocalView;
use ossgalley_core::view::RemoteView;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub templates: Arc<minijinja::Environment<'static>>,
    pub local_view: Arc<LocalView>,
    pub remote_view: Arc<RemoteView>,
    pub bucket: BucketName,
}
```

- [ ] **Step 2: Create `src/web/router.rs`**

Copy from `ossgallery-web/src/router.rs` with one change:
- `use crate::state::AppState` → `use crate::web::state::AppState`

```rust
use axum::{Router, routing::get};

use crate::web::handlers;
use crate::web::state::AppState;

/// Create the axum Router with shared application state.
pub fn create_router(app_state: AppState) -> Router {
    Router::new()
        .route("/", get(handlers::dashboard))
        .route("/browse", get(handlers::browse))
        .route("/gallery", get(handlers::gallery))
        .route("/search", get(handlers::search))
        .route("/timeline", get(handlers::timeline))
        .route("/tags", get(handlers::tags))
        .route("/files/*key", get(handlers::file_detail))
        .route("/duplicates", get(handlers::duplicates))
        .route("/stats", get(handlers::stats))
        .route("/settings", get(handlers::settings))
        .route("/thumbnails/*key", get(handlers::thumbnail))
        .route("/download/*key", get(handlers::download))
        .with_state(app_state)
}
```

- [ ] **Step 3: Create `src/web/handlers/mod.rs`**

Copy from `ossgallery-web/src/handlers/mod.rs` — no changes needed (the module names are the same).

```rust
pub mod browse;
pub mod dashboard;
pub mod download;
pub mod duplicates;
pub mod file_detail;
pub mod gallery;
pub mod search;
pub mod settings;
pub mod stats;
pub mod tags;
pub mod thumbnail;
pub mod timeline;

pub use browse::browse;
pub use dashboard::dashboard;
pub use download::download;
pub use duplicates::duplicates;
pub use file_detail::file_detail;
pub use gallery::gallery;
pub use search::search;
pub use settings::settings;
pub use stats::stats;
pub use tags::tags;
pub use thumbnail::thumbnail;
pub use timeline::timeline;
```

- [ ] **Step 4: Create all handler files**

Copy each handler from `ossgallery-web/src/handlers/<name>.rs` to `ossgalley-cli/src/web/handlers/<name>.rs`.
The only change in each file: replace `use crate::state::AppState;` with `use crate::web::state::AppState;`.

The handlers are:
- `browse.rs` — directory listing with breadcrumbs, sort, HTMX partial
- `dashboard.rs` — simple JSON status endpoint
- `download.rs` — file download from S3 via `RemoteView::fetch_file_content()`
- `duplicates.rs` — duplicate file groups via `LocalView::find_duplicates()`
- `file_detail.rs` — file metadata, grouped by namespace, thumbnail check
- `gallery.rs` — image gallery with pagination, HTMX infinite scroll
- `search.rs` — search by name or tag, HTMX live search
- `settings.rs` — show bucket name
- `stats.rs` — file statistics by category
- `tags.rs` — list all tags
- `thumbnail.rs` — serve cached thumbnails, fallback to S3 generation
- `timeline.rs` — files grouped by date

Each file uses `use crate::web::state::AppState` instead of `use crate::state::AppState`.

- [ ] **Step 5: Build to verify**

Run: `cargo check -p ossgalley-cli 2>&1`
Expected: success (no errors)

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: move web handlers, router, state into ossgalley-cli

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 4: Rewrite cmd_serve.rs to start axum server

**Files:**
- Modify: `crates/ossgalley-cli/src/cmd_serve.rs`

- [ ] **Step 1: Rewrite cmd_serve.rs**

Replace the current file with one that starts the real axum server:

```rust
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;

use ossgalley_core::error::{OssgalleyError, Result};
use ossgalley_core::types::*;
use ossgalley_core::s3::config::HostIdentifier;
use ossgalley_core::s3::config::OssConfig;
use ossgalley_core::s3::real::RealS3Client;
use ossgalley_core::s3::client::S3Client;
use ossgalley_core::db::pool::create_pool;
use ossgalley_core::db::schema::run_migrations;
use ossgalley_core::db::status::{check_db_status, decide_action, DbAction};
use ossgalley_core::view::LocalView;
use ossgalley_core::view::remote::RemoteView;

use crate::cli::Cli;
use crate::web::state::AppState;
use crate::web::router::create_router;

pub async fn run_serve(cli: &Cli, host: &str, port: u16, readonly: bool) -> Result<()> {
    let bucket = BucketName::new(&cli.bucket)
        .map_err(|e| OssgalleyError::InvalidConfig(format!("Invalid bucket: {}", e)))?;
    let host_id = HostIdentifier::new(bucket.clone(), host)
        .map_err(|e| OssgalleyError::InvalidConfig(format!("Invalid host: {}", e)))?;

    // Ensure DB is available
    let db_path = &cli.db_path;
    let config = OssConfig::validate(
        bucket.clone(),
        &cli.endpoint,
        &cli.region,
        &cli.access_key,
        &cli.secret_key,
        10,
    )?;
    let real = RealS3Client::from_config(&config);
    let s3 = Arc::new(real) as Arc<dyn S3Client>;

    // Check DB status
    let status = check_db_status(&db_path, s3.as_ref(), &host_id).await?;
    let action = decide_action(&status, readonly);

    match action {
        DbAction::UseLocal | DbAction::DownloadFromOss => {
            if !db_path.exists() {
                let db_key = host_id.db_path();
                let data = s3.get_object(&bucket, db_key).await?;
                if let Some(parent) = db_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(OssgalleyError::IoError)?;
                }
                std::fs::write(&db_path, &data)
                    .map_err(OssgalleyError::IoError)?;
                tracing::info!("DB downloaded from OSS.");
            }
        }
        DbAction::FullScanAndUpload => {
            return Err(OssgalleyError::NotFound(
                "No database found. Run 'ossgalley scan <host>' first.".to_string()
            ));
        }
        DbAction::Abort(msg) => {
            return Err(OssgalleyError::Internal(msg));
        }
    }

    // Create pool, run migrations
    let pool = create_pool(&db_path).await?;
    run_migrations(&pool).await?;

    // Create views
    let local_view = LocalView::new(pool);
    let remote_view = RemoteView::new(local_view.db().clone(), s3, bucket.clone());

    // Register templates from ossgallery-web
    let mut env = minijinja::Environment::new();
    let errors = ossgallery_web::register_templates(&mut env);
    if !errors.is_empty() {
        tracing::warn!("{} template(s) failed to register: {:?}", errors.len(), errors);
    }

    // Build AppState
    let app_state = AppState {
        templates: Arc::new(env),
        local_view: Arc::new(local_view),
        remote_view: Arc::new(remote_view),
        bucket,
    };

    // Create router and start server
    let app = create_router(app_state)
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("ossgalley web server starting on {}", addr);
    tracing::info!("  Host: {}", host);
    tracing::info!("  Read-only: {}", readonly);

    let listener = TcpListener::bind(addr).await
        .map_err(|e| OssgalleyError::Internal(format!("Failed to bind to {addr}: {e}")))?;

    axum::serve(listener, app).await
        .map_err(|e| OssgalleyError::Internal(format!("Server error: {e}")))?;

    Ok(())
}
```

- [ ] **Step 2: Build to verify**

Run: `cargo check -p ossgalley-cli 2>&1`
Expected: success (no errors)

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat: cmd_serve now starts axum server with real S3 client and templates

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5: Full build and test

**Files:**
- None (verification only)

- [ ] **Step 1: Full workspace build**

Run: `cargo build 2>&1`
Expected: all crates build successfully

- [ ] **Step 2: Run all tests**

Run: `cargo test 2>&1`
Expected: all tests pass

- [ ] **Step 3: Check clippy**

Run: `cargo clippy --all-targets 2>&1`
Expected: no warnings (or only pre-existing ones)

---

### Task 6: Clean up

**Files:**
- Delete: `crates/ossgallery-web/src/router.rs` (if still exists)
- Delete: `crates/ossgallery-web/src/state.rs` (if still exists)
- Delete: `crates/ossgallery-web/src/handlers/` (if still exists)

- [ ] **Step 1: Verify all old files are gone**

```bash
ls crates/ossgallery-web/src/
# Should only show: lib.rs, not main.rs, state.rs, router.rs, or handlers/
```

- [ ] **Step 2: Remove any leftover files**

```bash
rm -f crates/ossgallery-web/src/main.rs
rm -f crates/ossgallery-web/src/state.rs
rm -f crates/ossgallery-web/src/router.rs
rm -rf crates/ossgallery-web/src/handlers/
```

- [ ] **Step 3: Final build check**

Run: `cargo build 2>&1`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: remove leftover ossgallery-web source files

Co-Authored-By: Claude <noreply@anthropic.com>"
```