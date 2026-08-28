# ACP 集成

ACP (Agent Client Protocol) 集成模块允许 AIPP 与 ACP 代理进行交互，每个对话运行一个独立的 ACP 进程。

---

## ACP 会话管理

### 每个对话一个 ACP 进程
- 采用 per-conversation 进程模型
- 每个对话启动一个长期运行的 ACP 进程
- 会话存储在 `AcpSessionState` 中，以 `conversation_id` 为键
- 进程隔离，不同对话互不干扰

### 会话句柄路由
- `AcpSessionHandle` 会话句柄
- 后台任务保持单个 `ConnectionTo<Agent>` 连接活跃（agent-client-protocol v2 SDK）
- ACP 启动连接、提示、取消、配置项更新都通过会话句柄路由到后台任务
- 用户打开 ACP 对话时会自动建立 session 连接，用于提前获取 `configOptions`、Plan 和可用命令
- 自动连接不会创建空消息，也不会向 Agent 发送 prompt
- ACP session 空闲 15 分钟且没有活跃 prompt 时会自动释放，之后再次打开对话或发送消息时重新连接

### 会话任务运行时
- ACP 会话任务是普通的 `tokio::spawn` 任务
- agent-client-protocol v2 SDK（crates.io `agent-client-protocol` v2.0.0，schema 类型位于 `agent_client_protocol::schema::v1 as acp`）不绑定具体异步运行时，所有 future 均为 `Send`
- 不再需要专用单线程运行时和 `LocalSet`
- 客户端通过 `Client.builder()` + `on_receive_request` / `on_receive_notification` 回调构建；请求通过 `cx.send_request(...).block_task().await` 在 `ConnectionTo<Agent>` 上发送
- v2 `on_receive_*` 回调内联运行在连接调度循环上，因此每个客户端请求处理器（权限/文件/终端/elicitation）必须立即通过 `AcpTauriClient::spawn_request_task` 转发到 `cx.spawn` 任务，并在该任务里经由 `Responder` 响应——禁止在处理器内 `block_task`（死锁）
- 未知扩展方法/通知由 SDK 内建 fallback 处理（method-not-found / 忽略），不再保留旧的 `ext_method` / `ext_notification` 桩

---

## 会话持久化

### session_id 存储
- `session_id` 存储在 `conversation.db` 的 `acp_session` 表中
- 按 `conversation_id` 键值存储
- 会话创建/加载时更新

### 会话加载逻辑
- ACP 启动时检查 `initialize` capabilities
- 如果 `sessionCapabilities.resume` 能力支持且存在存储的 `session_id`，优先调用 `session/resume`
- 如果不支持 `session/resume` 或 resume 失败，但支持 `loadSession`，再调用 `session/load`
- 两种恢复能力都不支持或恢复失败时，回退到 `session/new` 创建新会话，并用 AIPP 本地 conversation 历史构建一次 history fallback prompt
- `claude-code-acp` 不支持旧的 `loadSession`，但可通过 `session/resume` 恢复已有 session
- 恢复成功后，前端会提示本次使用的是 `session/resume` 还是 `session/load`

### 会话重放抑制
- 在 `session/load` / `session/resume` 期间，抑制 ACP `session/update` 通知
- 避免重放内容污染 UI/数据库
- 加载完成后恢复正常事件通知

### 会话元数据同步
- `AcpSessionState` 除了保存 session handle，还保存当前 session 的前端快照
- 同步的内容包括：`session_id`、title、`updated_at`、config options、Plan、可用命令、当前是否有活跃 prompt
- ACP 的 `SessionInfoUpdate` 会同步更新本地 conversation 标题，并发出统一的 `title_change` 事件

### 会话配置
- ACP 会话控制 UI 以 `session_config_options` / `configOptions` 为唯一配置来源
- `configOptions` 来自 `session/new`、`session/load`、`session/resume` 或后续配置更新，不来自 `initialize`
- `category=model`、`category=mode`、`category=thought_level` 会优先显示为模型、运行模式、思考强度配置
- 用户修改配置时调用 `session/set_config_option`
- 成功后使用 Agent 返回的完整 `configOptions` 替换本地状态，避免只在本地猜测单个选项的新值
- 不再暴露旧的 `session/set_mode` UI/命令入口

---

## 工具调用映射

### ACP 工具调用转 MCP 工具调用 UI
- ACP 工具调用转换为 MCP 工具调用 UI 事件
- 统一的工具调用展示界面
- 用户体验一致

### 工具状态映射
- ACP 工具状态映射到：pending/executing/success/failed
- 状态流转与 MCP 工具一致
- 状态图标和提示复用
- ACP `Pending` 只有在明确需要确认时映射为 AIPP 的 pending，否则按 executing 展示，以便运行中的工具显示闪亮边框
- ACP 后续状态更新没有携带新结果时，会保留已保存的工具结果，避免成功后自动收起再展开时结果丢失

### 工具调用事件
- 工具调用事件发送到前端
- UI 实时更新工具状态
- 工具参数和结果展示

---

## 文件/终端操作桥接

### 文件读写操作桥接
- ACP 文件读/写操作桥接到内置操作
- 通过权限管理器进行权限控制
- 操作请求转发到内置文件操作模块

### 终端命令执行桥接
- ACP 终端命令按结构化字段执行：`command` 作为程序名，`args` 作为参数数组
- 支持 `cwd`、`env`、`outputByteLimit`
- 输出超过 `outputByteLimit` 时从开头截断，并保持 UTF-8 字符边界合法

### 权限管理器集成
- 与现有的权限管理器集成
- 权限请求对话框
- 权限决策持久化

### 权限请求审批
- ACP 权限请求会通过现有审批流发到前端
- Butler/Feishu 场景下也会同步转发
- 当用户取消当前 ACP prompt 时，未完成的 ACP 权限请求会被一起取消

---

## 会话生命周期清理

### 原生 Codex / Claude Code

- 打开或切换到历史对话不会仅因浏览而启动 Codex / Claude Code 进程；首次发送消息时才连接，已有热会话继续复用
- 切换离开对话不会中断正在生成、等待权限或执行工具的会话
- ACP、Codex、Claude Code 的本地运行实例统一在无活跃请求且空闲 15 分钟后释放；持久化的 session/thread ID 保留，下次请求通过原生恢复能力继续
- Codex 空闲释放只关闭本地 app-server 进程，不调用 `thread/archive` 或 `thread/delete`；Claude Code 空闲释放只关闭本地 stream-json 进程
- 恢复成功提示按连接代次去重：Codex 在 `thread/resume` 成功响应后确认，Claude Code 在 CLI 返回并核对原 session ID 后确认
- 删除 AIPP 对话时会停止三类本地运行实例并清理本地 session 映射；Codex / Claude Code 的 CLI 侧历史不会被隐式归档或删除

### session/close
- Agent 声明 `session_capabilities.close` 时，会话任务退出前（空闲释放/切换对话等）会先发送 `session/close`（5 秒超时）
- 失败只记录日志，进程仍按原逻辑退出

### session/delete
- 删除 ACP 对话时，`schedule_acp_session_delete` 会结束本地活跃会话、启动一次性 agent 进程按能力调用 `session/delete`，并删除本地 `acp_session` 记录
- Agent 不支持 `session/delete` 时跳过 agent 侧清理；任何失败都不影响本地对话删除

## 结构化提问（Elicitation）

- 客户端通过 `elicitation.form` 能力声明支持表单式结构化提问（依赖 SDK `unstable_elicitation` feature）
- Agent 发起 `elicitation/create` 时，前端复用 ask_user_question 的内联卡片（`AskUserQuestionCard`）渲染：JSON Schema 的文本/数字/整数/布尔/单选/多选字段转换为问题列表，提交时还原为类型化键值
- 选填字段（不在 schema `required` 中）可留空跳过；卡片取消按钮对应拒绝（decline）
- URL 模式、request 作用域和未知模式统一回复 decline 并记录原因
- 取消 prompt 或会话任务退出时，挂起的 elicitation 请求会随权限请求一起取消

---

## 配置输入

### 支持的 ACP CLI
- `claude-code-acp`：Claude Code 的 ACP 适配器（`@zed-industries/claude-code-acp`），支持 Bun 一键安装
- `gemini`：Gemini CLI 原生支持 ACP，需用户自行安装
- `kimi`：Kimi Code CLI 原生支持 ACP，启动时自动附加 `acp` 子命令（即 `kimi acp`），需用户按官方文档安装并先 `/login`
- `dsh-acp-server`：DeepSeek Harness 的 ACP 插件，需 Node.js >= 22 并手动安装 `@deepseek-ai/dsh` 与 `dsh-acp-server`，模型与密钥在 dsh 中配置
- node-shebang 脚本类 CLI（如 npm/bun 全局 bin）统一通过显式 node 运行时启动，保证 Windows 可用
- 启动失败时会按 CLI 给出对应的安装提示

### ACP CLI 命令配置
- 从 `llm_provider_config` 读取 ACP CLI 命令
- 从 `assistant_model_config` 读取助手覆盖配置
- 提供商默认配置 + 助手覆盖配置

### 工作目录配置
- 可配置 ACP 工作目录
- 助手配置界面支持通过文件夹选择器选择工作目录，也可手动编辑路径
- 工作目录传递给 ACP 进程
- 影响相对路径解析

### 环境变量配置
- 可配置环境变量
- 环境变量传递给 ACP 进程
- 支持多环境变量配置

### 额外参数配置
- 可配置额外 CLI 参数
- 参数追加到 ACP 命令
- 灵活的命令定制

### CLI 路径解析
- ACP CLI 按以下顺序解析：
  1. 绝对路径
  2. `~/.bun/bin` 目录
  3. `PATH` 环境变量查找
  4. 原始命令直接使用

---

## 其他功能

### 提示流
- 每个新用户请求创建新响应消息
- ACP 流式输出内容到该消息
- 发出 `message_update` 事件
- 内容持久化到数据库
- 初始化时会显式声明客户端能力：`fs/read_text_file`、`fs/write_text_file`、`terminal/*`
- ACP prompt 会按 Agent 的 prompt capabilities 转换内容块；图片能力不支持时会阻止发送并提示用户，文档内容在支持 `embeddedContext` 时以 Resource 发送

### 取消行为
- `cancel_ai` 对 ACP 会话发送 `session/cancel`
- 正常取消不会 tear down 整个 ACP 进程
- 只有 session 真正退出时才会清理 `AcpSessionState`

### 会话控制 UI
- Chat/Butler 的对话标题栏会显示 ACP 会话入口
- 用户可以查看当前 session 状态、工作目录、config options 和可用命令数量
- ACP Plan 会合并到右侧栏“计划”区域展示，同时在 ACP 会话入口中提供摘要查看
- 支持直接在标题栏更新 ACP config option
- 可用 ACP 命令会进入输入框 `/` 建议，并以 ACP 标识区分 AIPP 自有命令

### 已知限制
- `loadSession` 支持因代理而异
- 部分代理只支持 `session/resume`，不能通过 `session/load` 回放历史消息
- 会话持久化仅在代理支持 `loadSession` 或 `session/resume` 时有效
- 当代理不支持恢复能力或恢复失败时，AIPP 会使用本地 conversation 历史构建一次 history fallback prompt

---

## Codex MCP 桥接注入

- Codex app-server 通道通过 `thread/start` / `thread/resume` 的 `config` 覆盖（`mcp_servers.aipp.*`）把 AIPP 桥（`--aipp-acp-mcp-bridge`）注册为 `aipp` MCP server，以此挂载助手选中的 MCP 工具
- 工具执行经由与 ACP 桥相同的 TCP 代理和权限路径回流
- 选中工具负载是 session 签名的一部分：绑定变更会重建 session，同时恢复同一线程

---
相关源码:
- `src-tauri/src/api/ai/acp.rs` - ACP 集成主模块
- `src-tauri/src/state/activity_state.rs` - 活动状态管理
