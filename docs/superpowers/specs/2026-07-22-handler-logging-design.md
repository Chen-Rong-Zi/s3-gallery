# Handler Logging Design

## Goal

Add application-level `tracing` logging to every web handler in the `serve` command, complementing the existing HTTP-level `TraceLayer::new_for_http()` middleware.

## Current State

- 13 handlers, zero `tracing` calls
- `TraceLayer::new_for_http()` logs HTTP method, URI, status code, and latency
- `tracing` crate already a dependency

## Design

### Logging Pattern

Every handler follows three logging points:

1. **Entry** — `tracing::info!` with handler name and key parameters
2. **Success** — `tracing::info!` with resulting data volume
3. **Error** — `tracing::error!` with handler name, full error, and structured fields, **before** returning the error response

### Error Logging Detail

All error paths log the complete error using structured fields before returning:

```rust
tracing::error!(
    handler = "browse",
    path = %path,
    error = %e,
    "failed to list directory"
);
```

Structured fields per handler:

| Handler | Error fields |
|---------|-------------|
| `browse` | `path, sort_by, error` |
| `gallery` | `page, error` |
| `search` | `query, tag, error` |
| `file_detail` | `key, error` |
| `download` | `key, error` |
| `thumbnail` | `key, error` |
| `duplicates` | `error` |
| `stats` | `error` |
| `timeline` | `error` |
| `tags` | `error` |

### Per-Handler Logging

| Handler | Entry | Success | Error |
|---------|-------|---------|-------|
| `dashboard` | `"dashboard: rendering"` | — | — |
| `browse` | path, sort_by, sort_order | `"{} entries listed"` | `"failed to list directory"` |
| `gallery` | page | `"{} gallery items, has_more={}"` | `"failed to query gallery"` |
| `search` | query_or_tag | `"{} search results"` | `"search failed"` |
| `timeline` | — | `"{} timeline entries"` | `"failed to get timeline"` |
| `tags` | — | `"{} tags listed"` | `"failed to list tags"` |
| `file_detail` | key | `"metadata in {} namespaces"` | `"file not found"` / `"db error"` |
| `duplicates` | — | `"{} duplicate groups"` | `"failed to find duplicates"` |
| `stats` | — | `"stats: {} files, {} size"` | `"failed to get stats"` |
| `settings` | — | `"settings rendered"` | — |
| `thumbnail` | key | `"thumbnail served (cache hit/miss)"` | `"thumbnail generation failed"` |
| `download` | key | `"downloaded {} bytes"` | `"file not found"` / `"S3 fetch failed"` |

## Implementation

No new dependencies. Add `tracing::info!` and `tracing::error!` calls directly inside each handler function body. 13 files modified, each adding 1-4 logging lines.