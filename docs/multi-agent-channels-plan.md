# 多 Agent 调用通道接入计划（Claude stream-json / Codex app-server / ACP）

> 状态：待评审
> 日期：2026-08-20
> 关联文档：`docs/ai-api-technical-documentation.md`、根目录 `AGENTS.md`（ACP Integration Notes）

## 1. 背景与目标

### 1.1 目标

让 AIPP 能够以**最佳体验**调用主流 CLI agent，按 Happy / Paseo 验证过的行业共识落地：

- **协议优先、pty 兜底**：对头部 agent 使用厂商原生 headless 协议（结构化消息、权限回调、用量统计），长尾 agent 走 ACP。
- 不使用 pty 屏幕抓取作为主路径（AgentAPI / Omnara 早期方案，TUI 脆弱性已被业界验证）。

### 1.2 通道矩阵

| Agent | 通道 | 协议 | 参照实现 |
|---|---|---|---|
| Claude Code | **claude-stream-json**（新增） | spawn `claude` headless，stdio 双向 ndjson（`--output-format stream-json --input-format stream-json`） | Happy `claude/sdk/query.ts`、Paseo `providers/claude/` |
| Codex | **codex-app-server**（新增） | `codex app-server --listen stdio://`，JSON-RPC 2.0 over stdio（换行分隔 JSON） | Happy `codex/codexAppServerClient.ts`、Paseo `providers/codex-app-server-agent.ts` |
| Gemini / opencode / 长尾 | **acp**（现有） | Agent Client Protocol over stdio | AIPP `src-tauri/src/api/ai/acp.rs` |

设计决策依据：

- `claude-code-acp` 的 `load_session=false`，历史无法由 agent 恢复；stream-json 原生支持 `--resume <session_id>`。
- ACP 适配层质量参差，权限、mode、usage 的保真度不如原生协议。Happy/Paseo 均在 2026 年从"统一走 ACP"迁移为"头部两家走原生协议 + ACP 覆盖长尾"。
- `codex exec --json` 是一次性调用、无审批回调，不满足交互需求，必须用 app-server。

### 1.2.1 现状的关键偏差：当前并未直接驱动用户本机 CLI

经源码核实（2026-08-20），当前 ACP 通道的实际进程模型是：

- ACP 通道启动配置的 CLI 由提供商配置决定；原生 Codex 使用独立的 app-server 通道。
- **认证配置确实复用本机**：默认读 `~/.claude/settings.json`（`acp.rs:439`）与 `~/.codex/config.toml`（`acp.rs:395`），但这只是配置层面的复用。
- 全仓库无 `stream-json` / `app-server` / `--output-format` 的任何实现，官方直连协议路径完全不存在。

因此本计划的新通道带来一个额外收益：**用户只需安装官方 CLI（多数目标用户已装），不再需要额外的 zed 适配器包**；真正直接驱动用户电脑里的 `claude` / `codex` 本体，与其本机登录态、配置、插件生态天然一致。同时注意：新通道的认证读取逻辑可直接复用现有 `load_claude_settings_env_vars` / `load_codex_config_env_vars`。

### 1.3 非目标（本期不做）

- pty 真终端接管（VibeTunnel 路线）——独立能力，后续单独立项。
- transcript 镜像（监听 `~/.claude/projects/*.jsonl` 围观本地 TUI 会话）——列为可选增强（见 §9）。
- 远程控制/手机端——本计划只做桌面端调用通道，远程入口复用既有外部渠道架构另行设计。

## 2. 现状摘要（集成点）

现有 ACP 通道的关键事实（详细调查结论见本节，行号以调查时为准）：

- **通道身份由 `assistant_type == 4` 决定**，不是 api_type：`ask_ai` 在 `src-tauri/src/api/ai_api.rs:1244` 按 `assistant_type == Some(4)` 分发到 ACP 分支（:1246-1411），`cancel_ai` 在 :2719-2736 有平行分发。
- 同一 `api_type = 'acp'` 下用 `acp_cli_command` 区分具体 agent；后端进程层（`acp.rs`）是 CLI 无关的通用 ACP 客户端。
- 会话模型：`AcpSessionState`（`src-tauri/src/lib.rs:250`）按 `conversation_id` 持有长驻会话；`AcpSessionEntry { handle, snapshot, last_activity, config_signature, run_id }`（`acp.rs:906`）；命令枚举 `AcpSessionCommand::{Start, Prompt, CancelCurrentPrompt, SetConfigOption}`（`acp.rs:826`）。
- 快照事件：`AcpConversationSessionState`（`acp.rs:800`）经 `acp_session_state_snapshot` 推前端；前端监听在 `src/hooks/useConversationEvents.ts:953`，UI 在 `src/components/ConversationUI.tsx:1131-1580`。
- 权限：`AcpPermissionState`（`acp.rs:521`）+ `acp-permission-request` 事件 + `confirm_acp_permission`（`operation_api.rs:164`）；飞书审批回流经 `feishu/api.rs:711`、`feishu/events.rs:296/565/595`。
- 会话持久化表：`acp_session(conversation_id PK, session_id, updated_time)`（`conversation_db.rs:1462`）。
- `llm_db.rs` 多处 `= 'acp'` / `!= 'acp'` 二分（:176-192、:614、:632）是 provider 过滤的主要硬编码点。
- 配置：CLI 命令存 `llm_provider_config`；工作目录/参数/env 按 `assistant_model_config > llm_provider_config > 默认` 合并（`extract_acp_config`，`acp.rs:4794`）。

## 3. 总体设计

### Codex 原生通道当前实现状态（2026-08-20）

已落地的 Codex app-server 基础链路：

- `codex app-server --listen stdio://` 启动、JSON-RPC initialize、thread/start/thread/resume、turn/start、turn/interrupt。
- Windows 下 `codex` 的 `.cmd`/`.ps1` shim 通过 `cmd.exe`/`pwsh.exe` 启动，避免 Rust 直接执行 PowerShell shim 失败。
- `item/agentMessage/delta` 文本流、reasoning delta、command/file-change/MCP activity 的字符串 item ID、sequence 聚合和 UI 展示。
- Codex approval server request（command、file change、permissions）进入现有权限队列，通过 `confirm_codex_permission` 回传协议要求的 JSON-RPC result。
- `acp_session` 按 `(conversation_id, agent_kind)` 隔离，Codex thread 不再覆盖 ACP session；响应消息 metadata 保存 activity，重新加载会话后恢复展示。

仍未达到最终验收的项目：

- 已在本机 Codex CLI `0.148.0` 外部进程完成 initialize/thread/start 烟雾验证；三类 approval、thread resume、interrupt 和完整乱序通知链路仍需要专用 fixture/端到端测试，当前已有协议映射单元测试。
- activity 目前以响应消息为锚点插入，尚未实现独立 activity 表和完整跨消息历史索引；M3 的多目标 shine/父子层级仍未做。
- Vite 构建受环境中的 Tailwind oxide native binding 与 `spawn EPERM` 阻塞；TypeScript 检查已通过。

### 3.0 UI 适配结论（2026-08-20 评审）

**结论：可以复用当前 UI 展示大部分活动，但不能把原生事件直接塞进现有消息/MCP 数据结构。** 现有 UI 已能承载文本、推理、工具命令的参数/结果/状态；需要增加的是协议到展示模型的适配层，以及补丁的专用展示。当前实现的边界如下：

- `Message` / `MessageUpdateEvent` 以一个 `message_id` 承载一种 `message_type`（`response` / `reasoning` / `error`），没有 agent/channel/item 身份、父子关系、顺序号或并行流标识；`useConversationEvents` 会按 `message_id` 覆盖流式状态。
- 工具展示契约是 `MCPToolCallUpdateEvent` + `MCPToolCall`，已有工具名、参数、pending/executing/success/failed、结果和错误展示；普通 Claude tool、Codex exec、Codex mcp 可以通过适配器映射成同样的卡片。限制是现有模型要求数值 `call_id`、`server_id`，因此原生字符串 item_id 不能直接落入 MCP 表。
- 补丁（Codex patch 或 Claude 文件变更）当前没有专用 diff 视图；MVP 可展示为可折叠的 unified diff/文件摘要文本，后续再增加逐文件 diff 和应用/回滚按钮。
- 审批不是本期必需的主 UI；若目标 CLI 版本发出 approval request，复用现有权限弹窗队列即可，新增 `agent_kind` 与原生 request/item ID 关联。没有审批事件时不显示审批 UI。
- 运行态/闪亮边框目前只有一个 `primary_target`，而一个 turn 可能交错产生多个活动 item。这里的“并行 item”是同一轮请求中同时处于 pending/executing 的多个工具/命令（不代表子代理）；MVP 可按 sequence 顺序展示，M3 再支持多个活动目标。
- 前端快照类型是 `AcpConversationSessionState`，只覆盖 ACP 的 mode/config/plan/usage；原生通道还需要 provider、thread/session、turn、capabilities、活动 item 和 approval 状态。
- `acp_session` 只保存一个 session/thread 标识；要恢复原生会话中的 turn/item/channel 关系，必须至少保存 agent kind，并为活动项使用独立的字符串 ID/事件序列。

因此本计划的 M1/M2 必须先增加一个**通用 Agent UI 契约**，再实现协议适配器。最低范围如下（不要求第一版做完整 TUI）：

1. **事件信封**：所有原生事件统一为 `agent_activity`，包含 `conversation_id`、`agent_kind`、`session_id`、`item_id`（字符串）、单调 `sequence`、`activity_type`、`status`、`content_delta`、`metadata`；`channel_id`/`parent_item_id` 作为预留字段，不因不存在子代理而强制实现。
2. **通用活动模型**：新增 Agent activity 类型；MCP/普通 tool/command 共用 `kind=tool|command`，patch 使用 `kind=patch`，审批使用 `kind=approval`（可选）。工具 ID 保留字符串，不能写入现有数值 MCP 表。
3. **展示适配层**：在 `useConversationEvents` 与 `useMessageListElements` 之间增加 selector。text/reasoning 继续映射现有消息气泡；tool/command 映射现有 `McpToolCallRenderer` 的统一卡片输入；patch 映射可折叠 diff 文本块；同一 item 的 delta 原地合并。
4. **运行态**：MVP 只维护 conversation 级运行态和一个 primary shine，多个活动按 sequence 列表展示；M3 再扩展活动集合和多目标 shine。
5. **会话快照联合类型**：保留 `acp_session_state_snapshot` 兼容旧 ACP，同时增加 `agent_kind`、provider、session/thread、current_turn、channels、active_items、approval 状态、usage/cost/capabilities；`ConversationUI` 根据能力渲染，不把 Claude/Codex 强行套进 ACP mode/config 控件。
6. **权限 UI（可选增强）**：若协议发出 approval，统一 Agent approval 事件/命令，payload 携带字符串 `item_id`、工具类型、命令摘要和可选项；复用现有权限弹窗。没有 approval 事件时不增加额外交互。

### 3.0.1 分阶段兼容策略

- **M1/M2 MVP**：text/reasoning 走现有消息；tool/command 复用现有工具卡片视觉；patch 用只读折叠块；approval 按协议实际需要复用现有弹窗。必须保留字符串 item ID 和 sequence，不能把外部 agent 工具写成可由 AIPP 再次执行的 MCP call。
- **M3 UI 完整化**：实现多活动 shine、快照能力驱动控件和历史活动恢复；只有协议确实提供 channel/sub-agent 时才增加层级 UI。
- 完成 M0 的事件适配和展示 selector 后即可开始协议端到端实现；不必等待补丁 diff、审批或多活动 shine 的完整 UI。

### 3.0.2 UI 适配实施计划

#### 第一步：定义最小展示模型（M0-1）

- 后端在 `src-tauri/src/api/ai/events.rs` 增加 `AgentActivityEvent`：`item_id: String`、`sequence: u64`、`kind: tool | command | patch | approval | status`、`status: pending | executing | success | failed | cancelled`、`title`、`input`、`output`、`error`、`metadata`。
- 前端在 `src/data/Conversation.tsx` 增加对应类型；`item_id` 从协议入口到前端始终保持字符串，不转换为 hash/数值 ID。
- text/reasoning 不进入 activity 模型，继续使用现有 `message_update`，避免改动成熟的消息渲染和流式链路。

#### 第二步：复用工具/命令卡片（M0-2）

- 把 `McpToolCall.tsx` 的纯展示部分抽为 `ToolActivityCard`，props 使用字符串 `activityId`，包含 title/source/input/output/error/status；保留 `McpToolCall` 作为适配包装层，现有 MCP 执行、停止、重试逻辑不变。
- Claude `tool_use` 映射：工具名 → title，input → 参数区，tool_result → 结果区。
- Codex exec 映射：命令/工作目录 → title/input，stdout/stderr/exit code → output/error；UI 直接展示为现有样式的工具卡片。
- Codex MCP item 映射同普通工具；只有 AIPP 自己执行的 MCP call 继续进入 `mcp_tool_call` 表并显示执行/停止按钮，外部 agent 已执行的活动卡片只读，避免 AIPP 重复执行。

#### 第三步：活动聚合与消息列表插入（M0-3）

- `useConversationEvents.ts` 增加 `Map<string, AgentActivity>`，key 使用 `${agent_kind}:${session_id}:${item_id}`；只接受更大的 sequence，同 item delta 合并，多 item 不互相覆盖。
- `useMessageListElements.tsx` 把当前 turn 的 activity 按 sequence 插入 assistant 回复组；历史 activity 从数据库查询后走同一 selector。
- 当前运行中的 tool/command 默认展开，成功后沿用现有卡片自动收起行为；失败保持展开。conversation 仍只保留一个 shine 目标，指向最近活跃 item。

#### 第四步：补丁最小展示（M1/M2）

- 新增 `PatchActivityCard`，复用卡片外壳，显示文件列表、增删行统计与可折叠 unified diff；首版只读，不提供应用/回滚按钮。
- 若协议只给 patch 摘要而无 diff，则显示摘要与受影响文件，不从本地文件状态反推补丁内容。
- 后续逐文件 diff、语法高亮和应用/回滚属于 M3，不阻塞 Claude/Codex 通道上线。

#### 第五步：审批与高级活动（按协议实际需要）

- Claude/Codex 出现 approval request 时，泛化 `useAcpPermission` 为 `useAgentPermission`，继续复用 `OperationPermissionDialog` 的队列和按钮；新增 `agent_kind`、`request_id`、`item_id` 路由字段。
- 同一时间多个审批沿用现有请求队列逐个处理，不需要新的并行审批界面。
- 子代理/channel 层级不属于 M0-M2。只有目标 CLI 确实输出父子 agent/channel 事件时，M3 才使用预留的 `channel_id` / `parent_item_id` 做嵌套或分组展示。

#### 实施验收

- 一轮对话中交错出现两个工具/命令时，两张卡片各自更新，不串参数、结果或状态。
- 命令卡片能看到命令、工作目录、stdout/stderr、exit code 和最终状态；外部 agent 工具没有可重复执行按钮。
- 补丁至少能看到受影响文件和协议提供的 diff/摘要，不把 patch 原文塞进普通回复。
- 无 approval 事件时不出现审批 UI；有事件时现有弹窗能按 request/item ID 正确回传一次决策。
- 普通 Chat、现有 ACP 和 AIPP MCP 卡片的展示与执行行为保持不变。

### 3.0.3 真正的协议硬限制

以下不是 AIPP UI 缺功能，而是上游 CLI/协议不提供时无法凭空实现的能力。实现时必须明确降级，不把它们写成验收承诺：

- **协议没有发送的内容无法还原**：如果某个模型/CLI 不发送完整 reasoning、工具输入、stdout/stderr 或 unified diff，AIPP 只能展示供应商提供的摘要/占位状态，不能从最终文本可靠推导原始过程。
- **供应商未暴露的单项控制无法添加**：AIPP 不能强制实现“只取消某一个外部工具”“只重试某一个外部工具”“回滚某一个 patch”。只能使用 CLI 暴露的 cancel/approval/rollback 方法；没有对应方法时只能取消当前 turn 或再次发送用户请求。
- **不可保证跨版本协议稳定**：Claude stream-json 和 Codex app-server 都可能随 CLI 版本变化。AIPP 可以锁版本、生成 schema、做 fixture 和兼容解析，但不能保证未来任意版本不变。
- **不能保证真实 TUI 接管**：stream-json/app-server 是 headless 通道，不等于本地交互 TUI。要围观或接管另一个已运行的 TUI 会话，需要 transcript/daemon/pty 等额外能力；本计划不承诺仅靠这两条 stdio 协议实现 TUI 镜像。
- **外部 agent 的执行权归 CLI**：AIPP 展示 Claude/Codex 已执行的 command/tool/patch，但不能把它们当作 AIPP MCP 记录后自行再次执行；否则会造成重复执行和错误的权限边界。

截至本机验证版本（Claude Code `2.1.72`、Codex CLI `0.148.0`），Codex app-server schema 已包含 command/file-change approval、unified diff、collaboration/sub-agent 相关类型，因此这些功能属于“需要实现”，不是协议上完全不可能。Claude 的 `--agents`、stream-json、resume、permission-mode 入口也已存在；具体事件字段仍需用目标版本 fixture 锁定。

按用户可感知能力划分，绝对边界如下：

| 能力 | 结论 | 无法由 AIPP 单方面补齐的部分 |
|---|---|---|
| 工具/命令过程展示 | 可实现 | CLI 未发送的完整参数、stdout/stderr 或中间状态无法还原 |
| 补丁展示 | 可实现 | CLI 只给文件摘要而不给 diff 时，不能可靠重建“当时生成的原始补丁” |
| 审批 | 可实现，但受协议控制 | 上游没有发起 approval request 的动作，AIPP 不能在不接管执行权的前提下强插一次真实审批 |
| 多个并行 item 展示 | 可实现 | 上游把并行过程压平成一个状态时，AIPP 无法推断真实并发关系和精确时序 |
| 子代理展示 | Codex 可实现；Claude 以实际事件为准 | 未发送 child agent ID、父子关系或独立事件流时，只能显示普通活动，不能可靠重建子代理树 |
| 完整内部思考过程 | 不可承诺 | 模型隐藏的 chain-of-thought 不会因新增 UI、数据库或适配器而变得可获取；只展示协议允许输出的 reasoning summary/delta |
| 单 item 取消/重试/回滚 | 仅在协议提供对应方法时可实现 | 只有 turn 级 cancel 时，AIPP 无法安全制造 item 级控制语义 |
| 任意已运行 TUI 的无损接管 | 不能由本计划通道保证 | stdio headless 协议不能自动接管另一个独立进程；PTY/转录监听只能另做近似方案，也不能保证获得结构化事件和控制权 |
| 对未来所有 CLI 版本永久兼容 | 不可实现 | 只能锁定已验证版本、兼容已知 schema 并在协议变化时持续维护 |

因此，在本计划要求的核心展示范围内，**工具命令、补丁、审批、并行 item，以及 Codex 子代理都不存在“加代码也做不了”的阻塞项**。真正无法保证的是上游未输出的信息、上游未提供的控制操作、隐藏思维链、任意既有 TUI 的无损接管和未来协议永不变化；Claude 子代理层级是否能完整展示要以目标版本实际输出的结构化事件为准。

### 3.1 通道身份：复用 `assistant_type = 4`，按 provider `api_type` 二次分发

决策：**不新增 assistant_type**。`assistant_type = 4` 语义泛化为"Agent 助手"（原"ACP 助手"），具体通道由该助手所选 provider 的 `api_type` 决定：

- `api_type = 'acp'` → 现有 ACP 通道（不变）
- `api_type = 'claude_sdk'` → 新增 Claude stream-json 通道
- `api_type = 'codex_app_server'` → 新增 Codex app-server 通道

`ask_ai` 的 ACP 分支入口（`ai_api.rs:1244`）改为"Agent 分支"，内部先解析 provider 的 `api_type` 再分发到对应通道。`cancel_ai`、`dispatch_queued_message`、regenerate 同理。

理由：

- 前端助手表单、模型选择、Butler 任务派发都按 `assistant_type` 工作，新增 type 值会扩散到所有 `assistant_type === 4` 判断点（前端 `ConversationUI.tsx:1131`、`useAssistantFormConfig.ts:121` 等）。
- `llm_db.rs` 的 `= 'acp'` 二分本来就要改成集合判断（`api_type IN ('acp','claude_sdk','codex_app_server')`），一并处理。
- `resolve_acp_provider_id`（`assistant_api.rs:35`）泛化为 `resolve_agent_provider_id`，逻辑不变（model 的 provider_id 或 model_config 覆盖）。

### 3.2 代码组织：共享运行时 + 协议适配器

三条通道的**会话骨架完全相同**：每会话一个长驻子进程、命令 channel、prompt 队列、快照 emit、权限挂起/决议、用量统计、空闲回收。差异只在协议编解码和事件映射。

按仓库"最小改动"惯例，不抽象泛型 trait 大重构，而是：

```
src-tauri/src/api/ai/
├── acp.rs                  # 现有，不动结构
├── agent_runtime.rs        # 新增：三条通道共享的会话骨架
├── claude_sdk.rs           # 新增：Claude stream-json 通道
├── codex_app_server.rs     # 新增：Codex app-server 通道
└── ...
```

- `agent_runtime.rs` 提取自 `acp.rs` 中协议无关的部分：session task 生命周期（spawn_blocking + 单线程 runtime + LocalSet 模式按需）、prompt 队列、`config_signature` 复用判断、快照 emit helper、用量持久化（`persist_message_usage_in_db`）、done 事件、空闲回收。**acp.rs 保持自治、不回迁**——共享代码只服务新通道，避免动现有 ACP 行为。
- 快照/权限状态：新通道各自定义 `ClaudeSdkSessionState` / `CodexSessionState`（结构对标 `AcpSessionState`），**复制并改名**而非泛型抽象（隔离好、改动可评审；三条通道稳定后再考虑合并）。
- 快照事件短期复用 `acp_session_state_snapshot` 事件名以兼容旧 ACP，payload 改为 `agent_kind` 判别联合；共享字段为 provider/session-or-thread/current-turn/usage/capabilities，ACP、Claude、Codex 的专属字段分别放入通道 payload。不能只在现有 `AcpConversationSessionState` 上加一个 `agent_kind` 后继续复用全部 ACP 字段。

### 3.3 会话持久化表

`acp_session` 表加列迁移（`conversation_db.rs:1462`，迁移逻辑在 `db/mod.rs`）：

```sql
ALTER TABLE acp_session ADD COLUMN agent_kind TEXT NOT NULL DEFAULT 'acp';
```

- `session_id` 语义按 `agent_kind` 解释：acp → ACP session_id；claude_sdk → Claude session UUID（`--resume` 用）；codex_app_server → thread_id（`thread/resume` 用）。
- 读取处分流：`load_or_create` 逻辑按 `agent_kind` 走各自通道的 resume 能力。

### 3.4 权限模型

新通道复用现有两套机制，**不新增第三套**：

- Agent 自有权限（工具调用审批）：复用 `AcpPermissionState` 的挂起/决议模式，新通道各持一份实例（命名 `agent_permission` 或各自复制）；事件继续用 `acp-permission-request`（payload 加 `agent_kind`），`confirm_acp_permission` 命令按 `agent_kind` 路由到对应 state。前端泛化 `useAcpPermission` 的类型与路由字段，弹窗和请求队列复用。
  - 备选：直接改名为通用 `agent-permission-request` 事件 + 旧事件名兼容期。评审时定。
- Claude/Codex 内部的文件/终端操作：stream-json 的 `canUseTool` 回调和 app-server 的 exec/patch 审批都映射到上述 agent 权限事件，**不桥接到内置 operation 工具**（与 ACP 的 fs/terminal 桥接不同——原生协议里这些是 agent 自己的能力，审批语义在 agent 权限层）。
- 飞书审批回流：`feishu/api.rs:711`（`try_deliver_acp_permission_to_feishu`）泛化为按 `agent_kind` 渲染卡片文案，回流入口（`feishu/events.rs:296/565/595`）不变。
- Butler 任务板联动：`emit_butler_task_permission_state_changed` 的 `"acp"` 参数按通道传值。

## 4. 工作流 WS1：Claude stream-json 通道

### 4.1 协议要点（参照 Happy/Paseo 源码核实）

- 启动：`claude --output-format stream-json --input-format stream-json --verbose --print`（headless 双向模式），`stdio` 三管道，Windows 注意绕过 cmd.exe 直接 spawn。
- 输出 ndjson 行：`system/init`（含 `session_id`、model、tools 列表）、`assistant`（message content blocks 流式）、`user`（tool_result 回显）、`result`（终态，含 usage、cost、duration）。
- 输入 ndjson 行：`{"type":"user","message":{"role":"user","content":[...]},"session_id":...}`。
- 权限：控制协议 `control_request` / `control_response`（`canUseTool` 回调的 wire 形态）：CLI 发出 `{"type":"control_request","request":{"subtype":"can_use_tool","tool_name":...,"input":...,"permission_suggestions":[...]}}`，客户端回 `control_response` allow/deny。
- 中断：`control_request` 的 `interrupt` subtype（等价 `session/cancel`）。
- Resume：进程重启后用 `--resume <session_id>` + 首次 prompt 携带历史即可恢复（session transcript 在 `~/.claude/projects/`）。
- mode/model：启动参数 `--permission-mode`（default/acceptEdits/plan/bypassPermissions）、`--model`；运行期切换走 control protocol 或重启会话。
- MCP 注入：AIPP 现有 `--acp-mcp-bridge` 机制（`acp.rs:85`）的对应物是 `--mcp-config <json>` + `--allowed-tools`，把助手选中的 MCP server 透传给 claude 进程。

### 4.2 实现清单

后端：

- `src-tauri/src/api/ai/claude_sdk.rs`（新增，对标 acp.rs 结构）：
  - `ClaudeSdkSessionState` / `ClaudeSdkSessionEntry` / `ClaudeSdkSessionHandle`（命令枚举：`Start, Prompt, Interrupt, SetModel, SetPermissionMode`）
  - `spawn_claude_sdk_session_task`：复用 `agent_runtime.rs` 骨架；stdout 按行读 ndjson 解析
  - 事件映射：`assistant` text/thinking blocks → 现有 `message_update`；`tool_use` / `tool_result` / permission / status → §3.0 的 `agent_activity`。仅在事件确实来自 MCP server 时标记 `tool_kind=mcp`，普通 Claude 工具不能伪装成 MCP call
  - 权限：`control_request(can_use_tool)` → 挂起 → `acp-permission-request` 事件（`agent_kind=claude_sdk`）→ `confirm_acp_permission` 决议 → `control_response`
  - usage/cost：`result` 消息 → `persist_message_usage_in_db` 等价逻辑
  - `config_signature`：由 cli 路径、工作目录、env、model、permission-mode 组成
- `src-tauri/src/api/ai/agent_runtime.rs`（新增）：共享骨架（见 §3.2）
- `src-tauri/src/api/ai_api.rs`：ask_ai Agent 分支内按 api_type 分发；`cancel_ai` 加 claude_sdk handle 分支
- `src-tauri/src/api/assistant_api.rs`：`resolve_agent_provider_id` 泛化；`ensure_acp_session_connected`（:728）泛化或新增 `ensure_agent_session_connected`（内部按 api_type 分发）
- `src-tauri/src/lib.rs`：注册新 state 与命令
- `src-tauri/src/db/conversation_db.rs` + `db/mod.rs`：`acp_session` 表加 `agent_kind` 列迁移
- `src-tauri/src/artifacts/env_installer.rs`：**安装引导改为官方 CLI**——新增官方 `claude` CLI 检测（PATH 查找、版本、登录态探测）；`claude_sdk` provider 的安装引导文案/流程指向官方安装方式（`npm i -g @anthropic-ai/claude-code` 或 native installer），不再引导安装 zed 适配器。现有 `check_acp_library` / `install_acp_library`（zed 适配器的 bun 安装）保留给 ACP 通道使用，不动。

前端：

- `src/components/config/LLMProviderConfig.tsx:83`：apiTypes 下拉加 `claude_sdk`（标签"Claude Code (原生)"）
- `LLMProviderConfigForm.tsx`：`claude_sdk` 分支表单——CLI 路径（默认 `claude`）、认证模式（复用现有 `acp_claude_auth_mode` 双模式：`claude_settings` 读 `~/.claude/settings.json` / env_vars 注入 `ANTHROPIC_*`）、默认 model、默认 permission-mode
- `useAcpEnvironment.ts` 泛化为 `useAgentEnvironment`（或加 claude 分支）：检测 `claude` CLI 是否安装、版本、登录态（`claude auth status` 或探测命令）
- `ConversationUI.tsx`：会话信息 Popover 按 `agent_kind` 和 capabilities 渲染 Claude 专属的 model/permission-mode 选择器（数据来自快照）
- `src/data/Conversation.tsx`：增加 `AgentSessionState` 判别联合、`AgentActivityEvent`、字符串 `item_id`/`channel_id`/`parent_item_id` 与 `sequence`；旧 `AcpConversationSessionState` 保留为 ACP 分支
- `useConversationEvents.ts`：监听 `agent_activity`，按 `(agent_kind, session_id, channel_id, item_id)` 合并 delta，并用 `sequence` 丢弃乱序/重复更新
- `components/conversation/`：抽取 `ToolActivityCard` 复用现有 `McpToolCall` 视觉，tool/command 使用该卡片；新增只读 `PatchActivityCard`；approval 继续用现有权限弹窗。子代理卡片不在 M1 范围

### 4.3 验收标准

- 新建 Claude 助手（type=4 + claude_sdk provider）发消息，文本/推理流式输出正常；工具、命令、权限、状态以 Agent Activity 卡片展示且活动不丢失、不串 item
- 权限弹窗在前端与飞书均可批准/拒绝
- 应用重启后同一会话可 `--resume` 恢复历史（agent 侧真实恢复，不依赖历史拼接回退）
- token 用量与 cost 进入统计（`ConversationStatsDialog` 可见）
- 取消（interrupt）只中断当前 prompt，进程存活

## 5. 工作流 WS2：Codex app-server 通道

### 5.1 协议要点（参照 Happy/Paseo 源码核实）

- 启动：`codex app-server --listen stdio://`，换行分隔的 JSON-RPC 2.0 over stdio。
- 生命周期：`initialize` → `thread/start`（或 `thread/resume` 带 thread_id）→ `turn/started`、`item/*` 增量通知、`turn/completed`；`thread/read` 取历史；另有 `thread/fork`、`thread/rollback`、`thread/inject_items`、`thread/goal/set`。
- 审批：exec/patch/mcp 审批请求以 JSON-RPC server→client request 形式到达（这是选 app-server 而非 `codex exec` 的原因），客户端回 allow/deny。
- 用量：`thread/tokenUsage/updated` 通知。
- 认证：原生 Codex app-server 使用其独立的配置与认证设置。
- model：`thread/start` 参数或 `config` 相关方法指定。

### 5.2 实现清单

- `src-tauri/src/api/ai/codex_app_server.rs`（新增）：结构同 WS1
  - ndjson JSON-RPC 帧编解码（分包重组、id 配对、超时）——参照 Paseo `providers/jsonl-rpc-process.ts`
  - `CodexSessionState`（thread_id 存 `acp_session.session_id`，`agent_kind='codex_app_server'`）
  - 事件映射：Codex message/reasoning delta → `message_update`；exec/mcp → 可复用工具卡片的 `agent_activity`；patch → `PatchActivityCard`；plan/status → 简单 activity；approval request 关联原字符串 item/request ID。sub-agent 事件仅在目标版本实际提供时保留元数据，M2 不做层级 UI
  - `thread/resume` 恢复历史
- 分发/注册/迁移与 WS1 共用（`ai_api.rs`、`lib.rs`、`assistant_api.rs`）
- 前端：apiTypes 加 `codex_app_server`；表单复用 codex 认证分支；环境检测加 codex CLI 检测
- `src-tauri/src/artifacts/env_installer.rs`：官方 `codex` CLI 检测与安装引导（官方安装方式，如 `npm i -g @openai/codex` / Homebrew），不再引导安装 zed 适配器

### 5.3 验收标准

- 同 WS1 的六条，通道换成 Codex；重点验证 exec/patch/mcp 等不同 item 卡片、字符串 ID、乱序/重复通知、审批双向与 thread resume

## 6. 工作流 WS3：体系性改动（provider 过滤、配置、UI 泛化）

- `src-tauri/src/db/llm_db.rs:176-192/614/632`：`= 'acp'` / `!= 'acp'` 二分改为集合判断：
  - "agent 侧"：`api_type IN ('acp','claude_sdk','codex_app_server')`（assistant_type=4 可见）
  - "普通侧"：`api_type NOT IN (...)` 或保持 `!=` 逐一枚举；建议引入常量 `AGENT_API_TYPES: &[&str]` 集中定义，杜绝散落字符串
- `src-tauri/src/api/genai_client.rs:90`：新 api_type 不会走 genai 链路，无需加适配器；确认未知类型回退 OpenAI 的行为不会误伤（agent api_type 不会到达这里）
- 前端类型：`src/data/Assistant.tsx:11` 助手类型注释更新（type=4 语义泛化为 Agent 助手）；`llmModelTypes.ts:39` 如需 request_mode 支持
- `useAssistantFormConfig.ts:121-183`：type=4 表单字段按所选 provider 的 api_type 动态出字段（acp provider 出现有字段；claude_sdk/codex_app_server provider 出各自字段，工作目录/env 合并逻辑共用）
- `ModelSelectionDialog` / 模型列表：agent provider 下模型列表语义是"该 CLI 的模型标识"（如 `claude-sonnet-4`），沿用现有模型表即可
- Butler：task 派发走 `ask_ai`，天然支持新通道；验证 Butler 任务会话用 claude_sdk/codex provider 的端到端链路

### 6.1 ACP 通道保留与长尾扩展

**ACP 通道（`api_type = 'acp'`）完整保留、行为不变**，继续作为 Gemini CLI 与长尾 agent 的统一入口。在此基础上做增量扩展：

- **修复 gemini 现状缺陷**：当前后端对 `gemini` 无启动参数特判，`--experimental-acp` 需用户手填 `acp_additional_args` 才能工作。改为后端按 CLI 自动带默认启动参数（用户配置可覆盖）。
- **引入 per-agent 启动预设表**：参照 Paseo `packages/app/src/data/acp-provider-catalog.ts`（约 40 家：`gemini --experimental-acp`、`opencode acp`、`kimi`/`kiro`/`trae`/`copilot`/`cursor` 等各家 `acp` 子命令或 `--acp` 参数），把 `acpCliOptions`（`LLMProviderConfigForm.tsx:109`）从 3 项硬编码扩展为预设目录：每项含 CLI 命令、默认启动参数、认证提示文案、环境检测方式。后端 `acp.rs` 已有"任意 ACP CLI 字符串可用"的通用路径（`acp.rs:4829`），扩展主要是预设表 + 各家认证/env 特判的补齐。
- ACP 提供商配置仅展示仍受支持的 ACP CLI；Codex 使用原生通道。
- 扩展节奏建议放 M3，每个新 agent 的验收标准 = 能通过预设一键配好并完成一轮对话。

## 7. 测试计划

后端（内存 SQLite，参照 `src-tauri/src/api/tests/`、`db/tests/` 现有组织）：

- 协议解析单测：ndjson/JSON-RPC 帧解析、事件映射、权限请求/决议 round-trip——用录制的 fixture（Happy/Paseo 的真实输出样本 + 手工构造边界 case），不依赖真实 CLI
- 会话骨架单测：prompt 队列、config_signature 复用判断、空闲回收、cancel 语义
- DB 迁移测试：`acp_session` 加列后旧数据 `agent_kind='acp'` 默认值正确
- Mock CLI 集成测试：写一个 `src-tauri/src/bin/` 下或测试专用 fixture 脚本模拟 stream-json/app-server 协议对端，跑通 ask_ai → 流式 → 权限 → done 全链路
- 回归：现有 ACP 通道测试全绿（acp.rs 结构不动，只改 `ai_api.rs` 分发和 `llm_db.rs` 过滤，重点回归这两处）

前端：

- `LLMProviderConfigForm` 新 apiType 表单分支渲染测试
- 快照事件 `agent_kind` 兼容性测试（旧 payload 无该字段时默认 acp）
- `agent_activity` 聚合测试：字符串 item ID、同 item delta、两个交错活动、乱序/重复 sequence；父子 item/channel 留到 M3
- 展示测试：tool/command 复用卡片的 pending/executing/success/failed/cancelled，patch 的文件摘要/diff 折叠块；sub-agent 展示留到 M3
- 权限 UI 测试：Claude permission suggestion 与 Codex exec/patch approval 均能批准/拒绝，并且不会串到另一个活动 item
- 消息列表回归：普通对话与 ACP 的 `message_update` / 数值 MCP call 展示保持不变

验证命令遵循仓库约定：`cargo test --manifest-path src-tauri/Cargo.toml <精确范围>`、`npm run build`、`cargo check --manifest-path src-tauri/Cargo.toml`。

## 8. 里程碑与排期建议

| 里程碑 | 内容 | 产出 |
|---|---|---|
| M0 | 最小 Agent UI 适配：事件信封、字符串 item ID、sequence、活动聚合、抽取可复用工具/命令卡片 | Claude/Codex 工具命令可接入当前 UI |
| M1 | WS3 体系性改动 + WS1 Claude 通道 MVP（发消息/流式/Activity/权限/resume）+ **安装引导改为官方 claude CLI** | Claude Code 原生通道可用 |
| M2 | WS2 Codex app-server 通道（含 exec/patch/mcp item）+ **安装引导改为官方 codex CLI** | Codex 原生通道可用 |
| M3 | 打磨：ACP 长尾扩展（§6.1，含 gemini 启动参数修复）、多 channel/item UI、并行 shine、历史 Activity 恢复、用量统计完整性、文档同步（`AGENTS.md` ACP 章节改为"External Agent Channels"、`docs/product/` 对应功能页） | 多 agent 体验一致 |
| M4（可选） | transcript 镜像：监听 `~/.claude/projects/*.jsonl`，围观/接管本地 TUI 会话（Happy 本地模式） | 本地/远程无缝切换的基础 |

每个里程碑独立可交付、可回滚（api_type 白名单机制保证旧行为不受影响）。

## 9. 风险与开放问题

1. **协议稳定性**：stream-json 与 app-server 都是厂商内部协议的暴露面，版本漂移风险存在。缓解：协议解析层集中、容错解析（未知字段忽略）、fixture 测试锁定行为；关注 `@anthropic-ai/claude-agent-sdk` 与 codex-rs 的 changelog。
2. **认证依赖**：stream-json 依赖本机 claude 已登录（或 env 注入 API key）；需要在环境检测里明确给出"未登录"的可操作提示。
3. **权限双轨**：Claude 通道的 canUseTool 是工具级审批，与 ACP 的 fs/terminal 桥接到内置 operation 工具的语义不同——UI 文案需区分，避免用户困惑。
4. **`confirm_acp_permission` 命名**：若评审决定改为通用 `confirm_agent_permission`，需要旧命令别名兼容（前端、飞书回流、Butler MCP 内调用点 `mcp/builtin_mcp/mod.rs:2121`）。
5. **开放问题**：快照事件是否改名（`acp_session_state_snapshot` → `agent_session_state_snapshot` 并保留旧事件一个版本周期）？`AcpConversationSessionState` 类型是否随 `agent_kind` 拆判别联合？建议 M1 评审时定。
6. **通道边界**：Codex 走原生 app-server，ACP 保持通用 ACP agent 支持。
7. **原生活动与 MCP 的边界**：当前 MCP 卡片视觉可以复用，但 MCP 表/执行 API 不能作为外部 agent item 的通用存储，否则会暴露错误的执行/停止/重试行为。M0 使用独立 Agent Activity 持久化；`McpToolCall` 与 Agent Activity 都适配到共享的只展示 `ToolActivityCard`。
8. **协议版本核验**：Codex app-server 的方法名、通知名、审批 request schema 需在实现时以目标 CLI 版本导出的 schema/官方文档为准，并记录 `codex --version`；计划中的方法名是设计基线，不能作为无版本约束的稳定 API。Claude stream-json/control schema 同样需要 fixture 锁定版本。

## 10. 新增/修改文件清单（汇总）

**后端新增**：`api/ai/agent_runtime.rs`、`api/ai/agent_activity.rs`（通用事件/持久化映射）、`api/ai/claude_sdk.rs`、`api/ai/codex_app_server.rs`、`api/ai/tests/claude_sdk_tests.rs`、`api/ai/tests/codex_app_server_tests.rs`（协议 fixture）

**后端修改**：`api/ai/events.rs`（Agent Activity 信封）、`api/ai_api.rs`（ask_ai/cancel_ai 分发）、`api/assistant_api.rs`（provider 解析、ensure_session 泛化）、`lib.rs`（state/命令注册）、`db/conversation_db.rs` + `db/mod.rs`（agent_kind 与 Agent Activity 持久化迁移）、`db/llm_db.rs`（api_type 集合过滤）、`api/operation_api.rs`（权限命令路由）、`feishu/api.rs` + `feishu/events.rs`（审批文案泛化）、`artifacts/env_installer.rs`（官方 claude/codex CLI 检测与安装引导，M1/M2；zed 适配器安装逻辑保留）

**前端修改**：`components/config/LLMProviderConfig.tsx`、`LLMProviderConfigForm.tsx`、`hooks/feature/useAcpEnvironment.ts`（泛化）、`hooks/assistant/useAssistantFormConfig.ts`、`data/Assistant.tsx`、`data/Conversation.tsx`、`components/ConversationUI.tsx`、`hooks/useConversationEvents.ts`、`components/conversation/useMessageListElements.tsx`、Agent Activity 卡片及其测试

**文档修改**：`AGENTS.md`（ACP Integration Notes → External Agent Channels）、`docs/product/` 对应页、`docs/ai-api-technical-documentation.md`
