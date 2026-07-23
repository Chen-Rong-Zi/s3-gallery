# CLI E2E Test Design

## Goal

Add end-to-end tests for CLI commands (`scan`, `view`, `db`) that exercise the full data flow against a real MinIO instance, covering the scenarios where bugs have been found (host_id mismatch, directory listing, etc.).

## Approach

Use library API calls (not CLI subprocess) against a real MinIO instance. Follow the existing pattern in `tests/e2e_test.rs`.

## Test Scenarios

| # | Test | Covers | Bug scenario |
|---|------|--------|-------------|
| 1 | `e2e_view_tree` | scan + view tree | host_id mismatch, incomplete tree |
| 2 | `e2e_view_ls` | scan + view ls | host_id filtering, subdirectory listing |
| 3 | `e2e_view_stat` | scan + view stat | file count/size, type classification |
| 4 | `e2e_view_search` | scan + view search | search filtered by host_id |
| 5 | `e2e_view_duplicates` | scan + view duplicates | duplicate detection |
| 6 | `e2e_view_timeline` | scan + view timeline | timeline date grouping |
| 7 | `e2e_db_push_pull` | scan + db push + db pull | DB upload/download data consistency |
| 8 | `e2e_db_lock` | db lock + db unlock | lock state management |

## Test Data

Upload directory structure with unique prefix per test run:

```
e2e-cli-{uuid}/
  photos/
    2024/
      vacation.jpg       (image/jpeg, 10KB)
      party.mp4          (video/mp4, 100KB)
    2023/
      old-photo.jpg      (image/jpeg, 5KB, same content as vacation.jpg → duplicate)
  docs/
    readme.txt           (text/plain, 1KB)
```

## Assertions

- `view tree`: directory structure matches expected
- `view ls photos/2024`: 2 files listed
- `view stat`: total_files=4, correct type distribution
- `view search vacation`: finds `photos/2024/vacation.jpg`
- `view duplicates`: 1 duplicate group detected
- `view timeline`: entries grouped by date correctly
- `db push`: DB uploaded to S3 successfully
- `db pull`: downloaded DB data matches local
- `db lock/unlock`: lock acquired/released correctly

## Implementation

- New file: `tests/e2e_cli_test.rs`
- Helpers: reuse `env_or()`, `setup_e2e_db()`, `ensure_bucket()` from existing e2e tests
- All tests: `#[ignore]` (run on demand)
- Use `uuid::Uuid::new_v4()` for unique test prefixes
- Each test: upload → scan → verify → cleanup