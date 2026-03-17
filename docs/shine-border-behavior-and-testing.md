# 闪亮边框（Shine Border）行为说明与回归测试指南

## 概述

本文档用于记录聊天界面中“闪亮边框”的**正确行为、状态来源、调试入口、回归场景和验证方法**，避免以后再次出现边框停留错误、MCP 卡片与消息边框不同步、发送按钮状态错误、工具完成后未正确续写等问题时，只能靠临时猜测排查。

这份文档覆盖三个直接相关的 UI/状态：

- 消息外层的闪亮边框
- MCP 工具卡片的闪亮边框
- 发送按钮 / “是否仍在运行”状态

---

## 核心原则

### 1. 后端是唯一活动状态真相源

当前活动状态由后端 `ConversationActivityManager` 统一维护，位于：

- `src-tauri/src/state/activity_state.rs`

它负责生成并广播：

- `runtime_state_snapshot`
- `shine_state_snapshot`

前端不应该再自行“猜”当前到底是用户消息、助手消息还是 MCP 工具在发光。

### 2. 前端只消费快照，不自己决定活动边框

前端消费逻辑集中在：

- `src/hooks/useConversationEvents.ts`

这里会：

- 监听 `shine_state_snapshot`
- 监听 `runtime_state_snapshot`
- 用 `(epoch, revision)` 丢弃旧快照
- 在必要时通过 `get_shine_state` / `get_conversation_runtime_state` 做补偿同步

### 3. MCP 卡片必须只按 `call_id` 绑定

MCP 卡片组件位于：

- `src/components/McpToolCall.tsx`

当前约束是：

- `callId` / `effectiveCallId` 才是卡片身份
- 没拿到 `call_id` 时只能是“占位卡片”
- 占位卡片不能猜测匹配现有调用记录
- 占位卡片不能发光

对应的渲染入口位于：

- `src/hooks/useMcpToolCallProcessor.tsx`

### 4. pending MCP 不算“执行中”

这是一个非常重要的规则：

- `pending` MCP **可以显示卡片和状态徽标**
- 但 `pending` MCP **不能占用 shine**
- 也 **不能让发送按钮进入“仍在运行”状态**

只有 `executing` MCP 才能成为活动焦点。

### 5. 搜索定位高亮不是 activity shine

`ConversationUI` 中的“定位到某条消息”会临时对消息加 2 秒手动高亮：

- `src/components/ConversationUI.tsx`

这类高亮只是**临时视觉提示**，不是运行态，不应该覆盖后端 activity shine。  
当前实现中，`useConversationEvents` 会把：

- `activityShiningMessageIds`
- `manualShiningMessageIds`

做并集合并显示。

---

## 相关文件速查

### 后端

- `src-tauri/src/state/activity_state.rs`
  - 活动状态机
  - `set_user_pending`
  - `set_assistant_streaming`
  - `set_mcp_pending`
  - `set_mcp_executing`
  - `finish_mcp_call`
  - `restore_after_mcp`
- `src-tauri/src/mcp/execution_api.rs`
  - 工具执行
  - 成功 / 失败 / 停止后的状态切换
  - continuation 队列与 drain
  - MCP -> 消息边框 handoff
- `src-tauri/src/api/ai/acp.rs`
  - ACP 路径的工具状态同步
- `src-tauri/src/api/ai_api.rs`
  - ask / regenerate / tool_result continuation 入口

### 前端

- `src/hooks/useConversationEvents.ts`
  - shine/runtime 快照消费中心
  - MCP 完成补偿同步
- `src/components/McpToolCall.tsx`
  - MCP 卡片状态与边框渲染
- `src/hooks/useMcpToolCallProcessor.tsx`
  - 从消息内容中的 `MCP_TOOL_CALL` 注释渲染组件
- `src/components/ConversationUI.tsx`
  - 手动搜索定位高亮
- `src/components/conversation/MessageList.tsx`
  - 消息边框渲染

### 测试

- `src/hooks/useConversationEvents.test.ts`
- `src/components/McpToolCall.test.tsx`
- `src-tauri/src/state/activity_state.rs` 内联测试
- `src-tauri/src/mcp/execution_api.rs` 内联测试

---

## 当前状态模型

### 后端活动优先级

后端的当前焦点计算规则在 `ConversationActivityManager::recompute_focus()` 中：

1. 若存在 `executing` MCP，取**最新 executing** 的 call 作为焦点
2. 否则若存在 `streaming_message_id`，消息发光
3. 否则若存在 `pending_user_message_id`，用户消息发光
4. 否则为 `none`

注意：

- `pending` MCP 仍会保存在 `active_mcp_calls` 中
- 但 `pending` **不会进入焦点**
- 因此它不会点亮卡片，也不会让 runtime 变成 running

### runtime_state 与 shine_state 的关系

#### runtime_state

用于驱动发送按钮等“当前是否仍在运行”的 UI。

当前 phase：

- `idle`
- `user_pending`
- `assistant_streaming`
- `mcp_executing`

#### shine_state

用于驱动边框目标。

当前 target：

- `none`
- `message { reason: user_pending | assistant_streaming }`
- `mcp_call { reason: mcp_executing }`

### 前端显示规则

#### 消息边框

消息边框显示条件：

- `shiningMessageIds.has(message.id) === true`

其中 `shiningMessageIds` 由两部分组成：

- 后端 activity shine 派生出的 `activityShiningMessageIds`
- 搜索定位等临时效果使用的 `manualShiningMessageIds`

#### MCP 卡片边框

MCP 卡片显示边框的条件：

- `shiningMcpCallId === effectiveCallId`

这里的 `effectiveCallId` 来自：

- 服务端传来的 `callId`
- 或前端本次手动创建得到的 `createdCallId`

不会再按 `(messageId + server + tool + parameters)` 去猜。

---

## 正确行为定义

下面这些场景是以后判断“行为是否正确”的基准。

### 1. 普通对话，无工具

#### 期望顺序

1. 用户发送消息
2. 用户消息边框发光
3. 助手开始输出首个 token
4. 边框切到助手消息
5. 助手输出结束
6. 边框熄灭

#### 发送按钮

- 用户发送后应为 running
- 助手完成后应回到 idle

---

### 2. 助手消息中出现 MCP 工具，工具自动执行成功

#### 期望顺序

1. 助手消息流式输出时，消息边框发光
2. 工具卡片创建为 `pending`
3. `pending` 卡片**不发光**
4. 工具进入 `executing`
5. 边框从消息切到对应 MCP 卡片
6. 工具执行成功
7. 边框**先切回该工具所属的外层 assistant 消息**
8. 然后继续触发续写
9. 续写流式输出期间，消息边框保持在 assistant 消息上
10. 最终结束后熄灭

#### 错误表现

- 工具成功后卡片还在发光
- 工具成功后外层消息没有重新发光
- 工具成功后续写没有继续发生

---

### 3. 工具执行失败，但允许继续对话

#### 期望顺序

1. MCP 卡片执行中发光
2. 工具失败
3. 若触发 `continue_with_error` 或自动错误续写：
   - 边框应先回到工具所属的 assistant 消息
   - 然后继续续写
4. 续写期间不应继续停留在 MCP 卡片上

#### 错误表现

- 错误续写时，MCP 卡片熄灭了，但外层消息也没亮
- 错误续写时仍停留在工具卡片

---

### 4. 待人工交互的 MCP（pending）

#### 期望顺序

1. 工具卡片显示为“待执行”
2. 卡片可以展开、可操作
3. **卡片不发光**
4. 发送按钮**不能仅因为 pending 而显示为 running**

#### 允许的情况

如果当时助手消息本身还在 streaming，那么：

- 外层消息仍然可能发光

这是对的，因为活动焦点仍在消息，而不是 pending 工具。

---

### 5. 多个工具调用

#### 期望顺序

- 只有一个 executing MCP 可以成为当前 shine target
- 若有多个 executing，取**最新 executing**
- pending 工具只显示卡片状态，不抢边框
- 工具完成后，边框应转回消息或下一个 executing 工具

#### 错误表现

- 多个卡片同时亮
- 重复参数的两个工具串号
- 边框亮在错误的工具卡片上

---

### 6. 搜索定位消息

#### 期望顺序

1. 搜索或定位到某条消息
2. 该消息临时高亮约 2 秒
3. 该临时高亮只影响视觉，不改变 runtime
4. 若同时存在 activity shine，应与 activity shine 共存，不应覆盖后者

---

### 7. 取消

#### 期望顺序

- 任意阶段取消后，边框应在一个事件周期内熄灭
- 发送按钮回到 idle
- 进行中的 MCP 不应继续保持活动焦点

---

## 关键时序说明

### A. MCP 完成后的 handoff

这是本轮问题最多的地方。

#### 成功路径

在 `src-tauri/src/mcp/execution_api.rs` 中：

1. 工具状态更新为 `success`
2. 广播 `mcp_tool_call_update`
3. 若需要 continuation：
   - 先把焦点切回工具所属的 assistant 消息
   - 再移除 MCP 焦点
4. continuation 真正开始流式输出后，再由正常 streaming 路径接管

#### 失败继续路径

错误继续与成功继续的要求相同：

1. 不能直接从 MCP 卡片掉到 `none`
2. 必须先回到外层 assistant 消息
3. 然后再进入 continuation

### B. queued batch continuation

另一个高风险点是 continuation 锁。

当前规则：

- 同一会话的 continuation 通过会话级锁串行执行
- 如果锁正被持有，新的 batch continuation 会先排队
- 当前持锁方结束前必须 drain 掉排队 continuation

否则会出现：

- 工具执行成功
- 边框暂时切回消息
- 但续写其实没有真正继续

---

## 调试时应该重点看什么

### 1. 先看后端活动焦点日志

重点关注日志：

- `Activity focus changed ... UserPending`
- `Activity focus changed ... AssistantStreaming`
- `Activity focus changed ... McpExecuting`
- `Activity focus changed ... None`

#### 正确示例

成功工具续写的典型顺序应类似：

1. `AssistantStreaming { message_id = X }`
2. `McpExecuting { call_id = Y }`
3. 工具完成
4. `AssistantStreaming { message_id = X }`
5. continuation 真正启动后继续保持/更新为消息焦点
6. 最终 `None`

#### 错误信号

- 工具完成后直接 `None`
- 工具完成后一直停留在 `McpExecuting`
- pending 阶段就出现 `McpExecuting`

### 2. 再看前端是否收到快照

前端关键日志：

- `[ShineState] Synced state from backend: ...`
- `[RuntimeState] Synced state from backend: ...`
- `[MCP] stop compensation polling: ...`

要确认：

- 快照 revision 在增长
- 新快照没有被旧快照覆盖
- MCP 完成后的延迟补偿同步有真正触发

### 3. 再看 MCP 卡片身份是否稳定

确认：

- `McpToolCall` 是否拿到了正确 `callId`
- 是否是占位卡片
- 是否错误地复用了旧组件

如果看到“没有 callId 的卡片却亮了”，基本就是 bug。

---

## 常见错误现象与优先排查方向

### 现象 1：工具完成后卡片一直亮

优先排查：

- `finish_mcp_call()` 是否调用
- continuation handoff 是否发生
- `shine_state_snapshot` 是否更新
- 前端延迟同步是否被意外取消

### 现象 2：工具完成后卡片熄灭，但消息没有亮

优先排查：

- 是否少了“先切回 origin assistant message”的 handoff
- `assistant_message_id` / `message_id` 是否丢失
- 是否直接从 `McpExecuting` 切成 `None`

### 现象 3：pending 工具也在发光，发送按钮也显示运行中

优先排查：

- `pending` 是否被错误映射成 `McpExecuting`
- `recompute_focus()` 是否错误考虑了 pending
- `runtime_state.phase` 是否被 pending 覆盖

### 现象 4：只有 MCP 会影响边框，普通消息不再切换

优先排查：

- `useConversationEvents` 是否把 MCP 补偿逻辑扩散到了全局 shine 判断
- manual highlight 是否覆盖了 activity shine

### 现象 5：错误续写没有继续发生

优先排查：

- continuation lock 是否被占用
- 是否出现 `Continuation lock busy, queued pending batch continuation`
- 当前持锁方结束前是否 drain queue

### 现象 6：重复工具参数时边框亮错卡

优先排查：

- `McpToolCall` 是否仍在猜 call identity
- `useMcpToolCallProcessor` 的 key 是否带 `call_id`

---

## 当前自动化测试覆盖

### 前端测试

#### `src/hooks/useConversationEvents.test.ts`

当前覆盖：

- `clears a stale MCP shine state after delayed backend sync`
- `allows delayed MCP reconciliation to hand off shine state to assistant streaming`
- `allows failed MCP continuation to hand off shine state to assistant streaming`
- `keeps backend-driven shine when temporary manual highlights are cleared`

这些测试主要保证：

- MCP 完成后不会卡在旧的 `mcp_call`
- 成功续写和错误续写都能切回消息边框
- 临时搜索高亮不会把 activity shine 冲掉

#### `src/components/McpToolCall.test.tsx`

当前覆盖：

- `does not guess an existing tool call when streamed call_id is still missing`

这条测试用于保证：

- 没有 `call_id` 的占位卡片不会猜测已有调用
- 占位卡片不会错误发光

### Rust 测试

#### `src-tauri/src/state/activity_state.rs`

当前覆盖：

- `recompute_focus_prefers_newest_executing_call`
- `recompute_focus_ignores_pending_calls_when_waiting_for_user`
- `pending_mcp_does_not_override_streaming_message_focus`
- `finish_mcp_call_hands_off_to_streaming_message`

#### `src-tauri/src/mcp/execution_api.rs`

当前覆盖：

- `drain_pending_batch_continuations_replays_newly_queued_work`
- `drain_pending_batch_continuations_is_noop_without_queue`
- `continuation_anchor_prefers_assistant_message_id`
- `continuation_anchor_falls_back_to_message_id`

---

## 推荐验证命令

### 前端

```bash
npm run test -- src/hooks/useConversationEvents.test.ts src/components/McpToolCall.test.tsx
npm run test
npm run build
```

### Rust

```bash
cargo build --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml continuation_queue_tests --no-run
cargo test --manifest-path src-tauri/Cargo.toml activity_state::tests --no-run
```

如果你只改了 shine 逻辑，优先保证：

1. 前端定向测试通过
2. Rust build 通过
3. 相关 Rust 定向测试至少能编译通过

---

## 推荐手工验证清单

每次修改 shine 逻辑后，建议至少手工跑下面 6 个场景。

### 场景 1：普通对话

- 发送用户消息
- 确认用户消息亮
- 首 token 到来后切到 assistant 消息
- 完成后熄灭

### 场景 2：单工具成功

- assistant 输出工具卡片
- pending 不亮
- executing 时卡片亮
- 完成后立刻回到外层 assistant 消息
- 自动继续续写

### 场景 3：单工具失败后继续

- executing 时卡片亮
- 失败后点击“以错误继续对话”或自动继续
- 边框先回到外层 assistant 消息
- 然后继续续写

### 场景 4：待人工交互工具

- pending 卡片显示
- 不亮
- 发送按钮不应仅因为它而保持 running

### 场景 5：重复参数的多个工具

- 连续两次相同 server/tool/parameters
- 确认没有串号
- 确认只会亮当前 executing 的那张卡

### 场景 6：搜索定位

- 调用 `scrollToMessage`
- 被定位消息临时亮起
- 若当前 activity shine 也存在，不应被覆盖

---

## 修改 shine 逻辑时的最低要求

以后如果再改这块逻辑，最低要求是：

1. 不要重新引入前端按参数猜 `call_id`
2. 不要让 `pending` MCP 占用 runtime/shine
3. 成功 continuation 和错误 continuation 都必须做 message handoff
4. 手动搜索高亮必须和 activity shine 分离
5. 如果引入新的 continuation 排队逻辑，必须补 drain/并发回归测试

---

## 一句话判断当前实现是否正确

如果系统处于正确状态，那么应该满足下面这句话：

> **真正控制边框的是后端 shine snapshot；pending MCP 只显示状态不抢焦点；executing MCP 亮卡片；工具完成后边框必须先回到工具所属消息，再由续写或结束逻辑接管。**
