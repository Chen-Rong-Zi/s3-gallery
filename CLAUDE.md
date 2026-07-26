# s3-gallery 项目标准

> 提取自 `docs/superpowers/specs/2026-07-21-ossgalley-design.md` 中定义的原始标准。
> 这些是项目初始设计时确立的纪律，所有代码变更必须遵守。

---

## 一、架构核心原则

- **全异步架构**：所有 crate 统一使用 `tokio` 运行时，核心库对外暴露 async API
- **分层解耦**：`core` 不依赖任何展示层，CLI 和 Web 是薄包装
- **扫描者/消费者分离**：扫描者写 DB 并上传，消费者只读浏览（零 OSS 请求）
- **增量扫描**：基于 `start_after` 和 ETag 对比，避免全量 ListObjects

---

## 二、类型安全纪律

**原则：用类型系统将非法状态编码为编译错误，让运行时 bug 不可能发生。**

### 1. Newtype 模式
- 每个业务概念有独立类型（`BucketName`, `ObjectKey`, `Etag` 等），不同概念传反时编译错误
- 构造时验证合法性（bucket 名称长度、key 不能为空/不以 `/` 开头等）
- 函数签名使用 Newtype 而非原始 `String`/`i64`

### 2. 枚举替代字符串
- 所有业务分类使用枚举，不是 String
- 用户可扩展部分使用 `Custom(String)` 变体，不破坏枚举封闭性

### 3. 状态机模式
- 非法状态转换编译不通过（如无锁时释放锁、持锁时重复获取）

### 4. 枚举化错误类型
- 每个错误分支有明确类型，调用方必须处理所有分支

### 5. Result 类型表达业务语义
- 函数返回值用枚举表达多种结果，而非 `Option` 或 `bool`
- 扫描结果结构体所有字段必有值，无 Option

### 6. DB 查询数据返回类型化结构体
- 从 DB 读出的数据强制绑定到类型化结构体，不暴露原始 Row

---

## 三、副作用管理

**原则：所有副作用必须在类型签名中可见。不可能从类型签名中隐藏一个 IO 操作。**

### 1. 纯查询与可能 IO 通过类型隔离
- `LocalView`：类型系统保证不持有 S3 客户端，编译器保证不可能触发远程调用
- `RemoteView`：显式持有 S3，函数名以 `fetch_` 前缀编码副作用

### 2. ensure_db 拆分为检查 + 执行
- 第一步：纯检查（`check_db_status`），最多 1 次 HEAD
- 第二步：调用者根据 status 选择 action（`decide_action`），编译器保证 match 所有分支

### 3. 锁必须显式释放
- `LockGuard` 使用 `#[must_use]` 保证不会被忽略
- 没有 Drop 中释放锁的逻辑，必须显式调用 `.release()` 完成
- `.release()` 消耗 self，调用后 guard 不可再用

### 4. 命名约定：函数名编码副作用
- 无前缀 — 纯本地操作，零 OSS 请求，零写入
- `fetch_` — 可能触发远程 GET/HEAD，但不会写入 OSS
- `write_` — 可能写入本地 DB
- `upload_` — 可能 PUT 到 OSS

---

## 四、代码质量纪律（clippy 强制）

每个 crate 的 `lib.rs` 顶部必须包含以下配置（已按项目当前实际调整）：

```rust
#![forbid(unsafe_code)]
#![deny(unreachable_code)]
#![deny(unused_must_use)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]
#![deny(clippy::panic)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::let_underscore_must_use)]
#![deny(clippy::await_holding_lock)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
```

### 禁止的写法
- `.unwrap()` / `.expect()` — 必须用 `?` + `ok_or`/`map_err`
- `arr[i]` 索引 — 必须用 `.first()` / `.get()`
- `panic!()` / `todo!()` / `unimplemented!()` — 均禁止
- `let _ = must_use_value` — 禁止忽略 must_use 返回值
- 跨 `.await` 持有锁 — 禁止
- 函数缺少 `# Errors` 或 `# Panics` 文档 — 禁止（clippy deny）

### 所有可能失败的操作必须显式处理
```rust
// ❌ 禁止
let key = obj.key().unwrap();
let first = arr[0];
let _ = guard.release();

// ✅ 必须
let key = obj.key().ok_or(S3GalleryError::MissingField("key"))?;
let first = arr.first().ok_or(S3GalleryError::EmptyCollection)?;
guard.release().await?;
```

---

## 五、测试策略

### 测试层级
- **单元测试**：每个核心函数至少 3 条路径（正常、边界、错误）
- **集成测试**：MockS3Client + 真实 sqlite DB，测试模块间交互
- **E2E 测试**：真实 MinIO 实例，测试完整 CLI 命令流程
- **文档测试**：每个公开 API 必须有可运行的示例代码

### 测试指标
- 核心类型（Newtype、枚举）：100% 行覆盖
- 查询引擎（view）：> 90%
- 扫描引擎：> 85%
- 全局：> 80%

### 测试纪律
- 不允许使用 `#[should_panic]` — 错误必须通过 Result 返回
- 每个 `#[test]` 函数独立运行，互不依赖
- 集成测试失败 → PR 阻塞

---

## 六、CI 门禁规则

| 步骤 | 命令 | 门禁 |
|------|------|------|
| 代码格式 | `cargo fmt --check` | 格式一致，阻塞 |
| Lint | `cargo clippy -D warnings` | 零警告，阻塞 |
| 单元测试 | `cargo test --lib` | 全部通过，阻塞 |
| 集成测试 | `cargo test --test integration` | 全部通过，阻塞 |
| 文档测试 | `cargo test --doc` | 全部通过，阻塞 |
| 依赖审计 | `cargo audit` | 零已知漏洞，阻塞 |

---

## 七、输入验证

所有外部输入在进入系统边界时验证，之后不再重复验证。验证失败返回明确错误，不 panic。

- CLI 输入：clap 解析后立即验证，收集所有错误后一次性报告
- Web 输入：axum 提取器中验证，路径穿越防护、长度限制

---

## 八、OSS 请求优化策略

1. ListObjectsV2 一次性获取 key + ETag + size + last_modified，无需额外 HEAD
2. 增量扫描：`start_after=last_key`，避免全量遍历
3. ETag 变更检测：仅 ETag 变化的文件才需要下载内容
4. Range 请求：EXIF 仅下载文件头部 64KB（JPEG）/ 128KB（RAW）
5. 缩略图懒加载：仅当用户浏览时按需下载，扫描阶段不生成
6. 扫描者上传 DB，消费者直接下载使用，避免重复扫描