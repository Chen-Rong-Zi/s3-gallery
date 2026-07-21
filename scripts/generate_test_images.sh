#!/usr/bin/env bash
set -euo pipefail

# generate_test_images.sh
# Creates small test JPEG images, text files, and PDF placeholders
# for seeding the MinIO dev bucket.
# Uses ImageMagick (convert) for image generation.

OUTPUT_DIR="${1:-./test-files}"
mkdir -p "$OUTPUT_DIR"/{photos/2024,photos/2023,videos,docs}

echo "==> Generating test images in $OUTPUT_DIR"

# --- Photos ---

# 1. A landscape photo (1920x1080, ~100KB)
convert -size 1920x1080 \
  gradient:'#87CEEB'-'#228B22' \
  -fill white -gravity center \
  -pointsize 48 -annotate +0-80 'OSS Galley' \
  -pointsize 24 -annotate +0+20 'Landscape Test 2024' \
  -quality 85 \
  "$OUTPUT_DIR/photos/2024/landscape.jpg"

# 2. A portrait photo (1080x1920)
convert -size 1080x1920 \
  gradient:'#FFB6C1'-'#8B008B' \
  -fill white -gravity center \
  -pointsize 48 -annotate +0-80 'Sunset' \
  -pointsize 24 -annotate +0+20 'Portrait Test' \
  -quality 85 \
  "$OUTPUT_DIR/photos/2024/portrait.jpg"

# 3. A square photo (1024x1024)
convert -size 1024x1024 \
  radial-gradient:'#FFD700'-'#FF4500' \
  -fill white -gravity center \
  -pointsize 48 -annotate +0-80 'Sunburst' \
  -pointsize 24 -annotate +0+20 'Square 1024' \
  -quality 85 \
  "$OUTPUT_DIR/photos/2024/square.jpg"

# 4. A small thumbnail (320x240)
convert -size 320x240 \
  gradient:'#E0E0E0'-'#808080' \
  -fill '#333' -gravity center \
  -pointsize 20 -annotate +0+0 'Thumb' \
  -quality 80 \
  "$OUTPUT_DIR/photos/2024/thumbnail.jpg"

# 5. A photo from 2023 (640x480)
convert -size 640x480 \
  gradient:'#98FB98'-'#2E8B57' \
  -fill white -gravity center \
  -pointsize 36 -annotate +0-40 'OSS Galley' \
  -pointsize 20 -annotate +0+20 '2023 Archive' \
  -quality 85 \
  "$OUTPUT_DIR/photos/2023/summer.jpg"

# 6. Another 2023 photo (800x600)
convert -size 800x600 \
  gradient:'#FFDAB9'-'#CD853F' \
  -fill white -gravity center \
  -pointsize 36 -annotate +0-40 'Autumn' \
  -pointsize 20 -annotate +0+20 '2023 Memories' \
  -quality 85 \
  "$OUTPUT_DIR/photos/2023/autumn.jpg"

# --- Videos (poster frames) ---

convert -size 640x360 \
  gradient:'#2F4F4F'-'#000000' \
  -fill white -gravity center \
  -pointsize 36 -annotate +0-20 'Sample Video' \
  -pointsize 18 -annotate +0+20 '[ Play ]' \
  -quality 85 \
  "$OUTPUT_DIR/videos/sample-poster.jpg"

convert -size 640x360 \
  gradient:'#8B0000'-'#000000' \
  -fill white -gravity center \
  -pointsize 36 -annotate +0-20 'Demo Reel' \
  -pointsize 18 -annotate +0+20 '[ Play ]' \
  -quality 85 \
  "$OUTPUT_DIR/videos/demo-poster.jpg"

# --- Documents ---

# Text file
cat > "$OUTPUT_DIR/docs/readme.txt" <<-'EOF'
OSS Galley - Media Gallery Application
=======================================

Welcome to the OSS Galley development environment.

This is a sample text file for testing document uploads.

Features:
- S3-compatible storage via MinIO
- Media categorization and search
- Responsive web interface

EOF

# PDF placeholder (simple text file renamed — real PDFs would be created by
# an external tool, but for MinIO seed testing this is sufficient)
cat > "$OUTPUT_DIR/docs/sample-report.txt" <<-'EOF'
OSS Galley - Sample Report
==========================

This is a placeholder document that simulates a PDF report.
In production, actual PDF files would be uploaded here.

Date: 2024-01-15
Author: Development Team

EOF

# Another document
cat > "$OUTPUT_DIR/docs/notes.txt" <<-'EOF'
Development Notes
=================

1. Set up MinIO dev environment
2. Configure bucket policies
3. Implement upload API
4. Build gallery frontend
5. Write integration tests

EOF

# Summary
echo ""
echo "Generated files:"
find "$OUTPUT_DIR" -type f | sort | while read -r f; do
  size=$(wc -c < "$f" | tr -d ' ')
  echo "  $f  (${size} bytes)"
done
echo ""
echo "Done. Total files: $(find "$OUTPUT_DIR" -type f | wc -l | tr -d ' ')"