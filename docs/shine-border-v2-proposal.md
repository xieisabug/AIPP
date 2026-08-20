# 闪亮边框全新方案（V2）

## 1) 目标

让闪亮边框只由**一个后端状态机**决定，前端只做渲染，彻底解决：

- 该亮不亮
- 该灭不灭
- 多处同时误亮
- MCP 卡片与消息卡片不同步

---

## 2) 现状缺点（review 结论）

### 前端侧

1. **双状态源并存，互相覆盖**
   - `useConversationEvents` 同时使用 `activityFocus`、`pendingUserMessageId`、`streamingAssistantMessageIds`、`manualShineMessageId` 回退混用。
   - 结果是“后端状态 + 本地猜测状态”并行，容易竞态和残留。
   - 位置：`src/hooks/useConversationEvents.ts`

2. **MCP 闪亮逻辑与 activity_focus 脱节**
   - MCP 卡片是否闪亮由 `McpToolCall` 内部 `executionState==="executing"` 决定，不走统一焦点。
   - 消息边框和 MCP 边框不在一个决策系统里。
   - 位置：`src/components/McpToolCall.tsx`

3. **MCP 卡片 ID 绑定不稳定**
   - `toolCallId` 用 `useState(callId)` 仅初始化一次，后续 `callId` prop 变化不会自动同步。
   - `callId` 缺失时通过 `(messageId+server+tool+parameters)` 去 `find`，重复参数会串号。
   - 位置：`src/components/McpToolCall.tsx`

4. **MCP 组件 key 仅按 index，可能复用旧状态**
   - `key="mcp-${index}"`，当同一位置的调用变化时组件状态被复用，导致边框状态串台。
   - 位置：`src/hooks/useMcpToolCallProcessor.tsx`

5. **unknown 状态处理不完整**
   - ACP 可发出 `unknown`，前端状态机仅显式处理 `pending/executing/success/failed`，容易卡在旧态。
   - 位置：`src-tauri/src/api/ai/acp.rs` + `src/components/McpToolCall.tsx`

### 后端侧

1. **MCP 恢复策略是“单备份”，不支持复杂并发**
   - `pre_mcp_backup: Option<ActivityFocus>` 只存一份备份，多个工具并发/重叠时恢复容易错位。
   - 位置：`src-tauri/src/state/activity_state.rs`

2. **流结束与 MCP 恢复存在竞态**
   - 流结束时若当前焦点是 MCP，会跳过 clear；后续 `restore_after_mcp` 可能恢复到过期 `assistant_streaming`，造成“该灭不灭”。
   - 位置：`src-tauri/src/api/ai/chat.rs` + `src-tauri/src/mcp/execution_api.rs`

3. **ACP 路径没有完整接入 activity_focus 生命周期**
   - ACP 会发 `message_update` / `mcp_tool_call_update`，但缺少与 `activity_focus` 同步推进/清理，导致焦点长期停留或漂移。
   - 位置：`src-tauri/src/api/ai/acp.rs`

4. **事件缺乏版本号/epoch 防乱序机制**
   - 前端无法可靠丢弃过期事件；旧事件晚到会覆盖新状态。

---

## 3) 全新方案：Shine State V2（推荐）

核心原则：**后端单一真相源 + 前端纯消费快照**。

### 3.1 新状态模型（后端）

```rust
enum ShineTarget {
  None,
  Message { message_id: i64, reason: MessageReason },   // user_pending / assistant_streaming
  McpCall { call_id: i64, reason: McpReason },          // executing / pending
  SubTask { execution_id: i64 },                        // 预留
}

struct ConversationShineState {
  conversation_id: i64,
  epoch: u64,          // 每次 ask/regenerate/cancel/new-turn 递增
  revision: u64,       // 每次状态变更递增
  pending_user: Option<i64>,
  streaming_message: Option<i64>,
  active_mcp_calls: Vec<i64>,  // executing/pending，按 started_at 排序
  primary_target: ShineTarget, // 唯一边框目标（强约束：只允许一个）
  updated_at_ms: i64,
}
```

### 3.2 决策规则（统一计算）

每次状态变更后统一调用 `recompute_primary_target()`：

1. 若有 executing MCP -> `primary = 最新 executing call`
2. 否则若有 pending MCP -> `primary = 最早 pending call`
3. 否则若有 streaming_message -> `primary = Message(streaming)`
4. 否则若有 pending_user -> `primary = Message(user_pending)`
5. 否则 `None`

> 这样天然规避“多处同时闪亮”。如果未来希望“多处可亮”，可在协议里加 `secondary_targets`，但默认只渲染 `primary`。

### 3.3 通信协议（替换 activity_focus_change）

新增事件：`shine_state_snapshot`

```json
{
  "type": "shine_state_snapshot",
  "data": {
    "conversation_id": 123,
    "epoch": 18,
    "revision": 77,
    "primary_target": { "target_type": "mcp_call", "call_id": 456, "reason": "executing" }
  }
}
```

新增命令：

- `get_shine_state(conversation_id) -> ConversationShineState`

前端只接受 **(epoch, revision)** 更大的快照；更小/相等直接丢弃。

### 3.4 MCP 绑定规则（必须改）

1. MCP 卡片渲染必须以 `call_id` 为主键。
2. `McpToolCall` 不再通过 server/tool/parameters 猜测匹配 call。
3. 若流式阶段尚无 `call_id`，只渲染“占位卡片（不发光）”，拿到 `call_id` 后替换。
4. React key 改为 `mcp-${call_id}`（无 call_id 时临时 key 必须含 hash，拿到后重建组件）。

### 3.5 ACP 路径并入同一状态机

ACP 也必须调用同一套 reducer：

- prompt 开始：`pending_user`
- 首个 agent chunk：切换 `streaming_message`
- tool call pending/executing/success/failed：更新 `active_mcp_calls`
- prompt done/error/cancel：清理对应状态

不再允许 ACP 通过“只发 message_update，不维护 shine 状态”运行。

---

## 4) 前端落地策略

1. 新建 `useShineState(conversationId)`：
   - 监听 `shine_state_snapshot`
   - 初始化调用 `get_shine_state`
   - 只保留最新 `(epoch, revision)`

2. `useConversationEvents` 删除这些本地猜测状态：
   - `manualShineMessageId`
   - `pendingUserMessageId`（仅用于 UI 文案可保留，不参与边框）
   - `streamingAssistantMessageIds`（不再决定边框）

3. 组件渲染：
   - MessageItem: `shouldShowShineBorder = primary_target is Message && id match`
   - McpToolCall: `isShining = primary_target is McpCall && call_id match`
   - 不再使用 `executionState==="executing"` 直接控制边框

---

## 5) 后端落地策略

1. 用 `ShineStateManager` 替换 `ConversationActivityManager` 的 `pre_mcp_backup` 方案。
2. 所有入口统一走 reducer：
   - `ask_ai/regenerate/cancel`
   - `ai/chat` stream start/chunk done/error
   - `mcp/execution_api` pending/executing/success/failed/stop
   - `ai/acp` prompt/tool/update/done/error
3. 每次 reducer 完成后发 `shine_state_snapshot`。

---

## 6) 兼容与迁移（建议）

1. **阶段 1**：后端同时发 `activity_focus_change` + `shine_state_snapshot`（兼容旧前端）。
2. **阶段 2**：前端切到 `shine_state_snapshot`，旧逻辑 behind flag。
3. **阶段 3**：删除 `manual/fallback` 闪亮逻辑与 `pre_mcp_backup`。

---

## 7) 验收用例（必须覆盖）

1. 普通对话：发送 -> 首 token -> 完成 -> 边框准确迁移并结束。
2. 流中触发单工具：消息 -> MCP -> 回到消息/结束。
3. 多工具（含重复参数）：只高亮唯一 primary，且不串号。
4. 取消：任何阶段取消都能在 1 个事件周期内灭灯。
5. ACP：与普通路径一致，无残留 user_pending。
6. 乱序模拟：旧 revision 到达不会覆盖新状态。

---

## 8) 为什么这个方案更稳

- 不再依赖“前端猜测 + 后端事件补丁”的混合逻辑。
- 不再依赖“恢复备份”这种脆弱机制，改为可重算状态。
- 用 `epoch + revision` 解决乱序覆盖。
- MCP 彻底以 `call_id` 绑定，消除参数匹配歧义和组件复用串态问题。

