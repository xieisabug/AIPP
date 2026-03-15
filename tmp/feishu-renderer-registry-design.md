# 飞书渠道渲染注册表设计

## 1. 背景

当前总管家到飞书的回传链路已经具备基本可用性：

- 能按 relay scope 顺序回发新增消息，而不是只取最后一条 assistant 消息。
- 能把 `MCP_TOOL_CALL` 注释和部分工具结果转换成较可读的内容。
- 已经有“是否只返回飞书请求的响应到飞书”配置开关。

但当前的可读化实现仍然存在一个明显短板：

- 渲染逻辑集中写在 `src-tauri\src\external_channels\presentation.rs`
- builtin MCP 工具自己的 schema / description 在 `src-tauri\src\mcp\builtin_mcp\templates.rs`
- 工具定义与外部渠道呈现定义没有放在一起

这会导致几个问题：

- 新增一个 builtin 工具时，开发者很容易忘记同步补 Feishu 渲染。
- 渲染逻辑会持续堆积在中央模块，越来越难维护。
- 未来如果 plugin 想对某些工具注册飞书可读呈现，目前没有合适入口。

本设计文档的目标，是回答下面两个问题：

1. Feishu 渲染逻辑能否做成注册式体系，而不是继续硬编码在中心模块。
2. plugin 是否只要注册“飞书渠道模板”就够，还是需要更高一层的声明式扩展。

---

## 2. 结论摘要

结论是：

- **builtin 工具应改成注册式渲染，且注册点应尽量靠近各自工具定义。**
- **plugin 可以支持扩展，但第一阶段应采用声明式扩展，而不是任意 JS 函数执行。**
- **plugin 第一阶段“只注册飞书模板”是可行的最小方案，但不应该把总设计绑死在“模板字符串”上。**

更准确地说：

- 对当前需求，plugin 注册一份 `channel=feishu` 的模板声明，已经能覆盖很大一部分场景。
- 但从系统设计上，模板不应直接等同于“飞书 HTTP 原始消息体”。
- 更好的抽象应该是：plugin 注册的是“**渠道渲染声明**”，其中包含渠道、输出格式、适用工具、适用阶段、字段提取规则和模板内容。

这样既能满足当前 Feishu 文本/卡片场景，也不会限制以后扩展到其他渠道或其他 Feishu 消息类型。

---

## 3. 现状与可行性判断

## 3.1 当前 Feishu 发送能力并不只有纯文本

从当前 `src-tauri\src\feishu\mod.rs` 可以看到，飞书发送链路已经不是单一 text：

- 有 `reply_text_message(...)`
- 有 `reply_markdown_message(...)`
- 有 `build_feishu_markdown_card(...)`
- 有 `build_feishu_interactive_payload(...)`

这说明一个关键事实：

- 当前系统已经具备 **文本** 与 **交互卡片** 两种发送形态的基础能力。

因此，如果现在把插件扩展定义死为“插件只能注册一个 text 模板”，短期虽然简单，但会过早限制后续能力。

更合理的理解应该是：

- 插件第一阶段可以只注册 **Feishu 文本 / Markdown 模板**
- 但系统抽象层应该保留 `text / markdown / interactive_card` 这类更高一层的输出格式概念

## 3.2 plugin 当前是前端运行时，不适合承担后端飞书渲染执行

当前 `src\types\plugin.d.ts` 暴露给 plugin 的能力主要是：

- 注册主题
- 注册 markdown tag
- 调用通用 `invoke`
- 访问一些前端 UI Kit

这说明当前 plugin 体系本质上偏向：

- 前端 UI 扩展
- 前端运行时逻辑

而飞书回传发生在：

- Rust 后端
- 没有前端窗口依赖
- 可能在某个聊天窗口未打开时也照样发生

这意味着：

- **不能把飞书渲染建立在“执行前端 plugin JS”这个前提上**

否则会立刻遇到以下问题：

- 后端投递时前端 runtime 可能未加载
- 某些窗口未打开时 plugin 根本没有执行环境
- plugin JS 异常会影响后端外发链路
- 安全边界会变复杂
- 版本兼容和错误隔离会很难做

因此，plugin 的扩展方式应该优先选择：

- **后端可读**
- **可校验**
- **可缓存**
- **无需执行插件自定义代码**

也就是声明式扩展，而不是代码式扩展。

---

## 4. 为什么“只注册飞书模板”还不够

“plugin 只要注册一下对应飞书渠道的模板”这个想法，本身是对的，但有一个前提：

- 模板注册必须建立在稳定的输入数据模型之上。

如果没有这个前提，模板会很脆弱。

例如今天很多工具结果仍然是字符串协议：

```text
Tool execution completed:

Tool Call ID: ...
Tool: ...
Server: ...
Parameters: ...
Result:
...
```

如果插件模板只是直接从这段字符串里做插值，会出现几个问题：

- 字段提取逻辑分散
- 一旦字符串格式变化，所有模板都可能失效
- 调用阶段和结果阶段的数据不统一
- 某些工具调用信息在 `MCP_TOOL_CALL` 注释里，某些在 `tool_result` 字符串里，数据来源不一致

所以更合理的顺序是：

1. 先定义统一的 `ToolInvocationSnapshot / RelayProjection`
2. 再让模板或渲染器从这个结构化对象取值

换句话说：

- **“模板”应该面向结构化数据，而不是直接面向原始消息字符串。**

---

## 5. 推荐抽象：三层结构

推荐把外部渠道渲染抽象成三层：

## 5.1 Projection 层

作用：

- 从内部消息中提取结构化外发片段

建议产物：

```rust
struct RelayProjection {
    local_message_id: i64,
    message_type: String,
    segments: Vec<RelaySegment>,
}

enum RelaySegment {
    Text(TextSegment),
    ToolCall(ToolCallSegment),
    ToolResult(ToolResultSegment),
    Notice(NoticeSegment),
}
```

其中 `ToolCallSegment / ToolResultSegment` 应至少包含：

- `server_name`
- `tool_name`
- `call_id`
- `phase`
- `parameters`
- `result_summary_raw`
- `success`

Projection 层的职责只有一个：

- 把内部消息转换成稳定的结构化输入

它不负责 Feishu 文案，不负责最终模板拼接。

## 5.2 Registry 层

作用：

- 根据 `(channel, server, tool, phase)` 找到最匹配的渲染声明

建议匹配键：

- `channel`，例如 `feishu`
- `server_name`，例如 `aipp:operation`
- `tool_name`，例如 `read_file`
- `phase`，例如 `tool_call` / `tool_result`
- `format`，例如 `text` / `markdown`

Registry 的来源应支持三类：

1. 系统默认 fallback
2. builtin MCP 工具自带声明
3. plugin 声明式扩展

优先级建议：

1. plugin 显式声明
2. builtin 工具声明
3. server 级通用声明
4. 全局 fallback

## 5.3 Channel Renderer 层

作用：

- 把 Registry 返回的模板/声明与 Projection 数据组合成最终可发送内容

对 Feishu 来说，建议支持的输出格式：

- `text`
- `markdown`
- `interactive_card`

注意这里的 `interactive_card` 仍然可以继续用当前已有的：

- Markdown -> Feishu card 的转换链路

也就是说，第一阶段完全不需要让 plugin 直接提供飞书原始 card JSON。

---

## 6. builtin 工具如何注册

## 6.1 推荐方向

把 builtin 工具的外部渠道渲染声明尽量放到工具定义附近，而不是继续堆在 `external_channels\presentation.rs`。

理想形式是扩展 `BuiltinToolInfo`，增加类似字段：

```rust
pub struct BuiltinToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub external_presentations: Vec<ExternalToolPresentationSpec>,
}
```

其中：

```rust
pub struct ExternalToolPresentationSpec {
    pub channel: String,           // feishu
    pub phase: String,             // tool_call / tool_result
    pub format: String,            // text / markdown
    pub template: String,          // 模板正文
    pub field_rules: Vec<FieldRule>,
}
```

好处很明确：

- 工具 schema、description、外部呈现定义放在同一个地方
- 新增工具时不容易漏掉对应的 Feishu 呈现
- builtin 和 plugin 最终都可以收敛到同一套注册表结构

## 6.2 builtin 仍然允许保留少量代码式 formatter

不是所有工具都适合纯模板。

例如：

- `spawn_task_conversation`
- `ask_user_question`
- `preview_file`

这些工具可能更适合专门的 formatter，因为它们会：

- 读取复杂参数
- 根据结果状态生成更自然的文案
- 可能有多段输出

因此 builtin 层建议支持两种注册方式：

1. **声明式模板**
2. **已命名 formatter**

例如：

```rust
pub enum ExternalPresentationResolver {
    Template(ExternalToolPresentationSpec),
    FormatterKey(String),
}
```

其中 `FormatterKey` 仍由后端内置实现，例如：

- `builtin.agent.spawn_task_call`
- `builtin.ui.ask_user_result`

这样可以兼顾：

- 大部分简单工具直接写模板
- 少量复杂工具继续用专门 formatter

---

## 7. plugin 如何扩展

## 7.1 第一阶段建议：声明式扩展

plugin 第一阶段不建议注册自定义 JS 函数，而建议注册一份 manifest 中的声明，例如：

```json
{
  "externalChannelRenderers": [
    {
      "channel": "feishu",
      "server": "plugin:my_tools",
      "tool": "create_ticket",
      "phase": "tool_call",
      "format": "markdown",
      "template": "正在创建工单：**{{parameters.title}}**"
    },
    {
      "channel": "feishu",
      "server": "plugin:my_tools",
      "tool": "create_ticket",
      "phase": "tool_result",
      "format": "markdown",
      "template": "工单创建成功：`{{result.ticket_id}}`"
    }
  ]
}
```

这个设计的优点是：

- 后端可以直接读取
- 可以做 schema 校验
- 不需要执行插件代码
- 不依赖前端窗口存活
- 出问题时容易 fallback

## 7.2 plugin 只注册 Feishu 模板，是否足够

### 一期结论

足够。

如果当前目标只是：

- 让 plugin 的某些工具在飞书里不要出现生硬 fallback 文案

那么 plugin 注册：

- `channel = feishu`
- `format = text 或 markdown`
- `phase = tool_call / tool_result`
- `template`

已经能满足大部分实际场景。

### 但不建议把协议定义成“只有 Feishu 模板”

原因是：

- 未来可能还有别的渠道
- Feishu 本身也不止 text，一定还会继续用 markdown/card
- 某些工具以后可能需要按渠道差异输出不同结构

所以建议设计成：

- “外部渠道渲染声明”

而不是：

- “飞书模板字符串”

换句话说，Feishu 模板可以是一期的主要落地形态，但不应该成为抽象层名字。

---

## 8. 推荐的声明模型

推荐把 plugin 和 builtin 统一到一套声明结构：

```rust
struct ExternalRendererSpec {
    id: String,
    channel: String,
    selector: RendererSelector,
    phase: RendererPhase,
    output: RendererOutput,
    priority: i32,
}

struct RendererSelector {
    server_name: Option<String>,
    tool_name: Option<String>,
    conversation_kind: Option<String>,
}

enum RendererPhase {
    ToolCall,
    ToolResult,
    Message,
}

enum RendererOutput {
    Template {
        format: OutputFormat,
        body: String,
    },
    FormatterKey {
        key: String,
    },
}

enum OutputFormat {
    Text,
    Markdown,
    InteractiveCard,
}
```

这个结构的意义是：

- builtin 与 plugin 使用同一套注册表
- plugin 用 `Template`
- builtin 可用 `Template` 或 `FormatterKey`

---

## 9. 模板变量从哪里来

模板可用变量建议不要直接暴露内部消息原文，而是暴露经过 Projection 规范化后的字段：

```json
{
  "tool": {
    "server_name": "aipp:operation",
    "tool_name": "read_file",
    "call_id": "call_xxx",
    "success": true
  },
  "parameters": {
    "file_path": "E:\\workspace\\rust\\aipp\\README.md"
  },
  "result": {
    "text": "前 50 行内容 ...",
    "summary": "已读取 README.md"
  },
  "message": {
    "local_message_id": 1234
  }
}
```

这样模板只负责展示，不负责解析原始协议。

---

## 10. 需要先解决的问题

如果要真正落地注册式渲染，当前至少有 4 个技术问题需要先处理。

## 10.1 tool result 还不够结构化

现在很多 tool result 仍然是字符串协议，靠解析：

- `Tool execution completed:`
- `Tool:`
- `Server:`
- `Parameters:`
- `Result:`

这会让注册式渲染天然脆弱。

建议后续逐步引入更稳定的数据来源，例如：

- 优先读取结构化 `tool_calls_json`
- 为 tool_result 建一个更标准的投影提取器
- 仅在缺结构化数据时才 fallback 到字符串解析

## 10.2 builtin 模板文件目前不承载渠道渲染元数据

现在 `BuiltinToolInfo` 只管工具定义，不管外部渠道呈现。

要实现“各工具自己注册飞书渲染”，需要改动 builtin 模板结构。

## 10.3 plugin manifest 目前没有外部渠道渲染声明入口

当前 plugin API 有：

- `registerTheme`
- `registerMarkdownTag`

但没有：

- `registerExternalRenderer`

更稳妥的路径不是直接加 runtime API，而是先加 manifest/schema 入口，由后端加载。

## 10.4 卡片与文本的选择策略需要统一

当前 Feishu 已经支持 text 和 interactive card。

注册表落地时需要统一约定：

- `format = markdown` 时是否总尝试转 card
- card 构建失败时是否统一回退 text
- 不同 renderer 是否能声明“只允许 text”

建议策略是：

- `text`：直接文本发送
- `markdown`：优先尝试转 interactive card，失败回退 text
- `interactive_card`：要求直接生成 card payload 或 card-ready 中间结构

---

## 11. 推荐实施顺序

## 阶段一：整理抽象，不改 plugin

- 定义统一 `RelayProjection`
- 定义统一 `ExternalRendererSpec`
- 把当前 `presentation.rs` 中的 hardcode presenters 迁移为“registry + fallback”的内部结构

这一阶段仍然可以先只由 Rust 内置注册，不开放 plugin。

## 阶段二：builtin 工具分散注册

- 扩展 `BuiltinToolInfo`
- 让各 builtin 工具定义附近声明 Feishu 渲染规则
- 中央模块只保留 registry 装配与 fallback

## 阶段三：plugin 声明式扩展

- 为 plugin 增加 manifest 字段，如 `externalChannelRenderers`
- 后端启动或插件刷新时读取并注册
- 先只支持 `text / markdown`

## 阶段四：更复杂输出

- 视需求增加 `interactive_card`
- 对高价值工具增加 richer formatter
- 如有必要，再评估是否开放更高级但受限的 backend-side formatter 插件机制

---

## 12. 最终建议

如果只看近期价值，我建议：

- **先做最基础的声明式扩展**
- **plugin 第一版只支持 Feishu 的 `text / markdown` 模板注册**

这是最稳、投入最小、也最符合当前不确定需求状态的方案。

但在系统设计上，还是应该保留更通用的名字和分层：

- 不把它命名成“飞书模板系统”
- 而是命名成“外部渠道渲染注册表”

原因很简单：

- Feishu 只是当前第一个外部渠道
- 当前 Feishu 实际也已经不只有 text
- 一旦抽象层名字过窄，后续扩展会反复返工

因此推荐的最终方向是：

- **抽象层：外部渠道渲染注册表**
- **一期落地：Feishu 的声明式文本 / Markdown 模板扩展**
- **builtin：逐步迁移到工具定义附近注册**
- **plugin：先声明式，不执行自定义 JS**

这个方向既能解决当前维护问题，也不会把未来扩展路径堵死。
