# Scan 子命令重构设计

**Goal:** 将当前语义模糊的 `scan` 命令拆分为三个语义明确的子命令 `init`、`update`、`sync`，彻底区分全新扫描、增量更新、远程同步三种场景。

**Architecture:** 保持底层扫描核心不变，在 CLI 层拆分子命令 + 参数校验 + 前置条件检查。每个子命令有明确的前提条件，语义互斥，共享全部扫描参数。

**Tech Stack:** Rust, clap (子命令嵌套), s3-gallery-core (扫描核心 + DB 状态)

---

## 1. CLI 结构

```
# 当前（语义模糊）
s3-gallery scan --bucket X --with-thumbnails

# 目标（语义明确）
s3-gallery scan init   --bucket X --with-thumbnails   # 全新扫描
s3-gallery scan update --bucket X --with-thumbnails   # 增量更新
s3-gallery scan sync   --bucket X --with-thumbnails   # 远程同步 + 增量更新
```

### 子命令职责

| 子命令 | 前提条件 | 行为 |
|--------|----------|------|
| `init` | 无（本地 DB 已存在时需 `--confirm`） | 扫描 OSS 创建本地 DB，推送到远端 |
| `update` | 本地必须有 DB | 扫描 OSS，diff 比对，增量更新本地 DB，推送到远端 |
| `sync` | 无 | 先从 OSS 拉取远端 DB，再执行 `update` 逻辑 |

### 共享 flag

所有子命令共享当前 scan 的 flag 集合：
- `--bucket` / `-b`（必选）
- `--prefix`（可选，限定扫描范围）
- `--no-metadata`（跳过元数据提取）
- `--with-thumbnails`（生成缩略图）
- `--concurrency`（并发数，默认 10）
- `--force`（跳过锁检查）
- `--incremental`（断点续传，仅 update/sync 有意义，init 忽略此 flag）

### init 独有 flag

- `--confirm`（当本地 DB 已存在时，确认覆盖）

---

## 2. 交互流程

### init — 首次使用

```
$ s3-gallery scan init -b rongzi-bucket
  → 检查本地 DB 不存在
  → 扫描 OSS，创建本地 DB
  → 自动 push 到远端 OSS
```

### init — 重新扫描

```
$ s3-gallery scan init -b rongzi-bucket
  → 错误: 本地 DB 已存在，使用 --confirm 覆盖
$ s3-gallery scan init -b rongzi-bucket --confirm
  → 删除/覆盖本地 DB
  → 重新扫描 → push 到远端
```

### update — 日常增量更新

```
$ s3-gallery scan update -b rongzi-bucket
  → 检查本地 DB 存在
  → 扫描 OSS，diff 比对，更新本地 DB
  → 自动 push 到远端
```

### sync — 多机器协作

```
$ s3-gallery scan sync -b rongzi-bucket
  → 从 OSS 拉取远端 DB（覆盖本地）
  → 扫描 OSS，diff 比对，更新本地 DB
  → 自动 push 到远端
```

---

## 3. 错误处理

| 场景 | 行为 |
|------|------|
| `init` 时本地 DB 已存在，无 `--confirm` | 报错退出，提示使用 `--confirm` |
| `update` 时本地 DB 不存在 | 报错退出，提示使用 `scan init` |
| `sync` 时远端 DB 不存在 | 回退到 `init` 行为（全新扫描） |
| `sync` 时远端 DB 拉取失败（网络等） | 报错退出，不修改本地 DB |
| 任意子命令锁检查失败 | 报错退出，提示使用 `--force` |

---

## 4. 实现要点

### 代码变更

**`crates/s3-gallery-cli/src/cli.rs`**
- `Commands::Scan` 从扁平结构改为嵌套子命令结构
- `ScanCommand` 枚举：`Init`, `Update`, `Sync`
- 每个子命令持有相同的 flag 集合

**`crates/s3-gallery-cli/src/cmd_scan.rs`**
- `run_scan` 拆分为 `run_init`, `run_update`, `run_sync` 三个入口函数
- 公共逻辑提取为内部辅助函数
- `run_init` 检查本地 DB 存在时，无 `--confirm` 则报错

**`crates/s3-gallery-cli/src/main.rs`**
- 匹配 `Commands::Scan` 时再匹配子命令

### 扫描核心不变

`crates/s3-gallery-core/src/scan/scanner.rs` 中的 `run_scan` 函数、`ScanConfig`、`ScanResult` 保持不变。拆分的只是 CLI 层的控制流。

### 与现有 `db` 命令的关系

`db pull` / `db push` 作为底层命令保留，供高级用户直接操作 DB 文件。`scan sync` 内部调用 `db pull` 的等价逻辑，两者不冲突。

---

## 5. 向后兼容

- 旧 `scan` 命令被移除，不再支持 `s3-gallery scan --bucket X`（无子命令）的写法
- 迁移路径：`s3-gallery scan` → `s3-gallery scan init/update/sync`
- 所有现有 flag 名称不变，只是位置变为子命令之后