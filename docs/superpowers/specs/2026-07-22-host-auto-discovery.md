# Host Auto-Discovery Design

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:writing-plans to create the implementation plan, then superpowers:subagent-driven-development or superpowers:executing-plans to implement task-by-task.

**Goal:** `scan` auto-discovers hosts by checking for `.s3-gallery/host.config.json` in the scan scope and its immediate subdirectories. A "host" is a directory with an identity config, not a CLI argument.

**Architecture:** `--prefix` specifies the scan scope. The scanner checks if the scope root has `.s3-gallery` → it's a host. If not, it checks each first-level subdirectory → each with config is a separate host. Directories without config are scanned normally without host classification.

**Tech Stack:** Rust, OSS/S3

---

## Background

Previously, `scan` required a `host` CLI argument that served as both the identity and the directory prefix. This was confusing. The new design:
- `host` is **not** a CLI argument
- A "host" is a directory that has a `.s3-gallery/host.config.json` file
- `--prefix` only specifies the scan scope (which directory to scan)
- Hosts are auto-discovered during scan

## Host Discovery Rules

When scanning a bucket with a given `--prefix`:

```
1. Check the scan scope root for .s3-gallery/host.config.json

   → Found: The entire scan scope is ONE host.
     Store files with host_id/name from config.
     (Do NOT check first-level subdirectories.)

2. NOT found: Check each first-level subdirectory for .s3-gallery/host.config.json

   → Found in subdirectory: That subdirectory is a host.
     Scan its files with host_id/name from config.

   → NOT found: Scan normally, but without host classification.
```

## Examples

### Example 1: Root has config → single host

```
rongzi-bucket/
├── .s3-gallery/host.config.json  ← 根目录有 config
├── photos/2023/...               ← 整个 bucket 是一个 host
└── videos/...
```

```bash
s3-gallery scan --bucket rongzi-bucket
```
→ 根目录有 `.s3-gallery` → 整个 bucket 是一个 host
→ 扫描所有文件，用 config 的 host_id/name

### Example 2: Root has no config, subdirs have config → multiple hosts

```
rongzi-bucket/
├── photos/
│   ├── .s3-gallery/host.config.json  ← 一级子目录有 config
│   └── 2023/...
├── videos/
│   ├── .s3-gallery/host.config.json  ← 一级子目录有 config
│   └── ...
└── docs/
    └── report.pdf                     ← 没有 config → 正常扫描，无 host
```

```bash
s3-gallery scan --bucket rongzi-bucket
```
→ 根目录没有 `.s3-gallery`
→ 检查一级子目录：`photos/` 有 config → 是 host，扫描
→ 检查一级子目录：`videos/` 有 config → 是 host，扫描
→ 检查一级子目录：`docs/` 没有 config → 正常扫描，无 host 分类

### Example 3: Scan with prefix

```bash
s3-gallery scan --bucket rongzi-bucket --prefix photos/
```
→ 扫描范围是 `photos/`
→ 检查 `photos/.s3-gallery/host.config.json`
→ 有 → photos 是一个 host
→ 没有 → 检查 `photos/` 的一级子目录（如 `photos/2023/`）

## CLI Changes

### `init` command

```bash
s3-gallery init --bucket rongzi-bucket --prefix photos --name "我的相册"
```

Creates `host.config.json` at `{prefix}/.s3-gallery/host.config.json` in the bucket.

### `scan` command

```bash
# Scan entire bucket (auto-discover hosts)
s3-gallery scan --bucket rongzi-bucket

# Scan specific directory
s3-gallery scan --bucket rongzi-bucket --prefix photos/
```

No `host` argument. The host identity comes from the config file.

## Data Flow

### Scan flow

```
scan --bucket X --prefix P
  → Create S3 client
  → Try to read P/.s3-gallery/host.config.json
  → Found → scan P as a single host, use config host_id/name
  → Not found → list first-level subdirs of P
    → For each subdir D:
      → Try to read D/.s3-gallery/host.config.json
      → Found → scan D as a host, use config host_id/name
      → Not found → scan D normally, no host classification
  → Save all files to DB
  → Save host configs to host_config table
```

## Files to Modify

- `crates/s3-gallery-cli/src/cmd_scan.rs` — rewrite scan logic for auto-discovery
- `crates/s3-gallery-cli/src/cmd_init.rs` — update init to use --prefix
- `crates/s3-gallery-cli/src/cli.rs` — update init command signature
- `crates/s3-gallery-cli/src/main.rs` — update match arms