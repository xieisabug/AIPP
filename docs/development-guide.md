# 常见开发任务指南

本文档从根目录 `AGENTS.md` 提取，承载常见开发任务的操作步骤。架构明细见 [architecture.md](./architecture.md)，测试规范见 [testing-guidelines.md](./testing-guidelines.md)。

## Adding a New API Endpoint

1. Create Tauri command in `src-tauri/src/api/[module].rs`
2. Export command in `src-tauri/src/api/mod.rs`
3. Register in `src-tauri/src/lib.rs` `invoke_handler` list
4. Create TypeScript types in `src/data/`
5. Call from frontend using `invoke()`
6. Add tests in `src-tauri/src/api/tests/`

## Working with AI Features

-   Core AI logic is in `ai_api.rs` with modular implementations in `ai/` subdirectory
-   Stream processing uses genai client with event emission for real-time UI updates
-   MCP tools are automatically detected via `mcp/detection.rs` and can be called natively
-   All AI responses support versioning through `generation_group_id` and `parent_group_id`
-   Built-in MCP tools are organized under `mcp/builtin_mcp/`

## Adding a New UI Component

1. Check if shadcn/ui has the component
2. Follow existing component patterns in `src/components/`
3. Use domain-specific directories (config/, conversation/, etc.)
4. Use Tailwind classes for styling
5. Add component-specific styles in CSS modules if needed
6. 编写界面的时候，注意样式风格要和现在的界面一致，使用 ShadcnUI 的组件和 tailwind css 的写法，我的主色调是黑白灰，尽量少使用别的颜色

## Adding New Assistant Types

1. Define assistant type in `src/data/Assistant.tsx`
2. Create form configuration in `src/hooks/assistant/useAssistantFormConfig.ts`
3. Add form renderer in `src/components/config/assistant/AssistantFormRenderer.tsx`
4. Handle backend logic in `assistant_api.rs`

## Database Schema Changes

1. Update schema in `src-tauri/src/db/[entity].rs`
2. Handle migrations in `src-tauri/src/db/mod.rs`
3. Update corresponding TypeScript types
4. Key tables: conversations, messages (with versioning), assistants, mcp_servers, llm_models, artifacts

## MCP Integration Guidelines

-   MCP servers are managed through `mcp/registry_api.rs` and stored in SQLite
-   Tool detection happens automatically via `mcp/detection.rs::detect_and_process_mcp_calls`
-   Tool call creation/execution/state sync is handled in `mcp/execution_api.rs`
-   Built-in MCP command suites: `aipp:agent`, `aipp:ui_interaction`, `aipp:search`, `aipp:operation`, `aipp:artifact`
-   MCP auto-run should respect assistant/server/tool config (`is_auto_run` + overrides)
