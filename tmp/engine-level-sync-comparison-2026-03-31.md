# AIPP 引擎级同步方案对比与推荐

> 本文档是基于用户明确拒绝应用层 oplog 方案后，针对「引擎级、类似 MySQL binlog 的透明变更追踪」需求的深度调研。

## 0. 核心需求回顾

| 需求项 | 说明 |
|--------|------|
| 引擎级变更追踪 | 类似 MySQL binlog，不需要每张表手写 change tracking |
| 加表/改表友好 | 新增表或字段后，同步机制自动适应 |
| 长久之计 | 最佳实践，不是临时方案 |
| 自建部署 | 不依赖第三方 SaaS |
| 大团队可承受复杂度 | 不怕改动大，怕方向错 |

## 1. 决定性发现：libsql 自建同步已死

### 1.1 源码级确认

通过直接审查 libsql monorepo `main` 分支的源码：

**客户端（libsql crate 0.9.30，`libsql/src/sync.rs`）** 仍然调用：
```
GET  {sync_url}/info
GET  {sync_url}/export/{generation}
GET  {sync_url}/sync/{gen}/{start}/{end}
POST {sync_url}/sync/{gen}/{start}/{end}[/{baton}]
```

**服务端（libsql-server `main`，`libsql-server/src/http/user/mod.rs`）** 的全部路由：
```
/                        (POST=query, GET=upgrade)
/version, /console, /health, /dump
/beta/listen
/v1, /v1/execute, /v1/batch
/v2/pipeline, /v3/pipeline, /v3-protobuf/pipeline
/dev/:namespace/v:version/pipeline   ← 唯一的 namespace 路由
/v1/jobs, /v1/jobs/:job_id
```

**结论：服务端已彻底移除 `/info`、`/export`、`/sync` 端点。** 客户端的 `new_synced_database` 是孤儿代码。不是版本不匹配，是功能被废弃了。

### 1.2 含义

- 从同一 Git tag 构建 sqld 也**无法修复**——服务端代码已不存在
- 这不是临时 bug，是架构决策——Turso 将 sync 功能收入商业云服务
- `libsql` crate 的 `sync` feature 实际上只对 Turso Cloud 有效

### 1.3 Turso Cloud 作为备选？

理论上 AIPP 可以用 Turso Cloud 代替自建 sqld。但：
- 违背自建部署需求
- 价格：免费 9GB, Pro $29/月 24GB
- Vendor lock-in

**结论：libsql 自建同步路线已死，Turso Cloud 不满足自建需求。彻底放弃此方向。**

---

## 2. 引擎级方案全景评估

### 2.1 cr-sqlite（CRDT 扩展）

| 维度 | 评价 |
|------|------|
| **原理** | SQLite 运行时扩展，将普通表升级为 CRR (Conflict-free Replicated Relation)，自动创建 CRDT 元数据表 + 触发器 |
| **变更追踪** | 全自动。`SELECT crsql_as_crr('table_name')` 后，所有 INSERT/UPDATE/DELETE 自动被引擎捕获 |
| **变更提取** | `SELECT * FROM crsql_changes WHERE db_version > ?` — 获取所有增量变更 |
| **变更应用** | `INSERT INTO crsql_changes VALUES (...)` — 在另一端回放变更 |
| **冲突解决** | LWW (Last Writer Wins) 按列级别，使用 Lamport 时钟，完全自动 |
| **多主写入** | ✅ 支持，这是 CRDT 的核心能力 |
| **离线支持** | ✅ 完全离线，恢复后增量同步 |
| **Schema 迁移** | `crsql_begin_alter('table')` → 改表 → `crsql_commit_alter('table')` |
| **新增表** | 普通 CREATE TABLE 后 `crsql_as_crr('new_table')` |
| **写入开销** | ~2.5x 写慢于普通 SQLite（读取无影响） |
| **网络传输** | 自己实现，cr-sqlite 只管本地 merge |
| **维护状态** | 最后 release v0.16.3 (2024-01), 最后 commit 2024-10, 社区活跃，Turso/Fly.io/Expo 赞助 |
| **许可证** | MIT |

#### ⚠️ 关键限制

**1. 不支持外键约束**

cr-sqlite 明确禁止在 CRR 表上使用 FOREIGN KEY 约束。原因：
- 多端异步复制时，引用行可能还未到达
- 不同节点可能以不同顺序接收数据
- CRDT 语义与引用完整性存在根本冲突

AIPP 当前状态：所有表都使用了 `FOREIGN KEY`（通过 `PRAGMA foreign_keys = ON` 启用）。

**应对方式：**
- FK 列和 JOIN 查询仍然可以保留
- 只是引擎不再自动检查引用完整性
- 改为应用层检查 + 同步后清理孤儿记录
- 引入软删除避免硬删除导致的悬挂引用

**2. 主键要求**

cr-sqlite 要求 `INTEGER PRIMARY KEY`。AIPP 当前全部使用 `INTEGER PRIMARY KEY AUTOINCREMENT`。
- AUTOINCREMENT 仍然可用，但在多端场景下不推荐（会产生冲突）
- **最佳实践：改用 UUID 或 ULID 作为主键**，这是所有分布式同步系统的标准做法

**3. Rust 集成**

cr-sqlite 是 C 扩展（.dll/.so/.dylib），通过 `load_extension()` 加载：
```rust
// 使用 rusqlite
conn.load_extension("path/to/crsqlite", None)?;

// 使用 libsql (也支持)
conn.load_extension("path/to/crsqlite", None)?;
```

#### 架构设计

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Device A    │     │  Relay Server│     │  Device B    │
│  SQLite +    │────▶│  (存储转发)    │◀────│  SQLite +    │
│  cr-sqlite   │◀────│              │────▶│  cr-sqlite   │
└──────────────┘     └──────────────┘     └──────────────┘

Relay Server 职责（极简）：
- 接收各设备推送的 crsql_changes
- 存储变更（可以用普通 SQLite/PostgreSQL）
- 向其他设备分发未见过的变更
- 不需要理解业务逻辑
```

---

### 2.2 SQLite Session Extension

| 维度 | 评价 |
|------|------|
| **原理** | SQLite 内置功能（需编译时开启），创建 session 跟踪表变更，生成二进制 changeset |
| **变更追踪** | 半自动。需要代码中显式创建 session → attach 表 → 提取 changeset |
| **变更应用** | `sqlite3changeset_apply()` 函数应用 changeset |
| **冲突解决** | 手动实现冲突回调函数 |
| **多主写入** | 需要自己实现冲突策略 |
| **离线支持** | 需要自己管理 session 生命周期 |
| **Schema 迁移** | Changeset 格式绑定表结构，schema 变更后旧 changeset 不兼容 |
| **新增表** | 需要手动 attach 新表到 session |
| **写入开销** | 较低（只是记录变更，不需要 CRDT 元数据） |
| **网络传输** | 自己实现 |
| **维护状态** | SQLite 官方内置，永久维护 |
| **许可证** | Public Domain |

#### ⚠️ 关键问题

- **不是真正的引擎级透明追踪**：需要在代码中主动管理 session 生命周期
- **schema 变更后 changeset 不兼容**：加列后，旧的 changeset 无法应用到新 schema
- **冲突解决完全手动**：没有 CRDT 的自动 merge 能力
- **每次新增表都要修改追踪代码**：不满足「加表自动适应」的需求

**结论：比应用层 oplog 好一点，但本质上仍然需要大量手工管理，不满足「引擎级」需求。**

---

### 2.3 ElectricSQL

| 维度 | 评价 |
|------|------|
| **原理** | PostgreSQL → Elixir 同步服务 → 客户端 SQLite |
| **维护状态** | 活跃，2026 年稳定，9k+ GitHub stars |
| **问题** | 要求 PostgreSQL 后端，不是 SQLite-to-SQLite |

**与 AIPP 的不匹配：**
- AIPP 是桌面端应用，数据完全在本地 SQLite
- ElectricSQL 要求中心 PostgreSQL 作为 source of truth
- 这意味着要重写整个数据层从 SQLite → PostgreSQL 后端 + SQLite 本地缓存
- 架构完全不同：从 "纯本地 SQLite 桌面应用" 变成 "客户端-服务器 Web 应用"

**结论：架构不匹配，排除。**

---

### 2.4 sqlite-sync (SQLiteAI/SQLite Cloud)

| 维度 | 评价 |
|------|------|
| **原理** | CRDT 扩展，多种算法（CLS/GOS/DWS/AWS） |
| **同步目标** | SQLite Cloud 服务 |
| **自建** | 不支持——深度绑定 SQLite Cloud 生态 |

**结论：vendor lock-in，排除。**

---

### 2.5 PowerSync

已在 `tmp/powersync-research-2026-03-31.md` 详细评估。

**结论：Rust SDK pre-alpha，需 PG+MongoDB+API 后端，排除。**

---

## 3. 综合对比

| 维度 | cr-sqlite | Session Ext | ElectricSQL | sqlite-sync | libsql sync |
|------|-----------|-------------|-------------|-------------|-------------|
| 引擎级透明追踪 | ✅ | ❌ 半手动 | ✅ | ✅ | ✅ |
| 加表自动适应 | ✅ `crsql_as_crr` | ❌ 需改代码 | ✅ | ✅ | ✅ |
| Schema 迁移 | ✅ begin/commit_alter | ❌ changeset 不兼容 | ✅ | ✅ | ✅ |
| 多主写入 | ✅ CRDT | ❌ 需手写 | ✅ CRDT | ✅ CRDT | ❌ 单主 |
| 离线支持 | ✅ | ❌ 需管理 | ✅ | ✅ | ✅ |
| 自建部署 | ✅ | ✅ | ⚠️ 需要 PG | ❌ | ❌ 已死 |
| FK 支持 | ❌ | ✅ | ✅ | ❓ | ✅ |
| 写入开销 | ~2.5x | ~1.1x | N/A | ~2x | ~1x |
| 长期维护 | ⚠️ 需 fork | ✅ SQLite 内置 | ✅ | ⚠️ | ❌ |
| Rust 生态 | ✅ load_extension | ✅ 内置 | ❌ | ❌ | ❌ |

---

## 4. 推荐方案：cr-sqlite（Fork + 维护）

### 4.1 为什么是 cr-sqlite

在排除了所有不可行方案后，cr-sqlite 是**唯一满足全部核心需求的方案**：

1. **引擎级**：`crsql_as_crr('table')` 后完全透明，写入代码不需要任何修改
2. **加表友好**：新表 CREATE 后一行 SQL 升级为 CRR
3. **Schema 迁移**：`crsql_begin_alter` / `crsql_commit_alter` 专门为此设计
4. **多主写入**：CRDT 保证最终一致性，无需中心主库
5. **离线优先**：天然支持，恢复后增量 merge
6. **自建部署**：MIT 许可，网络层自己写
7. **Rust 友好**：loadable extension，rusqlite/libsql 均可加载

### 4.2 需要解决的问题及方案

#### 问题 1：外键约束

**影响范围**：AIPP 几乎所有表都有 FK 关系。

**解决方案**：
- FK 列保留，JOIN 查询不受影响
- 关闭 `PRAGMA foreign_keys = ON`（同步环境必须如此）
- 应用层 FK 校验：在写入前检查引用存在
- 同步后清理 job：定期扫描孤儿记录
- 这是 **所有分布式数据库（CockroachDB、TiDB 等）的标准做法**——延迟校验而非即时约束

#### 问题 2：主键冲突

**影响范围**：所有表使用 `INTEGER PRIMARY KEY AUTOINCREMENT`。

**解决方案**：
- **迁移到 UUID/ULID 主键**（推荐 ULID，有序且可排序）
- 这是分布式系统的标准做法
- 可以分阶段迁移：先对需要同步的表改主键

#### 问题 3：维护风险

**影响范围**：最后 release 2024-01，最后 commit 2024-10。

**解决方案**：
- Fork `vlcn-io/cr-sqlite` 到团队 GitHub org
- cr-sqlite 核心代码量不大（C + Rust），可维护性尚可
- MIT 许可，无法律风险
- 如有需要，可以只维护 C 扩展核心，不需要维护 JS/React 生态
- 监控上游合并新特性

#### 问题 4：多数据库

**影响范围**：AIPP 有 7+ 个独立 SQLite 数据库。

**解决方案**：
- 每个 DB 独立加载 cr-sqlite 扩展
- 每个 DB 独立同步（变更粒度天然隔离）
- Relay server 按 DB 名称区分不同数据流
- 或者考虑合并为少数几个 DB（减少连接管理复杂度）

### 4.3 Relay Server 设计（极简）

```
┌─────────────────────────────────────────────────┐
│                 Relay Server                     │
│                                                  │
│  POST /sync/{db_name}/push                       │
│    Body: { device_id, changes: [...] }           │
│    → 存入 changes 表                              │
│                                                  │
│  GET  /sync/{db_name}/pull?since_version=N        │
│    → 返回该设备未见过的所有 changes                  │
│                                                  │
│  数据存储：SQLite 或 PostgreSQL                     │
│  认证：JWT token per tenant                        │
│                                                  │
│  核心就两张表：                                     │
│  - devices(id, last_seen, ...)                    │
│  - changes(id, db_name, device_id,                │
│           table, pk, col, val, col_version,       │
│           db_version, site_id, cl, seq, created)  │
└─────────────────────────────────────────────────┘
```

Relay server 不需要理解业务逻辑，只做存储转发。这意味着：
- 加表/改表不需要动 server
- Server 可以用任何语言写（Go/Rust/Python）
- 水平扩展简单

### 4.4 实施路线图

```
Phase 0: 验证 (PoC)
├── Fork cr-sqlite，编译各平台扩展（Windows/macOS/Linux）
├── 在测试项目中验证：加载扩展、标记 CRR、提取/应用 changes
├── 验证与 libsql 0.9.30 的 load_extension 兼容性
└── 性能基准测试（2.5x write overhead 在 AIPP 场景下是否可接受）

Phase 1: 主键迁移
├── 设计 UUID/ULID 主键迁移方案
├── 逐表迁移（可分阶段，先迁移需要同步的核心表）
└── 确保 UI 层不依赖 INTEGER 自增 ID 的连续性

Phase 2: FK → 应用层校验
├── 将 foreign_keys PRAGMA 关闭（或软化）
├── 在 DB 层封装 FK 校验方法
├── 引入软删除（至少对被引用的表）
└── 同步后孤儿记录清理 job

Phase 3: cr-sqlite 集成
├── 在 DatabaseManager 中集成 load_extension
├── 对所有需同步的表执行 crsql_as_crr
├── 修改现有 migration 逻辑，包裹 begin_alter/commit_alter
└── 封装 SyncEngine：提取变更 / 应用变更 / 版本跟踪

Phase 4: Relay Server
├── 实现极简 Relay Server
├── 认证（复用现有 tenant 体系）
├── 变更存储与分发
└── 部署（Docker 一键部署）

Phase 5: 端到端集成
├── 客户端 SyncManager 重写
├── 后台定时同步 + 手动同步
├── 冲突可视化（可选：让用户看到冲突项）
└── 压力测试 + 数据一致性验证
```

---

## 5. 备选方案（如果 cr-sqlite 不可行）

如果 PoC 阶段发现 cr-sqlite 有不可逾越的问题（如与 libsql 不兼容、性能无法接受等），备选方案按优先级：

### 5.1 基于 SQLite Session Extension 的自动化封装

思路：在 Session Extension 上构建一层自动化框架，减少手工管理：
- 自动为所有表创建 session
- 自动提取 changeset
- 自动应用 + 冲突回调
- 需要大量框架代码，但底层是 SQLite 官方保证的

### 5.2 Turso Cloud（放弃自建需求）

如果团队可以接受：
- 使用 Turso Cloud 作为中心同步服务
- AIPP 的 `new_synced_database` 可能直接可用
- 代价：$29/月起，vendor dependency

### 5.3 全面拥抱 PostgreSQL（架构重做）

- 后端迁移到 PostgreSQL
- 本地 SQLite 仅作为缓存
- 使用 ElectricSQL 或 PowerSync 同步
- 这是最大的改动，但也是最"标准"的做法

---

## 6. 最终建议

**推荐 cr-sqlite Fork + Relay Server 方案。**

理由：
1. 它是目前唯一满足「引擎级、自建、多主、离线优先」全部需求的方案
2. CRDT 是学术界和工业界公认的分布式数据最终一致性最佳实践
3. 网络无关设计意味着 AIPP 完全控制传输层
4. FK 限制和主键迁移虽然工作量不小，但都是分布式系统的标准做法
5. MIT 许可 + fork 策略消除了上游维护风险

**不推荐**继续在 libsql sync 方向投入——这条路已经被 Turso 官方关闭了。
