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
- ACP 提示、取消、模式切换、配置项更新都通过会话句柄路由到后台任务

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
- 如果 `loadSession` 能力支持且存在存储的 `session_id`，调用 `session/load`
- 否则回退到 `session/new` 创建新会话
- `claude-code-acp` 报告 `agent_capabilities.load_session=false`，因此跳过加载

### 会话重放抑制
- 在 `session/load` 期间，抑制 ACP `session/update` 通知
- 避免重放内容污染 UI/数据库
- 加载完成后恢复正常事件通知

### 会话元数据同步
- `AcpSessionState` 除了保存 session handle，还保存当前 session 的前端快照
- 同步的内容包括：`session_id`、title、`updated_at`、当前 mode、可选 modes、config options、当前是否有活跃 prompt
- ACP 的 `SessionInfoUpdate` 会同步更新本地 conversation 标题，并发出统一的 `title_change` 事件

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
- ACP 终端命令桥接到内置 Shell 执行
- 支持 Bash/PowerShell 等
- 输出捕获和返回

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

### 取消行为
- `cancel_ai` 对 ACP 会话发送 `session/cancel`
- 正常取消不会 tear down 整个 ACP 进程
- 只有 session 真正退出时才会清理 `AcpSessionState`

### 会话控制 UI
- Chat/Butler 的对话标题栏会显示 ACP 会话入口
- 用户可以查看当前 session 状态、工作目录、当前模式与 config options
- 支持直接在标题栏切换 ACP mode 和更新 ACP config option

### 已知限制
- `loadSession` 支持因代理而异
- `claude-code-acp` 不支持会话加载
- 会话持久化仅在代理支持时有效
- 当代理不支持 `loadSession` 或加载失败时，AIPP 会使用本地 conversation 历史构建一次 history fallback prompt

---
相关源码:
- `src-tauri/src/api/ai/acp.rs` - ACP 集成主模块
- `src-tauri/src/state/activity_state.rs` - 活动状态管理
