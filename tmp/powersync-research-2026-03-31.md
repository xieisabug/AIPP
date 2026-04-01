# PowerSync 对 AIPP 的可行性调研报告

## 1. PowerSync 是什么

PowerSync 是一个 **local-first 同步引擎**，核心架构：

```
客户端 SQLite ←→ PowerSync Service ←→ 后端数据库 (PostgreSQL / MySQL / MongoDB)
```

**不是** SQLite ↔ SQLite 文件级同步，而是：
- 后端数据库是"源真相"（source of truth）
- PowerSync Service 通过 WAL 捕获后端变更，推送给客户端
- 客户端本地写入进入 upload queue，由 SDK 自动上传到你的后端 API
- 后端 API 负责把写入应用到后端数据库

## 2. Rust SDK 和 Tauri 插件现状

### 2.1 Rust SDK (`powersync` crate)

- **状态：pre-alpha / experimental**
- 底层使用 `rusqlite`，静态链接 PowerSync SQLite 扩展
- 支持 Tokio / smol 异步运行时
- **已实现的核心能力：**
  - 实时变更流（streaming sync）
  - 本地 SQLite 直接读写
  - 异步后台同步
  - Query watch（查询订阅，数据变更自动推送）
  - Upload queue（本地写入自动排队上传）
  - 自动 schema 管理（通过 SQLite views，无需客户端迁移）

### 2.2 Tauri 插件 (`tauri-plugin-powersync`)

- NPM 包：`@powersync/tauri-plugin`
- Rust crate：`tauri-plugin-powersync`
- **架构：前端 JS 定义 schema + 读写，Rust 端负责 sync connect**
- 安装方式：
  ```toml
  # src-tauri/Cargo.toml
  tauri-plugin-powersync = "..."
  ```
  ```ts
  // 前端
  import { PowerSyncTauriDatabase } from '@powersync/tauri-plugin';
  ```
- Rust 端注册插件：
  ```rust
  tauri::Builder::default()
      .plugin(tauri_plugin_powersync::init())
  ```

### 2.3 使用模式

```rust
// Rust 端：定义 BackendConnector
struct MyBackendConnector { db: PowerSyncDatabase }

#[async_trait]
impl BackendConnector for MyBackendConnector {
    async fn fetch_credentials(&self) -> Result<PowerSyncCredentials, PowerSyncError> {
        // 返回 PowerSync Service 的 endpoint + auth token
    }
    async fn upload_data(&self) -> Result<(), PowerSyncError> {
        // 从 upload queue 读取本地变更，调用你的后端 API 写入后端数据库
        let mut local_writes = self.db.crud_transactions();
        while let Some(tx) = local_writes.try_next().await? {
            // 检查 tx.crud 中的变更，调用后端 API
            tx.complete().await?;
        }
    }
}

// 启动同步
db.connect(SyncOptions::new(MyBackendConnector { db: db.clone() })).await;
```

## 3. 自建部署

### 3.1 服务端组件

PowerSync Service 可完全自建，使用 Docker Compose：

```yaml
services:
  powersync:
    image: journeyapps/powersync-service:latest
    command: ["start", "-r", "unified"]
    ports:
      - "8080:8080"
    volumes:
      - ./service.yaml:/config/service.yaml
      - ./sync-config.yaml:/config/sync-config.yaml

  postgres:
    image: postgres:latest
    command: ["postgres", "-c", "wal_level=logical"]

  mongo:  # 用于 sync bucket 存储
    image: mongo:7.0
```

### 3.2 许可证

- **Open Edition**：Functional Source License (FSL)
- **可以免费自建使用**，唯一限制是不能把 PowerSync 作为 SaaS 对外售卖
- 客户端 SDK 是 Apache 2.0 / MIT 开源
- **对 AIPP 来说完全没有许可问题**

### 3.3 部署要求

| 组件 | 说明 |
|------|------|
| PostgreSQL | 后端源数据库，需开启 logical replication |
| MongoDB | PowerSync 内部 bucket 存储（也可用 PG 替代） |
| PowerSync Service | Docker 容器，负责变更捕获和分发 |

## 4. 冲突解决机制

- **默认策略：Last-Write-Wins (LWW)**
- 客户端写入先进 upload queue，联网后上传到后端 API
- **冲突在服务端/后端 API 层解决**，不是在 PowerSync Service 内部
- 支持自定义合并策略（vector clock / CRDT-like）
- Atomic checkpoints 确保客户端只有在变更被确认后才推进
- 如果后端拒绝（约束冲突等），开发者可以自定义重试/通知逻辑

## 5. 对 AIPP 的适配性评估

### 5.1 ⚠️ 核心挑战：PowerSync 管理自己的 SQLite 数据库

**这是最关键的一点。**

PowerSync SDK **不能接管现有的 SQLite 数据库**。它需要：
- 自己创建和管理一个新的 SQLite 数据库文件
- 通过 SQLite views 暴露 schema
- 所有读写必须通过 PowerSync 的 `reader()` / `writer()` API

这意味着 **AIPP 现有的整个数据库层需要重写**：

| AIPP 现状 | PowerSync 要求 |
|-----------|---------------|
| 7 个独立 SQLite 数据库文件 | PowerSync 管理的单一/多个数据库文件 |
| libsql 0.9 + 自定义 Connection 封装 | PowerSync 的 `PowerSyncDatabase` API |
| 直接 SQL 执行（`conn.execute`） | 通过 `db.reader()` / `db.writer()` |
| 自定义迁移系统（0.0.1 → 0.0.11） | PowerSync 自动 schema 管理（无迁移） |
| 每个 DB 有独立 namespace 同步 | PowerSync 的 Sync Rules / Streams 定义同步范围 |

### 5.2 ⚠️ 需要一个后端数据库 + API

PowerSync 的写回模型是：

```
客户端 → upload_data() → 你的后端 API → PostgreSQL
```

**AIPP 目前没有后端 API 服务。** 要用 PowerSync，你需要：

1. 部署一个 PostgreSQL 数据库
2. 建立对应的 schema（从 AIPP 的 SQLite schema 转换）
3. 写一个后端 API 服务（接收 PowerSync 客户端上传的 CRUD 变更）
4. PowerSync Service 从 PG WAL 捕获变更推送给其他客户端

### 5.3 ⚠️ Rust SDK 处于 pre-alpha

- 明确标注 **不建议用于生产**
- API 可能随时变更
- Tauri 插件更是实验性质
- 社区反馈驱动，未来方向不确定

### 5.4 ✅ 能满足的需求

| AIPP 需求 | PowerSync 能力 |
|-----------|---------------|
| 客户端本地 SQLite | ✅ 原生支持 |
| 多端同步 | ✅ 核心能力 |
| 离线写入 | ✅ local-first 设计 |
| 自建部署 | ✅ Open Edition 免费自建 |
| Rust + Tauri | ✅ 有专门的 SDK 和插件 |
| 冲突处理 | ✅ 服务端解决 + 自定义策略 |

### 5.5 ❌ 不匹配的点

| 问题 | 影响 |
|------|------|
| 不能接管现有 SQLite DB | 需要完全重写数据层 |
| 需要后端 PG + API 服务 | 增加部署复杂度，AIPP 目前纯客户端 |
| SDK pre-alpha | 生产风险高 |
| 写回走后端 API 而非直接数据库 | 需要额外开发后端服务 |
| secure_config 等敏感数据 | 不应该同步，需要仔细分层 |

## 6. AIPP 如果用 PowerSync，改造量评估

### 6.1 需要新增的组件

1. **PostgreSQL 后端数据库** — 存放需要同步的表
2. **后端 API 服务** — 接收客户端写入，应用到 PG（可以用 Rust/Python/Node）
3. **PowerSync Service** — Docker 部署
4. **Sync Rules 配置** — 定义哪些表/字段同步给哪些用户

### 6.2 需要重写的 AIPP 代码

| 模块 | 工作量 | 说明 |
|------|--------|------|
| `src-tauri/src/db/connection.rs` | **重写** | 整个 Connection 抽象层替换为 PowerSync API |
| `src-tauri/src/db/sync_manager.rs` | **重写** | 改用 PowerSync connect 机制 |
| 所有 `*_db.rs` 文件 | **大改** | 所有 SQL 执行方式从 `conn.execute` 改为 `db.reader()`/`db.writer()` |
| `lib.rs` 启动逻辑 | **重写** | 初始化 PowerSync 而非 libsql |
| `system_api.rs` 同步相关 | **重写** | 配置和触发方式全部变化 |
| 前端同步配置 UI | **调整** | 配置项变化（PG 地址 → PowerSync endpoint） |
| **新增：后端 API 服务** | **新建** | 处理客户端写回 |

### 6.3 不需要同步的数据（保持本地）

根据 AIPP 数据分层：
- `secure_config` — 加密凭证，设备级别
- `mcp_tool_call` — 执行日志，设备级别
- `scheduled_task_run` / `scheduled_task_log` — 执行记录
- `PluginData` — 插件运行时数据
- 部分 `system_config` — 设备级配置（如窗口位置、主题等）

### 6.4 应该同步的核心数据

- `conversation` + `message` + `message_attachment` — 对话和消息
- `assistant` + `assistant_model` + `assistant_prompt` — 助手配置
- `llm_provider` + `llm_model` + `llm_provider_config` — 模型配置
- `mcp_server` + `mcp_server_tool` — MCP 服务配置
- `artifacts_collection` — 代码工件
- `feature_config` — 功能配置
- `scheduled_task` — 定时任务定义（不含运行记录）

## 7. 结论

### 7.1 PowerSync 技术上可行吗？

**理论上可行，但代价很大。**

PowerSync 的同步引擎本身是成熟的产品（在 Flutter/React Native 生态已经稳定），但它对 AIPP 意味着：

1. **全面替换数据库层** — 不是修修补补，是从 libsql Connection 到 PowerSync Database 的完整切换
2. **引入后端服务** — AIPP 从纯桌面应用变成 "桌面端 + 后端 API + PG + PowerSync Service + MongoDB" 的分布式架构
3. **Rust SDK 不成熟** — pre-alpha 状态下做生产级集成风险高

### 7.2 PowerSync vs 自研应用层同步 对比

| 维度 | PowerSync | 自研应用层同步 |
|------|-----------|---------------|
| **改造量** | 极大（重写数据层 + 新增后端） | 大（oplog + sync API） |
| **部署复杂度** | 高（PG + MongoDB + PowerSync + 后端 API） | 中（一个同步 API 服务） |
| **SDK 成熟度** | pre-alpha | 自己控制 |
| **冲突处理** | 内置 LWW + 可自定义 | 需要自己设计 |
| **离线支持** | 原生 | 需要自己实现 |
| **长期可控性** | 受限于 PowerSync 路线 | 完全可控 |
| **落地速度** | 慢（需要同时搞定太多新组件） | 中（可渐进式实现） |

### 7.3 我的建议

**PowerSync 目前不适合作为 AIPP 的首选方案。** 原因：

1. **Rust SDK 太新了** — pre-alpha 意味着 API 不稳定，遇到问题没有足够的社区支持
2. **改造成本远超预期** — 不是"接一个 SDK"的事，是要引入 PG + 后端 API + PowerSync Service 一整套
3. **AIPP 的数据模型不天然适配** — PowerSync 更适合 "后端已有 PG 数据库" 的场景，AIPP 是纯客户端 SQLite 起步

**更务实的路线仍然是自研应用层同步**：
- 可以渐进实现（先同步 conversation，再加其他表）
- 不引入额外基础设施依赖
- 完全可控，数据层改动最小
- 后端只需一个简单的 sync API（可以用 SQLite 或 PG）

**如果未来 PowerSync Rust SDK 到 beta/stable，可以重新评估。**
