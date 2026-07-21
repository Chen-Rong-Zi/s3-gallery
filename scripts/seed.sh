#!/usr/bin/env bash
# scripts/seed.sh
# Seeds the MinIO "ossgalley" bucket with test data.
# Idempotent — safe to run multiple times.
set -euo pipefail

# --- Configuration ---
MINIO_ALIAS="ossgalley-local"
MINIO_ENDPOINT="http://localhost:9000"
MINIO_ACCESS_KEY="s3oss"
MINIO_SECRET_KEY="s3oss1234"
BUCKET="ossgalley"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TEST_FILES_DIR="${SCRIPT_DIR}/test-files"

# --- Step 1: Configure mc alias ---
echo "==> Configuring MinIO client alias..."
mc alias set "$MINIO_ALIAS" "$MINIO_ENDPOINT" "$MINIO_ACCESS_KEY" "$MINIO_SECRET_KEY"

# --- Step 2: Create bucket (idempotent) ---
echo "==> Creating bucket '${BUCKET}' (if not exists)..."
mc mb "${MINIO_ALIAS}/${BUCKET}" --ignore-existing 2>/dev/null || true

# --- Step 3: Generate test files if not already present ---
if [ ! -d "$TEST_FILES_DIR" ] || [ -z "$(ls -A "$TEST_FILES_DIR" 2>/dev/null)" ]; then
    echo "==> Generating test files..."
    bash "${SCRIPT_DIR}/generate_test_images.sh" "$TEST_FILES_DIR"
else
    echo "==> Test files already exist at ${TEST_FILES_DIR}, skipping generation."
fi

# --- Step 4: Upload directory structure ---
echo "==> Uploading files to bucket..."

# Upload photos/2024/
if [ -d "$TEST_FILES_DIR/photos/2024" ]; then
    echo "  -> uploading photos/2024/..."
    mc cp --recursive "${TEST_FILES_DIR}/photos/2024/" "${MINIO_ALIAS}/${BUCKET}/photos/2024/"
fi

# Upload photos/2023/
if [ -d "$TEST_FILES_DIR/photos/2023" ]; then
    echo "  -> uploading photos/2023/..."
    mc cp --recursive "${TEST_FILES_DIR}/photos/2023/" "${MINIO_ALIAS}/${BUCKET}/photos/2023/"
fi

# Upload videos/
if [ -d "$TEST_FILES_DIR/videos" ]; then
    echo "  -> uploading videos/..."
    mc cp --recursive "${TEST_FILES_DIR}/videos/" "${MINIO_ALIAS}/${BUCKET}/videos/"
fi

# Upload docs/
if [ -d "$TEST_FILES_DIR/docs" ]; then
    echo "  -> uploading docs/..."
    mc cp --recursive "${TEST_FILES_DIR}/docs/" "${MINIO_ALIAS}/${BUCKET}/docs/"
fi

# --- Step 5: Verify ---
echo ""
echo "==> Verifying bucket contents..."
mc ls --recursive "${MINIO_ALIAS}/${BUCKET}/"

echo ""
echo "==> Seed complete!"
echo "    Bucket: ${BUCKET}"
echo "    Endpoint: ${MINIO_ENDPOINT}"
echo "    Console: http://localhost:9001"