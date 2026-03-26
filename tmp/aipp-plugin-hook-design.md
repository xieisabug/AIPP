# AIPP 插件 Hook 系统设计草案

## 1. 背景

AIPP 当前的插件系统已经具备这些基础：

- 后端有完整的插件注册、启停、配置、数据存储能力，入口在 `src-tauri/src/api/plugin_api.rs`，存储在 `src-tauri/src/db/plugin_db.rs`。
- 前端有统一的 `PluginRuntime`，负责加载启用插件、注入 `SystemApi`、调用 `onPluginLoad(systemApi)`，见 `src/services/PluginRuntime.ts:145-307`。
- 现有 `SystemApi` 更偏“能力供给”，例如 `runAssistantText`、`getData/setData`、`registerTheme`、`registerMarkdownTag`、`invoke`，见 `src/types/plugin.d.ts:123-143`。
- Butler 的“重开主会话 / 清空上下文”已经是明确链路：前端调用 `reset_butler_main_conversation`，后端归档旧主会话、创建新主会话，并发出 `butler_main_reset` 事件，见 `src/windows/ButlerExperimentWindow.tsx:661-678`、`src-tauri/src/api/butler_api.rs:2444-2503`。

问题也很明确：**插件目前可以“被加载”，但不能“拦截生命周期”**。  
也就是说，插件还做不到：

- 发送消息前校验或改写消息
- 发送消息后做埋点或耗时记录
- 收到消息前改写输出、做脱敏或结构化处理
- 收到消息后写入自己的索引/日志/统计
- 在 Butler 清空对话时触发额外工作流，例如“自动生成日记”

这份文档的目标，就是在**不推翻现有插件设计**的前提下，为 AIPP 增加一套可控、可扩展、可观察的 Hook 能力。

---

## 2. 参考 Claude Code，但不照搬

Claude Code 的 hooks 设计里，有几件事非常值得借鉴：

1. **按生命周期事件建模**，而不是只给一个全局回调。
2. **支持 before / after 阶段**，并明确哪些阶段可以阻断、哪些阶段只能观察。
3. **有 matcher 概念**，不是所有 hook 都对所有事件生效。
4. **有统一输入 / 输出协议**，尤其是“allow / block / patch / continue”这类决定模型。
5. **有只读查看器思路**，便于调试当前系统里到底注册了哪些 hook。

但 AIPP 不应该直接复制 Claude Code 的 shell / HTTP / prompt hook 形态，原因是：

- AIPP 当前插件是**前端运行时加载的 JS/TS 插件**，不是宿主外部脚本。
- AIPP 插件已经依赖 `SystemApi` 提供能力，不适合再引入一套完全分离的脚本协议。
- AIPP 的关键链路跨越前端窗口、Tauri 命令、Rust 后端、AI 流式事件，最适合的是**宿主内统一 HookBus**，而不是单纯的 shell 命令执行器。

所以这里的建议是：

**借 Claude 的事件模型、matcher、决策语义、可观察性；实现方式则采用 AIPP 当前插件运行时风格。**

---

## 3. 设计目标

### 3.1 目标

- 让插件能在 AIPP 的关键生命周期节点挂接逻辑。
- 明确区分可变更阶段和只观察阶段。
- 兼容当前 `PluginRuntime + SystemApi` 架构。
- 支持 Butler 专属 Hook，尤其是“清空主对话”场景。
- 支持未来扩展到 MCP、外部渠道、任务系统、导出链路。

### 3.2 非目标

- 这版不引入任意 shell 命令 hook。
- 这版不开放插件直接改写宿主任意内部状态对象。
- 这版不让 hook 绕过权限模型直接调用危险能力。
- 这版不追求一次性覆盖所有事件，优先覆盖消息主链路和 Butler 特殊链路。

---

## 4. 总体思路

建议在现有插件系统上补一层 **Hook Runtime / HookBus**：

```text
PluginRuntime
  └── HookRegistry
        ├── register(pluginCode, hookDefinition)
        ├── unregister(pluginCode, hookId)
        ├── dispatchBefore(event, payload)
        ├── dispatchAfter(event, payload)
        └── listRegisteredHooks()

Chat / Butler / MCP / Conversation runtime
  └── 在关键生命周期点调用 HookBus
```

### 核心原则

1. **插件在 `onPluginLoad(systemApi)` 中注册 hook**  
   保持和当前插件初始化方式一致，不另起一套体系。

2. **before hook 串行执行，after hook 可并行执行**  
   `before` 需要处理 patch 累积；`after` 更适合做记录、通知、索引、异步触发。

3. **只有 before hook 能 block 或 patch**

4. **所有 hook 都有超时和错误边界**

5. **宿主维护一份可查看的已注册 hook 列表**

---

## 5. 注册方式：贴近当前插件设计

### 5.1 推荐方案：运行时注册

在现有 `SystemApi` 下新增 `hooks` 能力：

```ts
interface SystemApi {
  // 已有能力...
  hooks: HookApi;
}

interface HookApi {
  register(definition: HookDefinition): Disposable;
  registerMany(definitions: HookDefinition[]): Disposable[];
  list(): Promise<RegisteredHookSummary[]>;
}
```

插件示意：

```ts
class DiaryPlugin {
  onPluginLoad(systemApi: SystemApi) {
    systemApi.hooks.register({
      id: "diary.butler.clear",
      event: "butler.mainConversation.afterClear",
      priority: 100,
      handler: async (ctx) => {
        const archivedId = ctx.payload.archivedConversationId;
        await systemApi.invoke("enqueue_plugin_diary_job", {
          archivedConversationId: archivedId,
        });
      },
    });
  }
}
```

### 5.2 为什么不优先放到 `plugin.json`

当前 `plugin.json` 更适合描述元数据、权限、contributions。  
但 hook 真正的处理器是 JS 函数，最终还是在运行时里。

因此 V1 更适合这样分工：

- `plugin.json`：声明权限、可选地声明 hook 元信息
- `dist/main.js`：真正注册处理器

后续如果插件中心需要做更强的静态展示，可以再补：

```json
{
  "permissions": [
    "hooks.register",
    "chat.outbound.intercept",
    "butler.clear.observe"
  ],
  "contributions": {
    "hookMetadata": [
      {
        "event": "chat.message.beforeSend",
        "title": "发送前消息整理"
      }
    ]
  }
}
```

---

## 6. Hook 定义模型

```ts
type HookEventName =
  | "chat.message.beforeSend"
  | "chat.message.afterSend"
  | "chat.message.beforeReceive"
  | "chat.message.afterReceive"
  | "chat.stream.start"
  | "chat.stream.chunk"
  | "chat.stream.end"
  | "chat.stream.error"
  | "conversation.beforeSwitch"
  | "conversation.afterSwitch"
  | "conversation.beforeDelete"
  | "conversation.afterDelete"
  | "mcp.tool.beforeExecute"
  | "mcp.tool.afterExecute"
  | "mcp.tool.afterError"
  | "butler.mainConversation.beforeClear"
  | "butler.mainConversation.afterClear"
  | "butler.task.beforeDispatch"
  | "butler.task.afterDispatch"
  | "butler.task.afterResult";

interface HookDefinition {
  id: string;
  event: HookEventName;
  priority?: number;
  timeoutMs?: number;
  matcher?: HookMatcher;
  when?: HookPredicate;
  handler: (ctx: HookContext) => Promise<HookResult | void> | HookResult | void;
}
```

### 6.1 matcher 设计

参考 Claude Code 的 matcher 思路，但改成更适合 AIPP 的对象结构：

```ts
interface HookMatcher {
  windowLabel?: string | string[];
  source?: string | string[];
  assistantId?: number | number[];
  assistantType?: number | number[];
  conversationKind?: string | string[];
  messageRole?: "user" | "assistant" | Array<"user" | "assistant">;
  toolName?: string | string[];
  butlerSlot?: "main";
}
```

示例：

- 只匹配 Butler 主会话清空：`{ source: "butler", butlerSlot: "main" }`
- 只匹配某些工具：`{ toolName: ["search", "operation"] }`
- 只在 `chat_ui` 窗口内触发：`{ windowLabel: "chat_ui" }`

### 6.2 priority 规则

- 数字越小越先执行
- 默认 `1000`
- 同插件内按注册顺序稳定执行
- `before*` 使用 priority 串行
- `after*` 默认可并行，只有明确要求依赖顺序时才串行

---

## 7. Hook 上下文与返回值

### 7.1 上下文结构

```ts
interface HookContext<TPayload = unknown> {
  event: HookEventName;
  phase: "before" | "after" | "stream";
  plugin: {
    pluginId: number;
    pluginCode: string;
  };
  runtime: {
    windowLabel?: string;
    source: "chat_ui" | "ask" | "butler" | "feishu" | "system";
    timestamp: string;
    traceId: string;
  };
  payload: TPayload;
  capabilities: HookCapabilities;
}
```

### 7.2 返回值

```ts
type HookResult =
  | {
      decision?: "allow" | "block";
      reason?: string;
      patch?: Record<string, unknown>;
      metadata?: Record<string, unknown>;
    }
  | void;
```

规则：

- `before*` 可返回 `decision: "block"`
- `before*` 可返回 `patch`
- `after*` 返回值默认忽略，只允许写 `metadata` 或走副作用
- `stream.*` 默认只观察，不允许 block 主流程

---

## 8. 关键生命周期与能力边界

下面是推荐优先落地的一组事件。

## 8.1 发送消息前：`chat.message.beforeSend`

### 触发时机

用户点击发送后、真正调用消息创建 / `ask_ai` 前。

### 典型用途

- 自动补前缀、补模板
- 关键词过滤
- 敏感词或路径脱敏
- 根据当前上下文自动补充 metadata
- 根据策略禁止发送

### 可用能力

- 读取当前会话、助手、输入内容、附件、模型选择
- 修改消息内容、附加 metadata、补充系统提示、补充上下文
- 阻止发送并返回原因

### 不建议开放

- 不应在这里直接修改数据库中的历史消息
- 不应在这里递归触发新的发送链路

### 建议 payload

```ts
interface BeforeSendPayload {
  conversationId?: number | null;
  assistantId?: number | null;
  userInput: string;
  attachments: Array<{ name: string; path?: string }>;
  source: "chat_ui" | "ask" | "butler" | "feishu";
  overrideModelId?: string | null;
  overrideSystemPrompt?: string | null;
  metadata: Record<string, unknown>;
}
```

### 建议 patch

```ts
interface BeforeSendPatch {
  userInput?: string;
  attachments?: Array<{ name: string; path?: string }>;
  overrideModelId?: string | null;
  overrideSystemPrompt?: string | null;
  metadata?: Record<string, unknown>;
}
```

---

## 8.2 发送消息后：`chat.message.afterSend`

### 触发时机

用户消息已经进入主链路，通常已经持久化并开始请求 AI。

### 典型用途

- 记录发送时间
- 打点统计
- 记录“原始输入 -> 实际发送输入”的差异
- 写插件私有日志

### 可用能力

- 读取最终发送 payload
- 写插件私有存储
- 发通知
- 启动非阻塞异步任务

### 限制

- 不允许改写已经发出的消息
- 失败不影响主链路

---

## 8.3 收到消息前：`chat.message.beforeReceive`

### 触发时机

AI 的最终输出已经形成，但还没有提交到会话消息存储 / UI 最终态之前。

### 典型用途

- 输出脱敏
- 统一后处理（例如提取结构片段、标签、摘要）
- 给消息打插件元数据
- 为后续渲染准备扩展字段

### 可用能力

- 读取 assistant 输出文本、模型信息、工具调用结果摘要、会话信息
- 修改最终写入的消息内容
- 增加结构化 metadata

### 限制

- 不应在这里重新发起同一条消息的 ask 流程
- 不建议在这里做长耗时联网任务

### 建议 payload

```ts
interface BeforeReceivePayload {
  conversationId: number;
  responseMessageId?: number | null;
  model?: string | null;
  content: string;
  source: "chat_ui" | "ask" | "butler" | "feishu";
  usage?: {
    promptTokens?: number | null;
    completionTokens?: number | null;
    totalTokens?: number | null;
  };
  metadata: Record<string, unknown>;
}
```

### 建议 patch

```ts
interface BeforeReceivePatch {
  content?: string;
  metadata?: Record<string, unknown>;
}
```

---

## 8.4 收到消息后：`chat.message.afterReceive`

### 触发时机

assistant 消息已落库 / UI 已完成最终交付。

### 典型用途

- 记录耗时与 token
- 做私人知识索引
- 做“今日工作日记”草稿积累
- 发送提醒或触发外部同步

### 能力

- 读最终消息内容
- 读 token、模型、会话信息
- 写插件存储
- 发起异步子任务

### 限制

- 不影响主链路成功与否

---

## 8.5 流式事件：`chat.stream.start / chunk / end / error`

这组事件是增强项，不建议在第一阶段就开放全部可变能力。

### 推荐策略

- `start`：只观察
- `chunk`：只观察，且需要节流
- `end`：只观察
- `error`：只观察

### 用途

- 自定义流式统计
- 自定义可视化
- 中途内容分类

### 注意

`chunk` 事件非常高频，如果直接开放给所有插件，性能风险很高。  
因此第一版建议：

- 默认关闭
- 需要显式权限 `chat.stream.observe`
- 每个插件默认节流，例如 `200ms` 或按 token 批量

---

## 8.6 会话生命周期

### 建议事件

- `conversation.beforeSwitch`
- `conversation.afterSwitch`
- `conversation.beforeDelete`
- `conversation.afterDelete`

### 用途

- 清理插件缓存
- 保存侧边状态
- 删除私有索引
- 做“离开会话时总结”

### 规则

- `beforeDelete` 可 block
- `beforeSwitch` 一般不建议 block，除非插件明确声明为保护性 hook

---

## 8.7 MCP / 工具生命周期

### 建议事件

- `mcp.tool.beforeExecute`
- `mcp.tool.afterExecute`
- `mcp.tool.afterError`

### 用途

- 审计某些高风险工具
- 记录工具耗时
- 根据工具结果更新插件内部状态

### 对应现状

AIPP 已经有 MCP 工具调用状态流转与前端补偿轮询机制，见 `src/hooks/useConversationEvents.ts`。  
因此这组 hook 很适合作为后续增强项，但不必抢在消息主链路前实现。

---

## 8.8 Butler 专属：清空 / 重开主会话 Hook

这是 AIPP 这次设计里最重要的定制能力。

### 事件 1：`butler.mainConversation.beforeClear`

#### 触发时机

调用 `reset_butler_main_conversation` 前。

#### 典型用途

- 判断是否允许清空
- 检查是否存在未处理任务
- 生成“清空前确认信息”

#### 能力

- 读取当前主会话 id
- 读取任务列表摘要
- 可 block

#### 不建议

- 在这里做长耗时 AI 调用，避免阻塞用户操作

### 事件 2：`butler.mainConversation.afterClear`

#### 触发时机

旧主会话已归档，新主会话已创建，且后端已完成 `butler_main_reset`。

#### 推荐作为“自动记日记”入口

这是最适合“清空对话时触发 AI 去记录日记”的位置，因为：

1. 旧会话并没有真的丢失，而是被归档了；
2. 插件可以拿到 `archivedConversationId`；
3. 清空主流程已经完成，不会因为日记生成失败而影响用户清空；
4. 更适合异步触发总结 / 日记任务。

#### 建议 payload

```ts
interface ButlerAfterClearPayload {
  previousConversationId?: number | null;
  archivedConversationId?: number | null;
  newConversationId: number;
  triggerSource: "ui_button" | "feishu_menu" | "system";
  taskCount: number;
}
```

#### 推荐能力

- 读取旧主会话摘要或完整消息
- 发起异步 AI 总结任务
- 写入插件私有数据库 / 日记存储
- 发通知

#### 推荐宿主补充能力

为了让插件不必通过通用 `invoke` 自己拼链路，建议提供：

```ts
interface ButlerHookCapabilities {
  getConversationSummary(conversationId: number): Promise<string>;
  getConversationMessages(conversationId: number): Promise<Message[]>;
  enqueueBackgroundAssistantRun(input: {
    assistantId?: number;
    prompt: string;
    metadata?: Record<string, unknown>;
  }): Promise<{ jobId: string }>;
}
```

---

## 9. 每个阶段允许什么，不允许什么

| 事件 | 可 block | 可 patch | 可异步副作用 | 失败是否影响主流程 |
|---|---|---|---|---|
| `chat.message.beforeSend` | 是 | 是 | 尽量少 | 是 |
| `chat.message.afterSend` | 否 | 否 | 是 | 否 |
| `chat.message.beforeReceive` | 是 | 是 | 尽量少 | 是 |
| `chat.message.afterReceive` | 否 | 否 | 是 | 否 |
| `chat.stream.*` | 否 | 否 | 是 | 否 |
| `conversation.beforeDelete` | 是 | 否 | 少量 | 是 |
| `conversation.afterDelete` | 否 | 否 | 是 | 否 |
| `mcp.tool.beforeExecute` | 可选 | 可选 | 少量 | 视工具级策略 |
| `mcp.tool.afterExecute` | 否 | 否 | 是 | 否 |
| `butler.mainConversation.beforeClear` | 是 | 否 | 不建议 | 是 |
| `butler.mainConversation.afterClear` | 否 | 否 | 是 | 否 |

建议默认原则：

- `before` = 可以拦截、可以改
- `after` = 只能观察、只能副作用

---

## 10. Hook 能力包设计

不同事件不该拿到同样的能力。  
如果所有 hook 都能拿到完整 `SystemApi`，会很快失控。

建议在 `HookContext` 里注入**事件裁剪后的 capabilities**：

```ts
interface HookCapabilities {
  storage: {
    get(key: string, sessionId?: string): Promise<string | null>;
    set(key: string, value: string | null, sessionId?: string): Promise<void>;
  };
  notify?: {
    toast(input: { title: string; description?: string; variant?: "default" | "destructive" }): void;
  };
  conversation?: {
    getActiveConversation(): Promise<{ id: number | null }>;
    getMessages(conversationId: number): Promise<Message[]>;
  };
  ai?: {
    runAssistantText(options: AippSystemApiRunAssistantTextOptions): Promise<AippSystemApiRunTextResult>;
    runModelText(options: AippSystemApiRunModelTextOptions): Promise<AippSystemApiRunTextResult>;
  };
  butler?: ButlerHookCapabilities;
}
```

### 规则

- `beforeSend` 不默认给 `ai.run*`，避免递归调用
- `afterClear` 可以给 `ai.run*` 或后台任务能力
- `stream.chunk` 不给重型能力，只给轻量只读能力

---

## 11. 权限模型建议

当前插件权限已经在 manifest 中存在，只是还没有扩展到 hook 维度。  
建议新增一组白名单权限：

```text
hooks.register
chat.outbound.observe
chat.outbound.intercept
chat.inbound.observe
chat.inbound.intercept
chat.stream.observe
conversation.observe
conversation.delete.intercept
mcp.observe
mcp.intercept
butler.clear.observe
butler.clear.intercept
butler.read
ai.background.run
storage.read
storage.write
notify.toast
```

### 权限判定规则

1. 未声明则无法注册对应 hook
2. 声明了 observe 权限，不代表有 intercept 权限
3. `intercept` 一律比 `observe` 更高风险
4. Butler 清空相关能力默认高风险

---

## 12. 执行与错误处理规则

## 12.1 执行顺序

- `before` hooks：按 priority 串行执行
- 上一个 hook 的 patch 会成为下一个 hook 的输入
- `after` hooks：默认并行执行

## 12.2 超时

建议默认：

- `beforeSend` / `beforeReceive`: `1500ms ~ 3000ms`
- `afterSend` / `afterReceive`: `5000ms`
- `afterClear`: `10000ms`，但推荐转后台任务

## 12.3 异常处理

- `before` hook 抛错：默认视为该 hook 失败，可配置为
  - `fail-open`：记录错误，继续主流程
  - `fail-closed`：阻止主流程
- 第一阶段建议宿主默认 `fail-open`，但 Butler 清空这种高价值操作可允许插件声明 `fail-closed`

## 12.4 可观察性

建议宿主记录：

- hook id
- plugin code
- event
- 开始时间 / 结束时间 / 耗时
- 结果：allow / block / patch / error / timeout
- block reason

后续可以做一个类似 Claude `/hooks` 的界面：

- 查看当前注册了哪些 hook
- 看来自哪个插件
- 看它挂在哪个事件上
- 看最近执行情况

---

## 13. 和当前代码的接入建议

## 13.1 前端：扩展 `PluginRuntime`

当前 `PluginRuntime` 已经会在实例化插件后调用 `onPluginLoad(systemApi)`。  
因此最自然的改法是：

1. 在 `SystemApi` 中加入 `hooks`
2. 在 `PluginRuntime` 中维护 `HookRegistry`
3. 当插件 reload / disable 时自动注销该插件的 hooks

这和当前 theme / markdown tag 的注册思路是一致的。

## 13.2 前端：消息发送入口埋点

把 hook 挂在这些点：

- 用户提交输入前：`beforeSend`
- 用户消息真正发出后：`afterSend`

这样能覆盖 ChatUI / Ask / Butler / Feishu 等不同入口，只要最后都走统一发送封装。

## 13.3 后端 / 前后桥：消息接收入口埋点

`beforeReceive / afterReceive` 更适合靠近 AI 响应收敛点实现，否则不同窗口可能重复处理。  
建议在 AI 响应最终入库 / 完成事件附近提供统一 dispatch 点。

## 13.4 Butler：在 `reset_butler_main_conversation` 周围加 HookBridge

因为这个命令已经是稳定边界，所以非常适合成为第一批 AIPP 自定义 hook：

- 进入 reset 前：`butler.mainConversation.beforeClear`
- reset 完成后：`butler.mainConversation.afterClear`

这也是满足“自动记录日记 plugin”需求的最短路径。

---

## 14. 示例：自动记录日记插件

### 目标

当用户清空 Butler 主对话时，自动基于被归档的旧主会话生成一篇工作日记。

### 推荐实现

注册 `butler.mainConversation.afterClear`：

```ts
systemApi.hooks.register({
  id: "daily-journal.on-butler-clear",
  event: "butler.mainConversation.afterClear",
  priority: 100,
  timeoutMs: 1000,
  handler: async (ctx) => {
    const archivedId = ctx.payload.archivedConversationId;
    if (!archivedId) {
      return;
    }

    const summary = await ctx.capabilities.butler?.getConversationSummary(archivedId);
    if (!summary) {
      return;
    }

    await ctx.capabilities.butler?.enqueueBackgroundAssistantRun({
      prompt: `请根据以下总管家会话总结生成今日日记：\n\n${summary}`,
      metadata: {
        source: "plugin.daily-journal",
        archivedConversationId: archivedId,
      },
    });
  },
});
```

### 为什么选 `afterClear`

- 不阻塞清空动作
- 不影响新主会话的创建
- 旧内容仍可从 archived conversation 读取
- 更符合“记录日记”这种事后副作用行为

---

## 15. 分阶段落地建议

## Phase 1：先把主链路 Hook 跑通

- `chat.message.beforeSend`
- `chat.message.afterSend`
- `chat.message.beforeReceive`
- `chat.message.afterReceive`
- `butler.mainConversation.beforeClear`
- `butler.mainConversation.afterClear`
- `SystemApi.hooks.register`
- 基础权限校验
- Hook 执行日志

## Phase 2：增强 matcher 和观察能力

- `conversation.*`
- `mcp.tool.*`
- `chat.stream.start/end/error`
- Hook 列表查看器
- 最近执行记录

## Phase 3：更深的 AIPP 专属能力

- Butler task 生命周期 hooks
- Feishu 入站 / 出站 hooks
- 定时任务前后 hooks
- 导出前后 hooks
- 插件中心的 hook 权限与风险提示

---

## 16. 最终建议

如果只说一句结论：

**AIPP 的 Hook 系统最适合做成“运行时注册的 in-process plugin hooks”，而不是 shell 脚本 hooks。**

具体上：

- 借鉴 Claude Code 的事件 / matcher / 决策模型
- 保持 AIPP 当前 `onPluginLoad(systemApi)` 的插件初始化方式
- 把 HookBus 收敛到 `PluginRuntime + 核心业务链路 dispatch 点`
- 第一批优先落地消息发送前后、消息接收前后、Butler 清空前后
- Butler 清空后的 `afterClear` 应该作为“自动记日记 plugin”的标准入口

这样既能满足你的诉求，也能最大程度复用当前 AIPP 已经存在的插件与运行时基础。
