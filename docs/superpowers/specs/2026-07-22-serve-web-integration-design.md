# Serve-Web Integration Design

**Date:** 2026-07-22
**Status:** Draft

## Summary

Integrate `ossgallery-web` (templates/design) and `ossgalley-cli serve` (server entry point)
so that `cargo run -p ossgalley-cli serve <host>` starts a real axum web server with
CLI-configured S3 client and database path.

## Architecture

```
ossgalley-cli (bin)
  │ 依赖: ossgallery-web, ossgalley-core, axum, minijinja, serde, tower-http
  │
  ▼
ossgallery-web (lib)
  │ 依赖: ossgalley-core, minijinja
  │
  ▼
ossgalley-core (lib)
```

### Dependency direction

- `ossgalley-core` — pure business logic, no web dependencies. Unchanged.
- `ossgallery-web` — templates and design assets only. Provides a `register_templates()`
  function. No axum/tokio/tower-http dependencies.
- `ossgalley-cli` — all web handler logic, router, AppState, server startup. Uses
  `ossgallery_web::register_templates()` to load templates.

## Component changes

### ossgallery-web (lib)

**Cargo.toml** — remove axum, tokio, tower-http, serde_json. Keep only:
- `ossgalley-core`
- `minijinja`

**src/lib.rs** (new) — single public function:
```rust
pub fn register_templates(env: &mut Environment<'static>) -> Vec<String>;
```
Registers all `templates/*.html` via `include_str!`. Returns names of failed templates.

**src/main.rs** — deleted (no dev entry point).

**src/state.rs** — deleted (moved to CLI).

**src/router.rs** — deleted (moved to CLI).

**src/handlers/** — deleted (moved to CLI).

**Cleanup list after migration:**
- `src/main.rs` — delete
- `src/state.rs` — delete
- `src/router.rs` — delete
- `src/handlers/` — delete entire directory
- `Cargo.toml` — remove axum, tokio, tower-http, serde_json, tracing-subscriber
- `src/lib.rs` — **new file** (replaces main.rs as crate root)

**templates/** — kept as-is. All HTML template files remain in `ossgallery-web`.

### ossgalley-cli (bin)

**Cargo.toml** — add dependencies:
- `axum = "0.7"`
- `minijinja = "2"`
- `tower-http = { version = "0.5", features = ["cors", "trace"] }`
- `serde = { version = "1", features = ["derive"] }`
- `serde_json = "1.0"`

**src/web/mod.rs** (new) — module declaration, re-exports.

**src/web/state.rs** (new) — AppState struct (moved from ossgallery-web):
```rust
pub struct AppState {
    pub templates: Arc<minijinja::Environment<'static>>,
    pub local_view: Arc<LocalView>,
    pub remote_view: Arc<RemoteView>,
    pub bucket: BucketName,
}
```

**src/web/router.rs** (new) — `create_router(app_state: AppState) -> Router` (moved from
ossgallery-web, unchanged).

**src/web/handlers/** (new) — all 13 handler modules (moved from ossgallery-web, unchanged
in logic).

**src/cmd_serve.rs** (rewritten) — replaces `TODO: Start web server` with real server
startup:
1. Create S3 client (unchanged)
2. Check/download DB (unchanged)
3. Create pool, run migrations (unchanged)
4. Create LocalView, RemoteView (unchanged)
5. Register templates from `ossgallery_web::register_templates()`
6. Build AppState
7. Create router, bind TCP listener, start axum serve

### Migration approach

All handler code is moved **as-is** — no refactoring, no logic changes. Only the following
are updated:

1. **Import paths** — `crate::state::AppState` → `crate::web::state::AppState` (or
   `super::state::AppState` from within `web/`). `use crate::router::create_router` →
   `use crate::web::router::create_router`.
2. **`render_template` helper** — The `browse.rs` handler defines a local helper that
   takes `&AppState`. This moves with the handler and automatically references the
   new `AppState` location.
3. **`AppState::new()`** — removed. Template loading moves to `cmd_serve.rs`; `AppState`
   is constructed directly with struct literal syntax.

### Template loading

Templates are loaded in `cmd_serve.rs` instead of `AppState::new()`:
```rust
let mut env = minijinja::Environment::new();
ossgallery_web::register_templates(&mut env);
let templates = Arc::new(env);
```

This decouples template loading from `AppState`, keeping `ossgallery-web` as a thin
template provider.

## Testing

### ossgallery-web
- Unit test: register all templates, verify no errors.
- Unit test: render key templates with minimal context, verify no panic.

### ossgalley-cli web handlers
- Inject `MockS3Client` + in-memory SQLite DB for handler unit tests.
- Test handler functions directly (not via axum integration tests).

### cmd_serve.rs
- Test that missing DB returns appropriate error.
- Test that valid config starts server (verify log output).

## Non-goals

- No changes to `ossgalley-core`.
- No changes to existing CLI commands (scan, view, db, init).
- No performance optimization — only structural integration.
- No new frontend features — only moving existing code.
- No restructuring of handler logic — handlers are moved with their existing error handling
  and response patterns intact.