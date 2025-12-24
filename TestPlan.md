# AIPP 测试计划

> 创建日期: 2024-12-24
> 版本: 1.0

## 目录

1. [项目模块概览](#项目模块概览)
2. [测试策略](#测试策略)
3. [后端测试计划](#后端测试计划)
4. [前端测试计划](#前端测试计划)
5. [集成测试计划](#集成测试计划)
6. [TODO 列表](#todo-列表)

---

## 项目模块概览

### 后端模块 (Rust/Tauri)

| 模块 | 文件位置 | API 数量 | 优先级 | 现有测试 |
|------|----------|----------|--------|----------|
| AI 对话 | `api/ai_api.rs`, `api/ai/*` | 5 | P0 | ✅ 部分 |
| 对话管理 | `api/conversation_api.rs` | 9 | P0 | ✅ 部分 |
| 助手管理 | `api/assistant_api.rs` | 14 | P1 | ❌ |
| LLM 配置 | `api/llm_api.rs` | 15 | P1 | ❌ |
| MCP 服务 | `mcp/*` | 多个 | P1 | ❌ |
| 子任务 | `api/sub_task_api.rs` | 17 | P2 | ❌ |
| 数据库层 | `db/*.rs` | N/A | P0 | ✅ 部分 |
| 模板引擎 | `template_engine/` | N/A | P2 | ✅ 有 |
| Artifacts | `artifacts/*` | 多个 | P2 | ❌ |
| Copilot | `api/copilot_api.rs` | 9 | P3 | ✅ 部分 |
| 系统 API | `api/system_api.rs` | 12 | P2 | ❌ |

### 前端模块 (React/TypeScript)

| 模块 | 文件位置 | 组件数量 | 优先级 | 现有测试 |
|------|----------|----------|--------|----------|
| 核心 Hooks | `hooks/*.ts` | 30+ | P0 | ❌ |
| 配置组件 | `components/config/*` | 20+ | P1 | ❌ |
| 对话组件 | `components/conversation/*` | 9 | P1 | ❌ |
| 消息组件 | `components/message-item/*` | 多个 | P1 | ❌ |
| 窗口入口 | `windows/*` | 8 | P2 | ❌ |
| UI 组件 | `components/ui/*` | 多个 | P3 | ❌ |

---

## 测试策略

### 优先级定义

- **P0 (关键)**: 核心功能，必须优先覆盖
- **P1 (重要)**: 主要功能，应尽早覆盖
- **P2 (一般)**: 次要功能，按需覆盖
- **P3 (低)**: 辅助功能，可选覆盖

### 测试类型

1. **单元测试**: 函数/方法级别的测试
2. **组件测试**: React 组件的渲染和交互测试
3. **集成测试**: 多模块协作的测试
4. **端到端测试**: 完整用户流程测试

### 测试文件组织规范

> ⚠️ **重要**: 测试代码必须按功能域分离到独立文件，禁止将所有测试写在单一文件中

#### 后端测试组织 (Rust)

```
src-tauri/src/
├── db/
│   └── tests/                    # 数据库层测试（模块化目录）
│       ├── mod.rs               # 测试模块入口
│       ├── test_helpers.rs      # 共享的测试辅助函数
│       ├── conversation_tests.rs # Conversation 测试
│       ├── message_tests.rs     # Message 测试
│       ├── attachment_tests.rs  # Attachment 测试
│       └── assistant_tests.rs   # Assistant 测试
├── api/
│   └── tests/                   # API 层测试
│       ├── mod.rs
│       ├── ai_api_tests.rs      # AI API 测试
│       ├── conversation_api_tests.rs
│       └── ...
└── template_engine/
    └── tests.rs                 # 小模块可用单文件
```

**规则**:
1. 每个功能域一个测试文件（如 conversation_tests.rs, message_tests.rs）
2. 共享的辅助函数放入 `test_helpers.rs`
3. 使用 `mod.rs` 统一导出所有测试模块
4. 测试函数命名: `test_[功能]_[场景]`

#### 前端测试组织 (React/TypeScript)

```
src/
├── __tests__/                   # 全局测试配置和 Mock
│   ├── setup.ts                # 测试环境初始化
│   └── mocks/
│       └── tauri.ts            # Tauri invoke Mock
├── components/
│   └── [Component]/
│       ├── Component.tsx
│       └── Component.test.tsx  # 组件测试（同级放置）
├── hooks/
│   └── useXxx.test.ts          # Hook 测试（同级放置）
└── utils/
    └── utils.test.ts           # 工具函数测试
```

**规则**:
1. 组件/Hook 测试与源文件同级放置
2. 测试文件命名: `[SourceFile].test.tsx` 或 `[SourceFile].test.ts`
3. 全局 Mock 和 Setup 放入 `__tests__/` 目录
4. 测试命名: `should [行为] when [条件]`

---

## 后端测试计划

### 1. 数据库层 (P0) - `db/*.rs`

**现状**: 测试已模块化到 `db/tests/` 目录

**测试文件结构**:
```
db/tests/
├── mod.rs              # 测试模块入口
├── test_helpers.rs     # 共享辅助函数
├── conversation_tests.rs  # ✅ 已完成
├── message_tests.rs       # ✅ 已完成
├── attachment_tests.rs    # TODO
└── assistant_tests.rs     # TODO
```

**测试目标**:
- 所有 Repository 的 CRUD 操作
- 消息版本管理逻辑
- 数据迁移兼容性

#### 1.1 conversation_db.rs

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_conversation_crud` | 对话的创建/读取/更新/删除 | ✅ |
| `test_message_crud` | 消息的创建/读取/更新/删除 | ✅ |
| `test_list_messages_by_conversation_id` | 按对话查询消息列表 | ✅ |
| `test_generation_group_id_management` | 消息版本组管理 | ✅ |
| `test_parent_child_relationships` | 消息父子关系 | ✅ |
| `test_message_regeneration_scenarios` | 消息重发场景 | ✅ |
| `test_message_attachment_crud` | 消息附件 CRUD | ❌ |
| `test_batch_delete_messages` | 批量删除消息 | ❌ |

#### 1.2 assistant_db.rs

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_assistant_crud` | 助手的创建/读取/更新/删除 | ❌ |
| `test_assistant_with_mcp_config` | 带 MCP 配置的助手 | ❌ |
| `test_assistant_with_feature_config` | 带功能配置的助手 | ❌ |
| `test_list_assistants_by_type` | 按类型查询助手列表 | ❌ |

#### 1.3 llm_db.rs

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_provider_crud` | LLM 提供商 CRUD | ❌ |
| `test_model_crud` | 模型 CRUD | ❌ |
| `test_list_models_by_provider` | 按提供商查询模型 | ❌ |
| `test_model_config_merge` | 模型配置合并逻辑 | ❌ |

#### 1.4 mcp_db.rs

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_mcp_server_crud` | MCP 服务器 CRUD | ❌ |
| `test_mcp_server_with_tools` | 带工具的 MCP 服务器 | ❌ |
| `test_list_enabled_mcp_servers` | 查询启用的服务器 | ❌ |

---

### 2. AI API (P0) - `api/ai_api.rs`, `api/ai/*`

**现状**: 已有 `tests/ai_api_tests.rs` 覆盖部分逻辑

**测试目标**:
- AI 对话请求/响应流程
- 消息版本管理
- MCP 工具调用集成

#### 2.1 核心对话逻辑

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_generation_group_id_logic` | generation_group_id 决策逻辑 | ✅ |
| `test_parent_id_logic_for_regeneration` | 重发时 parent_id 逻辑 | ✅ |
| `test_message_filtering_logic` | 消息过滤逻辑 | ✅ |
| `test_complex_version_chain` | 复杂版本链处理 | ✅ |
| `test_regenerate_with_reasoning_and_response_groups` | reasoning + response 分组 | ✅ |
| `test_stream_response_handling` | 流式响应处理 | ❌ |
| `test_non_stream_response_handling` | 非流式响应处理 | ❌ |
| `test_error_handling_in_chat` | 对话错误处理 | ❌ |

#### 2.2 MCP 集成

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_mcp_tool_detection` | MCP 工具检测 | ❌ |
| `test_mcp_native_tool_call` | 原生工具调用 | ❌ |
| `test_mcp_prompt_format_fallback` | Prompt 格式降级 | ❌ |
| `test_mcp_tool_result_processing` | 工具结果处理 | ❌ |

#### 2.3 配置管理 (`ai/config.rs`)

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_chat_options_building` | 聊天选项构建 | ❌ |
| `test_model_config_override` | 模型配置覆盖 | ❌ |
| `test_assistant_config_merge` | 助手配置合并 | ❌ |

---

### 3. 对话管理 API (P0) - `api/conversation_api.rs`

**现状**: 已有 `tests/conversation_api_tests.rs`

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_version_management_logic` | 版本管理逻辑 | ✅ |
| `test_empty_message_list` | 空消息列表处理 | ✅ |
| `test_single_user_message` | 单条用户消息 | ✅ |
| `test_create_conversation` | 创建对话 | ❌ |
| `test_delete_conversation` | 删除对话 | ❌ |
| `test_update_conversation_title` | 更新对话标题 | ❌ |
| `test_get_conversation_with_messages` | 获取对话及消息 | ❌ |
| `test_switch_message_version` | 切换消息版本 | ❌ |
| `test_delete_message` | 删除消息 | ❌ |

---

### 4. 助手管理 API (P1) - `api/assistant_api.rs`

**测试目标**:
- 助手 CRUD
- MCP 配置绑定
- 功能配置管理

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_list_assistants` | 列出所有助手 | ❌ |
| `test_get_assistant_by_id` | 按 ID 获取助手 | ❌ |
| `test_create_assistant` | 创建助手 | ❌ |
| `test_update_assistant` | 更新助手 | ❌ |
| `test_delete_assistant` | 删除助手 | ❌ |
| `test_duplicate_assistant` | 复制助手 | ❌ |
| `test_import_assistant` | 导入助手 | ❌ |
| `test_export_assistant` | 导出助手 | ❌ |
| `test_bind_mcp_to_assistant` | 绑定 MCP 到助手 | ❌ |
| `test_unbind_mcp_from_assistant` | 解绑 MCP | ❌ |
| `test_update_feature_config` | 更新功能配置 | ❌ |
| `test_validation_errors` | 验证错误处理 | ❌ |

---

### 5. LLM 配置 API (P1) - `api/llm_api.rs`

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_list_providers` | 列出提供商 | ❌ |
| `test_create_provider` | 创建提供商 | ❌ |
| `test_update_provider` | 更新提供商 | ❌ |
| `test_delete_provider` | 删除提供商 | ❌ |
| `test_list_models` | 列出模型 | ❌ |
| `test_create_model` | 创建模型 | ❌ |
| `test_update_model` | 更新模型 | ❌ |
| `test_delete_model` | 删除模型 | ❌ |
| `test_get_default_model` | 获取默认模型 | ❌ |
| `test_set_default_model` | 设置默认模型 | ❌ |
| `test_test_model_connection` | 测试模型连接 | ❌ |
| `test_sync_models_from_provider` | 从提供商同步模型 | ❌ |

---

### 6. MCP 服务 (P1) - `mcp/*`

#### 6.1 MCP 注册与管理

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_register_mcp_server` | 注册 MCP 服务器 | ❌ |
| `test_unregister_mcp_server` | 注销 MCP 服务器 | ❌ |
| `test_list_mcp_servers` | 列出 MCP 服务器 | ❌ |
| `test_enable_disable_mcp_server` | 启用/禁用服务器 | ❌ |
| `test_get_mcp_tools` | 获取 MCP 工具列表 | ❌ |

#### 6.2 MCP 执行

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_execute_mcp_tool` | 执行 MCP 工具 | ❌ |
| `test_mcp_tool_timeout` | 工具执行超时 | ❌ |
| `test_mcp_tool_error_handling` | 工具执行错误 | ❌ |

#### 6.3 内置 MCP 工具

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_web_search_google` | Google 搜索 | ❌ |
| `test_web_search_bing` | Bing 搜索 | ❌ |
| `test_web_search_fallback` | 搜索引擎降级 | ❌ |
| `test_url_fetch` | URL 内容抓取 | ❌ |
| `test_url_fetch_with_browser` | 浏览器抓取 | ❌ |

---

### 7. 模板引擎 (P2) - `template_engine/`

**现状**: 已有 `template_engine/tests.rs`

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_current_date_command` | !current_date 命令 | ❌ |
| `test_current_time_command` | !current_time 命令 | ❌ |
| `test_selected_text_command` | !selected_text 命令 | ❌ |
| `test_web_command` | !web 命令 | ❌ |
| `test_web_to_markdown_command` | !wm 命令 | ❌ |
| `test_sub_start_command` | !sub_start 命令 | ❌ |
| `test_nested_commands` | 嵌套命令解析 | ❌ |
| `test_context_variable_substitution` | 上下文变量替换 | ❌ |
| `test_unknown_command_handling` | 未知命令处理 | ❌ |

---

### 8. 子任务 API (P2) - `api/sub_task_api.rs`

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_create_sub_task` | 创建子任务 | ❌ |
| `test_update_sub_task_status` | 更新子任务状态 | ❌ |
| `test_list_sub_tasks` | 列出子任务 | ❌ |
| `test_cancel_sub_task` | 取消子任务 | ❌ |
| `test_sub_task_ai_execution` | 子任务 AI 执行 | ❌ |

---

### 9. Artifacts (P2) - `artifacts/*`

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_create_artifact` | 创建 Artifact | ❌ |
| `test_update_artifact` | 更新 Artifact | ❌ |
| `test_delete_artifact` | 删除 Artifact | ❌ |
| `test_react_preview_generation` | React 预览生成 | ❌ |
| `test_vue_preview_generation` | Vue 预览生成 | ❌ |
| `test_html_preview_generation` | HTML 预览生成 | ❌ |
| `test_collection_management` | Collection 管理 | ❌ |

---

### 10. 系统 API (P2) - `api/system_api.rs`

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `test_get_app_version` | 获取应用版本 | ❌ |
| `test_get_app_data_dir` | 获取数据目录 | ❌ |
| `test_check_environment` | 环境检查 | ❌ |
| `test_open_external_url` | 打开外部 URL | ❌ |

---

## 前端测试计划

### 1. 测试环境配置 (P0)

| 任务 | 描述 | 状态 |
|------|------|------|
| 安装 Vitest | 测试框架 | ❌ |
| 安装 Testing Library | 组件测试库 | ❌ |
| 配置 vitest.config.ts | Vitest 配置 | ❌ |
| 创建 Mock Tauri | Mock invoke 调用 | ❌ |
| 配置测试脚本 | package.json scripts | ❌ |

---

### 2. 核心 Hooks (P0) - `hooks/*.ts`

#### 2.1 数据管理 Hooks

| 测试用例 | Hook | 描述 | 状态 |
|----------|------|------|------|
| `should load conversations on mount` | useConversationManager | 加载对话列表 | ❌ |
| `should create conversation` | useConversationManager | 创建对话 | ❌ |
| `should delete conversation` | useConversationManager | 删除对话 | ❌ |
| `should update conversation title` | useConversationManager | 更新标题 | ❌ |
| `should load models on mount` | useModels | 加载模型列表 | ❌ |
| `should update model` | useModels | 更新模型 | ❌ |
| `should get current assistant` | useAssistantRuntime | 获取当前助手 | ❌ |
| `should save assistant` | useAssistantRuntime | 保存助手 | ❌ |
| `should load mcp servers` | useMcpServers | 加载 MCP 服务器 | ❌ |

#### 2.2 消息处理 Hooks

| 测试用例 | Hook | 描述 | 状态 |
|----------|------|------|------|
| `should process message versions` | useMessageProcessing | 处理消息版本 | ❌ |
| `should group messages` | useMessageGroups | 消息分组 | ❌ |
| `should handle message events` | useConversationEvents | 消息事件处理 | ❌ |

#### 2.3 UI 状态 Hooks

| 测试用例 | Hook | 描述 | 状态 |
|----------|------|------|------|
| `should manage scroll position` | useScrollManagement | 滚动位置管理 | ❌ |
| `should handle file drop` | useFileDropHandler | 文件拖拽处理 | ❌ |
| `should manage theme` | useTheme | 主题管理 | ❌ |
| `should handle copy` | useCopyHandler | 复制处理 | ❌ |

---

### 3. 配置组件 (P1) - `components/config/*`

#### 3.1 助手配置

| 测试用例 | 组件 | 描述 | 状态 |
|----------|------|------|------|
| `should render assistant list` | AssistantConfig | 渲染助手列表 | ❌ |
| `should open add dialog` | AddAssistantDialog | 打开添加对话框 | ❌ |
| `should validate form` | EditAssistantDialog | 表单验证 | ❌ |
| `should save assistant` | EditAssistantDialog | 保存助手 | ❌ |
| `should show mcp section` | AssistantMCPConfigSection | 显示 MCP 配置 | ❌ |

#### 3.2 LLM 配置

| 测试用例 | 组件 | 描述 | 状态 |
|----------|------|------|------|
| `should render provider list` | LLMProviderConfig | 渲染提供商列表 | ❌ |
| `should add provider` | LLMProviderConfigForm | 添加提供商 | ❌ |
| `should update provider` | LLMProviderConfigForm | 更新提供商 | ❌ |
| `should delete provider` | LLMProviderConfig | 删除提供商 | ❌ |
| `should show model list` | ReadOnlyModelList | 显示模型列表 | ❌ |

#### 3.3 MCP 配置

| 测试用例 | 组件 | 描述 | 状态 |
|----------|------|------|------|
| `should render mcp list` | MCPConfig | 渲染 MCP 列表 | ❌ |
| `should add mcp server` | MCPServerDialog | 添加 MCP 服务器 | ❌ |
| `should show tool list` | MCPToolParameters | 显示工具列表 | ❌ |
| `should toggle mcp status` | MCPConfig | 切换启用状态 | ❌ |

---

### 4. 对话组件 (P1) - `components/conversation/*`

| 测试用例 | 组件 | 描述 | 状态 |
|----------|------|------|------|
| `should render message list` | MessageList | 渲染消息列表 | ❌ |
| `should render input area` | InputArea | 渲染输入区域 | ❌ |
| `should handle send message` | InputArea | 发送消息 | ❌ |
| `should show completion list` | BangCompletionList | 显示 Bang 补全 | ❌ |
| `should render conversation header` | ConversationHeader | 渲染对话头部 | ❌ |
| `should edit title` | ConversationTitle | 编辑标题 | ❌ |

---

### 5. 消息组件 (P1) - `components/message-item/*`

| 测试用例 | 组件 | 描述 | 状态 |
|----------|------|------|------|
| `should render user message` | MessageItem | 渲染用户消息 | ❌ |
| `should render ai message` | MessageItem | 渲染 AI 消息 | ❌ |
| `should render reasoning` | ReasoningMessage | 渲染推理过程 | ❌ |
| `should render code block` | CodeBlock | 渲染代码块 | ❌ |
| `should show version pagination` | VersionPagination | 显示版本分页 | ❌ |
| `should handle regenerate` | MessageItem | 处理重发 | ❌ |
| `should render mcp tool call` | McpToolCall | 渲染 MCP 调用 | ❌ |

---

### 6. 通用组件 (P2)

| 测试用例 | 组件 | 描述 | 状态 |
|----------|------|------|------|
| `should render confirm dialog` | ConfirmDialog | 确认对话框 | ❌ |
| `should render form dialog` | FormDialog | 表单对话框 | ❌ |
| `should handle tag input` | TagInput | 标签输入 | ❌ |
| `should render markdown` | UnifiedMarkdown | Markdown 渲染 | ❌ |

---

## 集成测试计划

### 1. 关键用户流程 (P1)

| 测试用例 | 描述 | 状态 |
|----------|------|------|
| `配置助手完整流程` | 创建 → 编辑 → 保存 → 验证 | ❌ |
| `配置 LLM 提供商流程` | 添加 → 配置 → 测试连接 | ❌ |
| `配置 MCP 服务器流程` | 添加 → 获取工具 → 启用 | ❌ |
| `对话完整流程` | 新建 → 发送 → 接收 → 重发 | ❌ |
| `消息版本切换流程` | 重发 → 切换版本 → 验证内容 | ❌ |
| `文件附件流程` | 拖拽 → 上传 → 发送 → 显示 | ❌ |

---

## TODO 列表

### 阶段一：测试基础设施 (Week 1) ✅ 已完成

- [x] **T001** [P0] 安装前端测试依赖 (Vitest, Testing Library, happy-dom)
- [x] **T002** [P0] 创建 vitest.config.ts 配置文件
- [x] **T003** [P0] 创建 src/__tests__/setup.ts 测试环境配置
- [x] **T004** [P0] 创建 src/__tests__/mocks/tauri.ts Mock Tauri invoke
- [x] **T005** [P0] 在 package.json 添加测试脚本
- [x] **T006** [P0] 验证后端测试环境 (cargo test 正常运行)
- [x] **T006.1** [P0] 修复后端测试 schema (添加 tool_calls_json 列)
- [x] **T006.2** [P0] 创建前端基础设施验证测试 (infrastructure.test.tsx)
- [x] **T006.3** [P0] 创建工具函数测试 (utils.test.ts)

### 阶段二：后端核心测试 (Week 2-3)

#### 数据库层
- [ ] **T007** [P0] 完善 conversation_db 测试 - 附件 CRUD
- [ ] **T008** [P0] 添加 assistant_db 测试 - 完整 CRUD
- [ ] **T009** [P1] 添加 llm_db 测试 - 完整 CRUD
- [ ] **T010** [P1] 添加 mcp_db 测试 - 完整 CRUD

#### AI API
- [ ] **T011** [P0] 添加 stream 响应处理测试
- [ ] **T012** [P0] 添加 non-stream 响应处理测试
- [ ] **T013** [P0] 添加错误处理测试
- [ ] **T014** [P1] 添加 MCP 工具检测测试
- [ ] **T015** [P1] 添加 MCP 工具执行测试
- [ ] **T016** [P1] 添加配置合并逻辑测试

#### Conversation API
- [ ] **T017** [P0] 添加 create_conversation 测试
- [ ] **T018** [P0] 添加 delete_conversation 测试
- [ ] **T019** [P0] 添加 switch_message_version 测试
- [ ] **T020** [P1] 添加 update_conversation_title 测试

### 阶段三：后端扩展测试 (Week 4-5)

#### Assistant API
- [ ] **T021** [P1] 添加 list_assistants 测试
- [ ] **T022** [P1] 添加 create_assistant 测试
- [ ] **T023** [P1] 添加 update_assistant 测试
- [ ] **T024** [P1] 添加 delete_assistant 测试
- [ ] **T025** [P1] 添加 MCP 绑定/解绑测试
- [ ] **T026** [P1] 添加功能配置更新测试
- [ ] **T027** [P1] 添加导入/导出测试

#### LLM API
- [ ] **T028** [P1] 添加 provider CRUD 测试
- [ ] **T029** [P1] 添加 model CRUD 测试
- [ ] **T030** [P1] 添加默认模型管理测试
- [ ] **T031** [P2] 添加模型连接测试测试
- [ ] **T032** [P2] 添加模型同步测试

#### MCP 服务
- [ ] **T033** [P1] 添加 MCP 服务器注册测试
- [ ] **T034** [P1] 添加 MCP 工具获取测试
- [ ] **T035** [P1] 添加 MCP 执行测试
- [ ] **T036** [P2] 添加内置搜索工具测试
- [ ] **T037** [P2] 添加 URL 抓取工具测试

### 阶段四：前端核心测试 (Week 6-7)

#### 核心 Hooks
- [ ] **T038** [P0] 添加 useConversationManager 测试
- [ ] **T039** [P0] 添加 useModels 测试
- [ ] **T040** [P0] 添加 useAssistantRuntime 测试
- [ ] **T041** [P1] 添加 useMessageProcessing 测试
- [ ] **T042** [P1] 添加 useMessageGroups 测试
- [ ] **T043** [P1] 添加 useMcpServers 测试
- [ ] **T044** [P1] 添加 useConversationEvents 测试
- [ ] **T045** [P2] 添加 useScrollManagement 测试
- [ ] **T046** [P2] 添加 useTheme 测试

### 阶段五：前端组件测试 (Week 8-10)

#### 配置组件
- [ ] **T047** [P1] 添加 AssistantConfig 组件测试
- [ ] **T048** [P1] 添加 AddAssistantDialog 组件测试
- [ ] **T049** [P1] 添加 EditAssistantDialog 组件测试
- [ ] **T050** [P1] 添加 LLMProviderConfig 组件测试
- [ ] **T051** [P1] 添加 MCPConfig 组件测试
- [ ] **T052** [P1] 添加 MCPServerDialog 组件测试

#### 对话组件
- [ ] **T053** [P1] 添加 MessageList 组件测试
- [ ] **T054** [P1] 添加 InputArea 组件测试
- [ ] **T055** [P1] 添加 ConversationHeader 组件测试
- [ ] **T056** [P2] 添加 BangCompletionList 组件测试

#### 消息组件
- [ ] **T057** [P1] 添加 MessageItem 组件测试
- [ ] **T058** [P1] 添加 VersionPagination 组件测试
- [ ] **T059** [P2] 添加 CodeBlock 组件测试
- [ ] **T060** [P2] 添加 ReasoningMessage 组件测试
- [ ] **T061** [P2] 添加 McpToolCall 组件测试

### 阶段六：后端其他模块 (Week 11-12)

#### 模板引擎
- [ ] **T062** [P2] 添加 Bang 命令解析测试
- [ ] **T063** [P2] 添加上下文变量替换测试
- [ ] **T064** [P2] 添加嵌套命令测试

#### Sub Task API
- [ ] **T065** [P2] 添加子任务 CRUD 测试
- [ ] **T066** [P2] 添加子任务状态管理测试
- [ ] **T067** [P2] 添加子任务 AI 执行测试

#### Artifacts
- [ ] **T068** [P2] 添加 Artifact CRUD 测试
- [ ] **T069** [P2] 添加预览生成测试
- [ ] **T070** [P2] 添加 Collection 管理测试

#### System API
- [ ] **T071** [P2] 添加版本信息测试
- [ ] **T072** [P2] 添加环境检查测试

### 阶段七：集成测试 (Week 13-14)

- [ ] **T073** [P1] 添加助手配置完整流程测试
- [ ] **T074** [P1] 添加 LLM 配置完整流程测试
- [ ] **T075** [P1] 添加 MCP 配置完整流程测试
- [ ] **T076** [P1] 添加对话完整流程测试
- [ ] **T077** [P1] 添加消息版本切换流程测试
- [ ] **T078** [P2] 添加文件附件流程测试

### 阶段八：CI/CD 与维护 (Week 15+)

- [ ] **T079** [P2] 配置 GitHub Actions 自动测试
- [ ] **T080** [P2] 配置代码覆盖率报告
- [ ] **T081** [P3] 添加性能基准测试
- [ ] **T082** [P3] 编写测试编写指南文档

---

## 测试覆盖率目标

| 模块 | 目标覆盖率 |
|------|-----------|
| 数据库层 | 90%+ |
| AI API | 80%+ |
| Conversation API | 85%+ |
| Assistant API | 80%+ |
| LLM API | 80%+ |
| MCP 服务 | 75%+ |
| 核心 Hooks | 85%+ |
| 配置组件 | 75%+ |
| 对话组件 | 70%+ |

---

## 参考资源

- [Vitest 文档](https://vitest.dev/)
- [Testing Library 文档](https://testing-library.com/)
- [Rust 测试指南](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [Tauri 测试](https://tauri.app/v1/guides/testing/)
