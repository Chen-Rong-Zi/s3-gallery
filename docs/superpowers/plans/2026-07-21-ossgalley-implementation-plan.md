# ossgalley Implementation Plan

## Phase 1: Core Library Foundation (ossgalley-core)

### Task 1.1: Project Scaffolding
**Files:** Cargo.toml (workspace), crates/ossgalley-core/Cargo.toml, crates/ossgalley-core/src/lib.rs, crates/ossgalley-cli/Cargo.toml, crates/ossgalley-cli/src/lib.rs (skeleton), crates/ossgalley-web/Cargo.toml, crates/ossgalley-web/src/lib.rs (skeleton), .cargo/config.toml, rust-toolchain.toml, .gitignore
**Deps:** none
**AC:** cargo build --workspace succeeds, cargo clippy --workspace -- -D warnings succeeds, all deny attributes present

### Task 1.2: Error Types
**Files:** ossgalley-core/src/error.rs
**Deps:** 1.1
**AC:** All error variants implement Debug, Display, Error; Result<T> type alias available

### Task 1.3: Newtypes
**Files:** ossgalley-core/src/types.rs
**Deps:** 1.2
**AC:** All newtypes pass validation on construction, serde roundtrip, FileSize human-readable Display

### Task 1.4: Enums
**Files:** ossgalley-core/src/types.rs (add)
**Deps:** 1.2
**AC:** All enums support serde, Display, FromStr, Clone, Debug, PartialEq

### Task 1.5: S3Client Trait + MockS3Client
**Files:** ossgalley-core/src/s3/mod.rs, client.rs, mock.rs
**Deps:** 1.2, 1.3, 1.4
**AC:** S3Client trait is Send + Sync + 'static, MockS3Client implements all methods

### Task 1.6: OssConfig + HostIdentifier
**Files:** ossgalley-core/src/s3/config.rs
**Deps:** 1.2, 1.3, 1.5
**AC:** OssConfig validates all fields, HostIdentifier derives all .ossgallery/ paths

### Task 1.7: Database Schema
**Files:** ossgalley-core/src/db/mod.rs, schema.rs, pool.rs
**Deps:** 1.1, 1.2
**AC:** All 9 tables created, all 7 indexes created, WAL mode, foreign keys enforced

### Task 1.8: Database Models
**Files:** ossgalley-core/src/db/models.rs
**Deps:** 1.2, 1.3, 1.4, 1.7
**AC:** All models derive sqlx::FromRow, CRUD operations work, TypedFileEntry conversion

### Task 1.9: Classifier
**Files:** ossgalley-core/src/classify/mod.rs, classifier.rs
**Deps:** 1.2, 1.3, 1.4, 1.8
**AC:** All common media file extensions classified, case-insensitive, double extensions handled

### Task 1.10: Extractor Registry
**Files:** ossgalley-core/src/extractor/mod.rs, registry.rs, exif.rs, mp4.rs, audio.rs (stubs)
**Deps:** 1.2, 1.3, 1.4
**AC:** ExifExtractor extracts EXIF from JPEG, Registry finds matching extractors, bad data never panics

### Task 1.11: LockGuard
**Files:** ossgalley-core/src/s3/lock.rs
**Deps:** 1.2, 1.3, 1.5, 1.6
**AC:** Lock acquisition atomic, LockGuard consumed on release, #[must_use] present

### Task 1.12: ConcurrencyLimiter
**Files:** ossgalley-core/src/util/concurrency.rs
**Deps:** 1.1, 1.2
**AC:** ConcurrencyLimiter is Clone + Send + Sync, permits auto-released

### Task 1.13: ThumbnailCache
**Files:** ossgalley-core/src/thumbnail/mod.rs, generator.rs
**Deps:** 1.2, 1.3, 1.8
**AC:** Thumbnails generated as JPEG 320px, LRU eviction, never exceeds 110% of max

### Task 1.14: Scanner
**Files:** ossgalley-core/src/scan/mod.rs, scanner.rs, diff.rs
**Deps:** 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 1.9, 1.10, 1.11, 1.12, 1.13
**AC:** Full scan reads all objects, incremental scan uses start_after, ETag changes detected, deleted files soft-deleted

### Task 1.15: LocalView
**Files:** ossgalley-core/src/view/mod.rs, ls.rs, tree.rs, stat.rs, search.rs, timeline.rs, tags.rs, duplicates.rs, export.rs
**Deps:** 1.2, 1.3, 1.4, 1.7, 1.8
**AC:** LocalView has no S3Client field, all query methods work, directory listing simulates structure

### Task 1.16: RemoteView
**Files:** ossgalley-core/src/view/remote.rs
**Deps:** 1.2, 1.3, 1.5, 1.6, 1.8, 1.10, 1.13, 1.15
**AC:** RemoteView holds Arc<dyn S3Client>, fetch_* methods have doc warnings, lazy loading works

### Task 1.17: check_db_status + DbAction
**Files:** ossgalley-core/src/db/status.rs
**Deps:** 1.2, 1.5, 1.6, 1.7, 1.8, 1.11, 1.14
**AC:** check_db_status uses at most 1 HEAD request, decide_action exhaustive over DbStatus

### Task 1.18: ValidatedConfig
**Files:** ossgalley-core/src/config.rs
**Deps:** 1.2, 1.3, 1.4, 1.6
**AC:** All validation errors collected, priority chain correct, defaults match spec

## Phase 2: CLI (ossgalley-cli)

### Task 2.1: CLI Scaffolding
**Files:** ossgalley-cli/src/main.rs, cli.rs, lib.rs
**Deps:** 1.1, 1.2, 1.6, 1.18
**AC:** All subcommands defined, --help output correct, CLI parsing comprehensive

### Task 2.2: cmd_init
**Files:** ossgalley-cli/src/cmd_init.rs
**Deps:** 1.5, 1.6, 1.7, 1.8, 1.18, 2.1
**AC:** After init, .ossgallery/ exists with host.config.json and ossgallery.db

### Task 2.3: cmd_scan
**Files:** ossgalley-cli/src/cmd_scan.rs
**Deps:** 1.5, 1.6, 1.11, 1.12, 1.14, 1.18, 2.1
**AC:** Scan populates DB and uploads to OSS, lock acquired before scan, released after

### Task 2.4: cmd_view
**Files:** ossgalley-cli/src/cmd_view.rs
**Deps:** 1.5, 1.6, 1.15, 1.17, 1.18, 2.1
**AC:** All view subcommands produce correct output, DB downloaded if stale

### Task 2.5: cmd_db
**Files:** ossgalley-cli/src/cmd_db.rs
**Deps:** 1.5, 1.6, 1.11, 1.17, 1.18, 2.1
**AC:** All DB commands work, db unlock requires confirmation

### Task 2.6: cmd_serve
**Files:** ossgalley-cli/src/cmd_serve.rs
**Deps:** 1.5, 1.6, 1.15, 1.16, 1.17, 1.18, 2.1, 3.1
**AC:** Server starts on configured port, DB ensured before start

## Phase 3: Web (ossgalley-web)

### Task 3.1: Web Scaffolding
**Files:** ossgalley-web/src/main.rs, lib.rs, router.rs, state.rs
**Deps:** 1.1, 1.15, 1.16, 1.18, 2.1
**AC:** Server starts on configured port, all routes registered, templates loaded

### Task 3.2: Browse Handler + Template
**Files:** ossgalley-web/src/handlers/browse.rs, templates/browse.html, browse_table.html
**Deps:** 1.15, 3.1
**AC:** Browse page renders with directory listing, sorting via HTMX, breadcrumbs

### Task 3.3: Gallery Handler + Template
**Files:** ossgalley-web/src/handlers/gallery.rs, templates/gallery.html, gallery_items.html
**Deps:** 1.15, 1.16, 3.1
**AC:** Gallery renders with thumbnails, infinite scroll, lazy loading

### Task 3.4: File Detail Handler + Template
**Files:** ossgalley-web/src/handlers/files.rs, templates/file_detail.html
**Deps:** 1.15, 1.16, 3.1
**AC:** File detail shows all info, metadata grouped by namespace, thumbnail displayed

### Task 3.5: Search, Timeline, Tags, Stats, Duplicates Handlers
**Files:** Various handlers + templates
**Deps:** 1.15, 3.1
**AC:** All pages render with correct data

### Task 3.6: Download + Thumbnail Endpoints
**Files:** ossgalley-web/src/handlers/files.rs (add), thumbnails.rs
**Deps:** 1.15, 1.16, 3.1
**AC:** Thumbnails with Cache-Control, download with Content-Disposition

## Phase 4: CI + Infrastructure

### Task 4.1: CI Workflow
**Files:** .github/workflows/ci.yml
**Deps:** All Phase 1, 2, 3
**AC:** CI file valid YAML, all jobs match spec

### Task 4.2: Integration Tests
**Files:** tests/common/mod.rs, tests/integration/*, tests/e2e/*, tests/fuzz_targets/*
**Deps:** All Phase 1, 2, 3
**AC:** All integration tests pass, E2E tests pass with MinIO, fuzz targets compile

### Task 4.3: MinIO Dev Setup
**Files:** docker-compose.yml, scripts/*
**Deps:** none
**AC:** docker compose up starts MinIO, seed script creates test data