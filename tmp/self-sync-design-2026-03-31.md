# AIPP 自研应用层同步方案设计

## 1. 设计目标

- 客户端继续使用本地 SQLite，读写体验不变
- 多台设备离线写入，联网后自动合并
- 支持自建部署（服务端尽量简单）
- 对现有代码的改动可控、可渐进式推进
- 不依赖任何第三方同步引擎

## 2. 总体架构

```
┌─────────────────────┐       ┌─────────────────────┐
│     Device A        │       │     Device B        │
│  ┌───────────────┐  │       │  ┌───────────────┐  │
│  │ SQLite (本地)  │  │       │  │ SQLite (本地)  │  │
│  │ + sync_changes │  │       │  │ + sync_changes │  │
│  └───────┬───────┘  │       │  └───────┬───────┘  │
│          │          │       │          │          │
│  ┌───────┴───────┐  │       │  ┌───────┴───────┐  │
│  │  Sync Engine  │  │       │  │  Sync Engine  │  │
│  └───────┬───────┘  │       │  └───────┬───────┘  │
└──────────┼──────────┘       └──────────┼──────────┘
           │    push changes             │
           └──────────┬──────────────────┘
                      ▼
            ┌───────────────────┐
            │   Sync Server     │
            │  ┌─────────────┐  │
            │  │  SQLite /    │  │
            │  │  PostgreSQL  │  │
            │  │  (全量日志)   │  │
            │  └─────────────┘  │
            └───────────────────┘
```

**核心流程：**

1. 客户端写入本地 SQLite → 触发器自动记录变更到 `sync_changes` 表
2. Sync Engine 定期/手动把 `sync_changes` 推送到 Sync Server
3. Sync Engine 从 Server 拉取其他设备的变更，应用到本地
4. 冲突在客户端解决（Last-Write-Wins，基于 Hybrid Logical Clock）

## 3. 关键设计决策

### 3.1 主键策略：保留 INTEGER ID + 新增 sync_id

**现状问题：** AIPP 所有表都用 `INTEGER PRIMARY KEY AUTOINCREMENT`，不同设备上同一条记录的 ID 不同，无法作为同步标识。

**方案：不动现有 ID，新增 `sync_id TEXT` 列**

```sql
-- 迁移示例
ALTER TABLE conversation ADD COLUMN sync_id TEXT;
CREATE UNIQUE INDEX idx_conversation_sync_id ON conversation(sync_id);

-- 回填
UPDATE conversation SET sync_id = lower(hex(randomblob(4)) || '-' ||
  hex(randomblob(2)) || '-4' || substr(hex(randomblob(2)),2) || '-' ||
  substr('89ab', abs(random()) % 4 + 1, 1) ||
  substr(hex(randomblob(2)),2) || '-' || hex(randomblob(6)))
WHERE sync_id IS NULL;
```

**为什么不直接改成 UUID 主键？**

- 改主键意味着改所有外键引用，涉及几乎全部 `*_db.rs` 文件
- 现有 INTEGER ID 在本地查询和 JOIN 中性能更好
- `sync_id` 只在同步时使用，不影响现有业务逻辑

**规则：**
- 新建记录时，同时生成 `sync_id`（UUID v4）
- 同步时，用 `sync_id` 匹配远端和本地的同一条记录
- 本地 `id` 仍然用于本地 FK 关系，`sync_id` 用于跨设备引用

### 3.2 时间戳策略：Hybrid Logical Clock (HLC)

**为什么不用 wall clock（`CURRENT_TIMESTAMP`）？**
- 不同设备的系统时钟可能不同步
- 同一毫秒内可能有多个操作，无法排序

**HLC = 物理时间 + 逻辑计数器 + 设备 ID**

```rust
/// Hybrid Logical Clock
pub struct HLC {
    /// 物理时间戳（毫秒）
    pub timestamp_ms: i64,
    /// 逻辑计数器（同一毫秒内递增）
    pub counter: u32,
    /// 设备 ID（首次启动时生成的 UUID）
    pub device_id: String,
}

impl HLC {
    /// 编码为可排序的字符串："{timestamp_ms}:{counter:04}:{device_id}"
    /// 例如："1711929600000:0001:a1b2c3d4"
    pub fn encode(&self) -> String {
        format!("{}:{:04}:{}", self.timestamp_ms, self.counter, self.device_id)
    }
}
```

**用途：**
- 每条变更记录都附带 HLC 时间戳
- 冲突解决时比较 HLC：先比 `timestamp_ms`，再比 `counter`，最后用 `device_id` 保证全序
- HLC 值存储为 TEXT 类型，天然可排序

### 3.3 变更追踪：触发器 + sync_changes 表

**在每个需要同步的数据库中创建 `sync_changes` 表：**

```sql
CREATE TABLE IF NOT EXISTS sync_changes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    table_name TEXT NOT NULL,
    record_sync_id TEXT NOT NULL,       -- 被修改记录的 sync_id
    operation TEXT NOT NULL,             -- 'INSERT', 'UPDATE', 'DELETE'
    hlc TEXT NOT NULL,                   -- HLC 时间戳
    column_changes TEXT,                 -- JSON: {"name": "新值", "content": "新值"}
                                         -- INSERT/DELETE 时为完整行 JSON
    is_pushed INTEGER NOT NULL DEFAULT 0,-- 是否已推送到服务端
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_sync_changes_pushed ON sync_changes(is_pushed, id);
CREATE INDEX idx_sync_changes_table ON sync_changes(table_name, record_sync_id);
```

**为每张同步表创建触发器：**

```sql
-- conversation 表示例
CREATE TRIGGER IF NOT EXISTS tr_conversation_insert
AFTER INSERT ON conversation
WHEN NEW.sync_id IS NOT NULL
BEGIN
    INSERT INTO sync_changes (table_name, record_sync_id, operation, hlc, column_changes)
    VALUES ('conversation', NEW.sync_id, 'INSERT',
            '', -- HLC 由应用层在 INSERT 前设置到临时表/变量
            json_object(
                'name', NEW.name,
                'assistant_id', NEW.assistant_id,
                'conversation_kind', NEW.conversation_kind,
                'created_time', NEW.created_time
            ));
END;

CREATE TRIGGER IF NOT EXISTS tr_conversation_update
AFTER UPDATE ON conversation
WHEN NEW.sync_id IS NOT NULL
BEGIN
    INSERT INTO sync_changes (table_name, record_sync_id, operation, hlc, column_changes)
    VALUES ('conversation', NEW.sync_id, 'UPDATE',
            '',
            json_object(
                'name', NEW.name,
                'assistant_id', NEW.assistant_id,
                'updated_time', NEW.updated_time,
                'butler_task_status', NEW.butler_task_status
            ));
END;

CREATE TRIGGER IF NOT EXISTS tr_conversation_delete
BEFORE DELETE ON conversation
WHEN OLD.sync_id IS NOT NULL
BEGIN
    INSERT INTO sync_changes (table_name, record_sync_id, operation, hlc, column_changes)
    VALUES ('conversation', OLD.sync_id, 'DELETE', '', NULL);
END;
```

**注意：** 触发器中的 HLC 占位值 `''` 需要在应用层补填。更实际的做法是：

**方案 A（推荐）：应用层写 oplog**
- 不用触发器，在 Rust 业务代码中手动写 `sync_changes`
- 优点：完全控制 HLC 生成、column_changes 粒度
- 缺点：每个写操作都需要额外代码

**方案 B：触发器 + 临时表传递 HLC**
- 在写操作前，先写入 `sync_hlc_context` 临时表
- 触发器从临时表读取 HLC 值
- 优点：不漏变更
- 缺点：稍复杂

**建议选择方案 A**，因为 AIPP 的写操作都集中在 `*_db.rs` 文件中，改造路径清晰。

### 3.4 软删除

**现状：** AIPP 全部使用硬删除（`DELETE FROM`），同步需要知道"某条记录被删了"。

**方案：为同步表添加 `deleted_at` 列**

```sql
ALTER TABLE conversation ADD COLUMN deleted_at TEXT; -- HLC 时间戳
```

**删除逻辑变更：**

```rust
// 之前
pub fn delete_conversation(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM conversation WHERE id = ?", params![id])?;
    Ok(())
}

// 之后（同步模式下）
pub fn delete_conversation(conn: &Connection, id: i64, hlc: &str) -> Result<()> {
    // 标记为已删除（不立即物理删除）
    conn.execute(
        "UPDATE conversation SET deleted_at = ? WHERE id = ?",
        params![hlc, id],
    )?;
    // 记录变更
    let sync_id = conn.query_row(
        "SELECT sync_id FROM conversation WHERE id = ?",
        params![id], |r| r.get::<_, String>(0)
    )?;
    write_sync_change(conn, "conversation", &sync_id, "DELETE", hlc, None)?;
    Ok(())
}
```

**定期清理：** 同步确认后（所有设备都拉取了删除事件），再物理删除已标记记录。

**查询层面：** 所有 SELECT 查询加 `WHERE deleted_at IS NULL` 过滤。

### 3.5 冲突解决策略

**默认策略：Last-Write-Wins (LWW)，基于 HLC 比较**

```
设备 A 在 t=100 修改了 conversation.name = "聊天1"
设备 B 在 t=102 修改了 conversation.name = "聊天2"

同步后：所有设备都变成 "聊天2"（因为 t=102 > t=100）
```

**按表定制策略：**

| 表 | 冲突策略 | 说明 |
|---|---------|------|
| conversation | LWW | 最后修改的名字/状态赢 |
| message | 不冲突 | 消息只有 INSERT，无 UPDATE 冲突 |
| message_attachment | 不冲突 | 跟随 message，只 INSERT |
| assistant | LWW | 配置类数据，最后修改赢 |
| llm_provider | LWW | 配置类数据 |
| mcp_server | LWW | 配置类数据 |
| artifacts_collection | LWW | 最后修改赢 |
| feature_config | LWW | 配置类数据 |

**消息这类 append-only 数据几乎不会冲突**——同一条消息不会在两台设备上同时被编辑。

### 3.6 同步表与非同步表分类

```
需要同步的表（跨设备共享）          不需要同步的表（设备本地）
─────────────────────────          ──────────────────────────
conversation.db:                   conversation.db:
  ✅ conversation                    ❌ mcp_tool_call (执行日志)
  ✅ message                         ❌ acp_session (设备会话)
  ✅ message_attachment               ❌ external_channel_* (设备级)
  ✅ conversation_summary             ❌ butler_main_state (运行状态)
  ✅ conversation_todo                ❌ butler_task_result (运行结果)
                                    
assistant.db:                      system.db:
  ✅ assistant                        ❌ secure_config (设备加密)
  ✅ assistant_model                  ✅ feature_config (功能配置)
  ✅ assistant_prompt                 ✅ system_config (部分)
  ✅ assistant_model_config        
  ✅ assistant_mcp_config            plugin.db:
  ✅ assistant_mcp_tool_config        ❌ 全部（插件状态设备级）
  ✅ assistant_workspace           
  ✅ assistant_skill_config          scheduled_task.db:
                                     ✅ scheduled_task (任务定义)
llm.db:                              ❌ scheduled_task_log (执行日志)
  ✅ llm_provider                     ❌ scheduled_task_run (运行记录)
  ✅ llm_model                     
  ✅ llm_provider_config           mcp.db:
  ❌ llm_model_request_mode_pref     ✅ mcp_server (服务配置)
                                     ✅ mcp_server_tool (工具配置)
artifacts.db:                        ✅ mcp_server_prompt (提示配置)
  ✅ artifacts_collection             ❌ mcp_tool_call (执行日志)
                                     ❌ mcp_tool_catalog (本地缓存)
                                     ❌ mcp_server_capability_epoch_catalog
                                     ❌ conversation_mcp_loaded_tool
```

## 4. 同步协议

### 4.1 推送（Push）

```
Client → Server: POST /api/sync/push
{
    "device_id": "a1b2c3d4-...",
    "changes": [
        {
            "db": "conversation",
            "table": "conversation",
            "sync_id": "uuid-of-record",
            "operation": "UPDATE",
            "hlc": "1711929600000:0001:a1b2c3d4",
            "data": {
                "name": "新对话名",
                "updated_time": "2026-03-31T23:00:00Z"
            }
        },
        {
            "db": "conversation",
            "table": "message",
            "sync_id": "uuid-of-message",
            "operation": "INSERT",
            "hlc": "1711929600001:0000:a1b2c3d4",
            "data": {
                "conversation_sync_id": "uuid-of-conversation",
                "message_type": "user",
                "content": "Hello",
                "created_time": "2026-03-31T23:00:01Z"
            }
        }
    ]
}

Server → Client: 200 OK
{
    "accepted": 2,
    "server_cursor": "1711929600001:0000"
}
```

### 4.2 拉取（Pull）

```
Client → Server: GET /api/sync/pull?device_id=a1b2c3d4&cursor=1711929500000:0000
                                                                 ↑ 上次拉取的位置

Server → Client: 200 OK
{
    "changes": [
        {
            "db": "conversation",
            "table": "message",
            "sync_id": "uuid-of-another-message",
            "operation": "INSERT",
            "hlc": "1711929550000:0000:e5f6g7h8",
            "source_device": "e5f6g7h8",
            "data": {
                "conversation_sync_id": "uuid-of-conversation",
                "message_type": "assistant",
                "content": "Hi there!",
                "created_time": "2026-03-31T22:50:00Z"
            }
        }
    ],
    "cursor": "1711929550000:0000",
    "has_more": false
}
```

### 4.3 首次同步

**场景：新设备加入，需要获取全量数据**

```
1. 新设备注册 → 分配 device_id
2. Server 返回 full_snapshot 端点
3. 客户端按表分批下载全量数据
4. 设置 cursor 为最新位置
5. 后续走增量同步
```

## 5. Sync Server 设计

### 5.1 最简架构

Sync Server 本身可以非常简单——它只是一个 **变更日志存储 + HTTP API**：

```
Sync Server
├── /api/sync/push     接收客户端变更，写入日志
├── /api/sync/pull     返回指定游标之后的变更
├── /api/sync/register 设备注册
├── /api/sync/snapshot 全量快照（首次同步）
└── 存储：SQLite 或 PostgreSQL
```

### 5.2 服务端日志表

```sql
CREATE TABLE sync_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,           -- 用户标识
    device_id TEXT NOT NULL,         -- 来源设备
    db_name TEXT NOT NULL,           -- conversation / assistant / ...
    table_name TEXT NOT NULL,
    record_sync_id TEXT NOT NULL,
    operation TEXT NOT NULL,          -- INSERT / UPDATE / DELETE
    hlc TEXT NOT NULL,
    data TEXT,                        -- JSON 完整行或变更字段
    received_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_sync_log_user_hlc ON sync_log(user_id, hlc);
CREATE INDEX idx_sync_log_record ON sync_log(user_id, table_name, record_sync_id);

CREATE TABLE sync_device (
    device_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    device_name TEXT,
    last_push_hlc TEXT,
    last_pull_hlc TEXT,
    registered_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

### 5.3 服务端技术选型

| 选项 | 优点 | 缺点 |
|------|------|------|
| **Rust (Axum/Actix) + SQLite** | 与 AIPP 同技术栈，部署简单（单二进制） | 高并发时 SQLite 写锁 |
| **Rust (Axum) + PostgreSQL** | 生产级，高并发 | 多一个部署依赖 |
| **Python (FastAPI) + SQLite** | 开发快，部署简单 | 性能差一些 |

**推荐：Rust + SQLite 起步**，因为 AIPP 的同步场景是低并发的（个人多设备，不是多用户 SaaS），SQLite 完全够用。未来需要多用户支持时再切 PostgreSQL。

## 6. 客户端 Sync Engine 设计

### 6.1 模块结构

```
src-tauri/src/sync/
├── mod.rs              // 模块入口
├── engine.rs           // SyncEngine 核心逻辑
├── hlc.rs              // Hybrid Logical Clock 实现
├── changes.rs          // sync_changes 表操作
├── protocol.rs         // Push/Pull HTTP 协议
├── merge.rs            // 冲突解决与本地应用
├── schema.rs           // 同步表注册与元数据
└── migration.rs        // sync_id/deleted_at 迁移
```

### 6.2 SyncEngine 核心

```rust
pub struct SyncEngine {
    /// 设备唯一 ID（首次启动生成，存入 system_config）
    device_id: String,
    /// HLC 时钟
    clock: Arc<Mutex<HLC>>,
    /// 服务端地址
    server_url: String,
    /// 认证令牌
    auth_token: String,
    /// 同步间隔
    interval: Duration,
    /// 上次拉取游标（per-db）
    cursors: HashMap<String, String>,
}

impl SyncEngine {
    /// 执行一次完整同步周期
    pub async fn sync_cycle(&mut self) -> Result<SyncReport> {
        // 1. Push: 读取本地 sync_changes (is_pushed = 0)，推送到服务端
        let push_result = self.push_changes().await?;

        // 2. Pull: 从服务端拉取其他设备的变更
        let pull_result = self.pull_changes().await?;

        // 3. Apply: 将拉取的变更应用到本地（含冲突解决）
        let apply_result = self.apply_remote_changes(pull_result.changes).await?;

        // 4. 标记已推送的变更
        self.mark_pushed(push_result.pushed_ids).await?;

        Ok(SyncReport { push_result, apply_result })
    }

    /// 应用远端变更到本地
    async fn apply_remote_changes(&self, changes: Vec<RemoteChange>) -> Result<ApplyResult> {
        for change in changes {
            match change.operation.as_str() {
                "INSERT" => {
                    // 检查本地是否已存在同 sync_id 的记录
                    // 不存在 → INSERT
                    // 已存在 → 说明是重复推送，跳过
                }
                "UPDATE" => {
                    // 查找本地 sync_id 对应的记录
                    // 比较 HLC：远端更新 > 本地更新 → 应用远端值
                    //          远端更新 < 本地更新 → 跳过（本地赢）
                }
                "DELETE" => {
                    // 查找本地 sync_id 对应的记录
                    // 标记 deleted_at（如果远端 HLC > 本地最后更新）
                }
            }
        }
    }
}
```

### 6.3 跨数据库外键的同步处理

**问题：** `message.conversation_id` 是本地整数 ID，但对应的 `conversation` 在不同设备上 ID 不同。

**解决方案：同步时使用 sync_id 替代本地 ID**

```rust
// 推送 message 时
{
    "sync_id": "msg-uuid-xxx",
    "conversation_sync_id": "conv-uuid-yyy",  // 用 conversation 的 sync_id，不是 id
    "content": "Hello"
}

// 拉取并应用时
fn apply_message_insert(conn: &Connection, change: &RemoteChange) -> Result<()> {
    let conv_sync_id = change.data["conversation_sync_id"].as_str()?;
    // 通过 sync_id 查找本地 conversation 的 id
    let local_conv_id: i64 = conn.query_row(
        "SELECT id FROM conversation WHERE sync_id = ?",
        params![conv_sync_id],
        |r| r.get(0),
    )?;
    // 用本地 id 插入 message
    conn.execute(
        "INSERT INTO message (sync_id, conversation_id, content, ...) VALUES (?, ?, ?, ...)",
        params![change.sync_id, local_conv_id, change.data["content"]],
    )?;
    Ok(())
}
```

## 7. 迁移计划（分阶段）

### Phase 1：基础设施（对用户无感）

**目标：** 为同步做好 schema 准备，不改变现有行为

1. **添加 `sync_id` 列** — 所有同步表
2. **添加 `deleted_at` 列** — 所有同步表
3. **添加 `updated_at` 列** — 缺少此列的同步表
4. **生成 device_id** — 首次启动时写入 `system_config`
5. **实现 HLC** — `src-tauri/src/sync/hlc.rs`
6. **新建 `sync_changes` 表** — 每个需要同步的数据库

**代码改动：**
- `lib.rs` 中的迁移函数（`special_logic_0_0_12`）
- 新建 `src-tauri/src/sync/` 模块

### Phase 2：变更追踪

**目标：** 所有写操作开始记录变更，但还不同步

1. **在 `*_db.rs` 的写操作中记录 sync_changes**
   - 每个 INSERT/UPDATE/DELETE 函数增加 `write_sync_change()` 调用
2. **新建记录时自动填充 `sync_id`**
3. **DELETE 改为软删除**（设置 `deleted_at`）
4. **SELECT 查询加 `WHERE deleted_at IS NULL`**

### Phase 3：同步引擎

**目标：** 实现完整的 Push/Pull 同步

1. **实现 Sync Server**（单独的 Rust binary 或独立仓库）
2. **实现 `SyncEngine`** — Push/Pull/Apply 逻辑
3. **实现冲突解决** — LWW
4. **集成到 AIPP** — 复用 `sync_manager.rs` 的配置界面

### Phase 4：生产化

1. **首次同步（全量下载/上传）**
2. **断点续传**
3. **压缩和分页**（大表分批同步）
4. **定期清理** 已推送的 `sync_changes` 和已确认的 `deleted_at` 记录
5. **同步状态 UI**（进度条、错误提示）

## 8. 与现有 libsql 同步的关系

**替代关系。** 自研同步完全替代现有的 `libsql synced_database` 方案：

| 维度 | libsql synced_database | 自研应用层同步 |
|------|----------------------|---------------|
| 同步粒度 | 整个数据库文件 | 按表、按行 |
| 协议 | libsql 私有协议 | 自定义 HTTP JSON |
| 冲突处理 | 无（底层覆盖） | LWW + 可定制 |
| 服务端 | sqld（已不兼容） | 自建简单 API |
| 离线支持 | 依赖 embedded replica | 原生支持 |
| 选择性同步 | 不支持 | 按表配置 |

**迁移后可以把 `libsql` 依赖替换回标准 `rusqlite`**（如果需要），进一步简化依赖。

## 9. 工作量估计

| 阶段 | 主要工作 | 文件影响 |
|------|---------|---------|
| Phase 1 | Schema 迁移 + HLC | lib.rs, 新建 sync/ |
| Phase 2 | 变更追踪 | 所有 *_db.rs (~8 个文件) |
| Phase 3 | 同步引擎 + Server | 新建 sync/ + 独立服务 |
| Phase 4 | 生产化 | sync/ + 前端 UI |

**建议从 Phase 1 开始，先做 `conversation` + `message` 两张表的完整同步作为 MVP。** 这两张表：
- 数据价值最高（用户最关心对话历史）
- 写入模式最典型（INSERT 为主，偶尔 UPDATE）
- 能验证跨数据库 FK 处理（message → conversation）

## 10. Sync Server 最简实现参考

如果想最快跑通，Sync Server 可以用 ~200 行 Rust 实现：

```rust
// 伪代码
#[tokio::main]
async fn main() {
    let db = rusqlite::Connection::open("sync_server.db")?;
    init_server_schema(&db)?;

    let app = axum::Router::new()
        .route("/api/sync/push", post(handle_push))
        .route("/api/sync/pull", get(handle_pull))
        .route("/api/sync/register", post(handle_register));

    axum::Server::bind(&"0.0.0.0:9000".parse()?).serve(app).await?;
}

async fn handle_push(Json(req): Json<PushRequest>) -> Json<PushResponse> {
    // 验证 auth_token
    // 将 changes 写入 sync_log
    // 返回 accepted count + new cursor
}

async fn handle_pull(Query(params): Query<PullParams>) -> Json<PullResponse> {
    // 查询 sync_log WHERE user_id = ? AND hlc > ? AND device_id != ?
    // 返回 changes + new cursor
}
```

## 11. 风险与应对

| 风险 | 概率 | 应对 |
|------|------|------|
| 大表首次同步慢 | 中 | 分页 + 压缩 + 后台进行 |
| message 表数据量大 | 高 | 可配置同步范围（如只同步最近 N 天） |
| 跨数据库 FK 同步顺序 | 中 | 按依赖拓扑排序（先 assistant → 再 conversation） |
| 网络中断丢失 | 低 | 本地 sync_changes 持久化 + 重试 |
| HLC 时钟漂移 | 低 | 每次同步时与服务端校准 |
| 软删除查询遗漏 | 中 | 统一查询层封装 `WHERE deleted_at IS NULL` |
