# 飞书回传与工具调用可读化设计

## 1. 问题定义

总管家当前的飞书接入存在两类核心问题：

1. 飞书回发直接拿对话中的某一条 `assistant/response` 文本内容发送，导致 `<!-- MCP_TOOL_CALL: ... -->` 这类内部标记原样暴露到飞书，不可读。
2. 飞书回发逻辑只在一次处理结束后选取“最新一条已完成 assistant/response 消息”发送，无法稳定覆盖多阶段输出，所以会出现“工具调用内容发出去了，但后续更重要的回复没发出去”的情况。

同时，还缺少清晰的外发策略：

- 当前语义接近“只回飞书触发的一次最终结果”。
- 用户希望默认情况下，AIPP 与飞书都能看到总管家的对话内容。
- 只有在显式开启配置项“是否只返回飞书请求的响应到飞书”后，才将 Feishu 视为“仅对飞书来源请求做回流”的目标渠道。

本设计的目标是在不破坏 AIPP 原有对话存储和 UI 的前提下，补齐一个适合未来扩展到更多外部渠道 / MCP 工具提供方的外发呈现层与回传策略层。

## 2. 设计目标

### 2.1 目标

- 飞书不再看到原始 `MCP_TOOL_CALL` 注释或内部工具结果原文。
- 工具调用与工具结果在飞书中以“可读摘要”形式呈现。
- 内置 builtin MCP 工具拥有专门的格式化器。
- 为未来第三方 MCP provider / plugin 提供统一扩展点。
- 回传逻辑从“单条快照式发送”升级为“按消息序列稳定发送”。
- 新增配置项“是否只返回飞书请求的响应到飞书”，默认关闭。
- 默认关闭该配置时，AIPP 与飞书都能看到总管家可见对话内容。
- 开启该配置时，仅 Feishu 来源请求所产生的对话内容回发到飞书；AIPP 发起的对话只展示在 AIPP。

### 2.2 非目标

- 本阶段不设计飞书富文本卡片完整 DSL，只先定义文本渲染抽象。
- 本阶段不改动 AI 内部消息存储格式的主体结构，只在必要时增加外发表、scope 或投影层。
- 本阶段不尝试让所有任意 MCP 工具都自动拥有完美自然语言摘要；未知工具允许回退到通用格式化器。

## 3. 当前实现与根因

### 3.1 当前飞书入站/出站主链路

当前 `process_incoming_text_message` 的主流程是：

1. 接收飞书文本事件。
2. 调用总管家主会话执行一次 `ask_ai`。
3. `wait_for_butler_to_settle(...)` 等待总管家与任务状态稳定。
4. 调用 `find_latest_completed_assistant_message(...)`，在本次处理起点之后挑出一条“最新 assistant/response 消息”。
5. 直接将这条消息的 `content` 传入 `reply_text_message(...)` 发回飞书。

这一逻辑有两个天然缺陷：

- 它只挑一条消息，而不是按顺序处理“本次新增的全部可见消息”。
- 它直接复用消息原始 `content`，而消息内容里可能含有 MCP 注释、工具调用痕迹、内部中间态文本。

### 3.2 为什么会看到 `<!-- MCP_TOOL_CALL: ... -->`

当前 response 消息里本来就可能包含：

- `tool_calls_json`
- `<!-- MCP_TOOL_CALL: ... -->` 注释
- `Tool execution completed: ...` 这类工具结果原文

这些内容对于 AIPP 内部是合理的，因为：

- 前端需要它们来重建工具调用 UI。
- 总结 / 分支 / 回放逻辑也依赖这些结构。

但外部渠道不应该直接消费原始内部文本，而应该消费“投影后的外发文本”。

### 3.3 为什么后续更重要的回复会漏发

当前回传模型把一次处理视为“最终只发一条消息”。这对于以下场景都不稳：

- 先输出一条带工具调用注释的 response，随后再输出真正总结。
- 子任务回流后又继续推理，再补发新的最终结论。
- 一次处理内生成多条可见消息，例如中间进度、工具结果说明、最终摘要。

在这些情况下，单条快照式选择很容易选错：

- 选到工具调用中间态。
- 选到格式不适合外发的 response。
- 因 race / finish_time / 消息类型差异遗漏真正应该外发的消息。

## 4. 总体设计

建议把现有“飞书回发文本”改造成三层架构：

1. **消息收集层（Relay Capture）**
   负责稳定找出“哪些本地消息应该被外发”。
2. **消息投影层（Relay Projection）**
   负责把 AIPP 内部消息转换成对外可见的结构化片段。
3. **渠道渲染层（Channel Rendering）**
   负责把结构化片段渲染成飞书文本，未来也可扩展成卡片或其他渠道格式。

这三层中间新增一个关键组件：

- **工具呈现注册表（Tool Presentation Registry）**

它专门负责“把工具调用/结果转换成更可读的展示内容”。

## 5. 外发消息收集层设计

### 5.1 设计原则

- 不再使用“选一条最新 assistant/response”的方式。
- 改为收集某个 scope 内新增的所有“外发候选消息”，按 `message.id` 顺序依次发送。
- 每条消息是否已经对某渠道发送过，需要有显式去重记录。

### 5.2 外发候选消息

外发候选消息建议包括：

- `user`
- `assistant`
- `response`
- `tool_result`
- `butler_task_result` 对应的可见汇总消息（如果仍以普通消息存储，则按普通消息处理）

默认不外发：

- `system`
- `reasoning`
- 纯内部占位/空消息

注意：

- `response` 不能直接外发原始 content，必须先经过投影层清洗。
- `tool_result` 也不应原样外发，应先经过工具呈现层。

### 5.3 作用域（Relay Scope）

引入 `RelayScope` 概念，表示“一次连续对话回合的外发边界”。

建议字段：

- `scope_id`
- `conversation_id`
- `origin`：`feishu` / `aipp`
- `channel`
- `external_chat_id`
- `external_user_id`
- `reply_to_external_message_id`
- `start_local_message_id`
- `last_scanned_local_message_id`
- `last_delivered_local_message_id`
- `status`
- `created_time`
- `updated_time`

说明：

- Feishu 入站会创建一个 `origin = feishu` 的 scope。
- AIPP 本地发起总管家消息时，也会创建 `origin = aipp` 的 scope。
- 外发扫描器只处理某个 scope 起点之后新出现的候选消息。

### 5.4 外发去重

新增独立外发投递记录表，例如：

`external_channel_delivery`

建议字段：

- `channel`
- `conversation_id`
- `scope_id`
- `local_message_id`
- `projection_kind`
- `external_message_id`
- `status`
- `created_time`

唯一键建议：

- `(channel, local_message_id, projection_kind)`

这样可以避免：

- 重连后重复发送。
- 扫描补偿时重复发送。
- 同一条消息既作为正文又作为工具摘要被重复外发。

## 6. 外发策略设计

### 6.1 新配置项

新增 experimental 配置项：

- 键：`butler_feishu_only_reply_feishu_originated`
- 文案：`是否只返回飞书请求的响应到飞书`
- 默认值：`false`

前端配置位置：

- `FeatureAssistantConfig.tsx` 增加默认值与读写映射。
- `ExperimentalConfigForm.tsx` 在飞书机器人接入配置区域中新增一个 `Switch`。

### 6.2 行为语义

#### 默认值：关闭（false）

语义：

- 总管家主会话中的可见对话内容，默认同时存在于 AIPP 与飞书。
- Feishu 发来的消息会进入 AIPP，也会根据外发规则回到飞书。
- AIPP 发起的总管家消息也允许外发到飞书。

#### 开启（true）

语义：

- 只有 `origin = feishu` 的 relay scope 产生的内容，才会回发到飞书。
- Feishu 发来的消息仍会显示在 AIPP。
- AIPP 发起的消息与响应只留在 AIPP，不回飞书。

### 6.3 AIPP 发起内容默认回到哪个飞书目标

这是本需求里最需要显式定义的地方。

建议一期策略：

- **不做多群/多线程广播**。
- 每个 Butler 主会话最多只维护一个“主飞书回传目标（primary relay target）”。
- 该目标取最近一次有效 Feishu 入站 scope 的 `external_chat_id + reply_to_external_message_id`。
- 当 `butler_feishu_only_reply_feishu_originated = false` 且存在主飞书回传目标时，AIPP 发起的内容可以镜像到这个目标。
- 若当前没有任何可用主飞书回传目标，则 AIPP 内容只留在本地，不强行外发。

这样做的原因是避免在一个共享的 Butler 主会话上，误把 AIPP 内容广播给多个飞书群/用户，造成隐私泄露。

如果未来要支持多目标广播，应另做明确的订阅/绑定机制，不建议隐式 fan-out。

## 7. 消息投影层设计

### 7.1 投影目标

内部消息需要先转换成统一的 `RelayProjection`，再交给渠道渲染器。

建议结构：

```rust
struct RelayProjection {
    local_message_id: i64,
    message_type: String,
    segments: Vec<RelaySegment>,
}

enum RelaySegment {
    Text { text: String },
    ToolCall { tool: ToolRef, summary: String },
    ToolResult { tool: ToolRef, summary: String },
    Notice { text: String },
}

struct ToolRef {
    server_name: Option<String>,
    tool_name: String,
    call_id: Option<String>,
    parameters_json: Option<serde_json::Value>,
}
```

### 7.2 各类消息投影规则

#### user

- 直接投影为普通文本。
- 若来自 Feishu 入站，可保留为用户原文，不带内部 channel metadata。

#### assistant / response

- 去除 `<!-- MCP_TOOL_CALL: ... -->` 内部注释。
- 保留剩余自然语言文本。
- 若存在 `tool_calls_json` 或可解析出的 MCP_TOOL_CALL 注释，则额外生成 `ToolCall` 段。

#### tool_result

- 不直接暴露原始 `Tool execution completed: ...` 文本。
- 先解析：
  - Tool Call ID
  - Tool
  - Server
  - Parameters
  - Result
- 再走工具呈现注册表，生成可读摘要。

#### system / reasoning

- 默认不外发。

## 8. 工具呈现注册表设计

### 8.1 目标

为不同工具提供不同的可读化描述，并支持 builtin 工具优先、未知工具回退、未来插件扩展。

### 8.2 核心接口

建议定义统一接口：

```rust
trait ExternalToolPresenter: Send + Sync {
    fn matches(&self, tool: &ToolRef) -> PresenterMatch;
    fn present_call(&self, ctx: &ToolCallPresentationContext) -> Option<RelaySegment>;
    fn present_result(&self, ctx: &ToolResultPresentationContext) -> Option<RelaySegment>;
}
```

`PresenterMatch` 可包含优先级：

- `ExactTool`
- `BuiltinNamespace`
- `ProviderNamespace`
- `Fallback`

注册表解析顺序：

1. 精确匹配 builtin formatter
2. 精确匹配 provider/plugin formatter
3. server 级 formatter
4. 通用 fallback formatter

### 8.3 第一批 builtin formatter

建议优先为这些 builtin MCP 工具提供专门呈现：

- `agent::spawn_task_conversation`
- `agent::todo_write`
- `agent::load_skill`
- `ui_interaction::ask_user_question`
- `ui_interaction::preview_file`
- `search::search_web`
- `search::fetch_url`
- `operation::read_file`
- `operation::list_directory`
- `operation::write_file`
- `operation::edit_file`
- `operation::execute_bash`
- `operation::get_bash_output`
- `artifact::show_artifact`
- `artifact::get_artifact_workspace`

#### 呈现示例

`spawn_task_conversation`

- 调用摘要：`已派发任务：修复登录流程`
- 结果摘要：`任务已创建，执行助手：前端助手，任务会话 ID：1234`

`ask_user_question`

- 调用摘要：`正在向用户发起确认：是否继续发布？`
- 结果摘要：`用户已回答：继续发布`

`preview_file`

- 调用摘要：`正在预览文件：docs/spec.md`
- 结果摘要：`已打开文件预览：docs/spec.md`

`read_file`

- 调用摘要：`正在读取文件：src/main.rs`
- 结果摘要：`已读取文件：src/main.rs（共 240 行）`

`execute_bash`

- 调用摘要：`正在执行命令：npm run build`
- 结果摘要：`命令执行完成：退出码 0`

### 8.4 fallback formatter

对于未知工具，fallback 至统一格式：

- 调用：`正在调用工具 {server}::{tool}`
- 结果：`工具 {server}::{tool} 已返回结果`

如果参数较短，可附加一行精简参数摘要；如果结果可解析为文本，可截断展示前若干字符。

### 8.5 未来插件扩展方式

建议保留两种扩展机制：

#### 声明式扩展

插件 / provider 提供 manifest：

```json
{
  "external_presenters": [
    {
      "channel": "feishu",
      "tool": "my_server__deploy_app",
      "call_template": "正在发布应用：{{app_name}}",
      "result_template": "发布结果：{{status}}"
    }
  ]
}
```

适合大多数简单工具。

#### 编程式扩展

插件 / provider 注册运行时 presenter：

- Rust 内建 provider 可直接注册 trait 实现。
- 未来插件系统可暴露一个受控 API，让插件提供 JS/JSON 转换器，再由主进程执行受限渲染。

建议先做声明式，后补编程式。

## 9. 飞书渠道渲染层设计

### 9.1 初期输出形态

一期先继续使用飞书文本消息。

将 `RelayProjection.segments` 渲染为清晰文本，例如：

```text
总管家：
我已经为这件事拆分了两个子任务，并开始执行。

工具调用：
- 已派发任务：检查构建失败原因
- 已派发任务：整理修复方案

当前结论：
第一个任务已经完成，第二个任务正在收尾。
```

### 9.2 渲染原则

- 去除内部标记、JSON、注释。
- 合并同一条本地消息里的多个 segment，避免碎片化刷屏。
- 对长文本做截断与摘要。
- 对工具结果优先输出“动作 + 结论”，不要原样转储参数和长结果。

## 10. 外发调度模型

### 10.1 不再依赖单次 settle 后一次性挑消息

建议新增统一调度器，例如：

- `ExternalChannelRelayService`
- `FeishuRelayAdapter`

调度器负责：

- 监听总管家会话的新消息事件，或定期对 scope 做补偿扫描。
- 找出尚未外发的候选消息。
- 逐条投影并发送。
- 记录投递结果。

### 10.2 推荐的一期实现

一期可采用“补偿扫描 + 明确 cursor”的方式，降低改动风险：

1. 入站/本地发起时创建 scope。
2. 在关键时机触发 `flush_scope_to_channels(scope_id)`。
3. `flush_scope_to_channels` 查询 `local_message_id > last_delivered_local_message_id` 的候选消息。
4. 按顺序做投影、渲染、发送、落库。
5. 更新 cursor。

这比现在的“拿一条最新 assistant 文本”稳得多，也利于后续过渡到事件驱动。

### 10.3 为什么能解决“后续更重要的回复没有发出去”

因为新模型不再假设“本次只会有一个最终消息”，而是：

- 逐条扫描本次 scope 中新增的所有外发候选消息。
- 只要消息符合外发条件且尚未投递，就按顺序发出去。

这样：

- 工具调用摘要可以发。
- 后续真正总结也会继续发。
- 子任务回流后的新结果也不会被前一条消息覆盖掉。

## 11. 数据模型建议

### 11.1 保留现有表

保留：

- `external_channel_message_link`

它继续负责保存“本地消息与外部消息 ID 的映射”。

### 11.2 新增表

建议新增：

#### `external_channel_scope`

- scope 维度，用来标记一次回合与外部目标

#### `external_channel_delivery`

- 单条本地消息到外部渠道的实际投递记录

#### 可选：`external_channel_primary_target`

- 每个 conversation 当前默认回传目标

如果不想新增第三张表，也可以把 primary target 合并进 `external_channel_scope` 中，通过最近激活 scope 计算得出。

## 12. 配置改动设计

### 12.1 前端默认值

在 `FeatureAssistantConfig.tsx` 的 experimental 默认值中新增：

- `butler_feishu_only_reply_feishu_originated: "false"`

同时加入 feature config 读取/写回映射。

### 12.2 前端表单

在 `ExperimentalConfigForm.tsx` 的飞书机器人接入区域新增一个 `Switch`：

- 标题：`是否只返回飞书请求的响应到飞书`
- 说明：
  - 关闭时：总管家对话默认会同步到 AIPP 与飞书
  - 开启时：只有飞书发起的请求及其响应会回到飞书，AIPP 发起的内容仅保留在 AIPP

建议放置位置：

- `接收单聊 / 接收群聊 / @ 或回复后才处理群消息` 之后
- `allowed_open_ids / allowed_chat_ids` 之前

### 12.3 后端配置读取

在 `load_runtime_config(...)` 中新增字段：

- `only_reply_feishu_originated: bool`

配置键读取：

- `butler_feishu_only_reply_feishu_originated`

## 13. 实施顺序建议

### Phase 1：可读化与稳定外发

- 引入 `RelayProjection`
- 引入 `ToolPresentationRegistry`
- 实现 builtin formatter 第一批
- 把飞书当前回发逻辑改成“扫描全部新增可见消息并逐条投递”
- 禁止原始 MCP 注释直接出站

### Phase 2：策略与 scope

- 新增 `butler_feishu_only_reply_feishu_originated`
- 新增 scope / delivery 持久化
- 支持 AIPP 来源内容按主飞书目标镜像

### Phase 3：插件扩展

- 提供 provider/plugin 声明式 formatter 扩展
- 视需要增加飞书卡片渲染

## 14. 风险与注意事项

### 14.1 多群/多用户泄露风险

Butler 主会话是共享上下文。若默认把 AIPP 内容广播给多个飞书线程，可能造成越权泄露。

因此一期必须限制为：

- 只回传到单一“主飞书回传目标”
- 不做隐式多目标 fan-out

### 14.2 工具结果过长

工具结果不应全文搬运到飞书。

需要：

- 摘要优先
- 超长截断
- 失败时强调错误结论而不是原始堆栈全文

### 14.3 去重与补偿

外部发送可能超时、重试、网络断开，因此必须有 delivery 级别去重与补偿扫描。

## 15. 验收标准

- 飞书中不再看到 `<!-- MCP_TOOL_CALL: ... -->` 原始注释。
- builtin MCP 工具在飞书中能显示明确、简洁、可读的调用/结果摘要。
- 一次 Butler 处理内出现多条可见消息时，飞书能按顺序收到全部应外发内容，而不是只收到一条。
- 默认配置下，存在主飞书目标时，AIPP 发起的总管家对话内容也会镜像到飞书。
- 开启“是否只返回飞书请求的响应到飞书”后，只有 Feishu 来源 scope 的内容会回到飞书。
- 所有内容依然完整保留在 AIPP 内部对话历史中。
