# AIPP 远端同步实现方案

## 1. 结论

推荐做 **AIPP 专用业务同步服务**，不做 SQLite 文件级复制。

原因：

- AIPP 是多 SQLite 库：`conversation.db`、`system.db`、`assistant.db`、`llm.db`、`mcp.db`、`artifacts.db`、`plugin.db` 等。
- 表之间有业务关系：conversation/message/version/attachment、assistant/model/mcp、feature_config、MCP tool call、artifact collection。
- 本地自增 ID 跨设备必冲突，文件级复制无法自动合并。
- secure_config、本地路径、ACP session、运行中 tool call 这类数据有设备语义，不能无差别同步。
- 通用 SQLite 同步通常只能解决“变更传输”，不能解决 AIPP 的“哪些数据能合并、如何合并、哪些禁止同步”。

因此同步层应该放在 AIPP 业务层：

```
AIPP Desktop
  ├─ local SQLite 继续作为主读写路径
  ├─ sync metadata / outbox 记录本地变更
  ├─ sync client push/pull 增量
  └─ sync applier 将远端变更写回本地

AIPP Sync Server
  ├─ auth / device registry
  ├─ per-user change log
  ├─ object latest snapshot
  ├─ cursor-based pull
  └─ conflict resolution
```

## 2. 目标与非目标

### 目标

- 多设备同步 conversation、message、基础配置、assistant、LLM provider/model、MCP 配置、artifact collection。
- 本地优先：无网络时继续完整可用。
- 增量同步：只上传本地变更，只拉取游标之后的远端变更。
- 支持首次合并旧设备数据，不要求用户选一个设备覆盖另一个设备。
- 冲突可解释，核心数据不静默丢失。
- 服务端可自建，部署复杂度低。

### 非目标

- 不做 SQLite WAL/page 级复制。
- 不要求服务端能直接运行 AIPP 全量业务逻辑。
- MVP 不同步本机加密 secret。
- MVP 不同步运行中状态，比如正在流式生成的 message、ACP live session、executing MCP tool call。
- MVP 不同步本地文件系统里的插件/skill/artifact workspace 文件，只同步可序列化的配置和 collection 记录。

## 3. 同步范围

### 第一阶段必须支持

| 数据 | 本地库 | 同步策略 |
|---|---|---|
| conversation | conversation.db | 行级同步，soft delete |
| message | conversation.db | completed message 行级同步，append 优先 |
| message_attachment | conversation.db | 文本内容先同步；本地文件路径不直接同步 |
| conversation_summary | conversation.db | LWW，可重算 |
| conversation_todo | conversation.db | LWW/按 item 同步 |
| assistant | assistant.db | 行级同步 |
| assistant_prompt | assistant.db | 行级同步 |
| assistant_model | assistant.db | 依赖 provider/model 的 sync_id 映射 |
| assistant_model_config | assistant.db | `(assistant, name)` 粒度 LWW |
| assistant_mcp_config | assistant.db | 依赖 assistant/server sync_id |
| assistant_mcp_tool_config | assistant.db | 依赖 assistant/tool sync_id |
| feature_config | system.db | `(feature_code, key)` 粒度 LWW |
| system_config | system.db | allowlist 同步 |
| llm_provider | llm.db | 非 secret 字段同步 |
| llm_model | llm.db | 依赖 provider sync_id |
| llm_model_request_mode_preference | llm.db | `(provider/model)` 粒度 LWW |
| mcp_server | mcp.db | 配置同步，敏感字段单独处理 |
| mcp_server_tool/resource/prompt | mcp.db | 可重建，但同步能降低冷启动差异 |
| artifacts_collection | artifacts.db | collection 记录同步，code 字段同步 |

### 延后支持

| 数据 | 原因 | 后续方案 |
|---|---|---|
| secure_config | 本机密钥加密，跨设备不可解 | 端到端加密 vault |
| ACP session | agent 支持差异，session 不是纯数据 | 每设备本地保留 |
| queued_conversation_message | 队列语义和设备强相关 | 不同步 |
| running mcp_tool_call | 执行状态强设备相关 | 只同步 completed/failed 历史 |
| scheduled task runtime run/log | 执行归属需要 leader 语义 | 第二阶段加设备归属 |
| plugin/skill 实际文件 | 本地文件安装路径不一致 | 同步 manifest + 安装引用 |
| artifact workspace 文件 | 文件树/blob，需要独立存储 | content-addressed blob store |
| external channel delivery | Feishu 等外部投递有幂等风险 | 每设备/每 channel 独立设计 |

## 4. 核心数据模型

### 4.1 本地稳定 ID

现有表继续保留 `INTEGER PRIMARY KEY AUTOINCREMENT`，避免大面积改业务代码。

新增同步身份：

```sql
CREATE TABLE IF NOT EXISTS sync_object_map (
  object_type TEXT NOT NULL,
  local_table TEXT NOT NULL,
  local_id INTEGER NOT NULL,
  sync_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (object_type, local_id),
  UNIQUE (sync_id)
);
```

说明：

- `sync_id` 使用 UUID v7 或 ULID。
- 不强制第一阶段给每张业务表加 `sync_id` 列，降低迁移风险。
- 上传时通过 `sync_object_map` 将本地 ID 转为 sync_id。
- 下载时先查 `sync_id -> local_id`，没有就创建本地行并写入映射。

后续如果某些高频表需要优化，可以给表直接加 `sync_id` 列。

### 4.2 本地设备表

```sql
CREATE TABLE IF NOT EXISTS sync_device (
  device_id TEXT PRIMARY KEY,
  device_name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_sync_at TEXT
);
```

### 4.3 本地游标

```sql
CREATE TABLE IF NOT EXISTS sync_cursor (
  scope TEXT PRIMARY KEY,
  server_cursor INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);
```

`scope` 初期使用 `default`，后续可扩展为 workspace/account。

### 4.4 本地 outbox

```sql
CREATE TABLE IF NOT EXISTS sync_outbox (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id TEXT NOT NULL UNIQUE,
  object_type TEXT NOT NULL,
  object_id TEXT NOT NULL,
  operation TEXT NOT NULL CHECK(operation IN ('upsert', 'delete')),
  payload_json TEXT,
  base_version INTEGER,
  local_version INTEGER NOT NULL,
  device_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK(status IN ('pending', 'pushing', 'acked', 'failed')),
  retry_count INTEGER NOT NULL DEFAULT 0,
  last_error TEXT
);

CREATE INDEX IF NOT EXISTS idx_sync_outbox_status_id
  ON sync_outbox(status, id);
```

写入本地业务表后，在同一个 SQLite transaction 内写 `sync_outbox`。

### 4.5 本地 shadow

```sql
CREATE TABLE IF NOT EXISTS sync_shadow (
  object_type TEXT NOT NULL,
  object_id TEXT NOT NULL,
  server_version INTEGER NOT NULL,
  payload_hash TEXT NOT NULL,
  deleted_at TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (object_type, object_id)
);
```

用途：

- 判断本地是否基于旧版本修改。
- 避免重复 apply 同一远端变更。
- 做冲突检测和调试。

## 5. 服务端模型

建议服务端 MVP 使用 Python + FastAPI。同步服务本身主要是 API、JSON payload、事务写入、游标查询和少量冲突规则，Python 能明显降低开发与调试成本。

推荐技术栈：

| 层 | 推荐 | 说明 |
|---|---|---|
| Web 框架 | FastAPI | API 型服务直接，Pydantic 集成好 |
| ASGI Server | Uvicorn | 本地开发和生产入口简单 |
| 数据模型 | Pydantic v2 | 定义 push/pull/event payload，减少协议漂移 |
| ORM/SQL | SQLAlchemy 2.x | 事务、唯一约束、PostgreSQL/SQLite 切换成熟 |
| Migration | Alembic | 服务端 schema 版本演进 |
| 数据库 | PostgreSQL 优先 | 多用户和并发写更稳 |
| 轻量部署 | SQLite 可选 | 个人自建、小规模先跑通 |
| 测试 | pytest + httpx | API/事务/幂等测试方便 |
| 配置 | pydantic-settings | 环境变量配置清晰 |
| 部署 | Docker + compose | 自建部署门槛低 |

服务端存储：

- 小规模个人部署：SQLite 足够。
- 多用户/团队部署：PostgreSQL 更稳。

服务端逻辑不依赖 SQLite 特性。小规模可以先用 SQLite，正式多端长期使用建议切到 PostgreSQL。

Rust + Axum 不作为 MVP 推荐技术栈。只有在后续出现高并发、多租户、大 payload、复杂冲突合并或需要和 Rust 客户端共享大量类型时，再考虑把服务端核心迁移到 Rust。

### 5.1 account/device

```sql
CREATE TABLE sync_account (
  id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL
);

CREATE TABLE sync_device (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL,
  name TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_seen_at TEXT,
  UNIQUE(account_id, id)
);
```

### 5.2 object latest

```sql
CREATE TABLE sync_object (
  account_id TEXT NOT NULL,
  object_type TEXT NOT NULL,
  object_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  payload_json TEXT,
  payload_hash TEXT NOT NULL,
  deleted_at TEXT,
  updated_at TEXT NOT NULL,
  updated_by_device_id TEXT NOT NULL,
  PRIMARY KEY (account_id, object_type, object_id)
);
```

### 5.3 change log

```sql
CREATE TABLE sync_change (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id TEXT NOT NULL,
  event_id TEXT NOT NULL,
  object_type TEXT NOT NULL,
  object_id TEXT NOT NULL,
  operation TEXT NOT NULL,
  version INTEGER NOT NULL,
  payload_json TEXT,
  deleted_at TEXT,
  device_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(account_id, event_id)
);

CREATE INDEX idx_sync_change_account_seq
  ON sync_change(account_id, seq);
```

`seq` 是 pull cursor。服务端只保证同一 account 下单调递增。

## 6. 对象类型与 payload 规则

每个同步对象使用统一 envelope：

```json
{
  "object_type": "conversation.message",
  "object_id": "01J...",
  "operation": "upsert",
  "base_version": 12,
  "local_version": 13,
  "payload": {
    "fields": {},
    "refs": {}
  }
}
```

### 6.1 payload.fields

保存业务字段，不包含本地自增 ID。

示例：conversation

```json
{
  "fields": {
    "name": "需求讨论",
    "conversation_kind": "normal",
    "created_time": "2026-05-18T10:00:00.000Z",
    "updated_time": "2026-05-18T10:05:00.000Z",
    "is_hidden_from_normal_chat_list": false
  },
  "refs": {
    "assistant": "01J..."
  }
}
```

示例：message

```json
{
  "fields": {
    "message_type": "response",
    "content": "...",
    "llm_model_name": "gpt-5.5",
    "created_time": "2026-05-18T10:00:00.000Z",
    "finish_time": "2026-05-18T10:00:20.000Z",
    "token_count": 1234,
    "generation_group_id": "abc",
    "parent_group_id": null,
    "tool_calls_json": null,
    "metadata_json": "{}"
  },
  "refs": {
    "conversation": "01J...",
    "parent_message": "01J...",
    "llm_model": "01J..."
  }
}
```

### 6.2 refs

所有外键关系用 sync_id 表达。下载 apply 时按 refs 解析本地 ID。

如果父对象还没有到达：

- 将当前变更写入 `sync_deferred_apply`。
- pull 批次 apply 完后重试。
- 超过重试仍缺父对象则报错，不静默丢弃。

```sql
CREATE TABLE IF NOT EXISTS sync_deferred_apply (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  change_seq INTEGER NOT NULL,
  object_type TEXT NOT NULL,
  object_id TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  missing_refs_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  retry_count INTEGER NOT NULL DEFAULT 0,
  last_error TEXT
);
```

## 7. 本地写入路径

### 7.1 推荐实现方式

第一阶段不要用 SQL trigger 自动捕获所有表。原因：

- AIPP 有多库，trigger 跨库不可控。
- 删除/覆盖语义需要业务判断。
- feature_config 当前存在按 feature bucket 替换的保存语义，trigger 只能看到行变化，看不到用户意图。
- message streaming 期间不能每个 token 都同步。

推荐在 repository/API 写路径显式调用 sync helper：

```rust
pub struct SyncRecorder<'a> {
    conn: &'a Connection,
    device_id: String,
}

impl<'a> SyncRecorder<'a> {
    pub fn record_upsert(
        &self,
        object_type: &str,
        local_table: &str,
        local_id: i64,
        payload: serde_json::Value,
    ) -> rusqlite::Result<()> {
        // ensure sync_id
        // read current shadow version
        // insert sync_outbox
        Ok(())
    }

    pub fn record_delete(
        &self,
        object_type: &str,
        local_table: &str,
        local_id: i64,
    ) -> rusqlite::Result<()> {
        // insert tombstone event before physical delete or use soft delete
        Ok(())
    }
}
```

### 7.2 message 特殊规则

流式生成期间不上传中间态。

触发同步的时机：

- user message 创建完成后记录 upsert。
- assistant/response message `finish_time` 写入后记录 upsert。
- token usage/metadata 最终更新后记录 upsert。
- regenerate/version 切换涉及 `parent_id`、`generation_group_id`、`parent_group_id`，按完整 message payload 同步。

如果需要跨设备展示“正在生成”，第二阶段做 presence，不进入持久同步。

### 7.3 feature_config 特殊规则

feature_config 以 `(feature_code, key)` 作为 object identity：

```
object_type = "system.feature_config"
object_id = stable_hash(feature_code + "\0" + key)
```

保存时必须拆成 key 级 outbox，不能以整个 feature_code bucket 覆盖远端。

原因：不同设备可能同时修改同一 feature 下不同 key，bucket 级覆盖会互相擦掉。

### 7.4 secret 和本地路径

默认不同步：

- `secure_config`
- provider API key
- MCP headers 中的认证字段
- assistant workspace path
- ACP working directory
- CLI absolute path
- env vars

如果字段混在普通 config 里，需要 sync allowlist/denylist：

```rust
fn is_syncable_config(scope: &str, key: &str) -> bool {
    !matches!(
        (scope, key),
        ("secure_config", _)
        | (_, "api_key")
        | (_, "authorization")
        | (_, "acp_working_directory")
        | (_, "acp_env_vars")
    )
}
```

## 8. 同步协议

### 8.1 Auth

MVP：

- 用户在服务端创建 sync token。
- 客户端配置 sync URL + token。
- token 绑定 account。
- device 首次连接时注册 `device_id`。

后续：

- Web 登录 / OAuth。
- token rotation。
- 设备撤销。
- 端到端加密密钥协商。

### 8.2 Push

```
POST /v1/sync/push
Authorization: Bearer <token>
```

Request:

```json
{
  "device_id": "01J...",
  "events": [
    {
      "event_id": "01J...",
      "object_type": "conversation.message",
      "object_id": "01J...",
      "operation": "upsert",
      "base_version": 12,
      "local_version": 13,
      "payload": {
        "fields": {},
        "refs": {}
      },
      "created_at": "2026-05-18T10:00:00.000Z"
    }
  ]
}
```

Response:

```json
{
  "accepted": [
    {
      "event_id": "01J...",
      "object_type": "conversation.message",
      "object_id": "01J...",
      "server_version": 13,
      "server_seq": 1024
    }
  ],
  "conflicts": [
    {
      "event_id": "01J...",
      "object_type": "system.feature_config",
      "object_id": "01J...",
      "server_version": 15,
      "server_payload": {}
    }
  ],
  "rejected": [
    {
      "event_id": "01J...",
      "reason": "schema_version_too_new"
    }
  ]
}
```

Push 幂等：

- `event_id` 唯一。
- 服务端已见过 event_id，直接返回 accepted。
- 客户端收到 accepted 后将 outbox 标记为 `acked`，更新 shadow。

### 8.3 Pull

```
GET /v1/sync/pull?cursor=1024&limit=500
Authorization: Bearer <token>
X-AIPP-Device-ID: <device_id>
```

Response:

```json
{
  "cursor": 1090,
  "has_more": false,
  "changes": [
    {
      "seq": 1025,
      "event_id": "01J...",
      "device_id": "other-device",
      "object_type": "conversation.message",
      "object_id": "01J...",
      "operation": "upsert",
      "version": 14,
      "payload": {
        "fields": {},
        "refs": {}
      },
      "deleted_at": null,
      "created_at": "2026-05-18T10:00:00.000Z"
    }
  ]
}
```

客户端 apply 后再更新本地 cursor。不能先更新 cursor。

### 8.4 Bootstrap

首次开启同步：

1. 客户端调用 `GET /v1/sync/status`。
2. 如果远端为空：本地全量扫描，生成 outbox，push。
3. 如果远端不为空且本地无数据：pull 全量。
4. 如果远端不为空且本地也有数据：进入 merge bootstrap。

Merge bootstrap：

- 本地所有可同步行生成 sync_id。
- 全量上传为 upsert。
- 服务端按 object identity 合并。
- 无稳定 natural key 的对象保留为新对象。
- 有稳定 natural key 的对象做冲突处理。

稳定 natural key：

| object_type | natural key |
|---|---|
| system.feature_config | `(feature_code, key)` |
| system.system_config | `key` |
| assistant.model_config | `(assistant_sync_id, name)` |
| assistant.workspace | 不同步或 `(assistant_sync_id, path_hash)` |
| llm.provider | `(api_type, name)` |
| llm.model | `(provider_sync_id, code)` |
| mcp.server | `name`，但 builtin server 固定 |
| mcp.tool | `(server_sync_id, tool_name)` |
| conversation | 无，保留两边 |
| message | 无，跟随 conversation |
| artifact.collection | 无，或 `(name, artifact_type, code_hash)` 可选去重 |

## 9. 冲突策略

### 9.1 默认规则

| 类型 | 策略 |
|---|---|
| conversation | 字段级 LWW，删除 wins 但保留 tombstone |
| message | append 优先；同 object 修改冲突时保留远端并创建本地 conflict copy |
| message metadata/token usage | LWW，优先 finish_time 更晚的完整结果 |
| feature_config | `(feature_code,key)` LWW |
| assistant prompt | LWW，冲突可保留历史版本 |
| assistant/model binding | set merge |
| MCP server config | LWW；secret-like 字段不同步 |
| MCP tool catalog | LWW 或可重建 |
| artifact collection | LWW；code 冲突时创建副本 |

### 9.2 LWW 时间来源

不要直接相信设备本地时间。

服务端合并优先级：

1. `base_version == current_version`：直接接受，version + 1。
2. `base_version < current_version`：按 object_type 冲突规则处理。
3. 同为 LWW 类型：使用服务端收到顺序决定，记录 conflict audit。

设备时间只作为展示字段，不作为最终裁决依据。

### 9.3 删除

删除统一使用 tombstone：

```json
{
  "operation": "delete",
  "deleted_at": "2026-05-18T10:00:00.000Z"
}
```

本地可以物理删除业务表行，但必须保留：

- `sync_shadow.deleted_at`
- `sync_object_map`
- outbox delete event

服务端 tombstone 保留至少 90 天，之后可 compaction。

### 9.4 message 冲突

message 不应该频繁编辑。实际冲突主要来自：

- 两台设备同时修改同一个 assistant response metadata。
- 一台设备 regenerate，另一台设备继续旧分支。
- 同一历史 message 手工编辑。

策略：

- 不覆盖不同内容的 message。
- 如果同一个 `object_id` 内容不同且 base_version 落后，创建一个新的 local conflict message：
  - `metadata_json.conflict_of = <object_id>`
  - `metadata_json.conflict_reason = "remote_modified"`
- UI 后续可显示“同步冲突副本”。

MVP 可以先不做 UI，只保证数据不丢。

## 10. 客户端模块划分

建议新增：

```
src-tauri/src/sync/
├── mod.rs
├── schema.rs              # sync_* 本地表创建/迁移
├── identity.rs            # sync_id / object map
├── recorder.rs            # 写 outbox
├── serializer.rs          # 业务行 -> sync payload
├── applier.rs             # sync payload -> 本地行
├── client.rs              # HTTP push/pull
├── worker.rs              # 后台同步循环
├── conflict.rs            # 冲突策略
├── allowlist.rs           # 字段同步 allow/deny
└── tests/
```

Tauri commands：

```rust
#[tauri::command]
async fn get_sync_status() -> Result<SyncStatus, String>;

#[tauri::command]
async fn configure_sync(config: SyncConfig) -> Result<(), String>;

#[tauri::command]
async fn run_sync_once() -> Result<SyncRunResult, String>;

#[tauri::command]
async fn disable_sync() -> Result<(), String>;
```

后台 worker：

- app 启动后读取 sync 配置。
- 每 30-60 秒尝试同步一次。
- 本地 outbox 有 pending 时加快。
- 网络失败只标记 `failed` 和 backoff，不影响本地使用。
- 不做静默降级：配置错误、schema 不兼容、鉴权失败要在 UI 明确展示。

## 11. 服务端模块划分

建议新建独立 repo 或 `sync-server/`。MVP 使用 Python/FastAPI：

```
sync-server/
├── app/
│   ├── main.py
│   ├── config.py
│   ├── auth.py
│   ├── db.py
│   ├── models.py
│   ├── schemas.py
│   ├── routes/
│   │   ├── status.py
│   │   ├── push.py
│   │   └── pull.py
│   ├── services/
│   │   ├── merge.py
│   │   ├── conflict.py
│   │   └── cursor.py
│   └── tests/
├── alembic/
├── alembic.ini
├── pyproject.toml
└── Dockerfile
```

技术选型：

- Python 3.12+。
- FastAPI + Uvicorn。
- Pydantic v2 + pydantic-settings。
- SQLAlchemy 2.x。
- Alembic。
- PostgreSQL 作为推荐生产后端。
- SQLite 作为个人部署/本地开发后端。
- pytest + httpx 做 API 测试。
- JSON payload 用 Pydantic model + `dict[str, Any]` 承载。
- OpenAPI 文档可后置。

部署：

```yaml
services:
  aipp-sync:
    image: aipp-sync:latest
    ports:
      - "8080:8080"
    volumes:
      - ./data:/data
    environment:
      AIPP_SYNC_DATABASE_URL: sqlite:///data/aipp-sync.db
      AIPP_SYNC_BASE_URL: https://sync.example.com
```

## 12. Schema 版本与兼容

每个 event 带：

```json
{
  "client_schema_version": 1,
  "object_schema_version": 1
}
```

服务端维护支持范围：

```json
{
  "min_client_schema_version": 1,
  "max_client_schema_version": 1
}
```

规则：

- 客户端版本太新：服务端 reject，提示升级服务端。
- 客户端版本太旧：服务端 reject，提示升级客户端。
- 新字段默认 ignore 不安全；必须按 schema version 显式处理。

## 13. 数据安全

### 13.1 传输安全

- 必须 HTTPS。
- token 不写入普通 feature_config，写入本地 secure_config。
- 服务端 token hash 存储，不存明文。

### 13.2 Secret 同步

MVP 不同步 secret。

后续如果要同步：

- 单独设计 `sync_secret_object`。
- 使用用户主密码或恢复密钥派生 E2EE key。
- 服务端只保存 ciphertext。
- 每设备本地解密后再写入设备自己的 secure_config。

不要直接上传当前 secure_config 的 ciphertext，因为它绑定设备本地密钥。

## 14. First Sync 体验

配置页显示：

- 同步服务地址。
- 当前设备名。
- 登录/Token 状态。
- 上次成功同步时间。
- pending outbox 数量。
- 最近错误。
- 手动“立即同步”。

首次启用时：

1. 检测本地可同步对象数量。
2. 检测远端对象数量。
3. 显示三种操作：
   - 合并本机与云端数据，推荐。
   - 仅上传本机数据到空云端，仅远端为空时可选。
   - 使用云端数据重建本机数据，需要先备份本地库。

不提供“静默覆盖”。

## 15. 分阶段实施

### Phase 0：验证设计

目标：不用改所有业务写路径，先验证 sync_id、push/pull、apply 能跑通。

任务：

- 新增 `src-tauri/src/sync/schema.rs` 创建本地 sync 表。
- 实现 object map、outbox、shadow。
- 对 conversation/message 做手动 export/import prototype。
- 写 server 最小 push/pull。
- 用两份临时 SQLite 模拟设备 A/B。

验收：

- A 创建 conversation/message，push 后 B pull 能看到。
- B 创建不同 conversation/message，A pull 后都存在。
- 两边本地自增 ID 不同，但关系正确。

### Phase 1：conversation/message 同步

任务：

- 接入 conversation create/update/delete。
- 接入 message create/finalize/update metadata。
- 接入 message_attachment 文本内容。
- 实现 deferred apply。
- 实现 message 冲突副本。

验收：

- 多设备离线创建不同对话，联网后合并。
- 同一对话多设备追加消息，按 created_time 展示。
- regenerate 分支关系不丢。
- 删除 conversation 后其他设备同步删除或隐藏。

### Phase 2：配置与 assistant 同步

任务：

- feature_config key 级同步。
- system_config allowlist。
- assistant/prompt/model/config 同步。
- llm provider/model 非 secret 同步。
- MCP server/tool 配置同步。

验收：

- A 修改 assistant prompt，B 同步后可见。
- A 修改 network_config 某个 key，B 不覆盖其他 key。
- API key 不出现在服务端数据库。
- 本地路径类配置不被同步。

### Phase 3：artifact/plugin/skill 元数据

任务：

- artifacts_collection 同步。
- plugin config/data allowlist。
- skill binding 同步。
- plugin/skill 文件安装状态只作为本地状态，不强制同步。

验收：

- A 保存 artifact collection，B 可见并可使用 code。
- A 给 assistant 绑定 skill，B 在已有 skill 时启用；缺失时提示安装，不静默失败。

### Phase 4：完善能力

任务：

- blob store：附件、artifact workspace 文件。
- E2EE secret vault。
- sync conflict UI。
- server admin UI。
- 数据导出/恢复。
- scheduled task 多设备执行归属。

## 16. 测试计划

### 16.1 Rust 单元测试

新增测试目录：

```
src-tauri/src/sync/tests/
├── identity_tests.rs
├── outbox_tests.rs
├── serializer_tests.rs
├── applier_tests.rs
├── conflict_tests.rs
└── two_device_sync_tests.rs
```

测试要求：

- 全部使用 `Connection::open_in_memory()`。
- 不读写真实 AIPP db。
- 每个业务域单独测试文件。

核心用例：

- sync_id 生成后稳定复用。
- 本地自增 ID 冲突时远端 apply 能重映射。
- message refs 能解析 conversation/parent message。
- 父对象缺失时进入 deferred apply。
- feature_config 不同 key 并发修改不互相覆盖。
- secret/path denylist 生效。
- delete tombstone 不复活。
- push 幂等。
- pull cursor 只有 apply 成功后推进。

### 16.2 服务端测试

核心用例：

- event_id 幂等。
- cursor 单调。
- base_version 正常接受。
- base_version 过旧触发冲突策略。
- account 隔离。
- device 撤销后不能 push/pull。

### 16.3 集成测试

用两个本地临时目录模拟两台设备：

```
tmp/sync-test/device-a/
tmp/sync-test/device-b/
tmp/sync-test/server.db
```

流程：

1. 初始化 A/B。
2. A 离线创建 conversation/message。
3. B 离线创建 conversation/message。
4. 启动 server。
5. A push/pull。
6. B push/pull。
7. A 再 pull。
8. 比较两端业务对象集合一致。

## 17. 主要风险与处理

| 风险 | 处理 |
|---|---|
| 写路径遗漏 outbox | 先覆盖高价值路径；加 nightly consistency scanner 发现未映射行 |
| 本地 ID 引用错乱 | 所有传输 refs 用 sync_id，不传本地 id |
| feature_config 覆盖 | key 级对象，不做 bucket 级覆盖 |
| secret 泄露 | 默认 denylist，服务端测试扫描敏感 key |
| 父子对象乱序 | deferred apply |
| 删除复活 | tombstone + shadow |
| 老客户端 schema 不兼容 | 服务端按 schema version reject |
| 大内容同步慢 | Phase 4 做 blob store，MVP 限制 payload 大小 |
| 多设备同时跑 scheduled task | MVP 不同步 runtime；后续加 leader/lease |

## 18. 开发顺序建议

最小可落地顺序：

1. 本地 sync 表和 device 初始化。
2. sync_id/object map。
3. conversation/message serializer。
4. outbox recorder。
5. FastAPI sync server push/pull。
6. applier 和 deferred apply。
7. conversation/message 写路径接入。
8. 两设备集成测试。
9. feature_config key 级同步。
10. assistant/llm/mcp 同步。
11. 配置页状态与手动同步。

不要一开始追求同步所有表。先把 conversation/message 的闭环做扎实，再扩展配置域。
