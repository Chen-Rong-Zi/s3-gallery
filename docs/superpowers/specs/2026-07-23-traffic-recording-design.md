# Traffic Recording 细粒度改造设计

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将流量记录从当前仅输出 `__aggregated` 一条记录，改造为按 business、host、operation、file_key 多维度批量写入

**Architecture:** 用 `tokio::sync::mpsc` 通道替换原子计数器，`BusinessS3Client` 将完整的 `TrafficRecord` 发入通道，后台 `TrafficBatchWriter` 攒批后按维度分组聚合写入 `traffic_log` 和 `traffic_file_log` 表

**Tech Stack:** Rust, tokio (mpsc), sqlx, SQLite

---

## 背景

当前流量记录链路：

```
BusinessS3Client → TrafficRecord(host, biz, op, file_key, bytes) → 原子计数器 → __aggregated 一条
         ↑ 有全部细节                          ↑ 全丢了                 ↑ 只剩总数
```

`TrafficRecord` 结构体已包含所有需要的维度（`host_id`, `business`, `operation`, `direction`, `file_key`, `bytes`, `count`），但 `TrafficRecorder::record()` 只更新了原子计数器的总和，`flush_counters` 写入 DB 时只剩 `__aggregated`。

## 目标

1. 流量明细按 `(host_id, business, operation, direction)` 分组写入 `traffic_log`
2. 文件级流量写入 `traffic_file_log`，支持 Top N 查询
3. 实时流量端点按 business 分组展示
4. 扫描报告从 DB 查询代替原子计数器
5. 批量写入减少 DB 压力（攒批 5s 或 100 条）

## 架构

```
BusinessS3Client
  record_traffic(TrafficRecord)
    → mpsc::Sender.send(record)

TrafficBatchWriter (后台任务)
  接收通道中的 TrafficRecord
  每 5 秒 或 每 100 条 触发写入：
    1. 按 (host_id, business, operation, direction) 分组聚合 bytes/count
       → INSERT INTO traffic_log
    2. 逐条写入 file_key
       → INSERT INTO traffic_file_log
    3. 整个 batch 在一个事务中完成
```

## 组件设计

### TrafficBatchWriter

新增结构体，替代 `spawn_aggregator` 和 `flush_counters`：

```rust
pub struct TrafficBatchWriter {
    receiver: mpsc::Receiver<TrafficRecord>,
    pool: SqlitePool,
    buffer: Vec<TrafficRecord>,
    flush_interval: Duration,
    batch_size: usize,
}
```

行为：
- `spawn()` 启动后台任务，循环 `select!` 等待通道消息或定时器
- 收到消息 → 加入 buffer
- buffer 达到 `batch_size`（100）→ 触发写入
- 定时器到期（5s）→ 触发写入
- 写入时使用事务，`traffic_log` 分组聚合，`traffic_file_log` 逐条写入

### TrafficRecorder 改造

移除 `TrafficCounters`，持有 `mpsc::Sender`：

```rust
pub struct TrafficRecorder {
    sender: mpsc::Sender<TrafficRecord>,
}
```

`record()` 方法改为 `sender.try_send(record)`（非阻塞，channel 满则丢弃）。

### BusinessS3Client 不变

已创建完整 `TrafficRecord`，调用 `recorder.record()` 即可。

### AggregateLayer 改造

扫描结束时查询 `traffic_log` 获取本次扫描的流量，代替从原子计数器读取：

```sql
SELECT business, operation, direction, SUM(bytes), SUM(count)
FROM traffic_log
WHERE recorded_at BETWEEN ? AND ?
  AND (business LIKE 'scan_%')
GROUP BY business, operation, direction
```

### 实时流量端点改造

`GET /api/traffic/live` 改为按 business 分组返回：

```json
{
  "businesses": {
    "web_download":  { "download_kbps": 125.3, "requests": 8 },
    "web_thumbnail": { "download_kbps": 15.1,  "requests": 3 },
    "scan_exif":     { "download_kbps": 0,     "requests": 0 }
  },
  "total_download_kbps": 140.4,
  "total_requests": 11
}
```

## 数据库

不需要改 schema。现有表结构已支持全部维度：

- `traffic_log`: `host_id, operation, business, direction, bytes, count, recorded_at`
- `traffic_file_log`: `host_id, file_key, business, bytes, count, recorded_at`

## 文件改动清单

| 文件 | 改动类型 | 说明 |
|------|----------|------|
| `s3/traffic_recorder.rs` | 修改 | 移除 `TrafficCounters`，`TrafficRecorder` 持有 `mpsc::Sender` |
| `s3/traffic_persist.rs` | 重写 | 重写为 `TrafficBatchWriter`，通道接收 + 批量写入 |
| `s3/layers.rs` | 无改动 | `TrafficLayer` 接口不变 |
| `s3/mod.rs` | 可能 | 更新 re-export |
| `scan/aggregate.rs` | 修改 | 改为从 DB 查询流量代替原子计数器 |
| `scan/scanner.rs` | 修改 | 启动 `spawn_batch_writer` 替代 `spawn_aggregator` |
| `view/traffic.rs` | 修改 | 补充按 `(host_id, business, operation)` 分组查询 |
| `web/handlers/traffic_handler.rs` | 修改 | `traffic_live` 端点按 business 分组返回 |
| `web/state.rs` | 无改动 | 不直接引用 `TrafficCounters` |
| `cmd_serve.rs` / `cmd_scan.rs` | 可能 | 适配新接口 |
| `cmd_traffic.rs` | 无改动 | 输出格式不变，数据来源已自动丰富 |

## 保留策略

沿用现有配置：
- `traffic_log` / `traffic_file_log`: 保留 90 天
- `traffic_stats`: 保留 365 天