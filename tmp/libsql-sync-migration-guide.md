# AIPP 多端同步方案：libSQL Embedded Replica 迁移指南

## 一、方案概述

### 核心架构

```
┌──────────────────┐        HTTP/WS         ┌──────────────────┐
│  AIPP Desktop A  │  ◄──── WAL Sync ────►  │   sqld 服务器     │
│  (libsql crate)  │                         │  (自建, Docker)   │
│  本地 .db 文件    │                         │  多 namespace     │
└──────────────────┘                         │  ┌─conversation─┐│
                                             │  ├─assistant────┤│
┌──────────────────┐                         │  ├─system───────┤│
│  AIPP Desktop B  │  ◄──── WAL Sync ────►  │  ├─llm──────────┤│
│  (libsql crate)  │                         │  └─skill────────┘│
└──────────────────┘                         └──────────────────┘
                                                    ▲
┌──────────────────┐                                │
│  AIPP Mobile     │  ◄──── WAL Sync ───────────────┘
│  (Tauri 2.0)     │
│  同一套 Rust 代码 │
│  iOS / Android   │
└──────────────────┘
```

**核心思路：Local-First + Embedded Replica**

- 每台设备保留完整的本地 SQLite 数据库文件，读写都走本地（零延迟）
- 通过 WAL 帧增量同步到自建的 sqld 服务器
- 其他设备从 sqld 拉取增量，实现多端数据一致
- 不依赖 Turso 云服务，完全自建

---

## 二、用户关心的三个核心问题

### Q1: 已有多台设备的旧数据能合并吗？

**⚠️ 需要说明一个重要事实：libsql embedded replica 不是 CRDT，无法自动合并两份独立数据库。**

libsql 的同步模型是 **Primary-Replica（主从）模式**：
- sqld 服务器上的数据库是 **Primary（主库）**
- 每台设备的本地 .db 是 **Replica（副本）**
- 同步方向：Primary ↔ Replica，而非 Replica ↔ Replica

这意味着：如果你在 A 机器和 B 机器上各有一份独立的本地数据，**不能自动合并成一份**。第一台设备上传后，第二台设备开启同步时，远端数据会**覆盖**本地数据。

#### 推荐交互设计：「选择主设备」流程

```
┌─────────────────────────────────────────────────┐
│          首次开启云同步                            │
│                                                   │
│  检测到您在此设备上有本地数据。                       │
│  云端服务器当前状态：                               │
│                                                   │
│  ┌─────────────────────────────────────────┐      │
│  │  ○ 云端为空（首次使用同步）               │      │
│  │     → 将本设备数据上传到云端              │      │
│  │                                         │      │
│  │  ○ 云端已有数据（来自其他设备）           │      │
│  │     → 选择下列操作：                     │      │
│  │     ┌──────────────────────────────┐    │      │
│  │     │ 📥 使用云端数据（推荐）        │    │      │
│  │     │    丢弃本设备数据，同步云端     │    │      │
│  │     ├──────────────────────────────┤    │      │
│  │     │ 📤 上传本设备数据              │    │      │
│  │     │    替换云端数据（其他设备会同步）│    │      │
│  │     ├──────────────────────────────┤    │      │
│  │     │ 📋 导出本地数据后再同步        │    │      │
│  │     │    先备份，再使用云端数据       │    │      │
│  │     └──────────────────────────────┘    │      │
│  └─────────────────────────────────────────┘      │
└─────────────────────────────────────────────────┘
```

#### 实现逻辑

```rust
pub enum FirstSyncAction {
    /// 云端为空，将本地数据上传
    UploadToEmpty,
    /// 云端已有数据，用云端覆盖本地
    UseCloudData,
    /// 云端已有数据，用本地覆盖云端
    OverwriteCloud,
    /// 云端已有数据，将本地数据追加合并到云端
    AppendToCloud,
    /// 先导出本地数据再同步
    ExportThenSync,
}

async fn handle_first_sync(
    local_db_path: &Path,
    sync_url: &str,
    auth_token: &str,
    action: FirstSyncAction,
) -> Result<libsql::Database> {
    match action {
        FirstSyncAction::UploadToEmpty | FirstSyncAction::OverwriteCloud => {
            // 本地数据作为主数据：
            // 1. 先将本地 .db 内容通过 HTTP API 推送到 sqld（需要实现上传逻辑）
            // 2. 然后建立 replica 同步关系
            upload_local_db_to_server(local_db_path, sync_url, auth_token).await?;
            let db = Builder::new_local_replica(local_db_path)
                .sync_url(sync_url, Some(auth_token.into()))
                .build().await?;
            Ok(db)
        }
        FirstSyncAction::UseCloudData => {
            // 云端数据覆盖本地：
            // 1. 备份旧本地文件（以防万一）
            // 2. 删除本地 .db，让 replica 从云端全量同步
            backup_local_db(local_db_path)?;
            std::fs::remove_file(local_db_path).ok();
            let db = Builder::new_local_replica(local_db_path)
                .sync_url(sync_url, Some(auth_token.into()))
                .build().await?;
            db.sync().await?; // 从云端拉取全量数据
            Ok(db)
        }
        FirstSyncAction::ExportThenSync => {
            // 1. 导出为 JSON/CSV 文件供用户保留
            export_db_as_json(local_db_path, &export_path)?;
            // 2. 然后走 UseCloudData 路径
            // ...
        }
        FirstSyncAction::AppendToCloud => {
            // 将本地数据追加到云端（ID 重映射，详见下方 Append 模式说明）
            append_local_to_cloud(local_db_path, sync_url, auth_token).await?;
            // 追加完成后建立正常 replica 同步关系
            let db = Builder::new_local_replica(local_db_path)
                .sync_url(sync_url, Some(auth_token.into()))
                .build().await?;
            db.sync().await?;
            Ok(db)
        }
    }
}
```

#### Append 模式（合并本地数据到云端）

**✅ 可以做，但需要应用层实现 ID 重映射。**

##### 为什么不能直接 INSERT？

AIPP 所有表都使用 `INTEGER PRIMARY KEY AUTOINCREMENT`。两台设备独立使用后，ID 必然冲突：

```
设备 A: conversation id=1,2,3,4,5     message id=1,2,3...100
设备 B: conversation id=1,2,3         message id=1,2,3...50
                      ↑ 完全不同的数据，但 ID 相同
```

直接 INSERT 会因为主键冲突失败，或者因为外键错乱导致数据关系断裂。

##### Append 的实现思路：ID 重映射

```rust
/// Append 模式：将本地数据追加到云端已有数据中
async fn append_local_to_cloud(
    local_db_path: &Path,
    sync_url: &str,
    auth_token: &str,
) -> Result<()> {
    // 1. 打开本地旧数据库（只读）
    let local_conn = Connection::open(local_db_path)?;

    // 2. 连接到云端数据库
    let cloud = Builder::new_remote(sync_url, auth_token).build().await?;
    let cloud_conn = cloud.connect()?;

    // 3. 查询云端各表当前 max(id)，作为偏移基数
    let offsets = get_max_ids(&cloud_conn).await?;
    // 例如: { "conversation": 500, "message": 10000, "assistant": 20 }

    // 4. 按依赖顺序导入，逐表重映射 ID
    // 先导入父表，再导入子表
    let conversation_id_map = append_table_with_remap(
        &local_conn, &cloud_conn,
        "conversation",
        offsets["conversation"],
        &[], // 无外键依赖
    ).await?;

    let message_id_map = append_table_with_remap(
        &local_conn, &cloud_conn,
        "message",
        offsets["message"],
        &[("conversation_id", &conversation_id_map)], // 外键重映射
    ).await?;

    append_table_with_remap(
        &local_conn, &cloud_conn,
        "message_attachment",
        offsets["message_attachment"],
        &[("message_id", &message_id_map)],
    ).await?;

    // ... 其他子表同理

    // 5. 完成后建立正常 replica 同步关系
    Ok(())
}

/// 将一张表的数据追加到云端，返回 old_id → new_id 映射
async fn append_table_with_remap(
    local: &Connection,
    cloud: &libsql::Connection,
    table: &str,
    id_offset: i64,
    fk_remaps: &[(&str, &HashMap<i64, i64>)],
) -> Result<HashMap<i64, i64>> {
    let mut id_map = HashMap::new();
    let rows = local.prepare(&format!("SELECT * FROM {}", table))?
        .query_map([], |row| { /* ... */ })?;

    for row in rows {
        let old_id = row.get_id();
        let new_id = old_id + id_offset;
        id_map.insert(old_id, new_id);

        // 替换 ID 和外键引用后 INSERT
        let mut values = row.clone_values();
        values.set_id(new_id);
        for (fk_col, remap) in fk_remaps {
            if let Some(old_fk) = values.get(fk_col) {
                values.set(fk_col, remap[&old_fk]);
            }
        }
        cloud.execute(&insert_sql(table, &values), values.params()).await?;
    }
    Ok(id_map)
}
```

##### Append 的表依赖顺序

```
conversation (先)
  ├─ message (conversation_id → conversation.id)
  │    ├─ message_attachment (message_id → message.id)
  │    └─ mcp_tool_call (message_id → message.id)  [在 mcp.db 中]
  ├─ conversation_summary
  ├─ conversation_todo
  ├─ butler_main_state
  ├─ butler_task_definition
  └─ butler_task_result

assistant (先)
  ├─ assistant_model
  ├─ assistant_prompt
  │    └─ assistant_prompt_param
  ├─ assistant_model_config
  ├─ assistant_mcp_config
  ├─ assistant_mcp_tool_config
  ├─ assistant_workspace
  └─ assistant_summary

llm_provider (先)
  └─ llm_model (llm_provider_id → llm_provider.id)
      └─ llm_provider_config
```

##### Append 的交互设计

在首次同步的选项中增加第四个选项：

```
┌──────────────────────────────────────┐
│ 📥 使用云端数据（推荐）               │
│    丢弃本设备数据，同步云端            │
├──────────────────────────────────────┤
│ 📤 上传本设备数据                     │
│    替换云端数据（其他设备会同步）       │
├──────────────────────────────────────┤
│ 🔀 合并数据（Append）                 │  ← 新增
│    将本设备数据追加到云端              │
│    ⚠️ 可能产生重复的对话/助手          │
├──────────────────────────────────────┤
│ 📋 导出本地数据后再同步               │
│    先备份，再使用云端数据              │
└──────────────────────────────────────┘
```

> ⚠️ **Append 的局限**：由于两边可能有相同内容（比如都导入了同一个助手模板），Append 后可能出现重复记录。可以用 `created_time` + 内容哈希做去重，但不保证 100% 完美。建议 UI 中提示用户 "合并后可能需要手动删除重复项"。

> 💡 **长期方案**：考虑将核心表的主键从 `INTEGER AUTOINCREMENT` 迁移到 **UUID (TEXT)**。这样不同设备产生的数据天然不会冲突，Append 变成简单的 `INSERT OR IGNORE`。此迁移可以在后续版本中渐进完成。

#### 实际操作建议

对于大多数用户来说，最合理的流程是：

1. **选一台数据最全的设备**作为"主设备"，首先开启同步 → 数据上传到 sqld
2. **其他设备**开启同步时选择"使用云端数据" → 自动同步到最新数据
3. 如果其他设备上有独特数据不想丢，选择"合并数据"或先"导出"再同步

> 💡 **关于文件格式兼容**：libSQL 是 SQLite 超集 fork，旧的 rusqlite 生成的 .db 文件可以直接被 libsql 读取，无需任何格式转换。

### Q2: 用户关闭同步会有问题吗？

**✅ 完全没问题。libsql 原生支持三种运行模式：**

| 模式 | 配置方式 | 行为 | 适用场景 |
|------|---------|------|---------|
| **纯本地模式** | `Builder::new_local(path)` | 等同于普通 SQLite，不做任何网络操作 | 用户关闭同步 |
| **手动同步模式** | `Builder::new_local_replica(path).sync_url(...)` | 本地读写即时生效，用户手动触发 `db.sync()` | 用户想控制同步时机 |
| **自动同步模式** | 同上 + `sync_interval(Duration)` | 后台定时自动同步 | 用户开启同步 |

**对应 AIPP 的设计：**

```rust
pub enum SyncMode {
    Disabled,                    // 纯本地，不联网
    Manual,                      // 用户点"同步"按钮时才同步
    Auto { interval_secs: u64 }, // 自动定时同步（如 60 秒一次）
}

// 用户可以随时在设置中切换，无需重启应用
// 关闭同步 → 所有操作完全本地，无性能影响
// 开启同步 → 增量同步，不会丢失关闭期间的数据
```

**关键保证：**
- 关闭同步期间的所有写入都保存在本地 .db 文件中
- 重新开启同步后，调用 `db.sync()` 会自动增量推送所有变更
- 不会丢数据，不会冲突（单用户多设备场景，Last-Write-Wins 足够）

### Q3: 移动端 (Android / iOS) 能同步吗？

**✅ 可以。AIPP 移动端基于 Tauri 2.0，共享同一套 Rust 后端代码。**

由于 AIPP 的移动端同样使用 Tauri 2.0 构建（而非原生 Swift/Kotlin），所以移动端和桌面端**共用完全相同的 Rust 代码**，包括 libsql crate。

#### 架构优势

```
┌─────────────────────────────────────────────────┐
│                 共享 Rust 后端                     │
│                                                   │
│  ┌──────────────┐  ┌──────────────┐              │
│  │ libsql crate │  │  DB 模块      │              │
│  │ (同步逻辑)    │  │ (CRUD 操作)   │              │
│  └──────────────┘  └──────────────┘              │
│          │                    │                    │
│  ┌───────┴────────────────────┴──────────┐       │
│  │         Tauri Command Layer           │       │
│  └───────┬─────────┬──────────┬──────────┘       │
│          │         │          │                    │
│     ┌────┴───┐ ┌───┴────┐ ┌──┴─────┐            │
│     │macOS/  │ │Android │ │  iOS   │            │
│     │Windows/│ │WebView │ │WKWebView│            │
│     │Linux   │ │        │ │        │            │
│     └────────┘ └────────┘ └────────┘            │
└─────────────────────────────────────────────────┘
```

#### 移动端注意事项

1. **libsql crate 交叉编译**：需要确保 libsql 能编译到 `aarch64-linux-android` 和 `aarch64-apple-ios` 目标平台。libsql 基于 SQLite（纯 C），在移动端有很长的运行历史，理论上无问题，但需要实际测试构建
2. **文件系统权限**：移动端的数据库文件需要存放在应用沙盒目录内，通过 `tauri::api::path::app_data_dir()` 获取（与桌面端一致）
3. **后台同步限制**：iOS 对后台网络操作有限制，建议在移动端使用手动同步或在 App 进入前台时触发同步，而非依赖定时后台同步
4. **代码复用率 ≈ 100%**：DB 层代码、同步逻辑、API 层代码在桌面和移动端完全一致，无需为移动端写额外代码

#### 潜在风险

| 风险 | 概率 | 应对策略 |
|------|------|---------|
| libsql crate 交叉编译失败 | 低 | libsql 底层是 C 代码，SQLite 在 Android/iOS 上经过充分验证；如遇问题可回退到 rusqlite + 自建同步层 |
| 移动端 SQLite 文件锁定问题 | 低 | 使用 WAL 模式 + `busy_timeout` PRAGMA（当前已配置） |
| iOS 后台同步被系统杀死 | 中 | 采用前台触发同步策略；或使用 iOS BackgroundTasks API（需 Tauri 插件） |

---

## 三、自建服务器方案

### sqld 部署（一行 Docker 命令）

```bash
docker run -d \
  --name aipp-sync-server \
  -p 8080:8080 \
  -v /data/aipp-db:/var/lib/sqld \
  -e SQLD_AUTH_JWT_KEY="your-ed25519-public-key" \
  ghcr.io/tursodatabase/libsql-server:latest
```

### 免费/低成本托管推荐

| 方案 | 成本 | 推荐度 | 备注 |
|------|------|--------|------|
| **Fly.io 免费 VM** | 免费（3 shared VMs） | ⭐⭐⭐ | 最简单，原生 Docker 支持，全球节点 |
| **Oracle Cloud 永久免费 ARM** | 免费（4核 24GB） | ⭐⭐⭐ | 性能最强免费方案 |
| **家用 NAS + Cloudflare Tunnel** | 免费 | ⭐⭐ | 零成本但需要穿透配置 |
| **Railway / Render** | 有限免费额度 | ⭐⭐ | 简单但免费额度可能不够 |
| **自购 VPS（Vultr/Hetzner）** | ~$3-5/月 | ⭐⭐⭐ | 最稳定，适合付费用户 |

> ⚠️ **Cloudflare Workers 不适合**：sqld 是长驻进程 + 需要本地文件系统，Workers 是无状态 serverless。但可以用 Cloudflare Tunnel 穿透到自建服务器。

### 鉴权方案与交互设计

#### 核心原则：用户只需按一个按钮

普通用户不会也不应该手动运行 `openssl` 生成密钥。无论是自建还是官方服务，鉴权对用户来说必须是**一键完成**的。

---

#### 场景一：AIPP 官方云同步服务（面向普通用户，推荐优先实现）

##### 用户交互流程

```
┌─────────────────────────────────────────────────────┐
│                  设置 > 云同步                         │
│                                                       │
│  ┌─────────────────────────────────────────────────┐  │
│  │                                                   │  │
│  │  [ ☁️  开启 AIPP 云同步 ]                         │  │
│  │                                                   │  │
│  │    点击后跳转浏览器 →                              │  │
│  │    使用 GitHub / Google / 邮箱 登录或注册          │  │
│  │    → 自动完成所有配置                              │  │
│  │                                                   │  │
│  │  ─── 或者 ───                                     │  │
│  │                                                   │  │
│  │  [ ⚙️  连接自建服务器 ]                            │  │
│  │    适合高级用户                                    │  │
│  │                                                   │  │
│  └─────────────────────────────────────────────────┘  │
│                                                       │
│  已同步后：                                            │
│  ┌─────────────────────────────────────────────────┐  │
│  │  同步状态: 🟢 已连接 (cloud.aipp.dev)             │  │
│  │  上次同步: 30 秒前                                 │  │
│  │  同步模式: [自动 ▾]  同步间隔: [60秒 ▾]           │  │
│  │                                                   │  │
│  │  [ 立即同步 ]  [ 断开连接 ]                        │  │
│  └─────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

##### 官方服务后端架构

```
┌──────────┐     HTTPS      ┌───────────────────────────────────┐
│ AIPP 客户端│ ◄────────────► │         AIPP Sync Service          │
│          │               │                                     │
│          │               │  ┌─────────────┐  ┌──────────────┐ │
│          │  OAuth/Login  │  │ Auth Gateway │  │ User Manager │ │
│          │ ─────────────►│  │ (JWT 签发)   │  │ (注册/配额)   │ │
│          │               │  └──────┬──────┘  └──────────────┘ │
│          │               │         │                           │
│          │  WAL Sync     │  ┌──────▼──────────────────────┐   │
│          │ ◄────────────►│  │     sqld (多 namespace)      │   │
│          │               │  │                              │   │
│          │               │  │  user_abc/conversation       │   │
│          │               │  │  user_abc/assistant          │   │
│          │               │  │  user_abc/system             │   │
│          │               │  │  user_xyz/conversation       │   │
│          │               │  │  ...                         │   │
│          │               │  └──────────────────────────────┘   │
└──────────┘               └───────────────────────────────────┘
```

##### 技术实现

```rust
// ========== 1. Auth Gateway（独立 HTTP 服务） ==========

/// 用户登录后，Auth Gateway 自动完成：
/// 1. 验证 OAuth token（GitHub/Google）或邮箱密码
/// 2. 查找或创建用户记录
/// 3. 为该用户生成 sqld JWT token（绑定 namespace 前缀）
/// 4. 返回给客户端所需的全部配置

#[derive(Serialize)]
struct SyncConfig {
    sync_url: String,        // "https://cloud.aipp.dev"
    auth_token: String,      // 自动签发的 JWT
    user_id: String,         // 用户唯一标识
    namespaces: Vec<String>, // ["user_abc/conversation", "user_abc/assistant", ...]
    expires_at: i64,         // Token 过期时间
}

/// 客户端调用：POST /auth/login
async fn login(provider: &str, oauth_token: &str) -> Result<SyncConfig> {
    // 1. 验证 OAuth token
    let user = verify_oauth(provider, oauth_token).await?;

    // 2. 使用服务端私钥签发 JWT（sqld 用对应公钥验证）
    let jwt = sign_jwt(&server_private_key, Claims {
        sub: user.id.clone(),
        // 限制该 token 只能访问此用户的 namespace
        namespace_prefix: format!("user_{}", user.id),
        exp: now() + Duration::days(30),
    })?;

    // 3. 确保 sqld 上该用户的 namespace 已存在（首次自动创建）
    ensure_namespaces(&user.id).await?;

    Ok(SyncConfig {
        sync_url: "https://cloud.aipp.dev".into(),
        auth_token: jwt,
        user_id: user.id,
        namespaces: vec![
            format!("user_{}/conversation", user.id),
            format!("user_{}/assistant", user.id),
            format!("user_{}/system", user.id),
            format!("user_{}/llm", user.id),
            format!("user_{}/skill", user.id),
        ],
        expires_at: (now() + Duration::days(30)).timestamp(),
    })
}

// ========== 2. 客户端一键流程 ==========

/// 用户点击"开启云同步"按钮后的完整流程
async fn enable_cloud_sync(app: &AppHandle) -> Result<()> {
    // 1. 打开浏览器进行 OAuth 登录
    let oauth_result = oauth_login_via_browser(app).await?;

    // 2. 用 OAuth token 换取同步配置（一个 HTTP 请求）
    let config: SyncConfig = reqwest::Client::new()
        .post("https://cloud.aipp.dev/auth/login")
        .json(&json!({
            "provider": oauth_result.provider,
            "token": oauth_result.access_token,
        }))
        .send().await?
        .json().await?;

    // 3. 将配置安全存储到本地（Keychain/Credential Manager）
    save_sync_config_secure(app, &config)?;

    // 4. 用获取的配置初始化所有数据库同步
    init_sync_databases(app, &config).await?;

    // 完成！用户全程只点了一个按钮 + 浏览器授权
    Ok(())
}
```

##### Token 自动刷新

```rust
/// 客户端在后台自动刷新即将过期的 token
async fn auto_refresh_token(config: &mut SyncConfig) -> Result<()> {
    if config.expires_at - now().timestamp() < 7 * 86400 {
        // 还有不到 7 天过期，静默刷新
        let new_config: SyncConfig = reqwest::Client::new()
            .post("https://cloud.aipp.dev/auth/refresh")
            .bearer_auth(&config.auth_token)
            .send().await?
            .json().await?;
        *config = new_config;
        save_sync_config_secure(app, config)?;
    }
    Ok(())
}
```

---

#### 场景二：自建服务器（高级用户）

对于自建用户，也要做到**一键生成**，而不是让用户去跑 openssl 命令：

##### 交互流程

```
┌─────────────────────────────────────────────────────┐
│            设置 > 云同步 > 连接自建服务器               │
│                                                       │
│  步骤 1: 部署服务器                                    │
│  ┌─────────────────────────────────────────────────┐  │
│  │  在你的服务器上运行以下命令：                       │  │
│  │                                                   │  │
│  │  ┌─────────────────────────────────────────┐     │  │
│  │  │ docker run -d -p 8080:8080 \            │     │  │
│  │  │   ghcr.io/tursodatabase/libsql-server   │     │  │
│  │  └─────────────────────────────────────────┘     │  │
│  │                                   [ 📋 复制 ]     │  │
│  └─────────────────────────────────────────────────┘  │
│                                                       │
│  步骤 2: 连接                                          │
│  ┌─────────────────────────────────────────────────┐  │
│  │  服务器地址: ┌────────────────────────────┐      │  │
│  │              │ https://my-server.com:8080  │      │  │
│  │              └────────────────────────────┘      │  │
│  │                                                   │  │
│  │  [ 🔗 一键配置鉴权 ]                              │  │
│  │    ↓                                              │  │
│  │  AIPP 自动完成以下操作：                           │  │
│  │  ✓ 生成 Ed25519 密钥对                            │  │
│  │  ✓ 将公钥上传到你的 sqld 服务器                    │  │
│  │  ✓ 生成并保存 JWT token                           │  │
│  │  ✓ 测试连接                                       │  │
│  │                                                   │  │
│  │  同步模式: [自动 ▾]                                │  │
│  │                                                   │  │
│  │  [ 开始同步 ]                                      │  │
│  └─────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

##### 一键鉴权的实现

```rust
/// 用户只需输入服务器地址，点一个按钮
async fn setup_self_hosted_auth(server_url: &str) -> Result<SyncCredentials> {
    // 1. AIPP 在本地自动生成 Ed25519 密钥对
    let keypair = Ed25519KeyPair::generate();

    // 2. 通过 sqld 的管理 API 上传公钥
    //    （sqld 首次启动如果没有配置鉴权，可以通过 admin API 设置）
    let admin_url = format!("{}/admin/auth/setup", server_url);
    reqwest::Client::new()
        .post(&admin_url)
        .json(&json!({ "public_key_pem": keypair.public_key_pem() }))
        .send().await?;

    // 3. 用私钥签发 JWT token
    let jwt = sign_jwt(&keypair.private_key, Claims {
        sub: "aipp-user".into(),
        exp: now() + Duration::days(365),
    })?;

    // 4. 安全存储到系统 Keychain
    let creds = SyncCredentials {
        server_url: server_url.into(),
        auth_token: jwt,
        private_key_pem: keypair.private_key_pem(), // 用于续期
    };
    save_to_keychain(&creds)?;

    Ok(creds)
}
```

> 💡 **如果 sqld 已经配置了鉴权**（高级场景），用户可以展开"高级设置"手动粘贴 token。但默认路径应该是全自动的。

---

#### 安全设计总结

| 维度 | 官方服务 | 自建服务器 |
|------|---------|-----------|
| **用户操作** | 点击"开启" → 浏览器 OAuth → 完成 | 输入地址 → 点击"一键配置" → 完成 |
| **密钥管理** | 服务端自动管理 | AIPP 客户端自动生成+上传 |
| **Token 签发** | Auth Gateway 签发 | 客户端本地签发 |
| **Token 存储** | macOS Keychain / Win Credential / Linux Secret Service | 同左 |
| **Token 续期** | 自动静默刷新（30天） | 长期 Token（365天）+ 本地续期 |
| **传输加密** | HTTPS 强制 | 建议 HTTPS（Cloudflare Tunnel 可免费） |
| **数据隔离** | namespace 前缀按 user_id | 单用户独占 sqld 实例 |

---

## 三点五、AIPP 官方云同步服务架构设计

如果你要构建一个面向普通用户的官方同步服务，以下是完整的架构和实现方案。

### 服务组成

```
┌─────────────────────────────────────────────────────────┐
│                  AIPP Sync Cloud                          │
│                                                           │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐ │
│  │ Auth Gateway  │   │ User Manager │   │ Admin Panel  │ │
│  │ (Rust/Axum)  │   │ (用户/配额)   │   │ (监控/管理)   │ │
│  └──────┬───────┘   └──────┬───────┘   └──────────────┘ │
│         │                  │                              │
│         ▼                  ▼                              │
│  ┌──────────────────────────────────────────────────┐    │
│  │              PostgreSQL / SQLite                   │    │
│  │         (用户表、配额表、审计日志)                    │    │
│  └──────────────────────────────────────────────────┘    │
│         │                                                 │
│         ▼                                                 │
│  ┌──────────────────────────────────────────────────┐    │
│  │           sqld (多 namespace 模式)                 │    │
│  │                                                    │    │
│  │  user_abc/conversation  user_abc/assistant  ...   │    │
│  │  user_xyz/conversation  user_xyz/assistant  ...   │    │
│  │                                                    │    │
│  │  每用户 ≈ 5-8 个 namespace (对应 AIPP 的各 .db)    │    │
│  └──────────────────────────────────────────────────┘    │
│                                                           │
│  部署在: Fly.io / Hetzner / 任意 VPS + Docker             │
└─────────────────────────────────────────────────────────┘
```

### API 设计

```
POST   /auth/register        # 注册（邮箱/OAuth）
POST   /auth/login            # 登录，返回 SyncConfig
POST   /auth/refresh          # Token 续期
GET    /auth/me               # 查询当前用户信息和配额

GET    /sync/status           # 查询同步状态（各 namespace 数据量）
POST   /sync/namespaces       # 查询/创建用户的 namespace 列表
DELETE /sync/data              # 用户删除云端数据

# sqld 的 WAL Sync 端口直接暴露给客户端（经 JWT 鉴权）
# 客户端通过 libsql embedded replica 协议直连 sqld
```

### 用户配额管理

```rust
struct UserQuota {
    max_total_size_mb: u64,     // 免费用户: 100MB, 付费: 10GB
    max_namespaces: u32,        // 免费: 6, 付费: 20
    max_devices: u32,           // 免费: 3, 付费: 10
    sync_interval_min_secs: u32,// 免费: 300s, 付费: 30s
}
```

### 成本估算（面向开发者的参考）

| 规模 | 用户数 | 存储 | 服务器 | 月成本 |
|------|--------|------|--------|--------|
| 起步 | < 100 | < 10GB | 1x Fly.io 免费 VM | **$0** |
| 小型 | 100-1000 | 10-100GB | 1x 4GB VPS | **$5-10** |
| 中型 | 1000-10000 | 100GB-1TB | 2x 8GB VPS + 负载均衡 | **$40-80** |

> SQLite + sqld 的效率极高，单台 4GB VPS 可以轻松服务数千用户的多端同步。

### 部署方案

```yaml
# docker-compose.yml（最小可用部署）
version: '3'
services:
  # Auth Gateway + User Manager
  aipp-auth:
    build: ./auth-service
    ports: ["3000:3000"]
    environment:
      - DATABASE_URL=sqlite:///data/users.db
      - SQLD_URL=http://sqld:8080
      - JWT_PRIVATE_KEY_FILE=/keys/private.pem
      - GITHUB_CLIENT_ID=xxx
      - GITHUB_CLIENT_SECRET=xxx

  # sqld 数据同步服务
  sqld:
    image: ghcr.io/tursodatabase/libsql-server:latest
    ports: ["8080:8080"]
    environment:
      - SQLD_ENABLE_NAMESPACES=true
      - SQLD_AUTH_JWT_KEY_FILE=/keys/public.pem
    volumes:
      - sqld-data:/var/lib/sqld
      - ./keys:/keys:ro

  # 反向代理（HTTPS + 路由）
  caddy:
    image: caddy:latest
    ports: ["443:443"]
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile

volumes:
  sqld-data:
```

### Auth Gateway 核心代码（Rust/Axum 示例）

```rust
// auth-service/src/main.rs
use axum::{Router, Json, extract::State};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh))
        .with_state(AppState::new().await);

    axum::serve(listener, app).await.unwrap();
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<SyncConfig>, AppError> {
    // 1. 验证 OAuth（GitHub/Google）
    let user_info = match req.provider.as_str() {
        "github" => verify_github_oauth(&req.token).await?,
        "google" => verify_google_oauth(&req.token).await?,
        _ => return Err(AppError::InvalidProvider),
    };

    // 2. 查找或创建用户
    let user = state.db.find_or_create_user(&user_info).await?;

    // 3. 签发 JWT（sqld 用公钥验证）
    let token = state.jwt_signer.sign(Claims {
        sub: user.id.clone(),
        namespace_prefix: format!("u_{}", user.id),
        exp: (Utc::now() + Duration::days(30)).timestamp(),
    })?;

    // 4. 确保 namespace 已创建
    let namespaces = ["conversation", "assistant", "system", "llm", "skill"];
    for ns in &namespaces {
        state.sqld_admin.ensure_namespace(
            &format!("u_{}/{}", user.id, ns)
        ).await?;
    }

    // 5. 返回客户端开箱即用的配置
    Ok(Json(SyncConfig {
        sync_url: state.config.sqld_public_url.clone(),
        auth_token: token,
        user_id: user.id,
        namespaces: namespaces.iter()
            .map(|ns| format!("u_{}/{}", user.id, ns))
            .collect(),
        expires_at: (Utc::now() + Duration::days(30)).timestamp(),
    }))
}
```

---

## 四、AIPP 现有代码库迁移评估

### 4.1 当前架构概览

| 维度 | 现状 |
|------|------|
| **ORM/驱动** | `rusqlite 0.31.0`（features: bundled, chrono） |
| **数据库文件** | 10+ 独立 .db 文件（system, assistant, conversation, llm, mcp, plugin, skill, scheduled_task, artifacts, artifact_data/*） |
| **连接模式** | 每个 DB 结构体持有 `pub conn: Connection`，按需创建，无连接池 |
| **SQL 操作总量** | **~539 处**（383 execute + 74 query_row + 82 query_map） |
| **DB 模块代码量** | ~8,052 行（9 个核心 db 文件） |
| **迁移系统** | 版本号驱动，11 个增量迁移函数（当前 v0.0.11） |
| **SQLite 特性使用** | WAL 模式、PRAGMA、外键约束、手动事务（7 处）、PRAGMA table_info 动态检查 |
| **未使用的高级特性** | ❌ FTS ❌ 虚拟表 ❌ 触发器 ❌ 自定义函数 ❌ JSON1 扩展 |

### 4.2 需要迁移的文件清单

#### 核心 DB 模块（9 个文件，改动最大）

| 文件 | 行数 | SQL 操作数 | 迁移复杂度 |
|------|------|-----------|-----------|
| `src-tauri/src/db/conversation_db.rs` | 2414 | ~114 | 🟡 中（最大文件） |
| `src-tauri/src/db/mcp_db.rs` | 1847 | ~97 | 🟡 中 |
| `src-tauri/src/db/assistant_db.rs` | 1012 | ~52 | 🟢 低 |
| `src-tauri/src/db/llm_db.rs` | 632 | ~36 | 🟢 低 |
| `src-tauri/src/db/scheduled_task_db.rs` | 482 | ~30 | 🟢 低 |
| `src-tauri/src/db/plugin_db.rs` | 392 | ~25 | 🟢 低 |
| `src-tauri/src/db/skill_db.rs` | 361 | ~20 | 🟢 低 |
| `src-tauri/src/db/system_db.rs` | 307 | ~18 | 🟢 低 |
| `src-tauri/src/db/mod.rs` | 605 | ~15 | 🟡 中（迁移逻辑） |

#### Artifact 模块（2 个文件）

| 文件 | 迁移复杂度 | 备注 |
|------|-----------|------|
| `src-tauri/src/artifacts/artifacts_db.rs` | 🟢 低 | 标准 CRUD |
| `src-tauri/src/artifacts/artifact_data_db.rs` | 🔴 高 | 动态创建 per-ID 数据库文件，需特殊处理 |

#### API/Feature 层（7 个文件）

| 文件 | 迁移复杂度 | 备注 |
|------|-----------|------|
| `src-tauri/src/api/plugin_api.rs` | 🟢 低 | 调用 db 层 |
| `src-tauri/src/api/scheduled_task_api.rs` | 🟢 低 | 调用 db 层 |
| `src-tauri/src/api/ai_api_tests.rs` | 🟢 低 | 测试代码 |
| `src-tauri/src/utils/db_utils.rs` | 🟢 低 | 工具函数 |
| `src-tauri/src/mcp/builtin_mcp/superadmin/audit.rs` | 🟢 低 | 审计日志 |
| `src-tauri/src/mcp/builtin_mcp/templates.rs` | 🟢 低 | 模板 |
| `src-tauri/src/feishu/mod.rs` | 🟢 低 | 飞书集成 |

#### 测试文件（10 个文件）

`src-tauri/src/db/tests/` 下的所有测试文件需要同步更新。

### 4.3 API 差异对照表

| rusqlite API | libsql API | 变更程度 |
|-------------|-----------|---------|
| `Connection::open(path)` | `Builder::new_local(path).build().await?` | 🟡 异步化 |
| `conn.execute(sql, params)` | `conn.execute(sql, params).await?` | 🟢 加 `.await` |
| `conn.query_row(sql, params, \|row\| ...)` | `conn.query(sql, params).await?.next().await?` | 🟡 API 不同 |
| `conn.prepare(sql)?.query_map(params, \|row\| ...)` | `conn.query(sql, params).await?` + 迭代 | 🟡 API 不同 |
| `row.get::<_, T>(idx)` | `row.get::<T>(idx)?` | 🟢 微调 |
| `rusqlite::Result<T>` | `libsql::Result<T>` | 🟢 类型替换 |
| `rusqlite::Error` | `libsql::Error` | 🟢 类型替换 |
| `conn.execute_batch(sql)` | `conn.execute_batch(sql).await?` | 🟢 加 `.await` |
| `params![...]` | `libsql::params![...]` | 🟢 宏替换 |

### 4.4 核心迁移难点

#### 难点 1：同步 → 异步 转换 ⚠️ 最大工作量

当前所有数据库操作都是**同步的**（`rusqlite::Connection` 是同步 API），但 `libsql` 的 embedded replica 是**异步 API**。

**影响范围：** 所有 539+ 处 SQL 操作都需要加 `.await`，相关函数签名需要改为 `async fn`。

**策略选项：**

```rust
// 方案 A: 全面异步化（推荐，但工作量大）
pub async fn get_conversation(&self, id: i64) -> Result<Conversation> {
    let row = self.conn.query("SELECT ...", params![id]).await?
        .next().await?
        .ok_or(Error::NotFound)?;
    // ...
}

// 方案 B: 用 block_on 包装（快速迁移，但不优雅）
pub fn get_conversation(&self, id: i64) -> Result<Conversation> {
    tauri::async_runtime::block_on(async {
        let row = self.conn.query("SELECT ...", params![id]).await?
            .next().await?
            .ok_or(Error::NotFound)?;
        // ...
    })
}
```

> 💡 **建议**：先用方案 B 快速迁移验证，后续逐步异步化。AIPP 本身已经大量使用 async（Tauri commands 都是 async），所以长期应该走方案 A。

#### 难点 2：动态 Artifact 数据库

`artifact_data_db.rs` 按 `db_id` 动态创建独立数据库文件。需要决定：

- **方案 A**：每个 artifact DB 独立同步（每个 DB 都配 sync_url） → 服务端需要支持多数据库
- **方案 B**：artifact 数据不同步，仅同步 metadata → 最简单
- **方案 C**：将 artifact 数据合并到主库的表中 → 需要重构

> 💡 **建议**：artifact 数据通常是用户本地的工具数据，初期可以选择方案 B（不同步），后续按需求再扩展。

#### 难点 3：Connection 生命周期

当前每个请求创建新的 `Connection`，libsql 推荐使用 `Database` 对象管理连接：

```rust
// 现有模式
pub struct ConversationDatabase {
    pub conn: Connection,  // rusqlite::Connection
}

// 迁移后建议模式
pub struct DatabaseManager {
    db: libsql::Database,  // 应用生命周期持有
}

impl DatabaseManager {
    pub fn connect(&self) -> Result<libsql::Connection> {
        self.db.connect()  // 轻量级，可频繁调用
    }
}
```

#### 难点 4：多数据库文件同步策略

AIPP 本身就是多数据库文件设计（conversation.db、llm.db、assistant.db 等分离），这是一个需要重点处理的问题。

**sqld 的多数据库支持**：sqld 原生支持单进程管理多个数据库，每个数据库通过 namespace 隔离。可以对 AIPP 的每个 .db 文件建立独立的同步通道。

**具体实现方式**：

```rust
/// 每个数据库文件对应一个独立的 libsql::Database 实例和 sync_url
pub struct SyncableDatabases {
    conversation: AppDatabase, // → sqld namespace: "conversation"
    assistant: AppDatabase,    // → sqld namespace: "assistant"
    system: AppDatabase,       // → sqld namespace: "system"
    llm: AppDatabase,          // → sqld namespace: "llm"
    skill: AppDatabase,        // → sqld namespace: "skill"
    // ... 其他需要同步的数据库
}

impl SyncableDatabases {
    pub async fn open_all(config: &SyncConfig) -> Result<Self> {
        let base_url = &config.server_url;
        let token = &config.auth_token;
        let db_dir = &config.db_dir;

        // 每个 DB 文件对应 sqld 上的一个 namespace
        // URL 格式: http://server:8080/v1/namespaces/{name}
        Ok(Self {
            conversation: AppDatabase::open(DatabaseMode::Synced {
                path: db_dir.join("conversation.db"),
                sync_url: format!("{}/v1/namespaces/conversation", base_url),
                auth_token: token.clone(),
                sync_interval: Some(Duration::from_secs(60)),
            }).await?,
            assistant: AppDatabase::open(DatabaseMode::Synced {
                path: db_dir.join("assistant.db"),
                sync_url: format!("{}/v1/namespaces/assistant", base_url),
                auth_token: token.clone(),
                sync_interval: Some(Duration::from_secs(300)),  // 配置类低频同步
            }).await?,
            // ... 其他数据库
        })
    }

    /// 手动触发全部数据库同步
    pub async fn sync_all(&self) -> Result<()> {
        // 并行同步所有数据库
        tokio::try_join!(
            self.conversation.sync(),
            self.assistant.sync(),
            self.system.sync(),
            self.llm.sync(),
            self.skill.sync(),
        )?;
        Ok(())
    }
}
```

**各数据库同步策略建议**：

| 数据库文件 | 是否同步 | 同步频率 | 理由 |
|-----------|---------|---------|------|
| conversation.db | ✅ 必须 | 高（60s） | 核心对话数据，用户最关心 |
| assistant.db | ✅ 必须 | 中（300s） | 助手配置，跨设备需要一致 |
| system.db | ✅ 必须 | 低（600s） | 用户设置，变更不频繁 |
| llm.db | ✅ 必须 | 低（600s） | LLM 配置，跨设备需要一致 |
| skill.db | ✅ 推荐 | 低（600s） | 技能配置 |
| mcp.db | 🟡 可选 | 低 | MCP 服务器路径可能因设备而异，需要设备标记 |
| plugin.db | 🟡 可选 | 低 | 插件安装是本地行为，但配置可同步 |
| scheduled_task.db | 🟡 可选 | 低 | 定时任务可能需要设备绑定 |
| artifacts.db | 🟡 可选 | 中 | 看用户需求 |
| artifact_data/*.db | ❌ 不同步 | - | 动态创建的工具数据库，初期不同步 |

> 💡 **对于标记为"可选"的数据库**：可以在设置 UI 中让用户按需勾选哪些数据需要同步。有些配置（如 MCP 服务器路径、插件本地路径）天然是设备相关的，强制同步反而会造成问题。

**sqld 服务端配置（多 namespace）**：

```bash
# sqld 启动时启用多 namespace 支持
docker run -d \
  --name aipp-sync-server \
  -p 8080:8080 \
  -v /data/aipp-db:/var/lib/sqld \
  -e SQLD_AUTH_JWT_KEY_FILE=/keys/public.pem \
  -e SQLD_ENABLE_NAMESPACES=true \
  ghcr.io/tursodatabase/libsql-server:latest

# namespace 会在首次连接时自动创建，无需手动管理
# 每个 namespace 对应服务端一个独立的 .db 文件
```

### 4.5 工作量估算

| 阶段 | 任务 | 涉及文件数 | 复杂度 |
|------|------|-----------|--------|
| **Phase 1: 基础替换** | Cargo.toml 依赖替换 + Connection 初始化 | 1 | 🟢 |
| **Phase 2: DB 层迁移** | 9 个核心 db 文件的 API 替换 | 9 | 🟡 |
| **Phase 3: Artifact 迁移** | artifact 模块适配 | 2 | 🟡 |
| **Phase 4: API 层更新** | 上层调用者签名更新 | 7 | 🟢 |
| **Phase 5: 同步功能** | 新增 SyncManager + 设置 UI | 3-5 新文件 | 🟡 |
| **Phase 6: 测试更新** | 测试文件迁移 + 新增同步测试 | 10+ | 🟡 |
| **Phase 7: 服务器部署** | sqld Docker + JWT 配置 | 基建 | 🟢 |

**总体评估：中等工程量。**

- 机械替换（API 替换、加 `.await`）占 70%，可以批量处理
- 架构决策（同步策略、多 DB 处理）占 20%
- 服务端部署占 10%

### 4.6 有利因素

1. **未使用复杂 SQLite 特性** — 没有 FTS、触发器、虚拟表、自定义函数，迁移障碍少
2. **JSON 存储为 TEXT** — 不依赖 JSON1 扩展，libsql 完全兼容
3. **WAL 模式已在用** — libsql 的同步正是基于 WAL，天然匹配
4. **PRAGMA 全部兼容** — libsql 支持所有标准 SQLite PRAGMA
5. **已有异步基础** — Tauri commands 已经是 async，异步化有基础

---

## 五、推荐迁移路线图

### Phase 1: 抽象层（低风险，高收益）

在现有代码和 libsql 之间加一层薄封装，让后续替换无痛：

```rust
// src-tauri/src/db/connection.rs（新增）

pub enum DatabaseMode {
    /// 纯本地模式，等同于原始 SQLite
    Local { path: PathBuf },
    /// 带同步的模式
    Synced {
        path: PathBuf,
        sync_url: String,
        auth_token: String,
        sync_interval: Option<Duration>,
    },
}

pub struct AppDatabase {
    inner: libsql::Database,
    mode: DatabaseMode,
}

impl AppDatabase {
    pub async fn open(mode: DatabaseMode) -> Result<Self> {
        let db = match &mode {
            DatabaseMode::Local { path } => {
                Builder::new_local(path).build().await?
            }
            DatabaseMode::Synced { path, sync_url, auth_token, sync_interval } => {
                let mut builder = Builder::new_local_replica(path)
                    .sync_url(sync_url, Some(auth_token.clone()));
                if let Some(interval) = sync_interval {
                    builder = builder.sync_interval(*interval);
                }
                builder.build().await?
            }
        };
        Ok(Self { inner: db, mode })
    }

    pub fn connect(&self) -> Result<libsql::Connection> {
        self.inner.connect()
    }

    pub async fn sync(&self) -> Result<()> {
        self.inner.sync().await?;
        Ok(())
    }
}
```

### Phase 2: 逐库迁移

按优先级逐个替换数据库模块：

1. `system_db.rs`（最小，练手）
2. `llm_db.rs`（中等大小）
3. `assistant_db.rs`
4. `conversation_db.rs`（最大，最后）
5. 其他模块

### Phase 3: 同步功能集成

1. 新增设置页面：同步开关、服务器地址、认证
2. 新增 `SyncManager`：管理同步状态、自动/手动同步
3. 新增同步状态指示器（UI）

### Phase 4: 移动端

由于 AIPP 移动端同样是 Tauri 2.0，共享同一套 Rust 后端，所以：

1. DB 层代码 100% 复用，无需额外开发
2. 同步逻辑 100% 复用（同一个 libsql crate）
3. 主要工作是测试 libsql 在 Android/iOS 目标平台的交叉编译
4. 注意 iOS 后台同步限制：建议在 App 进入前台时触发同步

---

## 六、风险与注意事项

### 冲突处理

单用户多设备场景下，libsql embedded replica 使用 **Last-Write-Wins** 策略。这对 AIPP 是足够的：
- 同一用户不太可能在两台设备上同时编辑同一条数据
- 如果发生冲突，最后的写入覆盖之前的（符合直觉）

### 网络断开

- 本地读写完全不受影响
- 同步失败会在下次连接时自动重试
- 不会丢数据

### 性能影响

- **读性能**：零影响（100% 本地）
- **写性能**：本地写入即时，同步是异步后台操作
- **首次同步**：如果数据量大（>100MB），首次全量同步可能需要几分钟

### sqld 服务器容量

- 单个 sqld 实例可以处理数千个并发连接
- 对于个人/小团队使用，免费 VPS 完全够用
- 如果用户规模增长，可以水平扩展（多 replica 节点）

---

## 七、参考资源

- [libSQL GitHub](https://github.com/tursodatabase/libsql)
- [libsql Rust crate 文档](https://docs.rs/libsql/latest/libsql/)
- [sqld Docker 部署指南](https://github.com/tursodatabase/libsql/blob/main/docs/DOCKER.md)
- [Embedded Replica 示例代码](https://github.com/tursodatabase/embedded-replica-examples)
- [Tauri + Turso 官方指南](https://docs.turso.tech/sdk/rust/guides/tauri)
- [实战: Tauri + Drizzle + libSQL 同步](https://dev.to/huakun/building-a-local-first-tauri-app-with-drizzle-orm-encryption-and-turso-sync-31pn)
- [Tauri 2.0 移动端开发文档](https://v2.tauri.app/develop/plugins/develop-mobile/)
- [自建 sqld 教程](https://hubertlin.me/posts/2024/11/self-hosting-turso-libsql/)
- [sqld 多数据库/Namespace 讨论](https://github.com/tursodatabase/libsql/discussions/1268)
