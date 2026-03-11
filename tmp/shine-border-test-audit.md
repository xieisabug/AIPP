# Shine Border 自动化测试审计

## 目标

对照 `docs/shine-border-behavior-and-testing.md`，审查当前前端与 Rust 侧自动化测试是否足以验证 shine border / runtime / MCP handoff 的关键行为，并列出缺失测试项与可测试性改造建议。

## 审计结论

当前测试**覆盖了最核心的一部分回归点**，尤其是：

- MCP 完成后的前端补偿同步不会长期卡在旧的 `mcp_call`
- 成功 continuation / 错误 continuation 都能在前端切回 assistant message shine
- 手动高亮不会在清除时把后端 activity shine 一起清掉
- 占位 MCP 卡片在 `call_id` 缺失时不会猜测绑定已有调用
- 后端 focus 优先级里，`pending` MCP 不会压过 streaming message
- continuation queue 的 drain 逻辑有最小单元测试

但是，**测试还不能称为“健全并覆盖所有重要功能”**。文档里强调的几个关键场景，目前仍然是“只覆盖了一部分”，或者完全依赖手工验证，没有自动化保护。

## 补齐进展（本次实现后）

本次已经补上以下自动化测试与轻量可测试性抽取：

- `src/hooks/useConversationEvents.test.ts`
  - 普通无工具对话的 `user_pending -> assistant_streaming -> idle`
  - `(epoch, revision)` 旧快照丢弃
  - `conversation_cancel` 清理
  - only-pending-MCP 保持 `idle` / 不发光
- `src/components/McpToolCall.test.tsx`
  - pending 卡片可见但不发光
- `src/components/ConversationUI.test.ts`
  - `scrollToMessage()` 使用的临时高亮 helper：立即高亮、2 秒清理
- `src/hooks/useMcpToolCallProcessor.test.tsx`
  - 多工具同参但不同 `call_id` 时不串号
  - placeholder 卡片收到真实 `call_id` 后正确升级
- `src-tauri/src/state/activity_state.rs`
  - runtime/shine 的普通 happy path
  - only-pending-MCP 仍为 `idle` / `none`
- `src-tauri/src/mcp/execution_api.rs`
  - `continue_with_error` 状态约束
  - `continue_with_error` 错误消息优先级
  - continuation lock busy -> queue
  - 预先排队 continuation 在执行后被 drain

相关验证已通过：

- `npm run test -- src/components/ConversationUI.test.ts src/hooks/useMcpToolCallProcessor.test.tsx src/hooks/useConversationEvents.test.ts src/components/McpToolCall.test.tsx`
- `cargo test --manifest-path src-tauri/Cargo.toml continuation_queue_tests`
- `cargo test --manifest-path src-tauri/Cargo.toml activity_state::tests`
- `npm run build`
- `cargo check --manifest-path src-tauri/Cargo.toml`

## 本次确认过的现有测试

### 前端

文件：

- `src/hooks/useConversationEvents.test.ts`
- `src/components/McpToolCall.test.tsx`

已存在的测试点：

1. `clears a stale MCP shine state after delayed backend sync`
2. `allows delayed MCP reconciliation to hand off shine state to assistant streaming`
3. `allows failed MCP continuation to hand off shine state to assistant streaming`
4. `keeps backend-driven shine when temporary manual highlights are cleared`
5. `does not guess an existing tool call when streamed call_id is still missing`

### Rust

文件：

- `src-tauri/src/state/activity_state.rs`
- `src-tauri/src/mcp/execution_api.rs`

已存在的测试点：

1. `recompute_focus_prefers_newest_executing_call`
2. `recompute_focus_ignores_pending_calls_when_waiting_for_user`
3. `pending_mcp_does_not_override_streaming_message_focus`
4. `finish_mcp_call_hands_off_to_streaming_message`
5. `drain_pending_batch_continuations_replays_newly_queued_work`
6. `drain_pending_batch_continuations_is_noop_without_queue`
7. `continuation_anchor_prefers_assistant_message_id`
8. `continuation_anchor_falls_back_to_message_id`

## 本次执行过的验证命令

以下命令本次已执行并通过：

- `npm run test -- src/hooks/useConversationEvents.test.ts src/components/McpToolCall.test.tsx`
- `npm run build`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml activity_state::tests`
- `cargo test --manifest-path src-tauri/Cargo.toml continuation_queue_tests`

## 按文档场景的覆盖判断

### 1. 普通对话，无工具

文档要求：

- 用户发送后，user message shine
- 首 token 到来后切到 assistant message
- 完成后熄灭
- runtime 从 `user_pending` -> `assistant_streaming` -> `idle`

当前状态：**缺失**

原因：

- 现有前端测试没有直接验证 `useConversationEvents` 对 `user_pending` / `assistant_streaming` / `idle` 的完整切换。
- 现有 Rust 测试也没有直接覆盖 `set_user_pending()`、`set_assistant_streaming()`、`clear_focus()` 这条最基本的 happy path。

### 2. 单工具成功，自动 continuation

文档要求：

- assistant message 先亮
- `pending` 卡片不亮
- `executing` 卡片亮
- 成功后先 handoff 回 assistant message
- 然后 continuation 继续

当前状态：**部分覆盖**

已有覆盖：

- 前端覆盖了“MCP 成功后切回 assistant message shine”
- 后端覆盖了“执行中 MCP 完成后，focus 可以回到 streaming message”

仍缺：

- 没有自动化测试覆盖“`pending` 卡片可见但不发光、也不让 runtime 进入 running”这整条行为。
- 没有自动化测试覆盖执行成功时 `handoff_mcp_focus_to_origin_message()` 的完整调用链。
- 没有自动化测试把 `useMcpToolCallProcessor.tsx`、`McpToolCall.tsx`、`useConversationEvents.ts` 串起来验证“卡片亮 -> 消息亮”的 UI 级联。

### 3. 工具失败，但允许继续对话

文档要求：

- executing 卡片先亮
- 失败后如果继续对话，先 handoff 到 assistant message
- 续写期间不能还停留在卡片上

当前状态：**部分覆盖**

已有覆盖：

- 前端已有失败 continuation handoff 测试。

仍缺：

- `continue_with_error()` 本身没有 Rust 自动化测试。
- 没有验证“只有 failed 状态允许 `continue_with_error`”。
- 没有验证 `continue_with_error()` 结束后确实调用了 `handoff_mcp_focus_to_origin_message()`。

### 4. 待人工交互的 MCP（pending）

文档要求：

- 卡片显示 pending 状态
- 卡片不亮
- 发送按钮不能仅因为 pending 进入 running

当前状态：**部分覆盖**

已有覆盖：

- Rust 侧 `recompute_focus_ignores_pending_calls_when_waiting_for_user`
- Rust 侧 `pending_mcp_does_not_override_streaming_message_focus`

仍缺：

- 没有前端自动化测试直接验证 `runtimeState.phase` 在“只有 pending MCP”时保持 `idle`。
- 没有组件测试验证 pending 卡片渲染状态正常但 `shine-border` 不出现。
- 没有 UI 级测试覆盖发送按钮不因 pending 误判 running。

### 5. 多个工具调用

文档要求：

- 只能有一个 executing MCP 成为 shine target
- 多个 executing 时取最新
- pending 不抢边框
- 完成后切回 message 或下一个 executing 工具
- 重复参数的两个工具不能串号

当前状态：**部分覆盖偏缺失**

已有覆盖：

- Rust 侧 `recompute_focus_prefers_newest_executing_call`
- 前端只覆盖了“没有 `call_id` 时不猜已有工具”

仍缺：

- 没有测试“两个参数完全相同的工具调用，UI 仍按 `call_id` 正确区分”。
- 没有测试“同一条消息里多个卡片时，只有当前 executing 的那张卡亮”。
- 没有测试“一个工具完成后，shine 是否正确切到下一个 executing 工具”。
- `useMcpToolCallProcessor.tsx` 的 `key={`mcp-${data.call_id ?? ...}`}` 当前没有对应回归测试。

### 6. 搜索定位消息

文档要求：

- `scrollToMessage()` 触发临时高亮
- 高亮约 2 秒
- 不改变 runtime
- 与后端 activity shine 共存，不互相覆盖

当前状态：**部分覆盖**

已有覆盖：

- `useConversationEvents.test.ts` 只覆盖了手动高亮集合清除后，后端 shine 仍在。

仍缺：

- 没有真正从 `ConversationUI.tsx` 的 `scrollToMessage()` 路径发起的测试。
- 没有验证 `setTimeout(2000)` 到期后高亮自动消失。
- 没有验证 `scrollToMessage()` 期间 runtime 不被修改。

### 7. 取消

文档要求：

- 取消后一个事件周期内边框熄灭
- runtime 回到 idle
- 进行中的 MCP 不再保持活动焦点

当前状态：**缺失**

原因：

- `useConversationEvents.ts` 对 `conversation_cancel` 有明确分支处理，但没有对应前端测试。
- Rust 侧也没有面向 activity state / execution state 的取消路径测试。

### 8. 关键时序 A：MCP 完成后的 handoff

文档要求：

- 成功/失败 continuation 都不能从 MCP 直接掉到 `none`
- 必须先回到 origin assistant message

当前状态：**部分覆盖**

已有覆盖：

- 前端通过延迟同步测试验证了最终能看到 assistant message shine。
- Rust 有 `finish_mcp_call_hands_off_to_streaming_message`，但它只验证纯状态，不验证 `execution_api.rs` 真实 handoff 调用。

仍缺：

- 没有直接测试 `handoff_mcp_focus_to_origin_message()`。
- 没有测试 `assistant_message_id` 优先于 `message_id` 的 handoff 效果，只测试了 anchor 选择函数本身。
- 没有测试“没有 anchor message id 时”的行为边界。

### 9. 关键时序 B：queued batch continuation

文档要求：

- 同会话 continuation 需要串行
- 锁被占用时后续 continuation 要排队
- 当前持锁方结束前要 drain queue

当前状态：**部分覆盖**

已有覆盖：

- 只覆盖了 `drain_pending_batch_continuations()` 本身的最小逻辑。

仍缺：

- 没有自动化测试覆盖 `Continuation lock busy, queued pending batch continuation` 分支。
- 没有自动化测试证明“锁释放后排队 continuation 真的会被 drain 并执行”。
- 没有把 `run_batch_continuation_once()` / `tool_result_continue_ask_ai_impl()` / `batch_tool_result_continue_ask_ai_impl()` 串联验证。

## 明确缺失的测试项

下面这些是建议补齐的自动化测试项。

### 前端建议新增

#### 1. `useConversationEvents`：普通无工具对话完整切换

建议文件：

- `src/hooks/useConversationEvents.test.ts`

测试内容：

- 输入 `shine_state_snapshot` / `runtime_state_snapshot`
- 验证 `user_pending` -> `assistant_streaming` -> `idle`
- 验证 `shiningMessageIds` 从用户消息切到 assistant 消息，最后清空

#### 2. `useConversationEvents`：拒绝旧快照

建议文件：

- `src/hooks/useConversationEvents.test.ts`

测试内容：

- 先应用较新的 `(epoch, revision)`
- 再发送较旧快照
- 验证旧快照不会覆盖当前 `shineState` / `runtimeState`

原因：

- 文档把 `(epoch, revision)` 丢弃旧快照列为核心规则，但目前没有直接单测。

#### 3. `useConversationEvents`：取消路径

建议文件：

- `src/hooks/useConversationEvents.test.ts`

测试内容：

- 发出 `conversation_cancel`
- 验证 `runtimeState` 归零、`shiningMcpCallId` 清空、消息 shine 清空
- 验证活跃 MCP 集合被清理

#### 4. `ConversationUI`：`scrollToMessage()` 临时高亮 2 秒

建议文件：

- 建议新增 `src/components/ConversationUI.test.tsx`

测试内容：

- 调用暴露的 `scrollToMessage()`
- 验证目标消息立即加入手动高亮
- `advanceTimersByTime(2000)` 后高亮移除
- 若后台已有 activity shine，则两者并存

#### 5. `McpToolCall`：pending 卡片不亮

建议文件：

- `src/components/McpToolCall.test.tsx`

测试内容：

- 给定 `mcpToolCallStates` 中该卡为 `pending`
- `shiningMcpCallId` 为空
- 验证 pending 状态渲染存在，但 `shine-border` 不渲染

#### 6. `McpToolCall` / `useMcpToolCallProcessor`：占位卡片收到真实 `call_id` 后不串号

建议文件：

- `src/components/McpToolCall.test.tsx` 或新增 `src/hooks/useMcpToolCallProcessor.test.tsx`

测试内容：

- 先渲染没有 `call_id` 的占位卡片
- 再 rerender 为带真实 `call_id`
- 验证没有错误复用到别的工具调用，且 shine 只跟真实 `call_id` 走

#### 7. `useMcpToolCallProcessor`：重复参数的多工具调用不串号

建议文件：

- 建议新增 `src/hooks/useMcpToolCallProcessor.test.tsx`

测试内容：

- 同一消息中两个 `server/tool/parameters` 完全相同，但 `call_id` 不同
- 验证 key、渲染顺序、shining card 都按 `call_id` 区分

#### 8. 发送按钮运行态：pending MCP 不得误判 running

建议文件：

- 更适合 `ConversationUI.test.tsx`

测试内容：

- 构造 only-pending-MCP 的状态
- 验证发送按钮没有进入“仍在运行”视觉状态

### Rust 建议新增

#### 9. `activity_state.rs`：基本 happy path

建议位置：

- `src-tauri/src/state/activity_state.rs` 内联 tests

测试内容：

- `set_user_pending()` 后 phase = `UserPending`
- `set_assistant_streaming()` 后 phase = `AssistantStreaming`
- `clear_focus()` 后 phase = `Idle`
- 验证 revision 递增、epoch 在新轮次开始时增长

#### 10. `activity_state.rs`：`pending` MCP 不驱动 runtime running

建议位置：

- `src-tauri/src/state/activity_state.rs` 内联 tests

测试内容：

- 只有 pending MCP 时，`build_runtime_state()` / focus 结果应为 `Idle`

虽然逻辑上可由 `recompute_focus()` 间接推出，但当前没有直接断言 runtime。

#### 11. `activity_state.rs`：`clear_message_focus_keep_mcp()` / `restore_after_mcp()` 的同步行为

建议位置：

- 更适合拆出可单测的纯函数后再测

测试内容：

- 从数据库或模拟输入同步 active calls
- 验证 pending 与 executing 排序正确
- 验证最终 focus 选中最新 executing

#### 12. `execution_api.rs`：`handoff_mcp_focus_to_origin_message()`

建议位置：

- `src-tauri/src/mcp/execution_api.rs`

测试内容：

- 有 `assistant_message_id` 时，先切回 assistant message，再 finish MCP
- 只有 `message_id` 时，回退到 `message_id`
- 两者都没有时，不崩溃且不会产生错误 handoff

#### 13. `execution_api.rs`：`continue_with_error()` 命令约束

建议位置：

- `src-tauri/src/mcp/execution_api.rs` 或 `src-tauri/src/api/tests/`

测试内容：

- 非 `failed` 状态调用时返回错误
- `failed` 状态调用时允许 continuation
- continuation 后会触发 handoff 到 origin message

#### 14. `execution_api.rs`：continuation lock busy 分支

建议位置：

- `src-tauri/src/mcp/execution_api.rs`

测试内容：

- 第一个 continuation 持锁
- 第二个 continuation 命中“busy -> queue”
- 第一个结束后 drain queue
- 被排队的 continuation 确实执行

这是当前文档里风险最高但自动化最薄弱的一块。

## 哪些代码需要修改，才能更稳定地补测试

下面这些不是为了修业务 bug，而是为了**把当前逻辑做成可稳定测试**。

### 1. `src/hooks/useConversationEvents.ts`

建议改造点：

- 抽出一个纯函数，例如 `isNewerSnapshot(current, next)` 或统一的版本比较 helper
- 抽出 `shine_state_snapshot` / `runtime_state_snapshot` 的纯 reducer
- 把 MCP completion compensation polling 的调度逻辑拆到独立 helper

原因：

- 当前 stale 判断与状态应用写在 hook 闭包内部，能测，但很依赖完整 hook 驱动。
- `setTimeout` + 轮询 + request id 这套逻辑现在更偏“黑盒”，补 race case 会比较脆。

### 2. `src/components/ConversationUI.tsx`

建议改造点：

- 把 `scrollToMessage()` 对应的“DOM 定位 + 高亮 2 秒”逻辑提取成独立 hook 或 helper
- 允许把时钟 / scheduler 注入测试环境，避免直接绑定真实 `setTimeout`

原因：

- 当前逻辑隐藏在 `useEffect` 里，依赖 `pendingScrollMessageId`、DOM 查询、`requestAnimationFrame` 和 `setTimeout(2000)`。
- 功能本身存在，但不拆的话集成测试会很重、也更脆弱。

### 3. `src/hooks/useMcpToolCallProcessor.tsx`

建议改造点：

- 导出或提取“解析 tool call 注释”和“生成组件 identity/key”的小型纯函数
- 单独暴露 placeholder -> real `call_id` 映射策略

原因：

- 当前关键行为在渲染流程里，能靠组件测试覆盖一部分，但要精确验证“重复参数不串号”仍然偏困难。
- 这个文件里的 `key={`mcp-${data.call_id ?? ...}`}` 是关键约束，建议给它更直接的可测入口。

### 4. `src-tauri/src/state/activity_state.rs`

建议改造点：

- 抽出一个纯状态转移函数，输入旧 `ConversationActivity` 与操作，输出新状态、focus、runtime snapshot、shine snapshot
- 或者至少把 `build_runtime_state()` / `build_shine_state()` 相关断言变得更容易独立验证

原因：

- 当前 `update_state()` 同时做了“状态修改 + revision 递增 + Tauri emit”。
- 纯逻辑测试容易，但“快照内容是否正确发出”不容易直接覆盖。

### 5. `src-tauri/src/mcp/execution_api.rs`

建议改造点：

- 把 `handoff_mcp_focus_to_origin_message()` 所依赖的 state write 操作包成可替换接口，或者至少分离出纯的 handoff plan
- 把 continuation 调度（拿锁、排队、drain）封到更小的可测单元
- 尽量降低对 `AppHandle` / `Window` / `spawn_blocking` / 全局 `OnceLock` 注册表的耦合

原因：

- 现在最关键的 continuation / queue / handoff 路径都是真实运行时对象驱动的。
- 单元测试只能测到局部纯函数，测不到完整编排顺序。

## 优先级建议

### P0：建议最先补

1. 普通无工具对话完整切换
2. pending MCP 不驱动 runtime / 不发光
3. `continue_with_error()` 命令路径
4. continuation lock busy -> queue -> drain
5. `ConversationUI.scrollToMessage()` 2 秒临时高亮

### P1：建议随后补

1. 重复参数多工具调用不串号
2. placeholder 卡片拿到真实 `call_id` 后的稳定绑定
3. 旧快照丢弃 `(epoch, revision)` 回归测试
4. 取消路径测试

## 最终判断

如果标准是“已经有最核心的回归保护”，那当前测试**及格**。

如果标准是“可以自动验证文档中列出的所有重要功能”，那当前测试**还不够**，尤其缺：

- 普通对话 happy path
- pending MCP 对 runtime 的负向保护
- cancel 路径
- `scrollToMessage()` 真正的 UI 高亮流程
- 多工具 / 重复参数 / placeholder -> real call_id 的身份稳定性
- continuation lock busy 与 queue drain 的真实编排验证
