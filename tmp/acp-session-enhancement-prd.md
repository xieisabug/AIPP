# AIPP ACP 会话能力增强 PRD

## 背景定位

AIPP 以本地对话系统为核心管理消息、附件、上下文、权限、工具状态和 UI。ACP Agent 作为助手运行时接入 AIPP，由 AIPP 负责会话入口、用户交互、文件与终端权限、状态展示和本地对话持久化。

本功能增强 AIPP 的 ACP 会话体验，使 ACP 助手在模型配置、多模态输入、终端执行、计划展示和命令发现上接近 AIPP 原生助手体验。

## 设计目标

- AIPP conversation 是主会话入口，ACP session 是运行时执行上下文。
- ACP 会话配置只使用 `session_config_options` / `configOptions`。
- AIPP 不使用旧 `modes` 或 `session/set_mode` 作为新版 UI 控制来源。
- AIPP 附件和上下文按 ACP Agent 能力转换为结构化 prompt 内容。
- 多模态内容不能被静默丢弃；不能发送时必须提示用户。
- 终端能力必须安全、可追踪、可截断、可释放。
- Agent 不支持关键能力时，AIPP 应清楚告知用户。
- AIPP 不实现 ACP 本地会话列表，AIPP conversation list 仍是唯一主入口。

## 核心需求

### ACP 会话配置面板

AIPP 在用户打开 ACP 对话时自动建立 session 连接。连接成功后读取 `configOptions`，并在对话标题区域提供会话配置入口。自动连接不创建空消息，不向 Agent 发送 prompt。

`configOptions` 来源为 ACP Session Setup 响应（`session/new`、`session/load`、`session/resume`）和后续 `config_option_update`，不是 `initialize` 响应。若 Agent 在恢复 session 时没有返回 `configOptions`，AIPP 不自行伪造配置项，界面按“未返回配置项”展示。

配置项按 Agent 返回顺序展示。当前主要支持 `select` 类型。配置项按 `category` 做重点识别：

- `model`：模型选择。
- `mode`：运行模式选择。
- `thought_level`：思考强度选择。
- 未知 `category`：普通配置项。

用户修改配置后，AIPP 调用 `session/set_config_option`。成功后，使用 Agent 返回的完整 `configOptions` 替换当前状态。

Agent 未提供 `configOptions` 时，面板显示“该 Agent 不支持新版会话配置”。

### ACP 多模态 Prompt

AIPP 发送 ACP prompt 前，应根据 Agent 的 `promptCapabilities` 构造内容块。

- 文本输入始终作为 `Text` 发送。
- 图片附件在 Agent 支持 `image` 时作为 `Image` 发送。
- Agent 不支持图片时，AIPP 阻止发送并提示“该 ACP Agent 不支持图片输入”。
- 文本文件、Artifact、上下文片段在 Agent 支持 `embeddedContext` 时作为 `Resource` 发送。
- Agent 不支持嵌入上下文时，AIPP 不得静默丢弃内容，应明确降级为可见文本或提示无法嵌入。
- AIPP 普通 LLM 多模态逻辑可保留，但 ACP 路径必须拥有独立的 ACP 内容块转换规则。

### 终端执行

ACP 终端请求必须按结构化字段执行：

- `command` 保持命令名。
- `args` 保持参数数组。
- `cwd` 作为实际工作目录。
- `env` 合并到进程环境。
- `outputByteLimit` 控制保留输出大小。

AIPP 不把 `command` 和 `args` 简单拼接成一条 shell 字符串。

终端输出超过限制时，从开头截断，并保证字符串合法。

终端生命周期状态包括：运行中、已完成、失败、已杀掉、已释放。

危险命令继续走 AIPP 权限体系。

### Session 恢复与加载提示

AIPP 初始化后读取 Agent 的恢复能力。

有已保存 `session_id` 时，恢复顺序为：

1. 支持 `session/resume` 时优先调用 `session/resume`。
2. `resume` 不支持或失败时，如果支持 `loadSession`，再调用 `session/load`。
3. 两者都不可用或失败时，创建新 session，并使用 AIPP 本地对话上下文继续。

恢复成功后，AIPP 弹出提示，明确本次使用的是 `session/resume` 还是 `session/load`。

Agent 不支持恢复历史 ACP 会话时，AIPP 显示“该 Agent 不支持恢复历史 ACP 会话，新会话会使用 AIPP 对话上下文继续”。

ACP session 空闲 15 分钟且没有活跃 prompt 时，AIPP 自动释放该 session。再次打开对话或发送消息时，AIPP 重新连接并按上述顺序恢复 session。

### Plan 展示

AIPP 接收 ACP Plan 更新后，在会话运行状态中展示计划，并在右侧栏“计划”区域展示。

计划项显示：

- 内容。
- 优先级。
- 状态。

每次收到新的 Plan，AIPP 用完整新列表替换旧列表。

状态包括：待处理、进行中、已完成。

Plan 不写入用户消息正文，作为会话运行状态展示。

### Available Commands

AIPP 接收 `available_commands_update` 后保存当前 ACP 会话可用命令。

用户在输入框输入 `/` 时，AIPP 展示 ACP 命令建议。

命令项显示：

- 名称。
- 说明。
- 输入提示。
- `ACP` 来源标识。

用户选择命令后，AIPP 将命令文本插入输入框。发送时仍作为普通 ACP prompt 内容发送。

ACP 命令与 AIPP 自有 slash 命令需要区分展示。

### 工具调用状态与结果展示

ACP 工具调用映射到 AIPP MCP 工具调用 UI。

工具状态映射到：

- 等待确认。
- 运行中。
- 成功。
- 失败。

ACP `Pending` 只有在明确需要用户确认时显示为等待；普通运行中的工具应显示运行中和闪亮边框。

工具成功后，后续状态更新没有携带新结果时，AIPP 保留已有结果，避免用户展开后看不到结果。

## 边界规则

- AIPP 不实现 ACP 本地会话列表作为主入口。
- Agent 未声明能力时，AIPP 按不支持处理。
- 配置项缺少 `category` 时仍可展示，但不进入模型、模式、思考强度的重点位置。
- 多模态内容不能被静默丢弃。
- ACP session 状态丢失不应破坏 AIPP conversation 历史。
- `session/resume` 恢复 Agent 内部上下文，不要求重放历史到 AIPP UI。
- `session/load` 期间 AIPP 应抑制历史回放写入，避免重复污染当前对话。

## 验收标准

- ACP 会话面板能显示并修改模型、模式、思考强度等 `configOptions`。
- 修改配置后，UI 使用 Agent 返回的完整 `configOptions` 刷新。
- AIPP 不再通过旧 `modes` 渲染 ACP 模式选择。
- 图片附件发送给支持 `image` capability 的 ACP Agent 时，Agent 能收到图片内容块。
- 不支持图片能力的 Agent 收到图片附件时，AIPP 给出明确提示。
- 终端命令使用结构化 `command` / `args` / `cwd` / `env` 执行。
- 终端输出按 `outputByteLimit` 截断，并保持合法字符串。
- `loadSession=false` 且不支持 `session/resume` 的 Agent 在 UI 中有明确提示。
- 加载 session 时，UI 明确提示使用的是 `session/resume` 还是 `session/load`。
- ACP session 空闲 15 分钟后自动释放，释放后再次打开对话可重新连接恢复。
- ACP Plan 能在 UI 中展示并随通知刷新。
- ACP Available Commands 能在输入框 `/` 建议中出现并可插入发送。
- 工具运行中状态显示正确，成功结果不会在后续空更新后丢失。

## Todo 跟踪清单

### ACP 会话配置

- [x] 打开 ACP 对话时自动建立 session 连接。
- [x] 自动连接不创建空消息。
- [x] 自动连接不发送 prompt。
- [x] 自动连接返回的 session 状态不会被旧的空状态同步覆盖。
- [x] 增加自动连接状态防覆盖的前端测试。
- [x] 在 ACP 会话标题入口显示 `configOptions`。
- [x] 按 `category=model` 显示模型选择。
- [x] 按 `category=mode` 显示运行模式选择。
- [x] 按 `category=thought_level` 显示思考强度选择。
- [x] 展示未知 `category` 的普通配置项。
- [x] 用户修改配置时调用 `session/set_config_option`。
- [x] 成功后用返回的完整 `configOptions` 替换本地状态。
- [x] Agent 未提供 `configOptions` 时显示“不支持新版会话配置”。
- [x] 移除旧 `session/set_mode` 对外命令入口。
- [x] UI 不再依赖旧 `modes` 渲染 ACP 模式选择。

### ACP 多模态 Prompt

- [x] 文本输入作为 ACP `Text` 内容块发送。
- [x] 图片附件在 Agent 支持 `image` 时作为 ACP `Image` 内容块发送。
- [x] Agent 不支持 `image` 时阻止发送并提示用户。
- [x] 文本/PDF/Word/PowerPoint/Excel 附件在支持 `embeddedContext` 时作为 `Resource` 发送。
- [x] ACP 路径拥有独立内容块转换逻辑，不复用普通 LLM 拼接逻辑。
- [ ] 音频附件作为 ACP `Audio` 内容块发送。
- [ ] Agent 不支持 `audio` 时阻止发送并提示用户。
- [ ] Artifact 与上下文片段按 ACP 能力转换为 `Resource` 或明确提示无法嵌入。

### 终端执行

- [x] ACP 终端用结构化 `command` 启动进程。
- [x] ACP 终端用结构化 `args` 传参。
- [x] ACP 终端支持 `cwd`。
- [x] ACP 终端支持 `env`。
- [x] ACP 终端支持 `outputByteLimit`。
- [x] 输出超限时从开头截断。
- [x] 截断时保持 UTF-8 字符边界合法。
- [ ] UI 明确展示终端“已杀掉”状态。
- [ ] UI 明确展示终端“已释放”状态。
- [ ] 客户端侧危险终端命令分类并强制进入 AIPP 权限体系。

### Session 恢复与加载

- [x] 初始化后读取 `loadSession` 能力。
- [x] 初始化后读取 `sessionCapabilities.resume` 能力。
- [x] 有已保存 session 时优先调用 `session/resume`。
- [x] `resume` 不支持或失败时再尝试 `session/load`。
- [x] 两者不可用或失败时创建新 session。
- [x] 恢复能力不可用时显示明确提示。
- [x] 恢复成功时提示使用的是 `session/resume` 还是 `session/load`。
- [x] `session/load` / `session/resume` 期间抑制历史回放写入。
- [x] 保留轻量 AIPP 历史上下文兜底。
- [x] ACP session 空闲 15 分钟且无活跃 prompt 时自动释放。
- [x] 增加 ACP session 空闲释放判断的 Rust 单元测试。

### Plan 展示

- [x] 接收 ACP Plan 更新。
- [x] 每次 Plan 更新用完整新列表替换旧列表。
- [x] 在 ACP 会话入口展示 Plan。
- [x] 在右侧栏“计划”区域展示 ACP Plan。
- [x] 展示待处理、进行中、已完成状态。
- [x] Plan 不写入用户消息正文。

### Available Commands

- [x] 接收 `available_commands_update`。
- [x] 保存当前 ACP 会话可用命令。
- [x] 输入框输入 `/` 时展示 ACP 命令建议。
- [x] 命令项显示名称和说明。
- [x] 命令项显示输入提示。
- [x] 命令项显示 `ACP` 来源标识。
- [x] 选择命令后插入输入框。
- [x] 发送时作为普通 ACP prompt 内容发送。
- [x] ACP 命令与 AIPP 自有 slash 命令区分展示。

### 工具调用状态与结果展示

- [x] ACP 工具调用映射到 AIPP MCP 工具调用 UI。
- [x] ACP `Pending` 仅在明确需要确认时显示等待。
- [x] 普通运行中工具显示运行中和闪亮边框。
- [x] 成功状态不被后续非终态更新覆盖。
- [x] 成功结果不被后续空结果更新清掉。
- [x] 增加工具结果保留的前端测试。

### 文档与验证

- [x] 更新产品文档 `docs/product/11-ACP集成.md`。
- [x] `npx tsc --noEmit` 通过。
- [ ] `npm run build` 通过。
- [ ] `npx vitest run src/components/McpToolCall.test.tsx` 通过。
- [ ] `npx vitest run src/hooks/useConversationEvents.test.tsx` 通过。
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml acp_session_entry_idle_timeout_ignores_active_prompt` 通过。
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` 通过。

## 当前验证阻塞

- `npm run build` 和 Vitest 在当前环境中因 `esbuild spawn EPERM` 无法启动。
- `cargo check` / `cargo test` 在当前环境中因写入或移动 `target` incremental 编译文件被 Windows 拒绝访问而中断。
- 以上阻塞需要在允许子进程和 Cargo 写入默认 target 的环境中复验。
