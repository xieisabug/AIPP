# MCP 工具系统

MCP (Model Context Protocol) 工具系统是 AIPP 的核心扩展机制，允许 AI 调用各种工具来增强能力。

---

## 应用与工具入口

设置中的「应用与工具」统一提供两个视图：

- **应用目录**：展示 AIPP 内置工具和 aipp-helper 发布的外部应用，支持搜索、分类和应用详情。内置工具来自本地注册表，远端目录不可覆盖其身份和限制。
- **我的应用**：保留已有自定义 MCP、内置工具和目录应用连接；可查看工具数量、连接状态，进入原有工具/提示词管理和高级配置。

外部应用的连接方式由服务端描述声明：无需认证、配置字段（如 API Key/PAT）或浏览器 OAuth。表单支持不同字段、帮助、默认值和条件显示；不要求所有应用走相同步骤。首批目录定义为 Notion OAuth 与 GitHub PAT，需发布 helper 接口后才会出现。

连接名称用于区分同一应用的多个连接。目录连接复用原 MCP server ID 和执行链，不复制工具体系；助手授权复用已有服务器绑定、工具开关和执行审核。本期不增加账号级权限矩阵，也不自动安装关联 Skills。

目录应用的秘密字段和 OAuth 令牌保存在系统凭据存储，普通配置仅保留引用。登录后必须完成连接验证才显示可用；重启后显示待验证。断开会停用连接、取消等待中的登录并删除本机凭据，服务商侧撤销授权仍需在其账户设置操作。普通编辑不能更换托管连接的认证地址或重新启用已断开的连接。

目录加载失败会显示明确错误；已有缓存会标注时间，内置工具和旧连接仍可管理。Android 暂不支持新目录认证连接的安全存储；已有远程自定义 MCP 和内置工具仍按原平台规则使用。Windows 以外的平台及真实账号 OAuth/PAT 流程尚待实机验收。

开发协议、实现边界与验收步骤见 [应用目录协议](../connected-apps-protocol.md)。这套应用连接不替代独立的 PluginRuntime 插件或 Skills 管理。

---

## MCP 服务器管理

### 服务器配置
- 支持添加多个 MCP 服务器
- 服务器配置包括：
  - 服务器名称
  - 命令（command）
  - 工作目录（cwd）
  - 环境变量（env）
  - 附加参数（args）
- 配置持久化到 `mcp_servers` 表

### 服务器 CRUD
- `add_mcp_server` - 添加新服务器
- `get_mcp_servers` - 获取服务器列表
- `update_mcp_server` - 更新服务器配置
- `delete_mcp_server` - 删除服务器，同时清理工具、资源、提示词、调用及审查日志、能力目录、加载记录和助手授权关联。
- 工具刷新时移除的工具会清理目录与助手配置，已加载快照保留为“工具已删除”，用于说明能力失效原因。
- `test_mcp_server` - 测试服务器连接

### 服务器启动与停止
- 按需启动 MCP 服务器进程
- 使用 rmcp crate 的客户端传输
- 进程生命周期管理
- 错误处理与重试

---

## 工具自动检测

### 工具发现
- 连接服务器后自动调用 `tools/list` 获取工具列表
- 解析工具名称、描述、输入参数 schema
- 工具信息缓存

### 动态 MCP 加载
- 支持按助手配置动态启用/禁用 MCP 服务器
- `is_dynamic_mcp_loading_enabled_for_assistant` 检查
- `collect_mcp_info_for_assistant` 收集可用工具

### 工具检测与处理
- `detect_and_process_mcp_calls` 自动检测工具调用
- 支持原生工具调用和提示词格式化两种模式
- 工具调用结果格式化
- 同一轮混合自动执行与手动确认时，自动工具可先执行，但必须等所有工具完成后才向模型续写；手动执行顺序不受卡片位置限制。开启失败后继续也不会跳过尚未确认或执行中的工具。

---

## 本地工具调用

### use_native_toolcall 模式
- 当 `use_native_toolcall` 为 true 时启用原生调用
- 使用 genai 客户端的原生工具调用能力
- 直接传递工具定义给模型
- 模型返回 ToolCall 后执行

### 原生工具调用执行
- `execute_mcp_tool_call` 执行工具调用
- 调用 MCP 服务器的 `tools/call` 方法
- 处理调用结果
- 超时与错误处理
- Codex app-server 和 Claude Code 通过 AIPP stdio MCP bridge 使用助手绑定的工具，实际执行仍由 AIPP 的统一传输与权限链路处理

### 工具结果返回
- 工具执行结果作为助手消息返回
- 支持内容和图片两种结果类型
- 错误信息友好展示

---

## 提示词格式化回退

### 非原生调用模式
- 当 `use_native_toolcall` 为 false 时使用提示词格式化
- 通过提示词让模型输出工具调用指令
- 正则表达式解析工具调用

### 提示词格式化
- `format_mcp_prompt` 构建工具调用提示词
- 包含可用工具列表和使用说明
- 格式化工具调用格式（XML 或 JSON）

### 结果解析
- 解析模型输出中的工具调用
- 提取工具名称和参数
- 执行工具并将结果插回对话

---

## 工具调用 UI

### 工具调用自动审核
- 只有可信内置的免审核操作直接执行；外部工具、未知操作和需要判断参数/场景的操作必须审核。旧自动运行开关和会话覆盖不能绕过此规则。
- 需要审核且助手审核方式为「模型自动审核」时，原生 ToolCall 与提示词模式在执行前由审核模型判断
- `call_mcp_tool` 先投影成真实工具，再把目标工具名和参数交给审核模型
- 审核模型会把该工具的描述和字段说明写成普通文本放进 prompt（schema 展开成字段列表，不走 tools API），用来理解字段含义；prompt 仍要求只审这次调用本身，不判断工具是否存在，也不尝试执行、调用或回答该调用
- 在「设置 → 程序功能 → 辅助AI」选择审核输出方式：自动（默认）、强制 Format（JSON Schema）、仅 Prompt（JSON）。只使用所选模型和服务商。
- 自动模式优先使用 JSON Schema；仅当接口明确返回不支持 `response_format` / `json_schema` 的能力错误时，移除 Format 参数，用同一模型以 Prompt JSON 重试一次。鉴权失败、限流、网络失败、一般参数错误与输出解析错误不触发重试。两次请求共用 20 秒审核时限。
- Prompt 返回合法审核结论（安全或有风险）后，才记住该接口使用 Prompt；按服务商、模型、请求协议和接口地址隔离本地检测结果。配置页显示检测状态，点击「重新检测」后下次审核重新尝试 Format。强制 Format 失败直接报错；仅 Prompt 不发送 Format 参数。Prompt 输出失败不会反向自动开启 Format。
- 所有模式均严格解析整个 JSON 对象，必须包含布尔值 `safe` 和非空字符串 `reason`，不允许额外字段。完整校验失败、拒答、输出截断或调用参数超过 8000 字符时转人工确认，不从自由文本提取放行结论。当前依赖会忽略 schema 的适配器在自动模式下直接使用 Prompt，强制 Format 时明确报错。
- 判定安全则直接执行；有风险或审核失败则保持 pending
- 人工审核时，非免审核操作等待用户确认；免审核操作仍直接执行
- 审核进行时卡片显示「自动审核中」。结束后无论通过、有风险还是失败，都在展开的详情里展示审核意见；工具卡片收起时，审核信息一并收起。确认执行走 `execute_mcp_tool_call`，拒绝走 `reject_mcp_tool_call`：调用记为 failed，理由写入 `tool_review_log.user_decision`，并回写工具结果后续写
- 审核期间隐藏人工执行操作，审核结束且仍待执行时提供执行与拒绝入口，展开和收起状态均可操作。审核提示与按钮共用实时状态；专用工具卡片在待审核/待确认时显示完整审核操作卡片，执行结束后恢复专用展示。普通人工待执行调用也可拒绝
- 留痕表 `tool_review_log` 可按 `conversation_id` 或 `mcp_tool_call_id` 查询，记录模型、结论、理由、耗时和用户决定
- 免审核来源按调用记录的服务器 ID、内置标记和内置命令核实，不凭工具名称判断；外部同名工具不豁免。

| 内置操作 | 默认审核 |
| --- | --- |
| 提问、文件预览、代码预览 | 免审核，原有预览权限仍有效 |
| 加载技能、查询工具目录、加载工具定义、更新待办 | 免审核；加载不等于执行 |
| 创建子任务、读取子任务、追加指令 | 免审核；子任务工具独立审核 |
| 助手间发送消息（内置 `message_bot`） | 免审核；接收助手后续工具调用独立审核 |
| 子任务待执行工具、代批权限、代答问题 | 需要审核 |
| 定时任务 list/get/disable | 免审核 |
| 定时任务创建、修改、启用、删除、立即运行 | 需要审核 |
| 搜索、获取网页、文件读取/列目录/写入/编辑、Shell 执行 | 需要审核 |
| 获取后台命令输出 | 免审核 |
| 获取 Artifact 工作区、展示 Artifact | 免审核 |
| Artifact 截图 | 需要审核 |
| SuperAdmin 目录、详情、审计查询 | 免审核 |
| SuperAdmin 助手/会话/任务/定时任务/MCP/提供商/模型的只读查询 | 免审核 |
| SuperAdmin 创建助手/会话、归档会话、启动/取消任务、添加模型 | 免审核 |
| SuperAdmin 修改助手、注入系统消息、修改 MCP/提供商、同步远程模型、撤销 | 需要审核 |
| SuperAdmin 批量操作 | 全部子操作免审核才直接执行，否则整批审核 |
| 转调用 | 按真实目标分类；包装入口不提供豁免 |

`llm.get_models` 会同步并覆盖本地模型集合，需要审核；`llm.get_model` 是只读详情查询。未知 action、参数无法解析或来源无法核实均不豁免。
- 文件路径信任确认仍在工具执行内部生效，审核放行不会跳过工作区检查

### 工具调用状态展示
- 工具调用卡片 UI
- 状态流转：pending → executing → success/failed

### 状态图标与提示
- pending：等待中
- executing：执行中（动画）
- success：成功（绿色勾）
- failed：失败（红色叉）

### 工具参数与结果展示
- 显示调用的工具名称
- 展示输入参数（JSON 格式化）
- 展示工具执行结果
- 支持展开/收起长内容

---

## 内置 MCP 工具集成

### 内置工具注册
- 内置工具与外部 MCP 服务器统一管理
- Web 搜索、文件操作等作为内置 MCP 工具
- 支持模板动态配置

### 模板管理
- `builtin_mcp/templates.rs` 管理 MCP 模板
- 动态配置 MCP 服务器
- 模板验证

---

## 其他功能

### 目录摘要
- `summarizer.rs` 提供目录摘要功能
- 递归遍历目录结构
- 生成目录树摘要

### 工具名称映射
- `resolve_tool_name`、`sanitize_tool_name` 处理工具名称
- 处理特殊字符和重复名称
- 工具名称规范化

### MCP 工具摘要生成
- 使用 AI 模型自动为 MCP 服务器和工具生成简洁的中文摘要
- 可配置专用的摘要生成模型
- 批量处理所有已启用的 MCP 服务器
- 通过 `mcp-summary-progress` 事件报告生成进度
- 摘要结果持久化到 `mcp_tool_catalog_entry` 表
- 支持重试机制和 JSON 解析回退

### 动态 MCP 加载（实验性）
- 功能开关：`dynamic_mcp_loading_enabled` 特性标志控制，可在助手上覆盖。关闭时仍把该助手选中的工具 schema 直接注入
- 开启后，模型 `tools` 只保留始终可用工具（Agent 工具、提问和预览）、`load_mcp_server`、`load_mcp_tool` 和 `call_mcp_tool`。会话过程中不因加载而追加 schema，系统提示词也不随加载改写
- 加载结果里的描述和参数定义只出现在 `load_mcp_tool` 的工具返回中。之后用 `call_mcp_tool` 调用已加载工具
- 聊天里按目标工具展示这次调用，权限、失败态和审计也落在目标工具上
- 仅暴露已生成摘要的工具目录条目
- 未生成摘要的服务器/工具自动隐藏
- 能力纪元（capability epoch）变更时自动重置摘要
- 相关数据库表：`dynamic_mcp_template`、`dynamic_mcp_server`、`conversation_loaded_mcp_tool`

### MCP 工具调用历史
- `mcp_tool_call` 表记录所有 MCP 工具调用历史
- 记录调用的工具名称、参数、结果、状态
- 支持历史调用查询和分析

### MCP 工具调用组件
- 聊天界面的 MCP 工具调用统一通过前端组件注册表解析
- 默认回退组件是通用工具调用卡片，内置组件也通过同一注册表注册
- 特定工具可注册专属组件，例如 `preview_code` 使用内联代码预览卡片，`load_mcp_server` / `load_mcp_tool` 使用轻量加载卡片
- 内置 `preview_code` 和 `preview_file` 只要已启用，就会自动执行并返回预览结果，不受自动执行开关影响。发送新消息或重新生成时，请求历史中尚未开始的工具调用会记录为未执行并跳过，避免未配对的工具调用阻断对话。重新生成在准备请求阶段失败时，会显示具体错误并结束运行状态。
- 插件注册的工具调用组件由「显示」设置控制，不写入 MCP 服务器或工具配置
- 组件匹配支持 `serverName`、`toolName`，自动匹配时按 `priority` 选择优先级更高的匹配组件
- 渲染前会为组件 props 注入 `currentToolCall`，优先来自全局 MCP 工具调用状态；如果状态尚未同步，则根据当前注释中的 `callId`、`conversationId`、工具名和参数构造一个 partial 记录
- `currentToolCall` 包含当前调用的 `call_id`、`conversation_id`、`message_id`、`status`、`llm_call_id`、`server_name`、`tool_name`、`parameters`、`result`、`error`、`started_time`、`finished_time` 等前端已知字段，供插件组件直接展示状态、参数、结果或错误

---
相关源码:
- `src-tauri/src/mcp/mod.rs` - MCP 模块总入口
- `src-tauri/src/mcp/registry_api.rs` - MCP 服务器注册管理
- `src-tauri/src/mcp/execution_api.rs` - 工具执行 API
- `src-tauri/src/mcp/detection.rs` - 工具检测
- `src-tauri/src/mcp/prompt.rs` - 提示词构建
- `src-tauri/src/mcp/summarizer.rs` - MCP 摘要生成
- `src-tauri/src/api/ai/mcp.rs` - AI 中的 MCP 集成
- `src/hooks/useMcpToolCallProcessor.tsx` - 工具调用处理 Hook
- `src/services/mcpToolComponentRegistry.tsx` - 工具调用组件注册表


## HTTP 搜索（手机与桌面）

内置「搜索工具」（`aipp:search`）提供 `search_http`，不依赖浏览器。设置 → MCP → 搜索工具中选择服务：

- **Tavily**：填写 API Key；默认地址 `https://api.tavily.com/search`。
- **Brave Search**：填写 API Key；默认地址 `https://api.search.brave.com/res/v1/web/search`。
- **SearXNG**：填写实例的完整 `/search` 地址，实例须开启 JSON 输出；API Key 可留空，填写时以 Bearer 发送。

HTTP 搜索默认未启用，须明确选择服务；不会在失败时自动换服务。服务可能有费用或配额，以所选服务为准。API Key 在配置界面遮蔽显示。超时可设为 1000–60000 毫秒，默认 30000 毫秒。

调用参数为 `query`（1–600 字符）及可选 `max_results`（1–20，默认 5）；返回标题、URL、摘要和排名。真正的空结果与鉴权失败、配额不足、限流、超时及协议错误分别返回。SearXNG 全部引擎失败时报告失败，部分结果以 `partial` 标识。响应上限 2 MiB，拒绝重定向，错误中保留服务、主机、阶段和会话标识并遮蔽配置密钥。

桌面端原有 `search_web` 继续使用浏览器，移动端只展示 `search_http` 和 `fetch_url`，MCP 工具列表、助手工具配置和数量统计均隐藏依赖桌面浏览器的 `search_web`；已有桌面工具配置仍保留。移动端发送前检查、模型工具列表及动态工具发现也排除该工具，因此桌面保存的启用状态不会阻止手机聊天；显式调用旧的浏览器工具仍会被拒绝。升级后刷新内置搜索工具列表，并在助手 MCP 工具授权中选用 `search_http`。

协议参考：[Tavily](https://docs.tavily.com/documentation/api-reference/endpoint/search)、[Brave](https://api-dashboard.search.brave.com/api-reference/web/search/get)、[SearXNG](https://docs.searxng.org/dev/search_api.html)。
