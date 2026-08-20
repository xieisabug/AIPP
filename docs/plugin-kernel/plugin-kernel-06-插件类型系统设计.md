# 插件类型系统设计（V2 收敛版）

> 本文档替换“类型不断扩张”的方案，改为当前 AIPP 可落地的插件类型内核。  
> 目标：先统一契约和生命周期，避免类型过多导致实现失控。

---

## 1. 设计原则

1. **类型收敛优先**：V2 只保留三类核心类型  
   - `assistant`：参与助手配置/对话执行  
   - `ui`：在宿主插槽提供界面能力  
   - `worker`：后台监听事件、维护状态
2. **贡献模型优先于直接渲染**：插件声明贡献，宿主渲染容器
3. **能力最小授权**：所有 capability 都有显式权限
4. **兼容现有 assistantType**：旧插件可平滑映射到 `assistant`

---

## 2. 类型定义（V2）

```ts
type PluginKind = "assistant" | "ui" | "worker";
```

### 为什么不在 V2 直接引入更多类型

- `theme/markdown/export/tool/message` 本质是能力域，不一定要成为一级运行时类型  
- 先把 runtime、权限、扩展点做稳，再按能力包逐步开放更合理  
- 过早扩类型会导致“定义先行、实现缺失”的文档债

---

## 3. Manifest V2

```json
{
  "id": "arena",
  "name": "AI 竞技场",
  "version": "1.0.0",
  "entry": "dist/main.js",
  "kinds": ["ui", "worker"],
  "permissions": ["chat.send", "chat.read", "ui.panel", "events.message", "storage.read", "storage.write"],
  "compat": {
    "aipp": ">=0.0.420"
  },
  "contributions": {
    "toolbarActions": [
      {
        "id": "arena.open",
        "title": "AI 竞技场",
        "icon": "Swords",
        "command": "arena.openPanel",
        "location": "chat.toolbar"
      }
    ],
    "panels": [
      {
        "id": "arena.panel",
        "title": "AI 竞技场",
        "location": "chat.sidebar.panel",
        "entry": "ui/arena-panel"
      }
    ]
  }
}
```

### 字段约束

- `kinds`：必须为非空数组，值只能来自 `assistant/ui/worker`
- `permissions`：白名单枚举，不允许自由拼接
- `contributions`：声明式结构，不能携带可执行代码

---

## 4. 生命周期（统一模型）

```ts
interface AippPluginV2 {
  // 插件脚本加载完成后，仅调用一次
  onLoad?(ctx: PluginContext): void | Promise<void>;

  // 在窗口或全局作用域激活
  onActivate?(scope: PluginScope, ctx: PluginContext): void | Promise<void>;

  // 作用域停用（窗口关闭、插件被禁用等）
  onDeactivate?(scope: PluginScope, ctx: PluginContext): void | Promise<void>;

  // 卸载前清理资源
  onDispose?(): void | Promise<void>;
}

type PluginScope = "chat_ui" | "config" | "global";
```

### 生命周期规则

1. `onLoad` 只执行一次  
2. `onActivate/onDeactivate` 允许按 scope 多次调用  
3. `onDeactivate` 必须可重入（重复调用不崩）  
4. 插件异常不外溢到宿主主流程

---

## 5. Contribution 模型（替代直接 ReactNode 注入）

### 5.1 核心思想

插件不把 React 组件对象直接塞给宿主主树，而是：

1. 在 manifest 声明挂载点与入口标识
2. 在运行时注册命令处理函数
3. 宿主负责容器渲染与事件转发

### 5.2 命令接口

```ts
interface CommandRegistry {
  registerCommand(id: string, handler: (payload?: unknown) => Promise<unknown> | unknown): Disposable;
  executeCommand(id: string, payload?: unknown): Promise<unknown>;
}

interface Disposable {
  dispose(): void;
}
```

这样可以避免：

- 直接共享 React 上下文导致版本耦合
- 插件组件异常污染宿主渲染树
- 不同窗口之间组件实例状态错乱

---

## 6. Capability API（V2 最小集）

```ts
interface PluginContext {
  plugin: {
    id: string;
    version: string;
    kinds: PluginKind[];
  };
  chat: ChatCapability;
  ui: UICapability;
  storage: StorageCapability;
  events: EventsCapability;
  notify: NotifyCapability;
  commands: CommandRegistry;
}
```

### 6.1 ChatCapability

```ts
interface ChatCapability {
  sendMessage(options: {
    conversationId: number;
    content: string;
    modelId?: string;
  }): Promise<{ messageId: number }>;

  getActiveConversationId(): number | null;
}
```

### 6.2 UICapability

```ts
type UiLocation = "chat.toolbar" | "chat.sidebar.panel" | "chat.overlay";

interface UICapability {
  registerContribution(location: UiLocation, contributionId: string): Disposable;
  openPanel(panelId: string): void;
  closePanel(panelId: string): void;
}
```

### 6.3 StorageCapability（按 plugin_id 自动隔离）

```ts
interface StorageCapability {
  get<T>(key: string): Promise<T | null>;
  set<T>(key: string, value: T): Promise<void>;
  delete(key: string): Promise<void>;
}
```

### 6.4 EventsCapability（统一命名后再暴露）

```ts
type PluginEventName =
  | "conversation.created"
  | "conversation.switched"
  | "message.created"
  | "message.updated"
  | "message.completed"
  | "mcp.tool.started"
  | "mcp.tool.completed"
  | "token.usage";

interface EventsCapability {
  on<T = unknown>(event: PluginEventName, handler: (payload: T) => void): Disposable;
}
```

---

## 7. 权限模型（最小白名单）

| 权限 | 说明 |
|---|---|
| `chat.read` | 读取当前对话信息 |
| `chat.send` | 发送消息 |
| `ui.toolbar` | 注册工具栏贡献 |
| `ui.panel` | 注册侧边栏面板 |
| `ui.overlay` | 注册悬浮挂件 |
| `storage.read` | 读取插件私有数据 |
| `storage.write` | 写入插件私有数据 |
| `events.message` | 订阅消息相关事件 |
| `events.conversation` | 订阅会话相关事件 |
| `events.mcp` | 订阅 MCP 工具事件 |
| `notify.toast` | 触发应用内提示 |

### 权限策略

1. 未声明即无权限  
2. 权限检查失败必须返回错误，不做静默降级  
3. 插件管理页可见权限申请与最近调用记录

---

## 8. 与现有插件兼容

### 8.1 兼容映射

| 旧字段 | 新字段 |
|---|---|
| `pluginType: ["assistantType"]` | `kinds: ["assistant"]` |
| `onAssistantTypeInit/Select/Run` | 继续保留（兼容层） |
| `config().type` | 兼容读取并映射到 `kinds` |

### 8.2 兼容策略

1. V2 Runtime 启动时先尝试读取 `manifest.kinds`
2. 若不存在，回退读取旧 `config().type`
3. 在插件管理页提示“旧协议插件”，引导升级但不强制中断

---

## 9. 反模式（明确禁止）

1. 插件内使用 `eval/new Function` 执行业务逻辑
2. 插件直接持有宿主内部 React 状态对象
3. 直接依赖原始 Tauri 事件名（必须走 EventBridge）
4. 无权限校验地读写全局数据

---

## 10. 推荐落地顺序

1. 先上 `assistant`（保持现有能力不退化）
2. 再上 `ui`（先 `toolbar + panel`）
3. 最后上 `worker`（成就/统计等后台逻辑）

在这三类稳定后，再讨论是否把 theme/markdown/export/tool/message 独立为新类型。
