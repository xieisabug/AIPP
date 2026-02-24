# MCP 状态链路排查说明（前后端通信与到达保障）

## 1) MCP 状态是如何从后端传到前端的

### A. 后端状态源（数据库）
- MCP 工具调用记录在 `mcp.db` 的 `mcp_tool_calls` 中维护，状态在后端按 `pending / executing / success / failed` 更新。
- 关键入口：
  - 创建调用后立即广播 `pending`：`src-tauri/src/mcp/execution_api.rs:445-500`
  - 开始执行时广播 `executing`：`src-tauri/src/mcp/execution_api.rs:716-733`
  - 执行成功/失败后广播最终态：`src-tauri/src/mcp/execution_api.rs:244-361`
  - ACP 路径也会落库并发 `mcp_tool_call_update`：`src-tauri/src/api/ai/acp.rs:821-1098, 1100-1337`

### B. 后端事件通道
- 统一事件名：`conversation_event_{conversation_id}`。
- MCP 状态事件类型：`mcp_tool_call_update`，载荷包含 `call_id / status / server_name / tool_name / parameters / result / error`。
- 广播函数：`send_conversation_event_to_chat_windows`（ask/chat_ui 双窗口）：`src-tauri/src/utils/window_utils.rs:68-84`

### C. 前端接收与状态聚合
- 前端在 `useConversationEvents` 订阅 `conversation_event_{id}`，遇到 `mcp_tool_call_update` 后更新 `mcpToolCallStates(Map)`：
  - `src/hooks/useConversationEvents.ts:428-477`
- Map 之外还有活跃集合 `activeMcpCallIds` 用于执行中 UI/焦点控制：
  - `src/hooks/useConversationEvents.ts:455-474`

### D. 前端渲染绑定
- `McpToolCall` 组件通过 `call_id` 到 `mcpToolCallStates` 取状态并渲染 badge（待执行/执行中/成功/失败）：
  - `src/components/McpToolCall.tsx:129-186`
- `call_id` 主要来自消息内容里的注释锚点 `<!-- MCP_TOOL_CALL:{...} -->`，由 `useMcpToolCallProcessor` 解析：
  - `src/hooks/useMcpToolCallProcessor.tsx:39-334, 416-576`

---

## 2) “状态正常抵达”的现有保障机制

## 2.1 持久化 + 事件双轨
- 后端先更新 DB，再发事件；事件丢失时前端仍可从 DB 拉取恢复（最终一致）。
- 前端在会话初始化时主动拉取 `get_mcp_tool_calls_by_conversation`，补齐订阅前丢失的状态：
  - `src/hooks/useConversationEvents.ts:184-243, 584-596`

## 2.2 多次触发同步点
- 除了实时 `mcp_tool_call_update`，前端还在 `message_type_end` 时触发一次 MCP 状态刷新：
  - `src/hooks/useConversationEvents.ts:420-427`

## 2.3 状态合并策略
- 前端收到增量状态时，会和已有状态合并，避免字段缺失导致 UI 丢字段：
  - `src/hooks/useConversationEvents.ts:437-453`

---

## 3) 这次复查里，仍可能导致“有时看不到状态”的风险点

## 风险点 A：UI 展示依赖 `MCP_TOOL_CALL` 注释锚点
- 现状：前端是否显示 `McpToolCall` 卡片，依赖消息文本里能解析到 `<!-- MCP_TOOL_CALL:... -->`。
- 若锚点未写入/写入延迟/格式异常，可能出现：
  - 后端状态在更新（Map 里也有），但消息区域没有对应工具卡片可展示。
- 相关位置：
  - 写入锚点：`src-tauri/src/api/ai/chat.rs:482-493, 688-699`，`src-tauri/src/api/ai/acp.rs:622-670`
  - 解析锚点：`src/hooks/useMcpToolCallProcessor.tsx:198-334, 416-503`

## 风险点 B：ACP 未识别状态会映射成 `unknown`
- ACP 状态映射函数对未知枚举返回 `"unknown"`：
  - `src-tauri/src/api/ai/acp.rs:216-223`
- 前端活跃态与卡片状态机只显式处理四态（pending/executing/success/failed），`unknown` 可能被当成非活跃，表现为“状态看起来没更新”：
  - `src/hooks/useConversationEvents.ts:460-467`
  - `src/components/McpToolCall.tsx:149-179`

## 风险点 C：事件发送是“尽力而为”，无 ACK/重试
- `send_conversation_event_to_chat_windows` 当前是 `let _ = emit(...)`，发送失败不会重试或补发。
- 设计上依赖“前端后续 DB 刷新”兜底最终一致，但不保证每个瞬时状态都实时可见。
- 相关位置：`src-tauri/src/utils/window_utils.rs:75-83`

---

## 4) 结论（你当前现象的解释）

- 你看到“有些 MCP 状态能看到、有些看不到”，更像是**渲染锚点绑定层**或**状态值落在前端未识别分支**导致的可见性不稳定，而不是单点的执行失败。
- 当前链路已经具备“最终状态可恢复”（DB 拉取）的保障，但“每个中间态必达 + 必可见”还不是强保证模型。

---

## 5) 下一步建议（最小改动方向）

1. **把 `unknown` 显式映射到可见态**（例如 `executing` 或 `failed` + 原因），避免状态被静默吞掉。  
2. **给 `McpToolCall` 加“无锚点但有状态”的兜底渲染策略**（按 message_id + 最近 call 匹配），降低对注释锚点的硬依赖。  
3. **在前端增加定时轻量补偿刷新**（仅在存在 activeMcpCallIds 时，每 1-2s 拉一次 DB，结束即停止），提升中间态可见性。  

---

## 6) 开发计划：定时轻量补偿刷新（重点：无定时器泄露）

> 目标：只在“确实需要补偿”的窗口开启轮询；任何退出条件都立即停止，并且不让旧会话/旧请求回写新会话状态。

### 6.1 实现位置
- 主实现放在 `src/hooks/useConversationEvents.ts`（已有 `refreshMcpToolCalls` / `activeMcpCallIds` / `conversationId`）。
- 不新增全局定时器，不跨 hook 共享实例；生命周期严格绑定当前 `conversationId`。

### 6.2 调度模型（避免重叠与泄露）
- 使用 `setTimeout` 递归，不用 `setInterval`（避免慢请求叠加导致并发轮询）。
- 关键 ref：
  - `pollTimerRef`: 当前 timeout id（唯一）
  - `pollGenerationRef`: 轮询代次（conversation 切换时 +1，使旧请求失效）
  - `pollInFlightRef`: 防止同一时刻并发 invoke
  - `destroyedRef`: 组件卸载标记（防止卸载后 setState）

### 6.3 启停条件（明确状态机）
- **启动**：`conversationId` 有效，且 `activeMcpCallIds.size > 0`。
- **继续**：本轮刷新后仍存在 active call（pending/executing）。
- **停止（立即清理）**：
  1. conversation 切换（`conversationId` 变化）
  2. 组件卸载（effect cleanup）
  3. `activeMcpCallIds.size === 0`（所有工具进入 success/failed）
  4. 收到 `conversation_cancel`
  5. 收到 `stream_complete` 且无 active call
  6. 手动清理入口（如 `clearShiningMessages`）触发重置

### 6.4 防止“旧请求污染新会话”
- 每次发起刷新前捕获 `generationSnapshot = pollGenerationRef.current`。
- `invoke(...).then/catch/finally` 内先校验：
  - `generationSnapshot === pollGenerationRef.current`
  - `!destroyedRef.current`
- 任一不满足则直接丢弃结果，不更新状态、不续排下一轮。

### 6.5 轮询频率与负载策略
- 基础间隔：`1200ms`（轻量补偿）。
- 失败退避：`1200 -> 2000 -> 3000ms`（上限 3000ms），成功后恢复 1200ms。
- 仅在 active call 存在时轮询；inactive 时完全停轮询（零额外负载）。

### 6.6 验证清单（防泄露重点）
1. 连续切换 conversation：旧会话 timer 数量不增长，且不再触发旧会话刷新。  
2. MCP 从 executing -> success/failed：在最终态后 1 个周期内停止轮询。  
3. 对话取消/窗口关闭/组件卸载：轮询立即停止，无后续 state update 警告。  
4. 人工制造慢请求：无并发重叠刷新（`pollInFlightRef` 生效）。  
5. 反复触发工具执行：timer 始终“至多一个”，不会累计。  

### 6.7 计划中的代码改动点（最小范围）
- `useConversationEvents.ts`：
  - 新增 `startMcpCompensationPolling / stopMcpCompensationPolling`
  - 新增上述 4 个 ref 与 cleanup
  - 在 `mcp_tool_call_update`、`conversation_cancel`、`stream_complete`、conversation 切换路径接入启停
- 不改后端协议，不改数据库结构，不改现有事件格式。
