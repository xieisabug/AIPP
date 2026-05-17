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
- 后台任务保持单个 `ClientSideConnection` 连接活跃
- ACP 启动连接、提示、取消、配置项更新都通过会话句柄路由到后台任务
- 用户打开 ACP 对话时会自动建立 session 连接，用于提前获取 `configOptions`、Plan 和可用命令
- 自动连接不会创建空消息，也不会向 Agent 发送 prompt
- ACP session 空闲 15 分钟且没有活跃 prompt 时会自动释放，之后再次打开对话或发送消息时重新连接

### 会话任务运行时
- ACP 会话任务运行在专用单线程运行时上
- 使用 `LocalSet` 支持非 `Send` 的 futures
- 独立的 Tokio 运行时避免阻塞主线程

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

## 配置输入

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
相关源码:
- `src-tauri/src/api/ai/acp.rs` - ACP 集成主模块
- `src-tauri/src/state/activity_state.rs` - 活动状态管理
