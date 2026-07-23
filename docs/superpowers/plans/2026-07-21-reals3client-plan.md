# RealS3Client + CLI Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move `RealS3Client` from E2E tests into the core crate so the CLI can connect to real MinIO/S3.

**Architecture:** Extract the `RealS3Client` struct from `tests/e2e_test.rs` into `crates/ossgalley-core/src/s3/real.rs`, making it accept `OssConfig` for construction. Update the CLI's `create_s3_client` to use it. Update E2E tests to use the new canonical implementation.

**Tech Stack:** `aws-sdk-s3 1.138.1`, `ossgalley-core`, `ossgalley-cli`

---

### Task 1: Move RealS3Client into core crate

**Files:**
- Create: `crates/ossgalley-core/src/s3/real.rs`
- Modify: `crates/ossgalley-core/src/s3/mod.rs` — add `pub mod real;`
- Modify: `tests/e2e_test.rs` — remove local `RealS3Client` definition, import from core crate

The `RealS3Client` should:
- Be a `pub struct` with `pub` constructor `from_config(config: &OssConfig) -> Self`
- Implement all 8 `S3Client` trait methods (same as current E2E version)
- Store the `aws_sdk_s3::Client` and `BucketName`
- Use `force_path_style(true)` for MinIO compatibility
- Use `ProvideErrorMetadata` for error code checking in `object_exists` and `put_object_if_none_match`

**Key design decision:** The constructor takes `OssConfig` (which already has validated bucket, endpoint, region, access_key, secret_key) instead of environment variables. This is consistent with the existing config pattern in the codebase.

**Changes to `e2e_test.rs`:**
- Remove the local `RealS3Client` struct definition (lines 53-82)
- Remove the `impl S3Client for RealS3Client` block (lines 84-289)
- Replace `RealS3Client::from_env()` calls with `RealS3Client::from_config()` using `OssConfig`
- Keep the `ensure_bucket` helper and `setup_e2e_db` helper

- [ ] **Step 1: Create `crates/ossgalley-core/src/s3/real.rs`** with the full implementation
- [ ] **Step 2: Add `pub mod real;` to `s3/mod.rs`**
- [ ] **Step 3: Update `tests/e2e_test.rs`** to use the new `RealS3Client` from core
- [ ] **Step 4: Run tests** (`cargo test --workspace`)
- [ ] **Step 5: Commit**

---

### Task 2: Update CLI to use RealS3Client

**Files:**
- Modify: `crates/ossgalley-cli/src/cmd_scan.rs` — update `create_s3_client()` to use `RealS3Client`
- Potentially modify: other command files that use `create_s3_client` or S3

The `create_s3_client` function in `cmd_scan.rs` should:
- Build `OssConfig::validate()` from the CLI's endpoint, access_key, secret_key, region, bucket, and concurrency
- Create `RealS3Client::from_config(&oss_config)`
- Wrap in `Arc` and return as `Arc<dyn S3Client>`

- [ ] **Step 1: Update `create_s3_client` in `cmd_scan.rs`** to use `RealS3Client`
- [ ] **Step 2: Verify compilation** (`cargo check -p ossgalley-cli`)
- [ ] **Step 3: Run tests** (`cargo test --workspace`)
- [ ] **Step 4: Commit**

---

### Verification

After both tasks, verify the CLI works with MinIO:

```bash
# Start MinIO if not running
docker run -d -p 9000:9000 -p 9001:9001 \
  -e MINIO_ROOT_USER=s3oss \
  -e MINIO_ROOT_PASSWORD=s3oss1234 \
  quay.io/minio/minio server /data --console-address ":9001"

# Create bucket
mc alias set local http://localhost:9000 s3oss s3oss1234
mc mb local/rongzi-bucket

# Run scan with correct syntax
cargo run -p ossgalley-cli -- \
  -b rongzi-bucket \
  -k s3oss \
  -s s3oss1234 \
  -e http://localhost:9000 \
  scan my-camera
```