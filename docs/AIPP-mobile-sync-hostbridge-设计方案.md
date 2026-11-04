# AIPP 同步分叉 × SSE notify × Host Bridge — 设计说明（与 PR 范围对齐）

> 本文描述 **已落地 / 本 PR 范围** 的数据面能力，以及明确延期项。  
> MobileShell 底栏／路由劫持等 UI 改动已撤销，不在本 PR 范围。

---

## 0. 本 PR 实际范围

| 项 | 状态 |
|----|------|
| 同对话并发续聊 → 客户端自动分叉 | **已落地**（`sync/divergence.rs` + `sync/mod.rs`） |
| sync-server + 客户端 SSE `GET /v1/sync/notify` | **已落地** |
| Host Bridge 类型／白名单 stub | **仅 stub**；WS／relay／E2EE／遥控执行延期 |
| MobileShell／mobile capability 扩窗 | **已撤销**，保持现有移动端 UI |
| 执行租约、设备 announce 扩展 | **延期** |

原则：数据面（sync）与控制面（Host Bridge）分离；分叉是客户端 apply 策略，服务端仍是 dumb store。

---

## 1. 身份与冲突（既有，保持）

- 本地 `INTEGER` id 不变；线上身份是 `sync_id`（`sync_object_map`）。
- Payload 外键走 `refs` 中的 sync_id；`apply_change` 解析成本地 id。
- 同 object 元数据／配置冲突：远端赢（`handle_push_response`）。
- **message 冲突优先分叉**，不覆盖本机 tip（见 §2）。

---

## 2. 同对话并发续聊 → 自动分叉（§4.3）

### 2.1 判定（apply 远端 message **insert** 之前）

对同一 conversation sync_id 取 tip（最新已 finalize 的 generation group）。

命中 `concurrent_divergence` 当且仅当新 insert 且：

1. 挂在本地对话 C；且
2. 本机已有并发 tip（`sync_shadow` 未确认的本地续聊）或未完成 response（runtime 近似）；或
3. 版本线：同 `parent_group_id` 下已有另一 `generation_group_id`；或
4. 遗留：同 `parent_id` 下已有兄弟行

**不再**使用「`parent_id == None` + 会话非空 → 分叉」。  
`conversation_api` 已不再写入 `parent_id`（始终为 None）；版本关系靠 `generation_group_id`／`parent_group_id`。

**保证：** 非空会话 + `parent_id` None 的普通 insert → **不分叉**。

同 `generation_group_id` 已在本地存在（同轮 user→response）→ 不分叉。

未 `finish_time` 的 response 仍不同步；检测对已 finalize／映射状态敏感。

### 2.2 分叉动作

| 侧 | 结果 |
|----|------|
| 本机已有 tip | 留在原 C |
| 远端冲突侧消息 | 不进 C；`fork_conversation_internal` 得 C'（新 sync_id），改 `conversation_id` 后 apply |
| 分叉点 | `parent_id` 有值时用其；否则只复制 **公共历史**（最后一条已 shadow 的消息），**不**把整段本地 tip 拷进 C' |

C' 经 `ensure_object_id` 获得 sync_id，随后 scan／upload。

### 2.3 挂点

1. `apply_change` → `maybe_fork_before_message_insert`
2. `handle_push_response` conflict → `try_fork_message_conflict_instead_of_overwrite`（同一套判定）
3. `fork_conversation_internal`（`ForkNameStyle::SyncDivergence`）

细节见 [sync-concurrent-fork.md](./sync-concurrent-fork.md)。

---

## 3. SSE 实时 notify

- `GET /v1/sync/notify?cursor=`（`text/event-stream`；Bearer + `X-AIPP-Device-ID`）
- **先 subscribe，再读 cursor**（或 subscribe 后重读），缩小漏事件窗口
- push 接受并 commit 后按账号 fan-out；同 device 回声跳过
- 客户端收到 `event: sync` 立即唤醒 worker；失败回退约 45s 轮询

---

## 4. Host Bridge（延期，仅 stub）

`src-tauri/src/host_bridge/*` 保留类型／白名单 stub。  
未实现：WS／relay／E2EE、`ai.ask` 遥控、执行租约、设备 announce 元数据扩展。

---

## 5. 测试要点（本范围）

- 非空会话 + `parent_id` None 普通 insert → **不 fork**
- 仅一端追加 → 不 fork；同 `generation_group` 同轮 → 不 fork
- 未确认本地 tip／未完成 response + 远端新 tip → fork；fork_point 为公共历史
- conflict 路径与 insert 路径共用判定；fork 后 conversation map 可上传
- notify：鉴权、catch-up、push→hub；subscribe-then-cursor

---

## 6. 延期清单

1. 普通 `ask_ai` 续聊写入 `parent_group_id`（指向上一 tip 的 `generation_group_id`），覆盖「两边都已 push 成功」的并发 tip  
2. execution lease  
3. Host Bridge WS／relay／E2EE／权限闭环  
4. 设备 announce UI／元数据  
5. MobileShell（若仍需要，单独 PR）
