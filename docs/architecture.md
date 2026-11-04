# 架构总览

本文档承载 AIPP 的详细架构信息，从根目录 `AGENTS.md` 提取。日常开发规范见 `AGENTS.md`，各功能的用户向文档见 `docs/product/`。

## Core Technologies

-   **Backend**: Rust with Tauri 2.0 framework, SQLite via rusqlite
-   **Frontend**: React 19 with TypeScript, Vite build system
-   **UI Framework**: shadcn/ui components, Radix UI primitives, Tailwind CSS v4
-   **AI Integration**: Custom forked genai client with streaming support
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
-   **ButlerExperiment Window**: Butler main conversation, task board, approvals, Feishu status
-   **Sidebar Window**: Conversation side panel for todo/context/artifact preview
-   **ArtifactPreview Window**: Content preview (HTML, SVG, components)
-   **Artifact Window**: Standalone artifact rendering window
-   **ArtifactCollections Window**: Manage artifact collections
-   **Plugin Window**: Plugin UI host window

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
-   `export_api.rs`: Markdown to PDF / DOCX export helpers
-   `plugin_api.rs`: Plugin registry, lifecycle, config and data storage
-   `token_statistics_api.rs`: Conversation/message token statistics
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
15. Butler Orchestration（主会话、任务会话、结果回流、审批）
16. Feishu Integration（实验性）
17. Self-Hosted Data Sync（server-wins 冲突、删除传播、死信重放）
