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
-   genai 协议和 provider 改动在独立的 [rust-genai](https://github.com/xieisabug/rust-genai) 仓库完成。先推送通过验证的库提交，再更新 AIPP `src-tauri/Cargo.toml` 中 Git 依赖的固定 `rev` 和 `Cargo.lock`。AIPP 不保留库源码、本地协议补丁或依赖本机目录的路径配置，普通构建和 GitHub Actions 由 Cargo 获取同一提交。Magpie 网关发现、目录解析、四协议客户端和 Responses 缓存 TTL 序列化在库中维护；应用身份、模型偏好、数据库与同步仍在 AIPP。
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
7. 设置类「列表 + 详情」页面用 `ConfigPageLayout` + `SidebarList` + `ListItemButton`（`src/components/common/`）：列表项只占一行，不放第二行小字；`SidebarList` 顶部只放搜索和页面自己的按钮（`addButton`），`title` 只作无障碍名称，不再显示标题和说明。列表需要分组时用 `SidebarListGroups`（传 `{ key, label, count, children }[]`），不要手写分组标题：标题吸顶并显示条目数，多个分组时点标题可跳到其他分组（参考 `SkillsConfig`）。样式只用主题变量（`bg-card`、`border-border`、`bg-primary/10` 等），主题通过 `data-aipp-slot` 定制：`config-split`、`config-list-pane`、`config-list`、`config-list-toolbar`、`config-list-search`、`config-list-actions`、`config-list-items`、`config-list-group`、`config-list-group-header`、`config-list-item`、`config-page-scroll`。

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
-   内置工具集在 `builtin_templates()` 注册。新增工具时，函数参数根节点用 object，字段与数组元素显式声明类型，字符串枚举填写 `type: "string"`。保留执行入口的参数校验。
-   `build_tools_with_mapping` 为内置工具生成模型侧声明，保留必填项、可选项、字段及数组结构，把范围、默认值和字典值类型等提示放进字段描述。运行 `cargo test --manifest-path src-tauri/Cargo.toml --lib test_builtin_tool_schemas` 可检查实际注册的全部内置工具。
-   MCP auto-run should respect assistant/server/tool config (`is_auto_run` + overrides)

## 角色动画扩展

总管家形象、动作与眼镜/帽子装饰的注册、坐标约定和验证方式见 [角色动画与装饰扩展](./avatar-animation.md)。
