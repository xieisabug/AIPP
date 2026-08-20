# AIPP 插件平台统一设计方案

## 1. 背景与问题

AIPP 当前已经有一套可运行的插件机制，并且这一轮改造已经补上了不少基础设施：

- 现有 JS 插件体系可用于主题、Markdown、Bang、助手类型、插件中心 UI 等前端能力。
- Rust 侧已经具备统一的 HookBus 基础，可以让插件介入 Chat / Tool 等关键生命周期。
- 运行时层已经不再局限于 JS，开始支持 JS / WASM / process 三类插件形态。
- `plugin.db` 中已经引入 runtime、hook 注册、hook audit 等注册表能力。

但如果插件系统想真正承担“用户自己扩展产品能力”的目标，仅有 Hook 仍然不够。

如果一个高级插件的落地仍然需要核心代码每次都增加一个专用 API，例如：

- 做统计要等 `usageApi`
- 做助手表单扩展要等 `assistantFormApi`
- 做多助手讨论要等内置 `roundtableApi`

那么这套系统本质上仍然只是“生命周期扩展层”，而不是“能力平台”。

因此，统一版设计的核心结论是：

> **AIPP 的插件系统应当由两层组成：**
>
> 1. **Hook 层**：负责在 Rust 主流程中拦截、改写、阻断、观察生命周期。
> 2. **Capability 层**：负责向插件开放通用的数据、存储、UI、动作和事件能力。

只有这两层一起存在，插件才真正能做产品级扩展。

## 2. 统一目标

统一后的插件平台要同时满足以下目标：

1. **后端统一主导生命周期**
   - 所有关键流程由 Rust 宿主统一触发 Hook。
   - 前端不再是唯一入口，Butler、Feishu、定时任务、MCP 续写、Artifact AI、ACP 等入口都能进入同一链路。

2. **支持多运行时插件**
   - JS：偏 UI、主题、Markdown、面板、轻交互。
   - WASM：偏 Rust 插件、生命周期 Hook、审计、策略控制。
   - process：偏长时任务、本地系统集成、浏览器控制、企业系统同步。
   - native：仅面向官方或受信插件，不作为公开生态主路径。

3. **插件不再依赖一个个专用 API**
   - 插件应能查询本地数据。
   - 插件应有自己的结构化存储。
   - 插件应能向明确区域注册 UI。
   - 插件应能调用稳定的 App Action API。

4. **保持现有 JS 插件兼容**
   - 现有 theme / markdown / bang / assistantType 插件不应被破坏。
   - 旧插件通过兼容适配继续运行。
   - 新插件优先使用 manifest v3 + capability / hook 方式。

5. **安全、可见、可审计**
   - 高风险权限必须显式声明。
   - 高风险 Hook 必须带权限、超时、错误策略和审计。
   - 插件中心必须能看到 runtime、permissions、hooks、审计和错误。

## 3. 核心设计结论

统一版设计有 7 条关键结论：

1. **HookBus 必须在 Rust 后端，而不是只在前端。**
2. **Rust 插件默认走 WASM，而不是动态库。**
3. **JS 插件继续保留，但定位更偏 UI 与 contribution。**
4. **Capability API 才是插件系统的主线，Hook 只是其中一层。**
5. **应用数据库对插件默认只开放只读查询，写操作必须通过受控动作 API。**
6. **高风险插件能力必须用户可见，不能静默降级。**
7. **assistantType 要逐步从“前端自定义运行逻辑”迁移到“assistant contribution + lifecycle hook + app action”。**

## 4. 总体架构

```text
plugin.json
  |
  v
插件注册表 / Registry
  |
  +--> 运行时层：JS / WASM / process / native(trusted)
  |
  +--> HookBus：生命周期拦截与审计
  |
  +--> Capability 层
        |
        +--> Data API：应用数据库只读查询
        +--> Storage API：插件私有 SQLite
        +--> UI Contribution API：视图、表单、动作、面板
        +--> App Action API：会话、消息、助手、artifact、窗口等动作
        +--> Events API：应用事件 / 插件事件
```

可以把它理解为：

- **HookBus** 负责“什么时候让插件介入”。
- **Capability API** 负责“插件拿到授权后能做什么”。

## 5. 运行时模型

### 5.1 JS Runtime

定位：

- 主题插件
- Markdown 渲染插件
- Bang 贡献插件
- 配置页 / Dashboard / 侧边栏 / 面板插件
- 低风险、轻量级 Hook

特点：

- 继续使用现有 `PluginRuntime`
- 通过 `onPluginLoad(systemApi)` 获取宿主能力
- 适合有窗口的前端环境

不适合：

- 后端关键阻断逻辑
- 无窗口环境中的核心主流程拦截
- 高可靠审批链

### 5.2 WASM Runtime

这是 Rust 插件的默认运行时。

适合：

- Prompt Guard
- Tool 参数拦截
- Tool 结果脱敏
- 对话自动标签
- 审计和策略类插件

优点：

- 跨平台分发简单
- 不直接把插件崩溃传播到主进程
- 权限和 ABI 可控
- Rust 插件作者体验好

### 5.3 Process Runtime

适合高级插件：

- 文件索引器
- 本地数据库同步器
- 企业系统对接
- 浏览器自动化
- 长连接服务
- 重后台任务

特点：

- 协议建议为 JSON-RPC over stdio
- 隔离性强
- 进程生命周期更复杂

### 5.4 Native Runtime

仅用于官方或受信插件。

不作为公开插件生态的主路径，原因包括：

- Rust ABI 不稳定
- 崩溃会直接影响主进程
- 跨平台签名、依赖、分发更复杂

## 6. Manifest 设计

统一后的 manifest v3 要覆盖 runtime、activation、permissions、contributions、hooks。

```json
{
  "id": "prompt-guard",
  "code": "prompt-guard",
  "name": "Prompt Guard",
  "version": "0.1.0",
  "description": "拦截危险 Prompt，并在工具调用前做审批",
  "author": "AIPP",
  "runtime": {
    "type": "wasm",
    "entry": "dist/plugin.wasm"
  },
  "pluginTypes": ["applicationType"],
  "activationEvents": [
    "onHook:chat.beforeSend",
    "onHook:tool.beforeCall"
  ],
  "permissions": [
    "hook.chat.beforeSend",
    "hook.tool.beforeCall",
    "conversation.read",
    "plugin.storage"
  ],
  "contributions": {
    "hooks": [
      {
        "name": "chat.beforeSend",
        "kind": "guard",
        "priority": 100,
        "timeoutMs": 1500,
        "failurePolicy": "block"
      },
      {
        "name": "tool.beforeCall",
        "kind": "filter",
        "priority": 100,
        "timeoutMs": 3000,
        "failurePolicy": "block"
      }
    ],
    "views": [],
    "assistantFormFields": [],
    "bangs": [],
    "themes": [],
    "markdownTags": []
  }
}
```

### 6.1 兼容规则

旧插件：

```json
{
  "pluginTypes": ["assistantType"],
  "entry": "dist/main.js"
}
```

等价理解为：

```json
{
  "runtime": {
    "type": "js",
    "entry": "dist/main.js"
  },
  "activationEvents": ["onStartup:ui"],
  "contributions": {
    "legacyAssistantType": true
  }
}
```

这样旧插件不需要立刻迁移。

## 7. Hook 层设计

## 7.1 Hook 分类

```rust
pub enum HookKind {
    Event,
    Filter,
    Guard,
}
```

### Event

只观察，不修改上下文。

执行规则：

- 可并行执行
- 默认不阻塞主流程
- 失败只记录审计

适合：

- 日志统计
- 外部同步
- token 使用量记录
- 响应完成通知

### Filter

输入上下文，返回修改后的上下文。

执行规则：

- 顺序执行
- 按 `priority` 从小到大
- 同优先级按 plugin code 排序
- 后一个插件接收前一个插件修改后的上下文

适合：

- 改 prompt
- 改 system prompt
- 改 tool 参数
- 对 tool result 脱敏
- 自动追加引用和来源

### Guard

决定流程是否允许继续。

执行规则：

- 顺序执行
- 任意插件返回 `block` 即中断
- 可返回 `approvalRequired`

适合：

- 敏感内容拦截
- 危险工具审批
- 禁止某助手访问某类 MCP
- Feishu 外部消息安全策略

## 7.2 Hook 返回值

统一返回结构：

```json
{
  "action": "continue",
  "patch": {},
  "message": null,
  "metadata": {}
}
```

动作语义：

- `continue`：继续，可附加 metadata
- `replace`：用返回上下文替换当前上下文
- `patch`：以 merge patch 或 JSON patch 方式修改上下文
- `block`：阻止流程
- `approvalRequired`：挂起流程等待用户确认

首期建议先把 `continue / replace / patch / block` 做稳定，审批链可以后续接入完整 UI。

## 7.3 第一批生命周期 Hook

### Chat 生命周期

```text
chat.beforeSend
chat.afterUserMessageCreated
chat.beforeBuildContext
chat.afterBuildContext
chat.beforeModelRequest
chat.onResponseStarted
chat.onResponseChunk
chat.beforeResponsePersist
chat.afterResponseCompleted
chat.onError
```

重点说明：

- `chat.beforeSend`
  - 适合改用户 prompt、调整 assistant、插入临时 skill、阻止发送
- `chat.beforeModelRequest`
  - 高风险 Hook
  - 适合最终模型上下文审计、补 system message、删除敏感片段、调整 tool 注入
- `chat.onResponseChunk`
  - 初期只做 event
  - 不允许修改 chunk
  - 必须低开销，可采样
- `chat.beforeResponsePersist`
  - 适合对最终回复脱敏、自动追加来源、做格式规范化
- `chat.afterResponseCompleted`
  - 适合自动总结、自动打标签、同步外部系统、写插件私有数据

### Tool 生命周期

```text
tool.beforeCreateCall
tool.afterCreateCall
tool.beforeCall
tool.afterCall
tool.onError
tool.beforeResultMessage
```

重点说明：

- `tool.beforeCall`
  - 修改参数
  - 阻止危险工具
  - 请求用户确认
  - 改超时
- `tool.afterCall`
  - 脱敏结果
  - 压缩长结果
  - 生成结构化摘要
  - 同步执行结果
- `tool.beforeResultMessage`
  - 改写最终回填给模型的 tool result message

### Conversation 生命周期

```text
conversation.beforeCreate
conversation.afterCreate
conversation.beforeTitleGenerate
conversation.afterTitleGenerate
conversation.beforeFork
conversation.afterFork
conversation.beforeDelete
conversation.afterDelete
```

### Assistant 生命周期

```text
assistant.beforeRun
assistant.afterRun
assistant.beforeConfigSave
assistant.afterConfigSave
```

### Artifact 生命周期

```text
artifact.beforeCreate
artifact.afterCreate
artifact.beforePreview
artifact.afterPreview
artifact.beforeExecute
artifact.afterExecute
```

### Butler / Feishu 生命周期

```text
butler.beforeTaskDispatch
butler.afterTaskDispatch
butler.beforeResultCallback
butler.afterResultCallback
feishu.beforeInboundMessage
feishu.afterInboundMessage
feishu.beforeOutboundRender
feishu.afterOutboundSend
```

## 8. Capability 层设计

Hook 负责拦截流程，但它不能替代通用能力开放层。  
统一设计中的 Capability 层至少包含以下 5 类能力。

## 8.1 Data API

### 目的

允许插件直接查询 AIPP 本地数据，而不是等待一个个专用 API。

### 前端 API

```ts
const result = await systemApi.data.query({
  database: "conversation",
  sql: `
    SELECT conversation_id, COUNT(*) AS count
    FROM message
    WHERE created_time >= ?
    GROUP BY conversation_id
    ORDER BY count DESC
    LIMIT 20
  `,
  params: ["2026-01-01T00:00:00Z"]
});
```

### 支持的数据库

| 名称 | 文件 | 权限 |
| --- | --- | --- |
| `conversation` | `conversation.db` | `data.read.conversation` |
| `assistant` | `assistant.db` | `data.read.assistant` |
| `llm` | `llm.db` | `data.read.llm` |
| `mcp` | `mcp.db` | `data.read.mcp` |
| `plugin` | `plugin.db` | `data.read.plugin` |
| `system` | `system.db` | `data.read.system` |
| `artifacts` | `artifacts.db` | `data.read.artifacts` |

`scheduled_task` 当前数据位于 `conversation.db`，归 `data.read.conversation` 覆盖。

### 安全规则

- 只允许 `SELECT` / `WITH`
- 必须是只读 statement
- 拒绝多语句
- 宿主限制 `maxRows`
- BLOB 返回 `base64:...`
- 参数以 JSON 标量为主，复杂结构序列化成字符串
- 后端根据 plugin id + manifest 权限二次校验

统一原则：

> **应用数据库不向插件直接开放写权限。**

## 8.2 插件私有存储

每个插件拥有一个独立 SQLite：

```text
<app-data>/plugin_data/<plugin-code>.db
```

能力：

- `systemApi.storage.query`
- `systemApi.storage.execute`
- `systemApi.storage.schema`

权限：

- `plugin.storage`

适合：

- Dashboard 缓存
- 同步游标
- 插件自定义实体
- 实验数据
- 本地索引

## 8.3 UI Contribution API

插件不应该只能挂到插件中心。统一版设计要支持明确的 UI zone：

| 位置 | 典型用途 |
| --- | --- |
| `config.plugins` | 插件设置和诊断 |
| `config.analytics` | Usage Dashboard |
| `assistant.form` | 助手字段扩展 |
| `conversation.sidebar` | 多助手编排、上下文分析、统计面板 |
| `conversation.message.action` | 消息操作按钮 / 菜单 |
| `artifact.toolbar` | artifact 工具栏动作 |

建议 manifest：

```json
{
  "contributions": {
    "views": [
      {
        "id": "roundtable",
        "title": "多助手讨论",
        "location": "conversation.sidebar"
      }
    ],
    "assistantFormFields": [
      {
        "key": "hiddenFirstTurnContext",
        "label": "首轮隐藏上下文",
        "type": "textarea",
        "scope": "assistant"
      }
    ]
  }
}
```

## 8.4 App Action API

应用状态写入必须走稳定 API，而不是让插件直接写应用 DB。

目标动作：

```ts
systemApi.conversations.list(...)
systemApi.conversations.create(...)
systemApi.messages.append(...)
systemApi.messages.updateMetadata(...)
systemApi.assistants.run(...)
systemApi.artifacts.create(...)
systemApi.windows.open(...)
```

权限：

- `conversation.read`
- `conversation.write`
- `message.write`
- `message.metadata.write`
- `assistant.run`
- `artifact.write`
- `window.open`

## 8.5 Events API

除 Hook 外，插件之间和宿主之间还需要事件能力：

- 应用广播事件
- 插件自定义事件
- 视图刷新事件
- 任务状态变化事件

这层可以用来减少插件之间对隐式全局状态的依赖。

## 9. WASM / Process / Host API 设计

## 9.1 Rust WASM 插件目录

```text
plugin/prompt-guard/
├── plugin.json
├── Cargo.toml
├── src/
│   └── lib.rs
└── dist/
    └── plugin.wasm
```

## 9.2 Rust 插件作者理想写法

```rust
use aipp_plugin_sdk::*;

#[aipp_plugin]
struct PromptGuard;

#[hook("chat.beforeSend")]
fn before_send(ctx: ChatBeforeSendContext) -> HookResult<ChatBeforeSendContext> {
    if ctx.prompt.contains("危险内容") {
        return HookResult::block("消息被 Prompt Guard 拦截");
    }

    let mut next = ctx;
    next.prompt = format!("请谨慎回答：\n{}", next.prompt);
    HookResult::replace(next)
}

#[hook("tool.beforeCall")]
fn before_tool_call(ctx: ToolBeforeCallContext) -> HookResult<ToolBeforeCallContext> {
    if ctx.server_name == "aipp:operation" && ctx.tool_name == "execute_bash" {
        return HookResult::approval_required("即将执行命令，请确认");
    }

    HookResult::continue_with(ctx)
}
```

## 9.3 初期 WASM ABI

初期使用 JSON ABI，优先把系统跑通：

```text
aipp_plugin_init() -> i32
aipp_plugin_handle_hook(ptr: i32, len: i32) -> i64
aipp_plugin_free(ptr: i32, len: i32)
```

输入：

```json
{
  "hook": "chat.beforeSend",
  "pluginCode": "prompt-guard",
  "context": {},
  "host": {
    "appVersion": "0.0.0",
    "schemaVersion": 1
  }
}
```

输出：

```json
{
  "action": "replace",
  "context": {},
  "message": null,
  "metadata": {}
}
```

优点：

- 实现快
- 调试简单
- JS / process runtime 也可复用同一协议

后续可升级到 WIT / component model。

## 9.4 Host API

WASM 插件不应直接访问 Tauri 或任意文件系统。宿主应只暴露最小能力集：

```rust
pub trait AippHostApi {
    fn get_plugin_data(&self, key: &str) -> Result<Option<String>>;
    fn set_plugin_data(&self, key: &str, value: Option<&str>) -> Result<()>;
    fn list_assistants(&self) -> Result<Vec<AssistantSummary>>;
    fn get_conversation_messages(&self, conversation_id: i64) -> Result<Vec<MessageSummary>>;
    fn emit_log(&self, level: LogLevel, message: &str) -> Result<()>;
}
```

统一后的 host 能力应继续扩展到：

- `data.query`
- `storage.query / execute`
- conversation / message / assistant action
- 日志与事件

每个 host function 都必须做权限检查。

## 9.5 Process Plugin

manifest 示例：

```json
{
  "runtime": {
    "type": "process",
    "entry": "bin/my-agent.exe",
    "protocol": "jsonrpc-stdio"
  },
  "activationEvents": ["onHook:chat.afterResponseCompleted"]
}
```

协议示例：

```json
{
  "jsonrpc": "2.0",
  "id": "call-1",
  "method": "hook.handle",
  "params": {
    "hook": "chat.afterResponseCompleted",
    "context": {}
  }
}
```

适合：

- 本地文件索引
- 企业系统同步
- 浏览器自动化
- 长连接能力

## 10. JS 插件系统的统一改造

### 10.1 继续保留的能力

现有 JS 插件能力继续保留：

- `onPluginLoad(systemApi)`
- `onAssistantTypeInit`
- `onAssistantTypeRun`
- `renderComponent`
- `systemApi.registerTheme`
- `systemApi.registerMarkdownTag`
- `systemApi.getData / setData`
- `systemApi.runAssistantText / runModelText`

### 10.2 新增 JS Hook API

```ts
interface SystemApi {
  hooks: {
    register<TContext>(
      name: string,
      handler: (context: TContext) => Promise<HookResult<TContext>> | HookResult<TContext>,
      options?: {
        kind?: "event" | "filter" | "guard";
        priority?: number;
        timeoutMs?: number;
      }
    ): Disposable;
  };
}
```

### 10.3 JS Hook 的边界

JS hook 适合：

- UI 相关 hook
- event hook
- 低风险 filter

不建议 JS 直接参与：

- `chat.beforeModelRequest`
- `tool.beforeCall` 的强阻断策略
- 无窗口环境中的关键主流程

### 10.4 assistantType 的迁移方向

新体系中，assistantType 建议拆成两部分：

1. **静态贡献**
   - `contributions.assistantTypes`
2. **运行时能力**
   - `assistant.beforeRun`
   - `chat.beforeSend`
   - `chat.beforeModelRequest`
   - `chat.afterResponseCompleted`

兼容策略：

- V1 `onAssistantTypeRun` 继续支持
- 新版 assistantType 优先走 contribution + hook + app action
- 插件中心明确标记 legacy

## 11. 数据库、注册表与插件中心

## 11.1 注册表表结构

`plugin.db` 统一维护以下核心能力：

- `Plugins`
- `PluginStatus`
- `PluginConfigurations`
- `PluginData`
- `PluginRuntime`
- `PluginHookRegistration`
- `PluginHookAuditLog`

其中：

- `PluginRuntime` 记录 runtime 类型、入口、协议、校验值
- `PluginHookRegistration` 记录插件声明的 Hook
- `PluginHookAuditLog` 记录执行结果、耗时、错误

原则：

- 默认不要记录完整 prompt / response
- 优先记录 hash、长度、是否修改、错误信息、reason

## 11.2 插件中心

插件中心应展示：

- runtime 类型：JS / WASM / Process / Native
- 权限列表
- hooks 列表
- contributions 列表
- 最近错误
- 最近 hook 调用耗时
- trusted 标记

高风险权限必须高亮：

- `hook.chat.beforeModelRequest`
- `hook.tool.beforeCall`
- `filesystem.write`
- `process.execute`
- `network.fetch`
- `native.trusted`

建议增加调试面板：

- 查看最近 100 条 hook audit
- 单独禁用某个 hook
- 注入测试 payload
- 查看插件运行日志

## 12. 现有插件与示例插件

## 12.1 现有插件如何迁移

### Benchmark 插件

更适合迁移到：

- `config.analytics` 或 `conversation.sidebar` 视图
- `plugin.storage` 做缓存
- `data.read.*` 查询实际统计数据

### 主题插件

继续使用：

- `registerTheme`
- 后续可纳入 `contributions.themes`

### Markdown 插件

继续使用：

- `registerMarkdownTag`
- 后续可纳入 `contributions.markdownTags`

### Bang 插件

继续使用：

- manifest contribution
- 内置 tool / MCP tool / plugin MCP tool 执行器

### AssistantType 插件

短期保持兼容，长期迁移到：

- `contributions.assistantTypes`
- `assistant.beforeRun`
- `chat.beforeSend`
- `chat.beforeModelRequest`
- `chat.afterResponseCompleted`

## 12.2 你之前提过的三个例子插件

### 例子 1：助手表单增加隐藏首轮上下文

需求：

- 在助手配置表单中新增 textarea / 开关等字段
- 在首轮 user message 后面自动追加一些隐藏文本
- 这些隐藏文本不直接显示在聊天消息 UI 中

所需能力：

- `assistant.form.extend`
- `plugin.storage`
- `hook.chat.beforeModelRequest`

建议实现方式：

1. 插件通过 `contributions.assistantFormFields` 增加字段。
2. 字段值按 assistant-scoped 配置存储。
3. `chat.beforeModelRequest` 判断是否首轮用户请求。
4. 插件 patch 最终发送给模型的 messages。
5. UI 原始 user message 保持不变。

这类插件**不能只靠 hook**，必须同时有：

- 表单扩展能力
- 配置存储能力
- 最终模型请求改写能力

### 例子 2：使用统计 Dashboard

需求：

- 提供多维度统计
- 支持图表
- 最好由插件自己定义统计维度，不依赖单独 usage API

所需能力：

- `data.read.conversation`
- `data.read.assistant`
- `data.read.llm`
- `data.read.mcp`
- `plugin.storage`
- `ui.view.register`

建议实现方式：

1. 插件贡献一个 `config.analytics` 页面或侧边栏面板。
2. 插件直接查询 conversation / message / assistant / model / tool_call 等数据。
3. 插件自行聚合和绘图。
4. 插件把高成本聚合结果缓存到私有存储。

这正是为什么统一设计中必须引入 **Data API**，而不是继续补一个个专用统计接口。

### 例子 3：多个助手在同一个对话里讨论

需求：

- 同一个 conversation 中有多个 assistant 围绕同一问题讨论
- 能控制参与者、轮数、策略
- 最终结果回写到当前 conversation 中

所需能力：

- `data.read.conversation`
- `conversation.read`
- `message.write`
- `message.metadata.write`
- `assistant.run`
- `ui.view.register`

建议实现方式：

1. 插件贡献一个 `conversation.sidebar` 面板。
2. 用户选择参与的助手与轮数。
3. 插件读取当前上下文。
4. 插件依次调用 `assistants.run(...)`。
5. 插件把输出写回当前 conversation。
6. 每条消息通过 metadata 标记 speaker / role / round。
7. 核心 UI 根据 metadata 做 speaker 展示。

这类插件说明：

> **如果没有 App Action API，仅靠 Hook 是做不出来的。**

因为它需要主动创建消息、写 metadata、调助手，而不是只拦截一次流程。

## 12.3 其他值得优先支持的插件类型

- Prompt Guard
- Tool Result Sanitizer
- Conversation Auto Tagger
- External Sync
- 审计合规插件
- MCP 安全策略插件

## 13. Rust 主流程接入点

### `ask_ai`

建议链路：

```text
ask_ai start
  -> chat.beforeSend
  -> assistant/slash/template processing
  -> collect skills/mcp
  -> initialize_conversation
  -> chat.afterUserMessageCreated
  -> build context
  -> chat.beforeModelRequest
  -> stream/non-stream call
```

### `handle_stream_chat`

建议链路：

```text
first response message created
  -> chat.onResponseStarted
each chunk
  -> chat.onResponseChunk
before final persist
  -> chat.beforeResponsePersist
after done
  -> chat.afterResponseCompleted
error
  -> chat.onError
```

### `handle_non_stream_chat`

与 stream 保持同名 Hook，只是没有 chunk 或只发一次 chunk event。

### `mcp::execution_api`

建议链路：

```text
create_mcp_tool_call
  -> tool.beforeCreateCall
  -> tool.afterCreateCall

execute_mcp_tool_call
  -> validate
  -> tool.beforeCall
  -> execute builtin/mcp/process
  -> tool.afterCall or tool.onError
  -> tool.beforeResultMessage
  -> continuation
```

## 14. 错误、性能与安全策略

## 14.1 错误策略

`failurePolicy`：

- `log`
- `skip`
- `block`
- `disableHook`

默认建议：

- event：`log`
- filter：按风险选择 `block` 或 `skip`
- guard：`block`

原则：

- 高风险 Hook 失败应明确报错
- 低风险 event 不影响用户主流程
- 所有错误在插件中心可见

## 14.2 性能策略

- Hook registry 启动时预加载
- Event hook 并行执行
- Filter / Guard 顺序执行
- 所有 Hook 带 timeout
- chunk 类 Hook 要支持采样和批量
- 审计异步写入

## 14.3 安全策略

- 权限最小化
- 沙箱优先
- 用户可见
- 审计优先
- 失败不静默降级

高风险能力必须单独提示：

- `hook.chat.beforeModelRequest`
- `hook.tool.beforeCall`
- `conversation.write`
- `message.write`
- `process.execute`
- `filesystem.write`
- `network.fetch`

## 15. 开发者体验

### 15.1 Rust SDK

新增：

```text
crates/aipp-plugin-sdk
```

提供：

- hook context 类型
- hook result 类型
- 宏：`#[aipp_plugin]`、`#[hook("...")]`
- manifest 生成辅助
- 本地测试工具

### 15.2 CLI / 模板

理想 CLI：

```bash
cargo aipp-plugin new prompt-guard
cargo aipp-plugin build
cargo aipp-plugin package
cargo aipp-plugin dev
```

如果暂时不做 CLI，至少提供模板：

```text
docs/plugin-template/rust-wasm-basic/
docs/plugin-template/rust-process-basic/
```

### 15.3 本地测试

```bash
cargo test
aipp-plugin-test --hook chat.beforeSend --payload fixtures/before_send.json
```

## 16. 分阶段实施计划

### Phase 0：规格冻结

- 定义 hook 命名规范
- 定义 manifest v3 schema
- 定义 `HookContext` / `HookResult` JSON ABI
- 明确旧插件兼容策略

### Phase 1：Rust HookBus

- 实现 `PluginHookBus`
- manifest 解析 `contributions.hooks`
- 支持 event / filter / guard
- 支持权限、timeout、failurePolicy、audit
- 首批接入：
  - `chat.beforeSend`
  - `chat.afterResponseCompleted`
  - `tool.beforeCall`
  - `tool.afterCall`

### Phase 2：WASM Rust Plugin

- 引入 `wasmtime`
- 实现 JSON ABI
- 实现 `aipp_plugin_sdk` 最小版本
- 支持 `runtime.type = "wasm"`

验证插件：

- Prompt Guard
- Tool Result Sanitizer
- Conversation Auto Tagger

### Phase 3：现有 JS 插件适配

- `PluginRuntime` 增加 `systemApi.hooks.register`
- theme / markdown / bang / assistantType 纳入 contribution registry
- 区分 legacy 与 v3 插件

### Phase 4：Data API 与插件私有存储

- 增加 `plugin_data_query`
- 增加 `plugin_data_schema`
- 增加 `plugin_storage_query / execute / schema`
- 前后端双重权限校验
- 暴露 `systemApi.data` 与 `systemApi.storage`

### Phase 5：UI Contribution Registry

- 支持 `contributions.views`
- 支持 `assistantFormFields`
- 建立前端 zone registry
- 支持插件中心之外的页面 / 面板挂载

### Phase 6：App Action API

- conversation / message / assistant / artifact / window action
- 增加 message metadata
- 为多助手插件铺路

### Phase 7：Process Plugin 与调试工具

- `runtime.type = "process"`
- JSON-RPC over stdio
- 进程生命周期、超时、重启
- 插件中心调试面板
- hook audit 可视化

### Phase 8：Native Trusted Plugin

- 仅对官方或受信插件开放
- 明确签名、ABI 版本、崩溃风险提示

## 17. 推荐优先级

最高优先级：

1. HookBus
2. Manifest hooks schema
3. `chat.beforeSend`
4. `tool.beforeCall`
5. WASM runtime MVP
6. Data API
7. Plugin private storage

中优先级：

1. `chat.beforeModelRequest`
2. `chat.afterResponseCompleted`
3. JS hook bridge
4. Plugin audit UI
5. UI contribution registry
6. assistant form extension

后续优先级：

1. App Action API
2. message metadata
3. Process plugin 生命周期
4. WIT component model
5. Trusted native plugin

## 18. 一个最小可行闭环

最小闭环应当同时覆盖 Hook 与 Capability：

```text
plugin.json 声明 wasm runtime + chat.beforeSend + plugin.storage
        |
        v
plugin_api 扫描 manifest，写入 PluginRuntime / PluginHookRegistration
        |
        v
ask_ai 调用 hook_bus.run_guard_filter("chat.beforeSend", ctx)
        |
        v
hook_bus 检查权限，加载 wasm plugin
        |
        v
plugin 返回 replace / block / continue
        |
        v
ask_ai 使用修改后的 prompt 或返回错误
        |
        v
PluginHookAuditLog 记录执行结果
        |
        v
插件可通过 storage 记录自己的状态
```

如果再补上 `data.read.conversation`，就可以进一步做出：

- 自动标签插件
- Usage Dashboard 插件
- 对话质量分析插件

## 19. 后续工程任务拆分建议

- `plugin::manifest`
- `plugin::hook_bus`
- `plugin::audit`
- `plugin::runtime::wasm`
- `plugin::runtime::process`
- `plugin::runtime::js_bridge`
- `aipp_plugin_sdk`
- `PluginCenterConfig` 调试面板
- `systemApi.data / storage`
- `contributions.views / assistantFormFields`
- `message.metadata`
- `assistants.run` / `messages.append`

## 20. 首阶段非目标

- 不开放对应用数据库的任意写权限
- 不建设远程插件市场和签名分发
- 不把 native 动态库作为公开插件主路径
- 不在核心中直接硬编码完整多助手产品 UI
- 不替换现有 JS 插件，保持兼容优先

