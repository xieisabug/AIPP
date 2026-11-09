# 架构总览

本文档承载 AIPP 的详细架构信息，从根目录 `AGENTS.md` 提取。日常开发规范见 `AGENTS.md`，各功能的用户向文档见 `docs/product/`。

## Core Technologies

-   **Backend**: Rust with Tauri 2.0 framework, SQLite via rusqlite
-   **Frontend**: React 19 with TypeScript, Vite build system
-   **UI Framework**: shadcn/ui components, Radix UI primitives, Tailwind CSS v4
-   **AI Integration**: Custom forked genai client with streaming support, built from `src-tauri/vendor/genai`; upstream revision and local protocol patches are recorded in its `AIPP-PATCHES.md`.
-   **MCP Protocol**: rmcp crate for Model Context Protocol integration
-   **State Management**: React hooks for frontend, Arc<TokioMutex<>> for Rust backend
-   **Content Execution**: Support for HTML, SVG, React, Vue, Python, Bash/PowerShell, AppleScript
-   **Platform Features**: System tray, global shortcuts (Ctrl+Shift+I/O), multi-window architecture
-   **Testing**: Comprehensive test suite with integration tests for AI functionality

## Window-Based Architecture

The application uses multiple Tauri windows for different features:

-   **Ask Window**: Quick AI query interface
-   **Config Window**: Settings and configuration
-   **ChatUI Window**: Main chat interface
-   **Schedule Window**: Scheduled task management and run logs
-   **ButlerExperiment Window**: Butler main conversation, contacts and group chats, task board, approvals, Feishu status
-   **Sidebar Window**: Conversation side panel for todo/context/artifact preview
-   **ArtifactPreview Window**: Content preview (HTML, SVG, components)
-   **Artifact Window**: Standalone artifact rendering window
-   **ArtifactCollections Window**: Manage artifact collections
-   **Plugin Window**: Plugin UI host window
-   **Native Chrome verification** (desktop only): Normal searches remain headless. Only a detected challenge starts a separate headed Chrome instance using the persistent manual profile and original request URL. HTML is read from the verified headed target; cancellation/success/error shuts that instance down. A profile lease serializes manual sessions and is held until process shutdown. Human wait pauses the active-search budget; no screenshot relay or remote-site Tauri IPC is used.

## Frontend Structure

```
src/
├── components/
│   ├── ui/          # shadcn/ui primitives
│   ├── common/      # Shared components (ConfigPageLayout, EmptyState, etc.)
│   ├── config/      # Configuration-related components
│   │   ├── assistant/     # Assistant form rendering
│   │   └── feature/       # Feature-specific forms
│   ├── conversation/      # Chat conversation components
│   ├── message-item/      # Message display components
│   └── magicui/     # Animation components
├── hooks/           # Custom React hooks
│   ├── assistant/   # Assistant management hooks
│   └── feature/     # Feature configuration hooks
├── data/            # TypeScript types and data models
├── lib/             # Utility functions
├── services/        # Runtime services (PluginRuntime, search/export, token stats)
├── windows/         # Window entry points (ask/chat/config/schedule/butler/sidebar/artifacts)
└── artifacts/       # React/Vue artifact templates
```

Key patterns:

-   Use `@/` import alias for `./src/`
-   Component-specific CSS modules alongside Tailwind
-   React Hook Form with Zod for form validation
-   Domain-specific hook organization (assistant/, feature/)

## Backend Structure

```
src-tauri/
├── src/
│   ├── api/                 # Tauri command handlers
│   │   ├── ai/              # AI modules (chat/acp/config/summary/title/types)
│   │   ├── scheduled_task_api.rs  # 定时任务（once/interval 调度、执行、日志、停止）
│   │   ├── skill_api.rs     # Skills 扫描、安装、助手绑定
│   │   ├── butler_api.rs    # Butler 主会话、任务派发、结果回流
│   │   ├── plugin_api.rs    # 插件安装、启停、配置与数据
│   │   ├── token_statistics_api.rs # Token 统计
│   │   ├── export_api.rs    # Markdown -> PDF / DOCX 导出
│   │   ├── copilot_api.rs & copilot_lsp.rs # GitHub Copilot 集成
│   │   └── [other apis]...
│   ├── mcp/                 # MCP 核心（注册/执行/检测/总结）
│   │   ├── builtin_mcp/     # 内置 MCP 工具（agent/ui/search/operation/artifact）
│   │   ├── registry_api.rs  # MCP server 管理
│   │   ├── execution_api.rs # MCP tool call 执行与状态
│   │   └── detection.rs     # AI 响应中的 tool call 检测
│   ├── artifacts/           # Artifact 渲染、运行与集合管理
│   ├── feishu/              # 飞书运行时、消息接入、回发与卡片回调
│   ├── external_channels/   # 外部渠道消息渲染抽象
│   ├── scheduler/           # 定时任务调度运行时
│   ├── skills/              # Skills 扫描、解析、提示词拼装
│   ├── db/                  # Database operations (SQLite)
│   ├── sync.rs              # 自建同步客户端（outbox/shadow/cursor、推拉、冲突与死信）
│   ├── state/               # Application state management
│   ├── template_engine/     # Prompt templating with bang commands
│   └── window.rs            # Window management
```

**Key API modules:**

-   `ai_api.rs`: Main AI entry points (ask/regenerate/cancel/runtime state/title generation)
-   `assistant_api.rs`: Assistant CRUD + model/MCP binding
-   `conversation_api.rs`: Conversation/message management with versioning
-   `llm_api.rs`: Provider/model management and model list sync
-   `scheduled_task_api.rs`: Scheduled task CRUD, structured scheduling, run, logs, cancellation
-   `skill_api.rs`: Skill scanning/installing and assistant skill configuration
-   `butler_api.rs`: Butler main conversation, task orchestration, task detail/result flow
-   `butler_group.rs` / `butler_group_runtime.rs` / `butler_group_router.rs`: group chat commands, the per-member reply dispatcher and automatic responder selection.
-   `export_api.rs`: Markdown to PDF / DOCX export helpers
-   `plugin_api.rs`: Plugin registry, lifecycle, config and data storage
-   `token_statistics_api.rs`: Conversation/group/message token statistics; aggregation rules live in `db/token_stats.rs` (delegation recursion, auxiliary model usage in `model_usage_record`)
-   `copilot_api.rs` & `copilot_lsp.rs`: GitHub Copilot auth and LSP lifecycle
-   `mcp/registry_api.rs`, `mcp/execution_api.rs`, `mcp/detection.rs`: MCP server/tool orchestration
-   `sync.rs`: Self-hosted sync client (settings/token, outbox push, cursor pull, conflict/dead-letter handling, domain change events); server side lives in `sync-server/` (FastAPI + SQLite, alembic migrations); 设计见 `docs/sync-remediation-plan.md`
-   `artifacts/collection_api.rs` & `artifacts/artifact_bridge_api.rs`: Artifact collections + plugin bridge calls

## Built-in MCP Tools

The application includes built-in MCP tools in `mcp/builtin_mcp/`:

-   **Agent Tools**: `load_skill`, `todo_write`, dynamic MCP catalog loading (`load_mcp_server`, `load_mcp_tool`)
-   **UI Interaction Tools**: `ask_user_question`, `preview_file`
-   **Search Tools**: `search_web`, `fetch_url` with browser profile/fingerprint support
-   **Operation Tools**: `read_file`, `write_file`, `edit_file`, `list_directory`, `execute_bash`, `get_bash_output`
-   **Artifact Tools**: `get_artifact_workspace`, `show_artifact`
-   **Template Management**: Built-in MCP template registration and sync

## Artifact Management

-   Artifacts support HTML, SVG, React, Vue components
-   Collections for organizing related artifacts
-   Preview windows with live rendering
-   Script execution environments (Python, Node.js, etc.)

## 关键产品面（Critical Features）

维护以下核心产品面时不要破坏既有行为；各功能的用户向细节见 `docs/product/` 下对应文档：

1.  Multi-Model Support（genai client）
2.  Local Data Storage（SQLite）+ 可选自建同步（`src-tauri/src/sync.rs` + `sync-server/`）
3.  Bang Commands（`!` 开头，template engine）
4.  Message Versioning & Runtime State（regeneration、parent/child chains）
5.  Content Preview & Artifact Workspace
6.  Script Execution（可配置环境）
7.  System Tray + Multi-Window UX
8.  MCP Integration（registry、tool-call 执行状态、内置工具套件）
9.  Scheduled Tasks（once/interval、run logs、notify、stop）
10. Skills System（文件系统技能、安装、助手绑定）
11. Plugin Runtime（加载/配置/数据 + 主题注册）
12. GitHub Copilot Integration（device flow + 可选 LSP）
13. Token Statistics & Export（Markdown / PNG / PDF / DOCX）
14. Assistant Types（自定义表单）
15. Butler Orchestration（桌面联系人、独立私聊、助手间异步请求与回复、管家历史检索、任务回流与审批）
16. Feishu Integration（实验性）
17. Self-Hosted Data Sync（server-wins 冲突、删除传播、死信重放）


### 管家会话的固定工具

`mcp/butler_tools.rs` 统一按 `butler_main` / `butler_contact` / `butler_task` 会话注入 SuperAdmin 和助手协作工具；覆盖助手 MCP 选择，但不持久化配置变更。标准 AI 首轮、工具续答、重新生成、XML 检测及 Codex/Claude Code/ACP 的选中工具桥接共用该策略。执行器对同一白名单免除动态加载门槛，内置分发仍校验管家会话，普通聊天和普通定时任务不获得权限。助手发现/详情与协作查询/停止定义在 `api/butler_assistant_tools.rs`，停止原因存于 `butler_peer_stop`；不把“停止协作”解释为进程取消。

## Agent 驱动边界

`api/ai/agent_driver/` 是本地 Agent 的统一调用层：

- `AgentTurnRequest` / `send_turn` 负责路由、原生配置准备以及统一的响应消息创建、prompt 提交和提交失败事件；`ask_ai` 不再维护三套执行分支。
- `AgentSessionKind` 明确选择 Codex app-server、Claude Code CLI 或 ACP。旧 Claude provider 配置仍经现有识别规则进入原生 Claude 通道；未知 provider 直接报错，不切换协议或模型。
- `AgentSessionHandle` 提供 prompt、取消和配置更新接口；`AgentSessionCapabilities` 表达本地适配器能力。只有 ACP 接收独立附件列表；Codex/Claude 保持原先运行时 prompt 的附件处理方式。ACP 服务端协商出的模型、模式、附件等能力仍由原生协议层校验，不能从本地能力标志推断。
- 已有 `AcpSessionState`、`CodexSessionState`、`ClaudeSessionState` 继续拥有会话、配置签名、空闲回收、恢复和持久化；未增加进程池、数据库表或第二套会话注册表。
- 原生协议层继续处理流式事件、MCP、权限请求、Plan 与 session 快照。已有前端命令和事件格式不变。失败事件写入复用同一帮助函数。
- 控制通道关闭导致 prompt 提交失败时，记录具体通道、conversation/message 标识和底层错误并结束响应，不在本次提交中自动重建会话并重试。

参考 t3code 的类型化 provider adapter 边界与可控命令通道测试方法，固定版本为 [`d5fc29fe5cb62db68bfbf824cfabe0ae94ac6ac4`](https://github.com/pingdotgg/t3code/tree/d5fc29fe5cb62db68bfbf824cfabe0ae94ac6ac4)，主要参考 `packages/provider-core/src/server/ProviderAdapter.ts`、`adapterDriver.ts` 和 `packages/provider-testing`。上游为 MIT（©2026 T3 Tools Inc.）；本实现未复制上游源码或引入其 Effect 编排框架。

Claude 当前保持 Rust 启动 CLI stream-json。Anthropic [官方 Agent SDK 文档](https://code.claude.com/docs/en/agent-sdk/overview)列出 TypeScript/Python，并建议其他语言使用 CLI 子进程；本次不引入额外语言运行时或 sidecar。

Agent 驱动的测试场景、上游参考及明确未覆盖的边界见 [Agent 驱动测试场景对照](./agent-driver-testing.md)。

### Butler group chat

Groups live in the Butler contact list and behave like a chat-app group: the user just posts; nobody picks recipients or discussion modes. `db/butler_group.rs` owns four tables: `butler_chat_group` (id = hidden `butler_group` conversation), `butler_chat_group_member` (one hidden `butler_group_member` execution conversation per membership, with `joined_after` / `delivered_through` cursors), `butler_chat_group_message` (the public log: user / member messages and system `notice` / `error` lines) and `butler_chat_group_turn` (the reply queue).

Routing (`api/butler_group_runtime.rs` → `decide`): explicit `@name` / `@所有人` in the text, then a quoted member message, then a single-member group; otherwise `api/butler_group_router.rs` asks the Butler helper model (`butler_helper_model_id`, or the Butler model when unset) for an ordered responder list. With two or more explicit mentions the responders are fixed and the router only decides `rounds`. Router failures post a specific `error` line in the group; there is no model fallback. Selected responders run concurrently (round 1); they catch up on each other's replies through their cursors on the next turn. `rounds > 1` is a discussion: when every depth-0 turn of a round is terminal (finished, silent, failed or skipped) and at least one member spoke, `advance_discussion` queues the next round for the still-active participants; it stops on a quiet round, the planned round count (≤ 5), a newer user message, or a stop. Each member runs one turn at a time (FIFO); a turn whose trigger was already delivered in an earlier turn is skipped.

A turn freezes the public upper bound, formats the other speakers' messages since the member's cursor as a plain transcript (`用户：…` / `成员名：…` / `〔系统〕…`, with quote excerpts and size caps) and calls `ask_ai` with origin `butler_group/{turn}` plus a per-turn `<group_chat>` prefix (roster, concurrent peers, discussion round guidance and a short chat-style reminder; not stored in history). `ask_ai` skips @assistant extraction, slash/template parsing and work context for that origin and rejects any other input into member conversations. `write_turn_input` inserts the user message only while the turn is still `running` and advances the cursor only after the write. Completion uses the last finished message after the input (same rule as peer exchanges): a complete reply is posted under the member's name with `source_conversation_id` / `source_message_id` pointing at that message (the UI renders group messages through the private-chat `MessageItem`, so token info, code preview/run and export target the real member message), `[[NO_REPLY]]` is silent, errors / interrupted ends post an `error` line, and an idle execution without a final reply fails after a grace period. `@name` in a member reply relays to that member (depth ≤ 3, ≤ 12 relays per user message); participants of a discussion with rounds remaining are left to the next round instead. Stop/remove cancel queued turns, fence running ones and call `butler_peer::halt_turn`; late results of cancelled turns are dropped. Restart recovery publishes finished answers and reports unknown ones without retrying.

Members keep Butler-mode SuperAdmin tools and `ask_user_question` (cards render inside the group view; approvals use the window-level dialogs), but contact-collaboration tools are hidden (`templates::GROUP_MEMBER_KIND`). The group rules are appended to the system prompt via `build_butler_member_prompt`. Private chats and other groups never enter a member's group context. The first collaboration-group implementation's queue tables are migrated (groups, members, public messages) and dropped by `butler_group::schema`.
