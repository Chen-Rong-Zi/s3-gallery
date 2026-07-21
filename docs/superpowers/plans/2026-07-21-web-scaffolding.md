# Web Scaffolding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the web server scaffolding for the ossgalley project, including the main server, router, state management, placeholder handlers, and HTML templates.

**Architecture:** Axum-based web server with minijinja templating, modular handlers, and shared application state.

**Tech Stack:** Rust, Axum, Tokio, MiniJinja, Tracing

---

## File Structure Map

| File | Responsibility |
|------|-----------------|
| `crates/ossgalley-web/Cargo.toml` | Dependencies (add tracing-subscriber) |
| `crates/ossgalley-web/src/lib.rs` | Library exports and deny attributes |
| `crates/ossgalley-web/src/main.rs` | Server entry point |
| `crates/ossgalley-web/src/state.rs` | Shared application state |
| `crates/ossgalley-web/src/router.rs` | Route definitions |
| `crates/ossgalley-web/src/handlers/mod.rs` | Handler exports and placeholder implementations |
| `crates/ossgalley-web/templates/layout.html` | Base HTML template |
| `crates/ossgalley-web/templates/browse.html` | Browse page template |
| `crates/ossgalley-web/templates/gallery.html` | Gallery page template |
| `crates/ossgalley-web/templates/file_detail.html` | File detail page template |
| `crates/ossgalley-web/templates/search.html` | Search page template |
| `crates/ossgalley-web/templates/timeline.html` | Timeline page template |
| `crates/ossgalley-web/templates/tags.html` | Tags page template |
| `crates/ossgalley-web/templates/stats.html` | Stats page template |

---

### Task 1: Update Cargo.toml with tracing-subscriber

**Files:**
- Modify: `crates/ossgalley-web/Cargo.toml`

- [ ] **Step 1: Add tracing-subscriber dependency**

Add to `[dependencies]`:
```toml
tracing-subscriber = "0.3"
```

---

### Task 2: Update lib.rs with module exports

**Files:**
- Modify: `crates/ossgalley-web/src/lib.rs`

- [ ] **Step 1: Add module declarations**

Add at the end of the file:
```rust
pub mod router;
pub mod state;
pub mod handlers;
```

---

### Task 3: Create main.rs server entry point

**Files:**
- Create/Overwrite: `crates/ossgalley-web/src/main.rs`

- [ ] **Step 1: Write main.rs content**

```rust
mod lib;
mod router;
mod state;
mod handlers;

use std::net::SocketAddr;
use axum::serve;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    
    let app = router::create_router();
    
    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("ossgalley web server starting on {}", addr);
    
    let listener = TcpListener::bind(addr).await?;
    serve(listener, app).await?;
    
    Ok(())
}
```

---

### Task 4: Create state.rs for shared application state

**Files:**
- Create: `crates/ossgalley-web/src/state.rs`

- [ ] **Step 1: Write state.rs content**

```rust
use std::sync::Arc;

/// Shared application state
pub struct AppState {
    pub templates: minijinja::Environment<'static>,
}

impl AppState {
    pub fn new() -> Self {
        let mut templates = minijinja::Environment::new();
        
        // Register templates
        templates.add_template("browse.html", include_str!("../templates/browse.html")).ok();
        templates.add_template("gallery.html", include_str!("../templates/gallery.html")).ok();
        templates.add_template("file_detail.html", include_str!("../templates/file_detail.html")).ok();
        templates.add_template("search.html", include_str!("../templates/search.html")).ok();
        templates.add_template("timeline.html", include_str!("../templates/timeline.html")).ok();
        templates.add_template("tags.html", include_str!("../templates/tags.html")).ok();
        templates.add_template("stats.html", include_str!("../templates/stats.html")).ok();
        templates.add_template("layout.html", include_str!("../templates/layout.html")).ok();
        
        Self { templates }
    }
}
```

---

### Task 5: Create router.rs for route definitions

**Files:**
- Create: `crates/ossgalley-web/src/router.rs`

- [ ] **Step 1: Write router.rs content**

```rust
use axum::{Router, routing::get};
use crate::handlers;

pub fn create_router() -> Router {
    Router::new()
        .route("/", get(handlers::dashboard))
        .route("/browse", get(handlers::browse))
        .route("/gallery", get(handlers::gallery))
        .route("/search", get(handlers::search))
        .route("/timeline", get(handlers::timeline))
        .route("/tags", get(handlers::tags))
        .route("/files/{*key}", get(handlers::file_detail))
        .route("/duplicates", get(handlers::duplicates))
        .route("/stats", get(handlers::stats))
        .route("/settings", get(handlers::settings))
        .route("/thumbnails/{*key}", get(handlers::thumbnail))
        .route("/download/{*key}", get(handlers::download))
}
```

---

### Task 6: Create handlers module

**Files:**
- Create: `crates/ossgalley-web/src/handlers/mod.rs`

- [ ] **Step 1: Write handlers/mod.rs content**

```rust
use axum::{Json, response::IntoResponse};
use serde_json::json;

/// Dashboard handler
pub async fn dashboard() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "ossgalley-web"
    }))
}

/// Browse handler (placeholder)
pub async fn browse() -> impl IntoResponse {
    Json(json!({"message": "browse page"}))
}

/// Gallery handler (placeholder)
pub async fn gallery() -> impl IntoResponse {
    Json(json!({"message": "gallery page"}))
}

/// Search handler (placeholder)
pub async fn search() -> impl IntoResponse {
    Json(json!({"message": "search page"}))
}

/// Timeline handler (placeholder)
pub async fn timeline() -> impl IntoResponse {
    Json(json!({"message": "timeline page"}))
}

/// Tags handler (placeholder)
pub async fn tags() -> impl IntoResponse {
    Json(json!({"message": "tags page"}))
}

/// File detail handler (placeholder)
pub async fn file_detail() -> impl IntoResponse {
    Json(json!({"message": "file detail page"}))
}

/// Duplicates handler (placeholder)
pub async fn duplicates() -> impl IntoResponse {
    Json(json!({"message": "duplicates page"}))
}

/// Stats handler (placeholder)
pub async fn stats() -> impl IntoResponse {
    Json(json!({"message": "stats page"}))
}

/// Settings handler (placeholder)
pub async fn settings() -> impl IntoResponse {
    Json(json!({"message": "settings page"}))
}

/// Thumbnail handler (placeholder)
pub async fn thumbnail() -> impl IntoResponse {
    Json(json!({"message": "thumbnail endpoint"}))
}

/// Download handler (placeholder)
pub async fn download() -> impl IntoResponse {
    Json(json!({"message": "download endpoint"}))
}
```

---

### Task 7: Create templates directory and layout.html

**Files:**
- Create: `crates/ossgalley-web/templates/layout.html`

- [ ] **Step 1: Create templates directory**

Run: `mkdir -p /Users/macbook/Project/ossgalley/crates/ossgalley-web/templates`

- [ ] **Step 2: Write layout.html content**

```html
<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{% block title %}ossgalley{% endblock %}</title>
    <script src="https://unpkg.com/htmx.org@2.0.0"></script>
    <style>
        body { font-family: -apple-system, sans-serif; max-width: 1200px; margin: 0 auto; padding: 20px; }
        nav a { margin-right: 15px; text-decoration: none; color: #333; }
        nav a:hover { color: #0066cc; }
        table { border-collapse: collapse; width: 100%; }
        th, td { padding: 8px; text-align: left; border-bottom: 1px solid #ddd; }
        tr:hover { background-color: #f5f5f5; }
        .breadcrumb { margin: 10px 0; }
        .breadcrumb a { color: #0066cc; text-decoration: none; }
        .thumbnail { width: 100px; height: 100px; object-fit: cover; }
        .gallery-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 10px; }
    </style>
</head>
<body>
    <nav>
        <a href="/">Dashboard</a>
        <a href="/browse">Browse</a>
        <a href="/gallery">Gallery</a>
        <a href="/search">Search</a>
        <a href="/timeline">Timeline</a>
        <a href="/tags">Tags</a>
        <a href="/duplicates">Duplicates</a>
        <a href="/stats">Stats</a>
    </nav>
    <hr>
    {% block content %}{% endblock %}
</body>
</html>
```

---

### Task 8: Create remaining HTML templates

**Files:**
- Create: `crates/ossgalley-web/templates/browse.html`
- Create: `crates/ossgalley-web/templates/gallery.html`
- Create: `crates/ossgalley-web/templates/file_detail.html`
- Create: `crates/ossgalley-web/templates/search.html`
- Create: `crates/ossgalley-web/templates/timeline.html`
- Create: `crates/ossgalley-web/templates/tags.html`
- Create: `crates/ossgalley-web/templates/stats.html`

- [ ] **Step 1: Write browse.html**

```html
{% extends "layout.html" %}
{% block title %}Browse - ossgalley{% endblock %}
{% block content %}
<h1>Browse</h1>
<div class="breadcrumb">
    <a href="/browse">/</a>
    {% for part in breadcrumbs %}
    / <a href="/browse?path={{ part.path }}">{{ part.name }}</a>
    {% endfor %}
</div>
<table>
    <thead>
        <tr>
            <th>Name</th>
            <th>Size</th>
            <th>Type</th>
            <th>Modified</th>
        </tr>
    </thead>
    <tbody>
        {% for entry in entries %}
        <tr>
            <td>
                {% if entry.is_directory %}
                <a href="/browse?path={{ entry.path }}">📁 {{ entry.name }}</a>
                {% else %}
                <a href="/files/{{ entry.path }}">📄 {{ entry.name }}</a>
                {% endif %}
            </td>
            <td>{{ entry.size }}</td>
            <td>{{ entry.file_type }}</td>
            <td>{{ entry.last_modified }}</td>
        </tr>
        {% endfor %}
    </tbody>
</table>
{% endblock %}
```

- [ ] **Step 2: Write gallery.html**

```html
{% extends "layout.html" %}
{% block title %}Gallery - ossgalley{% endblock %}
{% block content %}
<h1>Gallery</h1>
<div class="gallery-grid">
    {% for item in items %}
    <div class="gallery-item">
        <a href="/files/{{ item.key }}">
            <img class="thumbnail" src="/thumbnails/{{ item.key }}" loading="lazy" alt="{{ item.name }}">
        </a>
        <div>{{ item.name }}</div>
    </div>
    {% endfor %}
</div>
{% endblock %}
```

- [ ] **Step 3: Write file_detail.html**

```html
{% extends "layout.html" %}
{% block title %}{{ file.key }} - ossgalley{% endblock %}
{% block content %}
<h1>{{ file.key }}</h1>
<table>
    <tr><th>Size</th><td>{{ file.size }}</td></tr>
    <tr><th>Type</th><td>{{ file.file_type }}</td></tr>
    <tr><th>Modified</th><td>{{ file.last_modified }}</td></tr>
    <tr><th>ETag</th><td>{{ file.etag }}</td></tr>
</table>
{% if metadata %}
<h2>Metadata</h2>
<table>
    {% for item in metadata %}
    <tr><th>{{ item.key }}</th><td>{{ item.value }}</td></tr>
    {% endfor %}
</table>
{% endif %}
<a href="/download/{{ file.key }}">Download</a>
{% endblock %}
```

- [ ] **Step 4: Write search.html**

```html
{% extends "layout.html" %}
{% block title %}Search - ossgalley{% endblock %}
{% block content %}
<h1>Search</h1>
<input type="search" name="q" placeholder="Search files..." hx-get="/search" hx-trigger="keyup changed delay:500ms" hx-target="#results">
<div id="results">
    {% for file in results %}
    <div><a href="/files/{{ file.key }}">{{ file.key }}</a></div>
    {% endfor %}
</div>
{% endblock %}
```

- [ ] **Step 5: Write timeline.html**

```html
{% extends "layout.html" %}
{% block title %}Timeline - ossgalley{% endblock %}
{% block content %}
<h1>Timeline</h1>
{% for entry in timeline %}
<h2>{{ entry.date }}</h2>
<div class="gallery-grid">
    {% for file in entry.files %}
    <div><a href="/files/{{ file.key }}">{{ file.key }}</a></div>
    {% endfor %}
</div>
{% endfor %}
{% endblock %}
```

- [ ] **Step 6: Write tags.html**

```html
{% extends "layout.html" %}
{% block title %}Tags - ossgalley{% endblock %}
{% block content %}
<h1>Tags</h1>
<ul>
    {% for tag in tags %}
    <li><a href="/search?tag={{ tag.tag_name }}">{{ tag.tag_name }}</a></li>
    {% endfor %}
</ul>
{% endblock %}
```

- [ ] **Step 7: Write stats.html**

```html
{% extends "layout.html" %}
{% block title %}Stats - ossgalley{% endblock %}
{% block content %}
<h1>Statistics</h1>
<table>
    <tr><th>Total files</th><td>{{ stats.total_files }}</td></tr>
    <tr><th>Total size</th><td>{{ stats.total_size }}</td></tr>
</table>
<h2>By Category</h2>
<table>
    {% for cat, count in stats.by_category %}
    <tr><td>{{ cat }}</td><td>{{ count }}</td></tr>
    {% endfor %}
</table>
{% endblock %}
```

---

### Task 9: Verify the build

**Files:**
- N/A

- [ ] **Step 1: Run cargo build**

Run: `cd /Users/macbook/Project/ossgalley && cargo build --workspace`
Expected: Build succeeds without errors

- [ ] **Step 2: Run cargo clippy**

Run: `cd /Users/macbook/Project/ossgalley && cargo clippy --workspace -- -D warnings`
Expected: No warnings or errors

---

## Self-Review

**1. Spec coverage:** All requirements covered - main.rs, state.rs, router.rs, handlers, templates, Cargo.toml update, lib.rs update.

**2. Placeholder scan:** No placeholders - all code is provided.

**3. Type consistency:** All types, method signatures match across tasks.
