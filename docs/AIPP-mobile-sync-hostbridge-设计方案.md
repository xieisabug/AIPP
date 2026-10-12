# AIPP 移动端完整客户端 × 云端合并 × 控 PC — 代码设计方案

> 基于仓库 `xieisabug/AIPP` 现有 `sync.rs`／`sync-server`／飞书→`ask_ai` 入站，并结合设计讨论修订。  
> 目标：移动端是**完整客户端**（非薄壳）；数据经云端与 PC **合并**；移动端仍可经 Host Bridge **驱动 PC 执行**。  
> 不引入 Cursor Cloud；改动落在现有模块与可新增的 `host_bridge/`。

---

## 0. 目标拆分

| 目标 | 落点 |
|------|------|
| 完整移动端 | 同一套 `src/` + `src-tauri`；Android／iOS 跑完整 Chat／Config／Butler；本地多库 SQLite |
| 云端合并 | 现有业务级 sync（push／pull + outbox／shadow／`sync_object_map`）；补实时通知、设备元数据、**并发续聊自动分叉** |
| 移动端控 PC | 新模块 `host_bridge/`：PC=Host，手机=Peer；白名单 RPC 复用 `ask_ai`／权限命令 |

原则：

- **数据面**与**控制面**分离。
- 控制面只暴露白名单，内部调用已有 API，不复制 AI 管线。
- 手机可本地聊；需本机文件／桌面 MCP／ACP／bash 时走 Host。
- Host Bridge 的「连上并遥控」借鉴 Paseo；权威数据仍用 AIPP sync；执行入口对齐飞书→`ask_ai`。

---

## 1. 与 Paseo／现有代码的关系

### 借鉴 Paseo

- 角色：PC=Host，手机=Peer（完整客户端兼遥控器）
- 配对：密钥／二维码；控制面仅已配对设备
- 传输：双方出站 relay（载荷宜 E2EE）或 LAN／Tailscale 直连
- 双通道：实时 RPC／Event vs 权威落库（AIPP sync）
- 握手：`HostHello.capabilities`

### 不照搬

- 无独立 daemon；Host 为 PC 进程内 `host_bridge::server`
- 聊天／配置合并继续用现有 sync，不用 Paseo timeline 协议
- 遥控入口对齐飞书 `process_incoming_text_message` → `ask_ai`，非 Paseo 命令集

### 直接依赖的现有能力

- `sync.rs`：`CLIENT_SCHEMA_VERSION = 2`，约 45s 轮询；`schedule_sync_after_local_change` + `scan_local_changes`
- 已同步域：配置（llm／assistant／mcp／feature_config／artifacts）+ conversation／message（仅 `finish_time` 非空的 response）／attachment／summary／todo
- 冲突：服务端 `base_version` stale → `ConflictEvent`；客户端**远端赢**落地（`handle_push_response`）
- 飞书：`feishu/events.rs` → `ask_ai(..., RELAY_ORIGIN_FEISHU)` + 卡片审批回调

---

## 2. 建议文件树

```
src-tauri/src/
  host_bridge/
    mod.rs
    types.rs          # HostHello、RpcRequest/Response、HostEvent、Capability
    allowlist.rs      # 可远程调用的 method → 现有 command
    session.rs        # 配对、公钥、会话密钥
    server.rs         # PC：入站 RPC → ask_ai／权限等
    client.rs         # Mobile：出站 RPC
    transport.rs      # WS 直连 / relay
    stream.rs         # message_update 等 → HostEvent
  sync.rs             # 就地改：notify、divergence fork、写路径强化
  platform_caps.rs    # can_host / local_fs / acp_local
sync-server/app/
  routes/notify.py    # SSE 或 WS 变更通知
  routes/relay.py     # 可选：配对房间（密文转发）
  models.py           # sync_device 扩字段
src/
  windows/MobileShell.tsx
  hooks/useHostBridge.ts
  components/.../HostBridgeConfigForm.tsx
src-tauri/capabilities/
  mobile.json         # 扩大 windows
```

---

## 3. 完整移动端

### 3.1 Capability

现状：`capabilities/mobile.json` 的 `windows` **仅** `"chat_ui"`。

改：加入 `"config"`, `"butler_experiment"`, `"schedule"`, `"sidebar"` 等所需窗口标签，并按需补插件权限。

### 3.2 前端壳

- 新增 `MobileShell.tsx`（或扩展 `ChatUIWindow`）：Tab = 对话／总管家／设置／同步。
- 利用已有 `App.tsx` 的 `window.__setAppWindow` **单 Webview 切换**，移动端不要再开多 WebviewWindow。
- 用平台 API（非仅 CSS）区分 mobile／desktop。

### 3.3 平台能力分流

```rust
pub struct PlatformCaps {
  pub can_host: bool,        // PC true
  pub local_fs_tools: bool,
  pub acp_local: bool,
}
```

`execution router`：手机本地仅 sandbox；`execution_target = host` 或工具需桌面能力时 → `host_bridge::client::invoke`。逻辑集中在 MCP operation／`ask_ai` 入口前，前端不散落 if。

---

## 4. 云端合并：身份、冲突、并发分叉

### 4.1 ID 模型（确认结论）

- 本地表保留 `INTEGER PRIMARY KEY AUTOINCREMENT`；**线上身份是 `sync_id`**。
- `sync_object_map`：`(object_type, local_id) ↔ sync_id`。
- Payload **不传本地 rowid**；外键在 `refs` 中为 sync_id；`apply_change` 时解析成本地 id。

**「手机 conversation.id=5、PC conversation.id=5、sync_id 不同」**：

- 两边各自的本地 `id=5` **都不改**。
- 对方对象在本机 **另插一行**（下一自增 id，如 6），map 记 `6 ↔ 对方 sync_id`。
- 这是刻意并集，不是 bug。业务本机继续用本地 id；跨设备只认 sync_id。

**同 sync_id**：pull 命中 map → `UPDATE` 已有本地行，不插第二行。

特殊身份：

| 类型 | 策略 |
|------|------|
| conversation／message 等 | 无 natural key → 新建新 UUID，并集合并 |
| `system.feature_config` | natural key `(feature_code, key)` |
| 官方 provider／builtin MCP | `official:{type}:{local_id}` |

FK 未映射时跳过入队，避免裸本地 id 上云。

**本地自增撞号本身安全**；需警惕的是：语义同一线却两个 sync_id（列表重复）、map 丢失导致重复 UUID、以及同 sync_id 并发续聊弄乱上下文。

### 4.2 现有冲突语义（保持）

```
push(base_version) → merge.py
  base_version < server.version → ConflictEvent（默认拒绝 stale；仅 stale_lww_types 可 LWW）
客户端 handle_push_response：远端赢，apply server_payload，删除 outbox
```

适用于：同 object 元数据／配置类并发改写。

### 4.3 同对话并发续聊 → **自动分叉**（讨论修订，替代「无脑 accept 并集」）

**问题**：两边对同一 conversation sync_id 各跑一轮 `ask_ai`，message 各有独立 sync_id 都会被 accept，时间线交错，AI 上下文损坏。远端赢抹掉本机一轮也不合适。

**策略**：检测到并发分叉 → **自动 fork 成两个 conversation**，不要揉进同一时间线。

#### 判定（本机 apply 远端 message 之前）

对同一 conversation sync_id 取 tip（最新已 finalize 的 user→response 组／`generation_group_id`）。

若新到的远端 message：

1. 挂在本地已有对话 C；且  
2. 本机 C 在该时刻已有另一条本机 tip（或 runtime 正在 running）；且  
3. 两边 tip 无共同 `parent_id`／`parent_group_id`

→ `concurrent_divergence` → 自动分叉。

未 `finish_time` 的 response 仍不同步；检测只对已 finalize 行。

#### 分叉动作

```rust
fn fork_conversation_on_sync_divergence(
  app,
  local_conversation_local_id,
  diverging_remote_messages: &[PullChange],
) -> Result<(kept_local_id, forked_local_id), String>
```

| 侧 | 结果 |
|----|------|
| 本机已有消息 | 留在原 C（sync_id 不变） |
| 远端冲突侧消息 | 不进 C；`fork_conversation` 得 C'（**新 sync_id**），改 `refs.conversation` 后 apply |
| 分叉点前公共历史 | 复制到 C'（复用现有 fork 语义） |

Metadata：

```json
{
  "sync_fork_of": "<原 conversation sync_id>",
  "sync_fork_reason": "concurrent_continue",
  "sync_fork_at_message": "<分叉点 message sync_id>"
}
```

#### 挂点

1. `apply_change`（`conversation.message`）：insert／update 前 `detect_conversation_divergence`；命中则 fork 再写。  
2. `handle_push_response` conflict：若是 message 且本机同对话有更新 tip → **不要**用 server 覆盖本机 tip；改为保留本机 + 远端进 C'。  
3. 抽 `fork_conversation_internal`，与 `conversation_api::fork_conversation` 共用。  
4. 服务端仍可两条 message 都 accept；**分叉是客户端 apply 策略**；C' 正常 push。

### 4.4 执行租约（与分叉互补）

```text
conversation.execution_owner_device_id
conversation.execution_lease_until
```

- 在线：有未完成 response／running 时拒绝对端再本地 ask，或提示走 Host。  
- Host Bridge 的 `ai.ask`：只在 Host 跑；Peer 禁止同 sync_id 再本地 ask。  
- 租约减少分叉；分叉是离线／未协调时的安全网。

### 4.5 其它数据面加固

1. **写路径**：在 repository 写成功后显式入队（或强化现有 `schedule_sync_after_local_change` 触点）；扫描降为低频对账。触点：`conversation_api`／`assistant_api`／`llm_api`／`system_api`／`mcp/registry_api`／`artifacts/collection_api`。  
2. **实时 notify**：`GET /v1/sync/notify`（SSE／WS）；push 成功后推 cursor；客户端立刻 pull，失败回退 45s。  
3. **设备表**：platform／role／capabilities／public_key／last_seen；`POST /v1/devices/announce`。  
4. **流式**：token 不进 sync；经 HostEvent 或事后 finalize 再 upsert。  
5. **展示排序**：`ORDER BY created_time, sync_id`，不用本地 id；裁决用 server version，不用设备时钟做 LWW。

---

## 5. Host Bridge（控 PC）

### 5.1 角色与传输

| 端 | 行为 |
|----|------|
| Host（PC） | `server` 连 relay 或听端口；执行白名单 RPC |
| Peer（手机） | `client`；选 Host 后发 RPC |

`Transport::Direct | Relay`；relay 宜只转发 E2EE 密文。

### 5.2 协议要点

- `HostHello` + `capabilities`
- `RpcReq`／`RpcResp`／`Event`
- `HostEvent`：`MessageUpdate`、`PermissionNeeded`、`AskUserQuestion`、`RuntimeState`、`HostGone`
- 边界一律用 **conversation／assistant 的 sync_id**，Host 上再映射本地 rowid

### 5.3 白名单（method → 现有 command）

| method | 内部 |
|--------|------|
| `ai.ask`／`ai.cancel`／`ai.regenerate` | `ask_ai`／`cancel_ai`／`regenerate_ai` |
| `ai.tool_continue` | `tool_result_continue_ask_ai` |
| 队列三条 | `enqueue_`／`list_queued_`／`promote_queued_conversation_message` |
| conversation CRUD | `list_`／`get_`／`create_`／… |
| butler.* | `load_butler_main_conversation`、tasks、`spawn_butler_task_conversation` 等 |
| `permission.confirm_operation`／`confirm_acp` | 同名 command |
| `ask_user.resolve` | `submit_ask_user_question_response` |

禁止：任意 SQL／裸 shell／改 sync token／读 keychain／未映射 invoke。

权限：Host 弹审批 → `HostEvent::PermissionNeeded` → 手机确认 → `permission.confirm_*`（对齐飞书卡片）。PC 上权限事件优先 `butler_experiment` 窗口。

### 5.4 Host `ai.ask`（对齐飞书）

```text
resolve sync_id → local id
→ AiRequest { stream: true, … }
→ ask_ai(..., system_message, origin="host_bridge")
→ 订阅 activity／message_update → HostEvent
→ finalize 后本地 DB → 现有 sync push → Peer 权威收敛
```

### 5.5 Peer commands（Tauri）

`host_bridge_list_paired`／`pair_from_qr`／`connect`／`rpc`／`enable_host_mode`（仅 PC）

前端：`HostBridgeConfigForm`、Chat「本机｜PC 执行」、`useHostBridge` 听事件。

---

## 6. sync-server 改动清单

| 项 | 内容 |
|----|------|
| `SyncDevice` | platform／role／capabilities／public_key／last_seen |
| devices API | announce／list |
| `notify` | SSE／WS；merge 后 publish |
| 可选 relay | 房间转发密文 |
| 冲突 | 维持默认 conflict + 可配 `stale_lww_types`；**conversation 分叉不在服务端做** |

---

## 7. 端到端调用链

### 仅合并聊天

写 message → outbox／扫描 → push → notify → 对端 pull → `apply_change`（必要时 fork）→ UI

### 手机控 PC 跑一轮

1. Peer `host_bridge_rpc("ai.ask", { conversation_sync_id, prompt })`  
2. Host → `ask_ai`  
3. `HostEvent::MessageUpdate` 实时显示  
4. finalize → sync push／pull 权威落库（与 Event 按 sync_id／payload_hash 去重）  
5. 审批：`PermissionNeeded` → 手机 confirm → PC 继续  

### 同对话两边离线各聊一轮

上线 pull → `detect_conversation_divergence` → 原 C 保留本机侧；对端消息进自动 fork 的 C' → 两边列表可见两个对话，上下文各自完整。

---

## 8. 本地库增量

```sql
-- host bridge
CREATE TABLE host_bridge_peer (
  host_device_id TEXT PRIMARY KEY,
  display_name TEXT,
  public_key TEXT NOT NULL,
  relay_url TEXT,
  last_hello_json TEXT,
  created_at TEXT,
  last_connected_at TEXT
);
CREATE TABLE host_bridge_local (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- conversation 可选（进 specs）
-- execution_target: 'local' | 'host' | null
-- execution_owner_device_id, execution_lease_until
```

---

## 9. 测试要点

- sync_id 映射：两边本地 id 同为 5、异 sync_id → 各留 5、对方新行；同 sync_id → update 不双插  
- FK：无父不入队；有父后 refs 为 UUID  
- 并发 append 同分叉：各一轮 AI → 自动两 conversation，无交错脏上下文  
- 仅一端追加 → 不 fork  
- 只改标题 → 远端赢／LWW，不分叉  
- conflict 路径：message 分叉优先于覆盖本机 tip  
- Host：`ai.ask`＋权限闭环；未知 method 拒绝  
- 飞书回归：仍只走本地 `ask_ai`  
- 移动壳：capability 下可进 Config／Butler；桌面不变  

---

## 10. 建议提交切片

1. mobile capability + MobileShell（完整客户端壳）  
2. sync 写路径强化 + SSE notify  
3. device announce UI  
4. **conversation 并发分叉**（`detect` + `fork_conversation_internal` + conflict 分支）  
5. execution lease 字段与发送拦截  
6. host_bridge MVP（直连 WS + `ai.ask`／cancel／MessageUpdate）  
7. permission 远程闭环 + execution_target  
8. relay E2EE + QR 配对  
9. blob／secret vault（后置）  

---

## 11. 产品边界（对应需求）

- 手机：**完整**本地库 + Chat／设置／Butler + 可本地 `ask_ai`  
- 云端 sync：**合并**两端数据；id 用 sync_id；同对话冲突续聊 **自动分叉**  
- Host Bridge：把 PC 能力借给手机，手机不是纯遥控器  

---

## 12. 修订对照（相对初版设计）

| 点 | 初版 | 讨论后 |
|----|------|--------|
| 同对话两边继续聊 | message 并集 accept | **自动分叉**为两个 conversation |
| 同 object 冲突 | 远端赢 | 配置／元数据仍远端赢；**message 冲突优先分叉，不覆盖本机 tip** |
| 本地 id 同号 | 说明 map 即可 | 明确「本地 5 不变、对方新行」正确；风险在语义重复／并发上下文，不在自增撞号 |
| 执行协调 | 略 | 增加 **execution lease**，与分叉互补 |
| Paseo | Host 层借鉴 | 同上；数据面仍为 AIPP sync |
