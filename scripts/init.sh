#!/usr/bin/env bash
# scripts/init.sh
# Starts MinIO, waits for it to be ready, seeds test data, and prints connection info.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "================================================"
echo "  OSS Galley — MinIO Dev Environment"
echo "================================================"
echo ""

# --- Step 1: Start MinIO ---
echo "==> Starting MinIO with docker compose..."
cd "$PROJECT_DIR"
docker compose up -d

# --- Step 2: Wait for MinIO to be ready ---
echo "==> Waiting for MinIO to be ready..."
RETRIES=30
for i in $(seq 1 $RETRIES); do
    if curl -sf http://localhost:9000/minio/health/live > /dev/null 2>&1; then
        echo "    MinIO is ready! (attempt ${i})"
        break
    fi
    if [ "$i" -eq "$RETRIES" ]; then
        echo "    ERROR: MinIO did not become ready after ${RETRIES} attempts."
        echo "    Check container logs: docker compose logs minio"
        exit 1
    fi
    sleep 1
done

# --- Step 3: Run seed script ---
echo ""
echo "==> Seeding test data..."
bash "${SCRIPT_DIR}/seed.sh"

# --- Step 4: Print connection info ---
echo ""
echo "================================================"
echo "  MinIO is running!"
echo "================================================"
echo ""
echo "  S3 API Endpoint:  http://localhost:9000"
echo "  Web Console:      http://localhost:9001"
echo "  Access Key:       s3oss"
echo "  Secret Key:       s3oss1234"
echo "  Bucket:           ossgalley"
echo ""
echo "  To stop MinIO:    docker compose down"
echo "  To restart:       docker compose restart"
echo "  To re-seed:       bash scripts/seed.sh"
echo ""
echo "================================================"