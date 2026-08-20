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

- AIPP spawn 的是 **zed 适配器进程**（`@zed-industries/claude-code-acp` / `codex-acp`，`acp.rs:4177`），而非用户本机的 `claude` / `codex` 二进制。适配器包内部自带 `@anthropic-ai/claude-code` SDK，不依赖用户安装的官方 CLI——用户只装官方 `claude` 而不装适配器时，AIPP 会报错提示 `bun add -g @zed-industries/claude-code-acp`（`acp.rs:4181`）。
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
- 同一 `api_type = 'acp'` 下用 `acp_cli_command`（claude-code-acp / codex-acp / gemini）区分具体 agent；后端进程层（`acp.rs`，约 5700 行）已是 CLI 无关的通用 ACP 客户端。**注意：claude-code-acp / codex-acp 是 zed 适配器包，驱动的是适配器自带 SDK，不是用户本机的官方 CLI**（详见 §1.2.1）。
- 会话模型：`AcpSessionState`（`src-tauri/src/lib.rs:250`）按 `conversation_id` 持有长驻会话；`AcpSessionEntry { handle, snapshot, last_activity, config_signature, run_id }`（`acp.rs:906`）；命令枚举 `AcpSessionCommand::{Start, Prompt, CancelCurrentPrompt, SetConfigOption}`（`acp.rs:826`）。
- 快照事件：`AcpConversationSessionState`（`acp.rs:800`）经 `acp_session_state_snapshot` 推前端；前端监听在 `src/hooks/useConversationEvents.ts:953`，UI 在 `src/components/ConversationUI.tsx:1131-1580`。
- 权限：`AcpPermissionState`（`acp.rs:521`）+ `acp-permission-request` 事件 + `confirm_acp_permission`（`operation_api.rs:164`）；飞书审批回流经 `feishu/api.rs:711`、`feishu/events.rs:296/565/595`。
- 会话持久化表：`acp_session(conversation_id PK, session_id, updated_time)`（`conversation_db.rs:1462`）。
- `llm_db.rs` 多处 `= 'acp'` / `!= 'acp'` 二分（:176-192、:614、:632）是 provider 过滤的主要硬编码点。
- 配置：CLI 命令存 `llm_provider_config`；工作目录/参数/env 按 `assistant_model_config > llm_provider_config > 默认` 合并（`extract_acp_config`，`acp.rs:4794`）。

## 3. 总体设计

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
- 快照事件复用 `acp_session_state_snapshot` 同一事件类型，payload 中增加 `agent_kind: "acp" | "claude_sdk" | "codex_app_server"` 字段（serde 默认值向后兼容），前端同一监听点按 `agent_kind` 渲染差异部分。`AcpConversationSessionState` 相应泛化命名语义（类型名可保留以避免大面积改名，payload 加字段即可）。

### 3.3 会话持久化表

`acp_session` 表加列迁移（`conversation_db.rs:1462`，迁移逻辑在 `db/mod.rs`）：

```sql
ALTER TABLE acp_session ADD COLUMN agent_kind TEXT NOT NULL DEFAULT 'acp';
```

- `session_id` 语义按 `agent_kind` 解释：acp → ACP session_id；claude_sdk → Claude session UUID（`--resume` 用）；codex_app_server → thread_id（`thread/resume` 用）。
- 读取处分流：`load_or_create` 逻辑按 `agent_kind` 走各自通道的 resume 能力。

### 3.4 权限模型

新通道复用现有两套机制，**不新增第三套**：

- Agent 自有权限（工具调用审批）：复用 `AcpPermissionState` 的挂起/决议模式，新通道各持一份实例（命名 `agent_permission` 或各自复制）；事件继续用 `acp-permission-request`（payload 加 `agent_kind`），`confirm_acp_permission` 命令按 `agent_kind` 路由到对应 state。前端 `useOperationPermission.ts:255` 无需改。
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
  - 事件映射：`assistant` content blocks → 现有 `message_update` 流式事件 + MCP tool call UI 事件（tool_use/tool_result 映射成与 ACP 通道一致的前端事件格式，复用渲染组件）
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
- `ConversationUI.tsx`：会话信息 Popover 按 `agent_kind` 渲染 claude 专属的 model/permission-mode 选择器（数据来自快照）
- `src/data/Conversation.tsx`：快照类型加 `agent_kind` 字段

### 4.3 验收标准

- 新建 Claude 助手（type=4 + claude_sdk provider）发消息，流式输出、工具调用展示与 ACP 通道体验一致
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
- 认证：复用现有 `acp_codex_auth_mode` 双模式（`codex_config_toml` 读 `~/.codex/config.toml` 注入 `CODEX_HOME` / env_vars 注入 `OPENAI_*`）。
- model：`thread/start` 参数或 `config` 相关方法指定。

### 5.2 实现清单

- `src-tauri/src/api/ai/codex_app_server.rs`（新增）：结构同 WS1
  - ndjson JSON-RPC 帧编解码（分包重组、id 配对、超时）——参照 Paseo `providers/jsonl-rpc-process.ts`
  - `CodexSessionState`（thread_id 存 `acp_session.session_id`，`agent_kind='codex_app_server'`）
  - 事件映射：`item/*` 通知 → `message_update` 流式事件；approval request → agent 权限事件
  - `thread/resume` 恢复历史
- 分发/注册/迁移与 WS1 共用（`ai_api.rs`、`lib.rs`、`assistant_api.rs`）
- 前端：apiTypes 加 `codex_app_server`；表单复用 codex 认证分支；环境检测加 codex CLI 检测
- `src-tauri/src/artifacts/env_installer.rs`：官方 `codex` CLI 检测与安装引导（官方安装方式，如 `npm i -g @openai/codex` / Homebrew），不再引导安装 zed 适配器

### 5.3 验收标准

- 同 WS1 的六条，通道换成 Codex；重点验证审批（exec/patch）双向与 thread resume

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
- **zed 适配器选项保留为 legacy**：`claude-code-acp` / `codex-acp` 在 acp provider 下继续可选（存量用户配置不破坏），UI 标注"推荐改用原生通道"（见 §9.6）。
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

验证命令遵循仓库约定：`cargo test --manifest-path src-tauri/Cargo.toml <精确范围>`、`npm run build`、`cargo check --manifest-path src-tauri/Cargo.toml`。

## 8. 里程碑与排期建议

| 里程碑 | 内容 | 产出 |
|---|---|---|
| M1 | WS3 体系性改动 + WS1 Claude 通道 MVP（发消息/流式/权限/resume）+ **安装引导改为官方 claude CLI** | Claude Code 原生通道可用 |
| M2 | WS2 Codex app-server 通道 + **安装引导改为官方 codex CLI** | Codex 原生通道可用 |
| M3 | 打磨：ACP 长尾扩展（§6.1，含 gemini 启动参数修复）、快照 UI 差异化、用量统计完整性、文档同步（`AGENTS.md` ACP 章节改为"External Agent Channels"、`docs/product/` 对应功能页） | 多 agent 体验一致 |
| M4（可选） | transcript 镜像：监听 `~/.claude/projects/*.jsonl`，围观/接管本地 TUI 会话（Happy 本地模式） | 本地/远程无缝切换的基础 |

每个里程碑独立可交付、可回滚（api_type 白名单机制保证旧行为不受影响）。

## 9. 风险与开放问题

1. **协议稳定性**：stream-json 与 app-server 都是厂商内部协议的暴露面，版本漂移风险存在。缓解：协议解析层集中、容错解析（未知字段忽略）、fixture 测试锁定行为；关注 `@anthropic-ai/claude-agent-sdk` 与 codex-rs 的 changelog。
2. **认证依赖**：stream-json 依赖本机 claude 已登录（或 env 注入 API key）；需要在环境检测里明确给出"未登录"的可操作提示。
3. **权限双轨**：Claude 通道的 canUseTool 是工具级审批，与 ACP 的 fs/terminal 桥接到内置 operation 工具的语义不同——UI 文案需区分，避免用户困惑。
4. **`confirm_acp_permission` 命名**：若评审决定改为通用 `confirm_agent_permission`，需要旧命令别名兼容（前端、飞书回流、Butler MCP 内调用点 `mcp/builtin_mcp/mod.rs:2121`）。
5. **开放问题**：快照事件是否改名（`acp_session_state_snapshot` → `agent_session_state_snapshot` 并保留旧事件一个版本周期）？`AcpConversationSessionState` 类型是否随 `agent_kind` 拆判别联合？建议 M1 评审时定。
6. **zed 适配器的去留**：`claude-code-acp` / `codex-acp` 保留为 acp provider 下的 legacy 选项，不下线、不迁移存量用户配置；UI 标注"推荐改用原生通道"。长期是否移除，待原生通道覆盖度验证后再评估。

## 10. 新增/修改文件清单（汇总）

**后端新增**：`api/ai/agent_runtime.rs`、`api/ai/claude_sdk.rs`、`api/ai/codex_app_server.rs`、`api/ai/tests/claude_sdk_tests.rs`、`api/ai/tests/codex_app_server_tests.rs`（协议 fixture）

**后端修改**：`api/ai_api.rs`（ask_ai/cancel_ai 分发）、`api/assistant_api.rs`（provider 解析、ensure_session 泛化）、`lib.rs`（state/命令注册）、`db/conversation_db.rs` + `db/mod.rs`（agent_kind 列迁移）、`db/llm_db.rs`（api_type 集合过滤）、`api/operation_api.rs`（权限命令路由）、`feishu/api.rs` + `feishu/events.rs`（审批文案泛化）、`artifacts/env_installer.rs`（官方 claude/codex CLI 检测与安装引导，M1/M2；zed 适配器安装逻辑保留）

**前端修改**：`components/config/LLMProviderConfig.tsx`、`LLMProviderConfigForm.tsx`、`hooks/feature/useAcpEnvironment.ts`（泛化）、`hooks/assistant/useAssistantFormConfig.ts`、`data/Assistant.tsx`、`data/Conversation.tsx`、`components/ConversationUI.tsx`、`hooks/useConversationEvents.ts`

**文档修改**：`AGENTS.md`（ACP Integration Notes → External Agent Channels）、`docs/product/` 对应页、`docs/ai-api-technical-documentation.md`
