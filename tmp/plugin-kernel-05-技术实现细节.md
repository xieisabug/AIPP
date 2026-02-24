# 插件系统技术实现细节

> 本文档是对 01~04 文档的补充，聚焦于"怎么做"而非"为什么做"。
> 读完本文档你应该能回答：插件如何改变界面？如何和对话关联？四个玩法的插件代码长什么样？

---

## 目录

1. [现状问题总结](#1-现状问题总结)
2. [三种插件类型的本质区别](#2-三种插件类型的本质区别)
3. [插件如何改变界面（扩展点机制）](#3-插件如何改变界面扩展点机制)
4. [插件如何和对话关联](#4-插件如何和对话关联)
5. [Capability API 完整规格](#5-capability-api-完整规格)
6. [四个玩法的插件设计详解](#6-四个玩法的插件设计详解)
7. [从现有代码到新架构的迁移路径](#7-从现有代码到新架构的迁移路径)

---

## 1. 现状问题总结

通过阅读现有代码，当前插件系统的实际状态如下：

### 已实现的部分

| 已有能力 | 位置 | 说明 |
|---------|------|------|
| `AssistantType` 插件完整链路 | `plugin.d.ts` + hooks | 能注册助手类型、定制表单、执行运行逻辑 |
| 插件数据库表 | `plugin_db.rs` | 4张表：Plugins/PluginStatus/PluginConfigurations/PluginData |
| 插件脚本加载 | ConfigWindow/ChatUIWindow | 通过 `<script>` 注入 UMD 包 |
| 插件类型定义 | `plugin.d.ts` | AssistantType/InterfaceType/ApplicationType 枚举 |

### 关键缺口（必须补齐才能做新玩法）

| 缺口 | 现状 | 影响 |
|------|------|------|
| **插件列表硬编码** | ConfigWindow 和 ChatUIWindow 各维护一份 `pluginLoadList` | 无法动态安装/发现插件 |
| **后端 API 为零** | `plugin/mod.rs` 是空模块，DB 有表但无 Tauri Command | 前端无法管理插件 |
| **InterfaceType 无挂载点** | 类型已定义但无实际渲染逻辑 | 界面插件无法展示 |
| **ApplicationType 无运行容器** | 类型已定义但无后台执行机制 | 后台插件无法运行 |
| **SystemApi 为空接口** | `interface SystemApi { }` | 插件拿不到任何系统能力 |
| **无事件总线** | 插件无法订阅对话/消息/Token 等事件 | 成就、伙伴等玩法无法实现 |
| **无 UI 扩展点注册** | 插件无法往 ChatUI 注入按钮、面板、卡片 | 竞技场、剧场等玩法无法展示 |

---

## 2. 三种插件类型的本质区别

原始文档只说了"三种类型"，没解释它们的生命周期和挂载方式有什么不同。这里明确定义：

### AssistantType（已实现）—— 挂到"助手配置"和"对话运行"

```
触发时机：用户在助手配置页选择该类型时
挂载位置：助手配置表单 + 对话运行时
生命周期：
  1. onAssistantTypeInit()  — 应用启动，注册类型和表单字段
  2. onAssistantTypeSelect() — 用户选中此类型，动态调整表单
  3. onAssistantTypeRun()   — 用户发送消息，执行运行逻辑
数据关联：通过 assistantId 关联到助手配置
```

### InterfaceType（待实现）—— 挂到"UI 扩展点"

```
触发时机：插件被启用时
挂载位置：ChatUI 侧边栏面板 / 工具栏按钮 / 独立窗口 / 消息卡片
生命周期：
  1. onPluginLoad(systemApi)  — 插件加载，获取系统能力
  2. onActivate(context)      — 插件激活，注册 UI 扩展点
  3. onDeactivate()            — 插件停用，清理 UI
  4. renderPanel?()            — 渲染侧边栏面板内容（React 组件）
  5. renderToolbarAction?()    — 渲染工具栏按钮
数据关联：通过 pluginApi.storage 管理自己的数据，通过 events 监听对话
```

### ApplicationType（待实现）—— 后台运行，无 UI 或可选 UI

```
触发时机：应用启动或手动启用时
挂载位置：后台（无固定 UI 位置，可选注册设置页）
生命周期：
  1. onPluginLoad(systemApi) — 插件加载
  2. onActivate(context)     — 开始后台工作（监听事件、定时任务等）
  3. onDeactivate()           — 停止后台工作
数据关联：通过 events 监听全局事件，通过 storage 持久化数据
```

### 三者对比

| 维度 | AssistantType | InterfaceType | ApplicationType |
|------|--------------|---------------|-----------------|
| 有 UI | 有（通过助手表单） | 有（自定义面板/窗口） | 可选 |
| 关联对话 | 直接（运行时在对话中执行） | 间接（通过事件监听） | 间接（通过事件监听） |
| 运行时机 | 用户发消息时 | 面板可见时 | 始终运行 |
| 典型用途 | 代码生成助手、DeepResearch | 竞技场 UI、剧场 UI、伙伴挂件 | 成就规则引擎、统计后台 |

---

## 3. 插件如何改变界面（扩展点机制）

这是原始文档最模糊的部分。下面详细说明扩展点的具体实现方式。

### 核心概念：扩展点 = React 渲染插槽

扩展点不是什么抽象概念，就是 **React 组件中预留的渲染位置**。插件通过 API 注册内容，AIPP 在对应位置渲染它。

### 扩展点清单与实现方式

#### 3.1 ChatUI 工具栏按钮位 (`chat.toolbar`)

**位置**：ChatUI 输入框上方的工具栏

**实现方式**：

```tsx
// === AIPP 侧：在 ChatUIToolbar.tsx 中预留插槽 ===

function ChatUIToolbar({ pluginToolbarActions }) {
  return (
    <div className="flex items-center gap-1">
      {/* 现有按钮 */}
      <AttachFileButton />
      <McpButton />
      
      {/* 插件扩展按钮区域 */}
      {pluginToolbarActions.map(action => (
        <IconButton
          key={action.id}
          icon={action.icon}
          tooltip={action.tooltip}
          onClick={action.onClick}
        />
      ))}
    </div>
  );
}
```

**插件侧注册**：

```typescript
// === 插件侧：在 onActivate 中注册工具栏按钮 ===

class ArenaPlugin extends AippPlugin {
  onActivate(context: PluginContext) {
    context.ui.registerToolbarAction({
      id: 'arena-start',
      icon: 'Swords',          // lucide-react 图标名
      tooltip: 'AI 竞技场',
      onClick: () => {
        // 打开竞技场面板
        context.ui.openPanel('arena-panel');
      }
    });
  }
}
```

#### 3.2 ChatUI 侧边栏面板 (`chat.sidebar.panel`)

**位置**：ChatUI 右侧可折叠面板区域（类似 VS Code 的侧边栏）

**实现方式**：

```tsx
// === AIPP 侧：在 ChatUIWindow 中管理面板 ===

function ChatUIWindow() {
  const { activePanels } = usePluginPanels();
  
  return (
    <div className="flex h-full">
      {/* 主聊天区 */}
      <div className="flex-1">
        <ConversationUI />
      </div>
      
      {/* 插件面板区域 */}
      {activePanels.length > 0 && (
        <div className="w-80 border-l">
          {activePanels.map(panel => (
            <PluginPanelContainer key={panel.id} panel={panel} />
          ))}
        </div>
      )}
    </div>
  );
}

// PluginPanelContainer 负责隔离和渲染插件提供的 React 组件
function PluginPanelContainer({ panel }) {
  return (
    <ErrorBoundary fallback={<div>插件面板出错</div>}>
      {panel.render()}
    </ErrorBoundary>
  );
}
```

**插件侧注册**：

```typescript
class ArenaPlugin extends AippPlugin {
  onActivate(context: PluginContext) {
    context.ui.registerPanel({
      id: 'arena-panel',
      title: 'AI 竞技场',
      icon: 'Swords',
      render: () => <ArenaPanel api={context} />
    });
  }
}
```

#### 3.3 独立窗口 (`window.route`)

**位置**：独立的 Tauri 窗口

**实现方式**：

```typescript
// 插件请求打开独立窗口
context.window.open({
  id: 'arena-fullscreen',
  title: 'AI 竞技场',
  width: 1200,
  height: 800,
  render: () => <ArenaFullscreen api={context} />
});
```

**AIPP 侧**：复用现有的多窗口架构。新窗口本质上是一个新的 Tauri WebviewWindow，AIPP 在窗口中渲染插件提供的 React 组件。

#### 3.4 消息下方卡片位 (`chat.message.card`)

**位置**：AI 消息气泡下方

**实现方式**：

```tsx
// === AIPP 侧：在 MessageItem 中预留插槽 ===

function MessageItem({ message, pluginMessageCards }) {
  const cards = pluginMessageCards.filter(
    c => c.shouldShow(message)
  );
  
  return (
    <div>
      <MessageContent content={message.content} />
      
      {/* 插件卡片区域 */}
      {cards.map(card => (
        <div key={card.id} className="mt-2 border rounded-lg p-3">
          {card.render(message)}
        </div>
      ))}
    </div>
  );
}
```

**插件侧**：

```typescript
context.ui.registerMessageCard({
  id: 'arena-vote-card',
  shouldShow: (message) => {
    // 只在竞技场对战消息下显示投票卡片
    return message.metadata?.arenaMatch === true;
  },
  render: (message) => <VoteCard matchId={message.metadata.matchId} />
});
```

#### 3.5 全局悬浮组件 (`chat.overlay`)

**位置**：ChatUI 窗口角落（固定位置浮动）

```typescript
// 用于伙伴养成的角落挂件
context.ui.registerOverlay({
  id: 'companion-widget',
  position: 'bottom-right',
  render: () => <CompanionWidget />
});
```

### 扩展点注册的数据流

```
插件 onActivate()
  -> 调用 context.ui.registerPanel/registerToolbarAction/...
  -> PluginManager 维护一个 registrations Map
  -> React 组件通过 usePluginExtensions(pointName) hook 读取
  -> 渲染到对应位置
  -> 插件 onDeactivate() 时自动清理
```

---

## 4. 插件如何和对话关联

这是另一个原始文档没讲清楚的关键问题。

### 4.1 AssistantType 插件（直接关联）

AssistantType 插件天然和对话绑定——它就是在对话中运行的：

```
用户发消息 -> ask_ai / regenerate_ai
  -> 检查助手类型
  -> 如果是插件助手类型 -> 调用 plugin.onAssistantTypeRun(runApi)
  -> runApi 提供 conversationId、messageId 等
  -> 插件通过 runApi.askAssistant() 发起 AI 调用
  -> 结果写入当前对话
```

### 4.2 InterfaceType 插件（通过事件和 API 关联）

InterfaceType 插件不直接参与对话流程，但可以：

**a) 监听对话事件（只读）**

```typescript
class ArenaPlugin {
  onActivate(context: PluginContext) {
    // 监听新消息事件
    context.events.on('message:created', (event) => {
      console.log('新消息', event.conversationId, event.messageId);
    });
    
    // 监听 AI 响应完成
    context.events.on('message:ai_complete', (event) => {
      console.log('AI 响应完成', event.content);
    });
    
    // 监听流式输出（实时）
    context.events.on('message:stream_chunk', (event) => {
      // 可用于实时对比展示
    });
  }
}
```

**b) 主动发起对话（写入）**

```typescript
// 在竞技场中，同时向两个模型发问
async function startArenaMatch(context: PluginContext, prompt: string) {
  // 创建两个独立对话（不影响用户当前对话）
  const [conv1, conv2] = await Promise.all([
    context.chat.createConversation({
      systemPrompt: '你是参赛选手A',
      metadata: { arenaMatch: true, side: 'A' }
    }),
    context.chat.createConversation({
      systemPrompt: '你是参赛选手B',
      metadata: { arenaMatch: true, side: 'B' }
    })
  ]);
  
  // 并发请求两个模型
  const [streamA, streamB] = await Promise.all([
    context.chat.sendMessage(conv1.id, prompt, { modelId: modelA }),
    context.chat.sendMessage(conv2.id, prompt, { modelId: modelB })
  ]);
  
  // 流式输出到竞技场面板
  streamA.on('chunk', (text) => updatePanel('A', text));
  streamB.on('chunk', (text) => updatePanel('B', text));
}
```

**c) 在当前对话中插入内容**

```typescript
// 插件可以向当前活跃对话插入消息（需要权限）
context.chat.insertMessage(currentConversationId, {
  content: '🏆 竞技场结果：模型A获胜！',
  type: 'plugin_message',
  pluginId: 'arena'
});
```

### 4.3 ApplicationType 插件（通过事件间接关联）

ApplicationType 插件在后台运行，只通过事件和全局数据与对话产生联系：

```typescript
class AchievementPlugin {
  onActivate(context: PluginContext) {
    // 监听所有消息事件来统计
    context.events.on('message:created', async (event) => {
      const count = await context.storage.get('total_messages') || 0;
      await context.storage.set('total_messages', count + 1);
      
      // 检查成就
      if (count + 1 === 100) {
        context.notify.toast({
          title: '🎉 成就解锁：百问达人',
          description: '你已经发送了 100 条消息！'
        });
        await context.storage.set('achievement:100_messages', true);
      }
    });
  }
}
```

### 对话关联方式总结

| 关联方式 | 适用类型 | 说明 |
|---------|---------|------|
| 直接执行（通过 runApi） | AssistantType | 插件就是对话的一部分 |
| 事件监听（只读） | InterfaceType / ApplicationType | 被动接收对话变化 |
| 主动发消息（通过 chat API） | InterfaceType | 创建新对话或向现有对话写入 |
| 全局数据统计 | ApplicationType | 统计所有对话的聚合数据 |

---

## 5. Capability API 完整规格

原始文档只列了名字（`pluginApi.chat`、`pluginApi.ui` 等），没给出具体方法。这里定义每个 API 的完整签名。

### 5.1 `PluginContext` 总入口

插件在 `onActivate` 时拿到 `PluginContext`，它是所有能力的入口：

```typescript
interface PluginContext {
  /** 插件自身信息 */
  plugin: {
    id: string;
    name: string;
    version: string;
  };
  
  /** AI 对话能力 */
  chat: ChatCapability;
  
  /** UI 扩展能力 */
  ui: UICapability;
  
  /** 窗口管理能力 */
  window: WindowCapability;
  
  /** 插件私有存储 */
  storage: StorageCapability;
  
  /** 事件监听 */
  events: EventsCapability;
  
  /** 通知 */
  notify: NotifyCapability;
}
```

### 5.2 `ChatCapability` —— AI 对话能力

```typescript
interface ChatCapability {
  /**
   * 创建一个新对话（插件专用，不干扰用户当前对话）
   * 需要权限：chat.create
   */
  createConversation(options: {
    systemPrompt?: string;
    modelId?: string;
    metadata?: Record<string, any>;
  }): Promise<{ conversationId: number }>;

  /**
   * 向对话发送消息并获取 AI 响应（流式）
   * 需要权限：chat.send
   */
  sendMessage(conversationId: number, content: string, options?: {
    modelId?: string;         // 覆盖模型
    stream?: boolean;          // 是否流式（默认 true）
  }): Promise<ChatStream>;

  /**
   * 并发向多个模型发送同一问题（竞技场核心）
   * 需要权限：chat.send
   */
  sendToMultipleModels(prompt: string, modelIds: string[], options?: {
    systemPrompt?: string;
    stream?: boolean;
  }): Promise<MultiModelStream>;

  /**
   * 获取当前活跃对话的 ID（只读）
   * 需要权限：chat.read
   */
  getActiveConversationId(): number | null;

  /**
   * 读取对话的消息历史
   * 需要权限：chat.read
   */
  getMessages(conversationId: number): Promise<Message[]>;

  /**
   * 获取可用模型列表
   * 无需特殊权限
   */
  getAvailableModels(): Promise<ModelInfo[]>;
}

interface ChatStream {
  on(event: 'chunk', callback: (text: string) => void): void;
  on(event: 'done', callback: (fullText: string) => void): void;
  on(event: 'error', callback: (error: Error) => void): void;
  abort(): void;
}

interface MultiModelStream {
  /** 每个模型一个独立流 */
  streams: Map<string, ChatStream>;
  /** 所有模型完成后触发 */
  onAllDone(callback: (results: Map<string, string>) => void): void;
}
```

### 5.3 `UICapability` —— UI 扩展能力

```typescript
interface UICapability {
  /**
   * 注册工具栏按钮
   * 需要权限：ui.toolbar
   */
  registerToolbarAction(action: ToolbarAction): Disposable;

  /**
   * 注册侧边栏面板
   * 需要权限：ui.panel
   */
  registerPanel(panel: PanelRegistration): Disposable;

  /**
   * 打开/关闭侧边栏面板
   */
  openPanel(panelId: string): void;
  closePanel(panelId: string): void;

  /**
   * 注册消息卡片（显示在消息下方）
   * 需要权限：ui.message_card
   */
  registerMessageCard(card: MessageCardRegistration): Disposable;

  /**
   * 注册全局悬浮组件（如伙伴挂件）
   * 需要权限：ui.overlay
   */
  registerOverlay(overlay: OverlayRegistration): Disposable;

  /**
   * 注册设置页面
   * 需要权限：ui.settings
   */
  registerSettingsPage(page: SettingsPageRegistration): Disposable;
}

interface ToolbarAction {
  id: string;
  icon: string;          // lucide-react 图标名
  tooltip: string;
  onClick: () => void;
}

interface PanelRegistration {
  id: string;
  title: string;
  icon: string;
  render: () => React.ReactNode;
}

interface MessageCardRegistration {
  id: string;
  shouldShow: (message: Message) => boolean;
  render: (message: Message) => React.ReactNode;
}

interface OverlayRegistration {
  id: string;
  position: 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right';
  render: () => React.ReactNode;
}

interface SettingsPageRegistration {
  id: string;
  title: string;
  icon: string;
  render: () => React.ReactNode;
}

/** 用于取消注册 */
interface Disposable {
  dispose(): void;
}
```

### 5.4 `WindowCapability` —— 窗口管理

```typescript
interface WindowCapability {
  /**
   * 打开独立窗口
   * 需要权限：window.open
   */
  open(options: {
    id: string;
    title: string;
    width?: number;
    height?: number;
    render: () => React.ReactNode;
  }): Promise<void>;

  /**
   * 关闭窗口
   */
  close(windowId: string): Promise<void>;
}
```

### 5.5 `StorageCapability` —— 插件私有存储

```typescript
interface StorageCapability {
  /**
   * 读取插件数据（自动按 pluginId 隔离）
   * 数据存储在 plugin.db 的 PluginData 表
   */
  get<T = any>(key: string): Promise<T | null>;

  /**
   * 写入插件数据
   */
  set<T = any>(key: string, value: T): Promise<void>;

  /**
   * 删除插件数据
   */
  delete(key: string): Promise<void>;

  /**
   * 列出所有 key
   */
  keys(): Promise<string[]>;

  /**
   * 会话级存储（关联到具体对话）
   */
  session(conversationId: number): {
    get<T = any>(key: string): Promise<T | null>;
    set<T = any>(key: string, value: T): Promise<void>;
    delete(key: string): Promise<void>;
  };
}
```

### 5.6 `EventsCapability` —— 事件监听

```typescript
interface EventsCapability {
  /**
   * 订阅事件
   * 返回取消订阅函数
   */
  on<T = any>(event: EventName, callback: (payload: T) => void): () => void;

  /**
   * 一次性订阅
   */
  once<T = any>(event: EventName, callback: (payload: T) => void): () => void;
}

/** 可订阅的事件清单 */
type EventName =
  // 对话事件
  | 'conversation:created'      // 新对话创建
  | 'conversation:switched'     // 切换到另一个对话
  | 'conversation:deleted'      // 对话被删除
  // 消息事件
  | 'message:created'           // 新消息（用户或AI）
  | 'message:stream_chunk'      // AI 流式输出片段
  | 'message:ai_complete'       // AI 响应完成
  | 'message:regenerated'       // 消息被重新生成
  // Token 事件
  | 'token:usage'               // Token 使用统计
  // MCP 事件
  | 'mcp:tool_call_start'       // MCP 工具开始调用
  | 'mcp:tool_call_complete'    // MCP 工具调用完成
  // 活动事件
  | 'app:focus'                 // 应用获得焦点
  | 'app:blur'                  // 应用失去焦点
  | 'app:daily_first_open';     // 每天首次打开

/** 事件 payload 示例 */
interface MessageCreatedEvent {
  conversationId: number;
  messageId: number;
  messageType: 'user' | 'ai' | 'system';
  content: string;
  modelId?: string;
}

interface TokenUsageEvent {
  conversationId: number;
  modelId: string;
  promptTokens: number;
  completionTokens: number;
}
```

### 5.7 `NotifyCapability` —— 通知

```typescript
interface NotifyCapability {
  /**
   * 轻量 toast 提示
   */
  toast(options: {
    title: string;
    description?: string;
    duration?: number;    // 毫秒，默认 3000
    variant?: 'default' | 'success' | 'warning' | 'error';
  }): void;

  /**
   * 系统通知（桌面通知）
   * 需要权限：notify.system
   */
  systemNotification(options: {
    title: string;
    body: string;
  }): void;
}
```

### 5.8 权限模型

每个能力都有对应的权限字符串。插件在 `plugin.json` 中声明需要的权限：

```json
{
  "id": "arena",
  "name": "AI 竞技场",
  "version": "1.0.0",
  "permissions": [
    "chat.create",
    "chat.send",
    "chat.read",
    "ui.toolbar",
    "ui.panel",
    "storage",
    "events.message"
  ]
}
```

权限分组策略：

| 权限组 | 包含权限 | 说明 |
|--------|---------|------|
| `chat` | chat.create, chat.send, chat.read | 对话相关 |
| `ui` | ui.toolbar, ui.panel, ui.message_card, ui.overlay, ui.settings | 界面相关 |
| `window` | window.open | 窗口相关 |
| `storage` | （无子权限） | 存储 |
| `events.*` | events.message, events.conversation, events.token, events.mcp, events.app | 事件分类 |
| `notify` | notify.toast, notify.system | 通知 |

首次使用时弹窗确认，用户可选择"允许"或"拒绝"，并可勾选"记住选择"。

---

## 6. 四个玩法的插件设计详解

原始文档（04）只讲了"用户体验目标"和"MVP 功能"，没讲清楚**插件代码长什么样、数据怎么流转、界面怎么挂载**。这里逐个拆解。

### 6.1 AI 竞技场（Model Arena）

#### 插件组成

竞技场是一个 **InterfaceType 插件**，由以下部分组成：

```
arena-plugin/
├── plugin.json            # 插件清单
├── dist/
│   └── main.js            # 编译产物
└── src/
    ├── index.ts           # 插件入口（注册扩展点）
    ├── ArenaPanel.tsx      # 侧边栏面板（选模型、发起对战）
    ├── ArenaBattle.tsx     # 对战分屏展示组件
    ├── ArenaVote.tsx       # 投票卡片组件
    ├── ArenaLeaderboard.tsx # 榜单组件
    └── elo.ts             # Elo 评分算法
```

#### plugin.json

```json
{
  "id": "arena",
  "name": "AI 竞技场",
  "version": "1.0.0",
  "pluginTypes": ["InterfaceType"],
  "entry": "dist/main.js",
  "permissions": [
    "chat.create",
    "chat.send",
    "ui.toolbar",
    "ui.panel",
    "storage",
    "events.message"
  ],
  "compat": ">=0.5.0"
}
```

#### 插件入口代码

```typescript
// src/index.ts
class ArenaPlugin extends AippPlugin {
  private context: PluginContext;
  private battles: Map<string, BattleState> = new Map();

  config() {
    return { name: 'AI 竞技场', type: ['InterfaceType'] };
  }

  onPluginLoad(systemApi: SystemApi) {
    // Phase 1: 基础初始化
  }

  onActivate(context: PluginContext) {
    this.context = context;

    // 1. 注册工具栏按钮
    context.ui.registerToolbarAction({
      id: 'arena-toggle',
      icon: 'Swords',
      tooltip: '打开竞技场',
      onClick: () => context.ui.openPanel('arena-panel')
    });

    // 2. 注册侧边栏面板
    context.ui.registerPanel({
      id: 'arena-panel',
      title: 'AI 竞技场',
      icon: 'Swords',
      render: () => <ArenaPanel plugin={this} />
    });
  }

  // ---- 业务方法 ----

  async startBattle(prompt: string, modelA: string, modelB: string) {
    const battleId = crypto.randomUUID();

    // 并发调用两个模型
    const multiStream = await this.context.chat.sendToMultipleModels(
      prompt,
      [modelA, modelB],
      { stream: true }
    );

    const battle: BattleState = {
      id: battleId,
      prompt,
      models: [modelA, modelB],
      responses: ['', ''],
      status: 'streaming',
      startedAt: Date.now()
    };
    this.battles.set(battleId, battle);

    // 收集流式响应
    let idx = 0;
    for (const [modelId, stream] of multiStream.streams) {
      const i = idx++;
      stream.on('chunk', (text) => {
        battle.responses[i] += text;
        // 触发 React 重新渲染（通过状态管理）
      });
    }

    multiStream.onAllDone(async (results) => {
      battle.status = 'voting';
      // 持久化对战记录
      await this.context.storage.set(`battle:${battleId}`, battle);
    });

    return battleId;
  }

  async submitVote(battleId: string, winner: 'A' | 'B' | 'draw') {
    const battle = this.battles.get(battleId);
    if (!battle) return;

    // 更新 Elo 评分
    const ratingA = await this.context.storage.get(`elo:${battle.models[0]}`) || 1200;
    const ratingB = await this.context.storage.get(`elo:${battle.models[1]}`) || 1200;
    const [newA, newB] = calculateElo(ratingA, ratingB, winner);

    await this.context.storage.set(`elo:${battle.models[0]}`, newA);
    await this.context.storage.set(`elo:${battle.models[1]}`, newB);

    battle.status = 'completed';
    battle.winner = winner;
    await this.context.storage.set(`battle:${battleId}`, battle);
  }

  async getLeaderboard(): Promise<LeaderboardEntry[]> {
    const models = await this.context.chat.getAvailableModels();
    const entries = [];
    for (const model of models) {
      const elo = await this.context.storage.get(`elo:${model.id}`) || 1200;
      entries.push({ modelId: model.id, modelName: model.name, elo });
    }
    return entries.sort((a, b) => b.elo - a.elo);
  }
}

// 挂载到 window 全局（UMD 方式）
window.ArenaPlugin = ArenaPlugin;
```

#### 界面组件示例（ArenaPanel）

```tsx
// src/ArenaPanel.tsx
function ArenaPanel({ plugin }: { plugin: ArenaPlugin }) {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [modelA, setModelA] = useState('');
  const [modelB, setModelB] = useState('');
  const [prompt, setPrompt] = useState('');
  const [battleId, setBattleId] = useState<string | null>(null);

  useEffect(() => {
    plugin.context.chat.getAvailableModels().then(setModels);
  }, []);

  if (battleId) {
    return <ArenaBattle plugin={plugin} battleId={battleId} />;
  }

  return (
    <div className="flex flex-col gap-4 p-4">
      <h3 className="text-lg font-semibold">⚔️ AI 竞技场</h3>

      {/* 模型选择 */}
      <div className="flex gap-2">
        <Select value={modelA} onValueChange={setModelA}>
          <SelectTrigger><SelectValue placeholder="选手 A" /></SelectTrigger>
          <SelectContent>
            {models.map(m => (
              <SelectItem key={m.id} value={m.id}>{m.name}</SelectItem>
            ))}
          </SelectContent>
        </Select>

        <span className="self-center text-muted-foreground">VS</span>

        <Select value={modelB} onValueChange={setModelB}>
          {/* 同上 */}
        </Select>
      </div>

      {/* 问题输入 */}
      <Textarea
        placeholder="输入你的问题..."
        value={prompt}
        onChange={e => setPrompt(e.target.value)}
      />

      {/* 开始按钮 */}
      <Button
        onClick={async () => {
          const id = await plugin.startBattle(prompt, modelA, modelB);
          setBattleId(id);
        }}
        disabled={!modelA || !modelB || !prompt}
      >
        开始对战
      </Button>

      {/* 榜单入口 */}
      <Button variant="outline" onClick={() => {/* 切换到榜单视图 */}}>
        查看排行榜
      </Button>
    </div>
  );
}
```

#### 数据流完整路径

```
用户点击"开始对战"
  -> ArenaPlugin.startBattle(prompt, modelA, modelB)
  -> context.chat.sendToMultipleModels()
     -> AIPP 内部：创建两个临时对话 + 并发调用 ask_ai
     -> 返回 MultiModelStream
  -> 流式 chunk 更新 React 状态 -> 双栏实时渲染
  -> 完成后切换到"投票"状态
  -> 用户点击投票
  -> ArenaPlugin.submitVote()
  -> context.storage.set() -> 写入 plugin.db PluginData 表
  -> Elo 评分更新

数据存储位置：
  - 对战记录: plugin.db PluginData (plugin_id='arena', data_key='battle:{id}')
  - Elo 评分: plugin.db PluginData (plugin_id='arena', data_key='elo:{modelId}')
  - 对战对话: conversation.db (metadata 标记 arenaMatch=true)
```

---

### 6.2 多模型剧场（AI Theater）

#### 插件组成

剧场是一个 **InterfaceType + ApplicationType 复合插件**：
- InterfaceType 部分：剧场 UI（舞台、角色、台词展示）
- ApplicationType 部分：编排引擎（控制角色发言顺序和内容）

```
theater-plugin/
├── plugin.json
├── src/
│   ├── index.ts              # 插件入口
│   ├── TheaterPanel.tsx       # 剧场主界面
│   ├── ScriptEditor.tsx       # 剧本/主题编辑
│   ├── StageView.tsx          # 舞台视图（对话气泡）
│   ├── RoleConfig.tsx         # 角色配置
│   └── orchestrator.ts       # 编排引擎（核心逻辑）
```

#### 编排引擎（核心原理）

编排引擎的本质是一个 **状态机**，管理"谁在说话"和"说什么"：

```typescript
// src/orchestrator.ts

interface Role {
  id: string;
  name: string;
  modelId: string;
  systemPrompt: string;  // 角色人设
}

interface Round {
  roleId: string;
  content: string;
  status: 'pending' | 'streaming' | 'done';
}

class TheaterOrchestrator {
  private roles: Role[] = [];
  private rounds: Round[] = [];
  private currentRound = 0;
  private maxRounds: number;
  private topic: string;
  private context: PluginContext;

  constructor(context: PluginContext, config: TheaterConfig) {
    this.context = context;
    this.roles = config.roles;
    this.maxRounds = config.maxRounds;
    this.topic = config.topic;
  }

  /**
   * 构建当前角色的 prompt
   * 关键：把之前所有角色的发言作为上下文传入
   */
  private buildPromptForRole(role: Role): string {
    const history = this.rounds
      .filter(r => r.status === 'done')
      .map(r => {
        const speaker = this.roles.find(rl => rl.id === r.roleId);
        return `[${speaker?.name}]: ${r.content}`;
      })
      .join('\n\n');

    return `主题：${this.topic}

以下是之前的讨论记录：
${history || '（这是第一轮发言）'}

现在轮到你发言了。请以你的角色身份继续讨论。`;
  }

  /**
   * 执行下一轮
   * 按照预定顺序轮流调用不同模型
   */
  async executeNextRound(onChunk: (text: string) => void): Promise<Round> {
    const roleIndex = this.currentRound % this.roles.length;
    const role = this.roles[roleIndex];
    const prompt = this.buildPromptForRole(role);

    const round: Round = {
      roleId: role.id,
      content: '',
      status: 'streaming'
    };
    this.rounds.push(round);

    // 用该角色的模型和系统提示发起请求
    const conv = await this.context.chat.createConversation({
      systemPrompt: role.systemPrompt,
      modelId: role.modelId,
      metadata: { theater: true, roleId: role.id }
    });

    const stream = await this.context.chat.sendMessage(conv.conversationId, prompt, {
      modelId: role.modelId,
      stream: true
    });

    return new Promise((resolve) => {
      stream.on('chunk', (text) => {
        round.content += text;
        onChunk(text);
      });

      stream.on('done', () => {
        round.status = 'done';
        this.currentRound++;
        resolve(round);
      });
    });
  }

  /** 插入导演指令（用户中途干预） */
  insertDirectorNote(note: string) {
    this.rounds.push({
      roleId: 'director',
      content: `[导演指令] ${note}`,
      status: 'done'
    });
  }

  /** 导出剧本 */
  exportScript(): string {
    return this.rounds
      .filter(r => r.status === 'done')
      .map(r => {
        if (r.roleId === 'director') return r.content;
        const role = this.roles.find(rl => rl.id === r.roleId);
        return `【${role?.name}】\n${r.content}`;
      })
      .join('\n\n---\n\n');
  }
}
```

#### 界面如何展示

```tsx
// src/StageView.tsx — 舞台视图
function StageView({ rounds, roles }: { rounds: Round[], roles: Role[] }) {
  return (
    <div className="flex flex-col gap-4 p-4 overflow-y-auto">
      {rounds.map((round, i) => {
        if (round.roleId === 'director') {
          return (
            <div key={i} className="text-center text-sm text-muted-foreground italic">
              {round.content}
            </div>
          );
        }

        const role = roles.find(r => r.id === round.roleId);
        const isLeft = roles.indexOf(role!) % 2 === 0;

        return (
          <div key={i} className={`flex ${isLeft ? '' : 'flex-row-reverse'} gap-3`}>
            {/* 角色头像 */}
            <div className="w-10 h-10 rounded-full bg-muted flex items-center justify-center">
              {role?.name[0]}
            </div>
            {/* 台词气泡 */}
            <div className={`max-w-[70%] rounded-lg p-3 ${isLeft ? 'bg-muted' : 'bg-primary/10'}`}>
              <div className="text-xs text-muted-foreground mb-1">{role?.name}</div>
              <div className="text-sm whitespace-pre-wrap">
                {round.content}
                {round.status === 'streaming' && <span className="animate-pulse">▊</span>}
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}
```

#### 与竞技场的区别

| 维度 | 竞技场 | 剧场 |
|------|--------|------|
| 模型调用方式 | 并发（同时） | 串行（轮流） |
| 上下文传递 | 各自独立 | 前面角色的发言作为后续角色的上下文 |
| 用户参与 | 投票 | 插入导演指令 |
| 产出 | 评分/排行榜 | 剧本文本 |

---

### 6.3 AI 伙伴养成（Companion）

#### 插件组成

伙伴是一个 **InterfaceType + ApplicationType 复合插件**：
- InterfaceType 部分：角落挂件（表情/动画）
- ApplicationType 部分：成长状态机（经验/心情/等级计算）

```
companion-plugin/
├── plugin.json
├── src/
│   ├── index.ts              # 插件入口
│   ├── CompanionWidget.tsx    # 角落挂件组件
│   ├── CompanionPanel.tsx     # 详情面板（等级、皮肤）
│   ├── state-machine.ts      # 成长状态机
│   └── sprites/              # 角色表情图片/SVG
│       ├── normal.svg
│       ├── happy.svg
│       ├── thinking.svg
│       └── error.svg
```

#### 核心：成长状态机

```typescript
// src/state-machine.ts

interface CompanionState {
  level: number;
  xp: number;
  mood: 'normal' | 'happy' | 'thinking' | 'error' | 'excited';
  skin: string;
  streak: number;           // 连续活跃天数
  lastActiveDate: string;   // YYYY-MM-DD
  stats: {
    totalMessages: number;
    totalTokens: number;
    toolCallSuccess: number;
    toolCallFailed: number;
  };
}

class CompanionStateMachine {
  private state: CompanionState;
  private context: PluginContext;
  private moodTimer: number | null = null;

  constructor(context: PluginContext) {
    this.context = context;
  }

  async initialize() {
    // 从 storage 加载持久化状态
    const saved = await this.context.storage.get<CompanionState>('companion_state');
    this.state = saved || {
      level: 1, xp: 0, mood: 'normal', skin: 'default',
      streak: 0, lastActiveDate: '',
      stats: { totalMessages: 0, totalTokens: 0, toolCallSuccess: 0, toolCallFailed: 0 }
    };

    // 检查连续活跃
    this.checkStreak();

    // 注册事件监听
    this.setupEventListeners();
  }

  private setupEventListeners() {
    // 用户发消息 -> 获得经验 + 心情变化
    this.context.events.on('message:created', (event: MessageCreatedEvent) => {
      if (event.messageType === 'user') {
        this.addXp(5);
        this.state.stats.totalMessages++;
        // 发消息时伙伴进入"思考"表情
        this.setMood('thinking');
      }
    });

    // AI 响应完成 -> 伙伴开心
    this.context.events.on('message:ai_complete', () => {
      this.addXp(10);
      this.setMood('happy');
      // 3 秒后恢复正常
      this.scheduleMoodReset(3000);
    });

    // 工具调用成功 -> 兴奋
    this.context.events.on('mcp:tool_call_complete', (event) => {
      if (event.status === 'success') {
        this.state.stats.toolCallSuccess++;
        this.addXp(15);
        this.setMood('excited');
      } else {
        this.state.stats.toolCallFailed++;
        this.setMood('error');
      }
      this.scheduleMoodReset(3000);
    });

    // Token 消耗统计
    this.context.events.on('token:usage', (event: TokenUsageEvent) => {
      this.state.stats.totalTokens += event.promptTokens + event.completionTokens;
    });

    // 每天第一次打开
    this.context.events.on('app:daily_first_open', () => {
      this.checkStreak();
      this.setMood('happy');
    });
  }

  private addXp(amount: number) {
    this.state.xp += amount;
    // 升级阈值：每级需要 level * 100 XP
    const threshold = this.state.level * 100;
    if (this.state.xp >= threshold) {
      this.state.xp -= threshold;
      this.state.level++;
      this.context.notify.toast({
        title: `🎉 伙伴升级！`,
        description: `你的伙伴升到了 Lv.${this.state.level}`,
        variant: 'success'
      });
    }
    this.persist();
  }

  private setMood(mood: CompanionState['mood']) {
    this.state.mood = mood;
    // 通知 UI 更新（通过 React 状态同步）
  }

  private scheduleMoodReset(delay: number) {
    if (this.moodTimer) clearTimeout(this.moodTimer);
    this.moodTimer = window.setTimeout(() => {
      this.setMood('normal');
    }, delay);
  }

  private checkStreak() {
    const today = new Date().toISOString().split('T')[0];
    if (this.state.lastActiveDate === today) return;

    const yesterday = new Date(Date.now() - 86400000).toISOString().split('T')[0];
    if (this.state.lastActiveDate === yesterday) {
      this.state.streak++;
    } else {
      this.state.streak = 1;
    }
    this.state.lastActiveDate = today;
    this.persist();
  }

  private async persist() {
    await this.context.storage.set('companion_state', this.state);
  }

  getState(): CompanionState {
    return { ...this.state };
  }
}
```

#### 角落挂件组件

```tsx
// src/CompanionWidget.tsx
function CompanionWidget({ stateMachine }: { stateMachine: CompanionStateMachine }) {
  const [state, setState] = useState(stateMachine.getState());

  // 定期同步状态
  useEffect(() => {
    const interval = setInterval(() => {
      setState(stateMachine.getState());
    }, 500);
    return () => clearInterval(interval);
  }, []);

  // 根据心情选择表情
  const spriteUrl = `/sprites/${state.mood}.svg`;

  return (
    <div className="flex flex-col items-center gap-1 cursor-pointer"
      onClick={() => {/* 展开详情面板 */}}
      title={`Lv.${state.level} | 连续活跃 ${state.streak} 天`}
    >
      {/* 角色表情 */}
      <div className="w-12 h-12 transition-all duration-300">
        <img src={spriteUrl} alt={state.mood} className="w-full h-full" />
      </div>
      {/* 等级标签 */}
      <span className="text-xs text-muted-foreground">
        Lv.{state.level}
      </span>
    </div>
  );
}
```

#### 插件入口挂载

```typescript
class CompanionPlugin extends AippPlugin {
  private stateMachine: CompanionStateMachine;

  onActivate(context: PluginContext) {
    this.stateMachine = new CompanionStateMachine(context);
    this.stateMachine.initialize();

    // 注册角落挂件
    context.ui.registerOverlay({
      id: 'companion-widget',
      position: 'bottom-right',
      render: () => <CompanionWidget stateMachine={this.stateMachine} />
    });

    // 注册详情面板（点击挂件展开）
    context.ui.registerPanel({
      id: 'companion-detail',
      title: '我的伙伴',
      icon: 'Heart',
      render: () => <CompanionPanel stateMachine={this.stateMachine} />
    });
  }

  onDeactivate() {
    // 清理（Disposable 会自动清理 UI 注册）
  }
}
```

---

### 6.4 成就模块（Achievements）

#### 插件组成

成就是一个 **ApplicationType 为主 + InterfaceType 辅助的插件**：
- ApplicationType 部分：规则引擎（后台持续监听事件、判定达成）
- InterfaceType 部分：成就墙页面

```
achievement-plugin/
├── plugin.json
├── src/
│   ├── index.ts              # 插件入口
│   ├── AchievementPanel.tsx   # 成就墙面板
│   ├── AchievementToast.tsx   # 解锁弹窗
│   ├── rules.ts              # 成就规则定义
│   └── engine.ts             # 规则引擎
```

#### 成就规则定义

```typescript
// src/rules.ts

interface AchievementRule {
  id: string;
  name: string;
  description: string;
  icon: string;          // emoji 或图标名
  category: 'usage' | 'streak' | 'exploration' | 'mastery' | 'hidden';
  /** 判定是否达成，传入当前统计数据 */
  check(stats: AchievementStats): boolean;
  /** 进度（0~1），用于展示进度条 */
  progress(stats: AchievementStats): number;
}

interface AchievementStats {
  totalMessages: number;
  totalConversations: number;
  totalTokens: number;
  consecutiveDays: number;
  toolCallsSuccess: number;
  toolCallsFailed: number;
  modelsUsed: Set<string>;
  arenaVotes: number;        // 竞技场投票次数（跨插件数据）
  theaterShows: number;      // 剧场演出次数
}

// 所有成就规则
const ACHIEVEMENT_RULES: AchievementRule[] = [
  {
    id: 'first_message',
    name: '初次发问',
    description: '发送第一条消息',
    icon: '💬',
    category: 'usage',
    check: (s) => s.totalMessages >= 1,
    progress: (s) => Math.min(s.totalMessages / 1, 1),
  },
  {
    id: 'message_100',
    name: '健谈者',
    description: '发送 100 条消息',
    icon: '🗣️',
    category: 'usage',
    check: (s) => s.totalMessages >= 100,
    progress: (s) => Math.min(s.totalMessages / 100, 1),
  },
  {
    id: 'streak_7',
    name: '周活达人',
    description: '连续活跃 7 天',
    icon: '🔥',
    category: 'streak',
    check: (s) => s.consecutiveDays >= 7,
    progress: (s) => Math.min(s.consecutiveDays / 7, 1),
  },
  {
    id: 'streak_30',
    name: '月活冠军',
    description: '连续活跃 30 天',
    icon: '🏆',
    category: 'streak',
    check: (s) => s.consecutiveDays >= 30,
    progress: (s) => Math.min(s.consecutiveDays / 30, 1),
  },
  {
    id: 'tool_master',
    name: '工具达人',
    description: 'MCP 工具调用成功 100 次',
    icon: '🔧',
    category: 'mastery',
    check: (s) => s.toolCallsSuccess >= 100,
    progress: (s) => Math.min(s.toolCallsSuccess / 100, 1),
  },
  {
    id: 'multi_model',
    name: '模型收藏家',
    description: '使用过 5 种不同的模型',
    icon: '🎭',
    category: 'exploration',
    check: (s) => s.modelsUsed.size >= 5,
    progress: (s) => Math.min(s.modelsUsed.size / 5, 1),
  },
  {
    id: 'arena_judge',
    name: '公正裁判',
    description: '完成 30 场竞技场投票',
    icon: '⚖️',
    category: 'mastery',
    check: (s) => s.arenaVotes >= 30,
    progress: (s) => Math.min(s.arenaVotes / 30, 1),
  },
  {
    id: 'hidden_night_owl',
    name: '夜猫子',
    description: '在凌晨 2-5 点发送消息',
    icon: '🦉',
    category: 'hidden',
    check: (s) => s._nightMessageSent === true,
    progress: (s) => s._nightMessageSent ? 1 : 0,
  },
];
```

#### 规则引擎

```typescript
// src/engine.ts

class AchievementEngine {
  private context: PluginContext;
  private stats: AchievementStats;
  private unlocked: Set<string> = new Set();

  constructor(context: PluginContext) {
    this.context = context;
  }

  async initialize() {
    // 加载已解锁的成就
    const savedUnlocked = await this.context.storage.get<string[]>('unlocked') || [];
    this.unlocked = new Set(savedUnlocked);

    // 加载统计数据
    this.stats = await this.context.storage.get<AchievementStats>('stats') || {
      totalMessages: 0,
      totalConversations: 0,
      totalTokens: 0,
      consecutiveDays: 0,
      toolCallsSuccess: 0,
      toolCallsFailed: 0,
      modelsUsed: new Set(),
      arenaVotes: 0,
      theaterShows: 0,
    };

    this.setupEventListeners();
  }

  private setupEventListeners() {
    this.context.events.on('message:created', (event: MessageCreatedEvent) => {
      if (event.messageType === 'user') {
        this.stats.totalMessages++;

        // 隐藏成就：夜猫子检测
        const hour = new Date().getHours();
        if (hour >= 2 && hour <= 5) {
          (this.stats as any)._nightMessageSent = true;
        }

        this.checkAllRules();
      }
    });

    this.context.events.on('message:ai_complete', (event) => {
      if (event.modelId) {
        this.stats.modelsUsed.add(event.modelId);
      }
      this.checkAllRules();
    });

    this.context.events.on('conversation:created', () => {
      this.stats.totalConversations++;
      this.checkAllRules();
    });

    this.context.events.on('mcp:tool_call_complete', (event) => {
      if (event.status === 'success') {
        this.stats.toolCallsSuccess++;
      } else {
        this.stats.toolCallsFailed++;
      }
      this.checkAllRules();
    });

    this.context.events.on('token:usage', (event: TokenUsageEvent) => {
      this.stats.totalTokens += event.promptTokens + event.completionTokens;
      this.checkAllRules();
    });
  }

  private async checkAllRules() {
    for (const rule of ACHIEVEMENT_RULES) {
      if (this.unlocked.has(rule.id)) continue;

      if (rule.check(this.stats)) {
        // 新成就解锁！
        this.unlocked.add(rule.id);

        // 通知用户
        this.context.notify.toast({
          title: `${rule.icon} 成就解锁：${rule.name}`,
          description: rule.description,
          variant: 'success',
          duration: 5000
        });

        // 持久化
        await this.context.storage.set('unlocked', Array.from(this.unlocked));
      }
    }

    // 持久化统计（节流：不必每次都写）
    await this.context.storage.set('stats', this.stats);
  }

  getProgress(): AchievementProgress[] {
    return ACHIEVEMENT_RULES
      .filter(r => r.category !== 'hidden' || this.unlocked.has(r.id))
      .map(rule => ({
        ...rule,
        isUnlocked: this.unlocked.has(rule.id),
        currentProgress: rule.progress(this.stats),
        unlockedAt: null // TODO: 记录解锁时间
      }));
  }
}
```

#### 成就墙界面

```tsx
// src/AchievementPanel.tsx
function AchievementPanel({ engine }: { engine: AchievementEngine }) {
  const [filter, setFilter] = useState<'all' | 'unlocked' | 'locked'>('all');
  const achievements = engine.getProgress();

  const filtered = achievements.filter(a => {
    if (filter === 'unlocked') return a.isUnlocked;
    if (filter === 'locked') return !a.isUnlocked;
    return true;
  });

  return (
    <div className="flex flex-col gap-4 p-4">
      <h3 className="text-lg font-semibold">🏅 成就墙</h3>

      {/* 统计概览 */}
      <div className="text-sm text-muted-foreground">
        已解锁 {achievements.filter(a => a.isUnlocked).length} / {achievements.length}
      </div>

      {/* 过滤按钮 */}
      <div className="flex gap-2">
        <Button size="sm" variant={filter === 'all' ? 'default' : 'outline'}
          onClick={() => setFilter('all')}>全部</Button>
        <Button size="sm" variant={filter === 'unlocked' ? 'default' : 'outline'}
          onClick={() => setFilter('unlocked')}>已解锁</Button>
        <Button size="sm" variant={filter === 'locked' ? 'default' : 'outline'}
          onClick={() => setFilter('locked')}>未解锁</Button>
      </div>

      {/* 成就列表 */}
      <div className="grid gap-3">
        {filtered.map(achievement => (
          <div key={achievement.id}
            className={`border rounded-lg p-3 ${
              achievement.isUnlocked ? 'bg-primary/5' : 'opacity-60'
            }`}
          >
            <div className="flex items-center gap-3">
              <span className="text-2xl">{achievement.icon}</span>
              <div className="flex-1">
                <div className="font-medium">{achievement.name}</div>
                <div className="text-sm text-muted-foreground">
                  {achievement.description}
                </div>
                {/* 进度条 */}
                {!achievement.isUnlocked && (
                  <div className="mt-2 h-1.5 bg-muted rounded-full overflow-hidden">
                    <div
                      className="h-full bg-primary rounded-full transition-all"
                      style={{ width: `${achievement.currentProgress * 100}%` }}
                    />
                  </div>
                )}
              </div>
              {achievement.isUnlocked && (
                <span className="text-green-500">✓</span>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
```

### 6.5 四个玩法的联动设计

四个玩法不是孤岛，它们通过 **事件总线 + 存储** 实现松耦合联动：

```
竞技场投票 -> 事件 'plugin:arena:vote_submitted'
                -> 成就引擎监听 -> 更新 arenaVotes 计数 -> 判定"公正裁判"成就
                -> 伙伴系统监听 -> 增加经验值

剧场演出完成 -> 事件 'plugin:theater:show_completed'
                -> 成就引擎监听 -> 更新 theaterShows 计数
                -> 伙伴系统监听 -> 伙伴表情变为"兴奋"

成就解锁 -> 事件 'plugin:achievement:unlocked'
                -> 伙伴系统监听 -> 检查是否是皮肤解锁类成就 -> 解锁皮肤
```

**实现方式**：通过 `PluginContext.events` 的自定义事件。插件可以发出 `plugin:` 前缀的自定义事件，其他插件可以订阅：

```typescript
// 竞技场插件：投票后发出事件
this.context.events.emit('plugin:arena:vote_submitted', {
  matchId: battleId,
  winner: winnerModel
});

// 成就插件：监听竞技场事件
this.context.events.on('plugin:arena:vote_submitted', () => {
  this.stats.arenaVotes++;
  this.checkAllRules();
});
```

---

## 7. 从现有代码到新架构的迁移路径

这部分解释：现在的代码需要改哪些文件、每一步改什么、怎么保持向后兼容。

### 7.1 总览：要改什么、不改什么

**不需要改的（保留现有）**：
- `ask_ai`、`regenerate_ai` 等 AI 核心链路
- 对话、消息、助手的数据库和 API
- MCP 集成
- 多窗口架构
- 现有的两个 AssistantType 插件（代码生成、DeepResearch）

**需要改的**：

| 改动 | 涉及文件 | 改动内容 |
|------|---------|---------|
| ① 后端插件 API | 新建 `api/plugin_api.rs` | 暴露 plugin.db 的 CRUD 为 Tauri Command |
| ② 统一插件加载 | 新建 `src/services/PluginManager.ts` | 替代两处硬编码 `pluginLoadList` |
| ③ 改造窗口加载 | 改 `ConfigWindow.tsx`、`ChatUIWindow.tsx` | 用 PluginManager 替代内联加载逻辑 |
| ④ 事件总线 | 新建 `src/services/PluginEventBus.ts` | 桥接 Tauri 事件到插件事件 |
| ⑤ UI 扩展点 | 改 `ChatUIWindow.tsx`、`ConversationUI.tsx` | 预留面板、工具栏、悬浮组件的插槽 |
| ⑥ 填充 SystemApi | 改 `plugin.d.ts` + 新建 capability 实现 | 让 InterfaceType/ApplicationType 能拿到能力 |
| ⑦ 新增 plugin.json 规范 | 新建 `src/types/plugin-manifest.ts` | Manifest 类型定义和校验 |

### 7.2 Step 1：后端插件管理 API

**现状**：`plugin_db.rs` 有完整 CRUD 但没有 Tauri Command 暴露给前端。

**要做的**：

```rust
// 新文件：src-tauri/src/api/plugin_api.rs

#[tauri::command]
pub async fn list_plugins(app_handle: tauri::AppHandle) -> Result<Vec<Plugin>, String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.get_all_plugins().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn install_plugin(
    app_handle: tauri::AppHandle,
    name: String,
    version: String,
    folder_name: String,
    description: Option<String>,
    author: Option<String>
) -> Result<i64, String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.create_plugin(name, version, folder_name, description, author)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn enable_plugin(app_handle: tauri::AppHandle, plugin_id: i64) -> Result<(), String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.update_plugin_status(plugin_id, true).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn disable_plugin(app_handle: tauri::AppHandle, plugin_id: i64) -> Result<(), String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.update_plugin_status(plugin_id, false).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_plugin_config(
    app_handle: tauri::AppHandle,
    plugin_id: i64
) -> Result<Vec<PluginConfiguration>, String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.get_plugin_configurations(plugin_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_plugin_config(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    key: String,
    value: String
) -> Result<(), String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.upsert_plugin_configuration(plugin_id, &key, &value)
        .map_err(|e| e.to_string())
}

// ... 以及 plugin_data 的 CRUD（用于插件 storage API）
```

然后在 `main.rs` 注册这些 commands。

### 7.3 Step 2：前端统一 PluginManager

**现状**：`ConfigWindow.tsx` 和 `ChatUIWindow.tsx` 各有一份 ~50 行的内联插件加载代码。

**要做的**：

```typescript
// 新文件：src/services/PluginManager.ts

import { invoke } from '@tauri-apps/api/core';
import { appDataDir } from '@tauri-apps/api/path';
import { convertFileSrc } from '@tauri-apps/api/core';

interface PluginInstance {
  id: string;
  name: string;
  code: string;
  pluginTypes: string[];
  instance: AippPlugin | AippAssistantTypePlugin | null;
  status: 'loading' | 'active' | 'error' | 'disabled';
  error?: string;
}

class PluginManager {
  private static instance: PluginManager;
  private plugins: Map<string, PluginInstance> = new Map();
  private listeners: Set<() => void> = new Set();

  static getInstance(): PluginManager {
    if (!PluginManager.instance) {
      PluginManager.instance = new PluginManager();
    }
    return PluginManager.instance;
  }

  /**
   * 从后端加载已启用的插件列表，然后加载脚本
   */
  async loadAllPlugins(): Promise<void> {
    // 1. 从 DB 获取已启用的插件列表（未来）
    // const dbPlugins = await invoke('list_plugins');
    // const enabledPlugins = dbPlugins.filter(p => p.is_active);

    // 2. 过渡期：兼容硬编码列表
    const pluginLoadList = [
      { name: "代码生成", code: "code-generate", pluginTypes: ["assistantType"] },
      { name: "DeepResearch", code: "deepresearch", pluginTypes: ["assistantType"] },
    ];

    // 3. 并发加载所有插件脚本
    const dataDir = await appDataDir();
    await Promise.all(
      pluginLoadList.map(p => this.loadPlugin(p, dataDir))
    );

    this.notifyListeners();
  }

  private async loadPlugin(config: any, dataDir: string): Promise<void> {
    const entry: PluginInstance = {
      id: config.code,
      name: config.name,
      code: config.code,
      pluginTypes: config.pluginTypes,
      instance: null,
      status: 'loading'
    };
    this.plugins.set(config.code, entry);

    try {
      const scriptPath = `${dataDir}plugin/${config.code}/dist/main.js`;
      await this.injectScript(scriptPath);

      // 查找全局构造函数
      const Constructor = this.findConstructor(config.code);
      if (Constructor) {
        entry.instance = new Constructor();
        entry.status = 'active';
      } else {
        entry.status = 'error';
        entry.error = `No global constructor found for '${config.code}'`;
      }
    } catch (e) {
      entry.status = 'error';
      entry.error = String(e);
      console.error(`[PluginManager] Failed to load '${config.code}':`, e);
      // 单个插件失败不影响其他插件
    }
  }

  private injectScript(path: string): Promise<void> {
    return new Promise((resolve, reject) => {
      const script = document.createElement('script');
      script.src = convertFileSrc(path);
      script.onload = () => resolve();
      script.onerror = (e) => reject(new Error(`Script load failed: ${path}`));
      document.head.appendChild(script);
    });
  }

  private findConstructor(code: string): any {
    const pascalCase = code.replace(/(^|-)(\w)/g, (_, __, c) => c.toUpperCase());
    return (window as any).SamplePlugin
      || (window as any)[code]
      || (window as any)[pascalCase]
      || null;
  }

  /** 获取特定类型的插件 */
  getPluginsByType(type: string): PluginInstance[] {
    return Array.from(this.plugins.values())
      .filter(p => p.status === 'active' && p.pluginTypes.includes(type));
  }

  /** 获取所有插件 */
  getAllPlugins(): PluginInstance[] {
    return Array.from(this.plugins.values());
  }

  /** 订阅变化 */
  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private notifyListeners() {
    this.listeners.forEach(l => l());
  }
}

export default PluginManager;
```

然后 `ConfigWindow.tsx` 和 `ChatUIWindow.tsx` 都改为：

```typescript
// 替代原来的 50 行内联加载代码
useEffect(() => {
  window.React = React;
  window.ReactDOM = ReactDOM;
  PluginManager.getInstance().loadAllPlugins().then(() => {
    setPluginList(PluginManager.getInstance().getAllPlugins());
  });
}, []);
```

### 7.4 Step 3：事件总线

**现状**：AIPP 已有 Tauri 事件系统（`conversation_events` 等），但插件无法订阅。

**要做的**：桥接 Tauri 事件到插件的 `EventsCapability`。

```typescript
// 新文件：src/services/PluginEventBus.ts

import { listen } from '@tauri-apps/api/event';

class PluginEventBus {
  private subscriptions: Map<string, Set<(payload: any) => void>> = new Map();
  private tauriUnlisteners: (() => void)[] = [];

  async initialize() {
    // 桥接 Tauri 事件到插件事件
    const mappings: Record<string, string> = {
      'message_create':        'message:created',
      'message_update':        'message:stream_chunk',
      'ai_response_complete':  'message:ai_complete',
      'conversation_created':  'conversation:created',
      'conversation_switched': 'conversation:switched',
      'token_usage':           'token:usage',
      'mcp_tool_call_start':   'mcp:tool_call_start',
      'mcp_tool_call_complete':'mcp:tool_call_complete',
    };

    for (const [tauriEvent, pluginEvent] of Object.entries(mappings)) {
      const unlisten = await listen(tauriEvent, (event) => {
        this.emit(pluginEvent, event.payload);
      });
      this.tauriUnlisteners.push(unlisten);
    }
  }

  on(event: string, callback: (payload: any) => void): () => void {
    if (!this.subscriptions.has(event)) {
      this.subscriptions.set(event, new Set());
    }
    this.subscriptions.get(event)!.add(callback);
    return () => this.subscriptions.get(event)?.delete(callback);
  }

  emit(event: string, payload: any) {
    this.subscriptions.get(event)?.forEach(cb => {
      try {
        cb(payload);
      } catch (e) {
        console.error(`[PluginEventBus] Error in handler for '${event}':`, e);
      }
    });
  }

  destroy() {
    this.tauriUnlisteners.forEach(fn => fn());
    this.subscriptions.clear();
  }
}

export default PluginEventBus;
```

### 7.5 Step 4：UI 扩展点

在现有 `ChatUIWindow.tsx` / `ConversationUI.tsx` 中预留插槽。这是最小侵入的改动——只需在合适位置添加渲染占位：

```tsx
// ChatUIWindow.tsx 中添加

// 1. 引入 hook
const pluginPanels = usePluginPanels();    // 从 PluginManager 读取注册的面板
const pluginOverlays = usePluginOverlays(); // 从 PluginManager 读取注册的悬浮组件

// 2. 在布局中渲染
return (
  <div className="flex h-full relative">
    {/* 主聊天区 */}
    <div className="flex-1">
      <ConversationUI pluginToolbarActions={pluginToolbarActions} />
    </div>

    {/* 插件面板区（可折叠） */}
    {activePanel && (
      <div className="w-80 border-l overflow-auto">
        <ErrorBoundary>
          {activePanel.render()}
        </ErrorBoundary>
      </div>
    )}

    {/* 插件悬浮组件 */}
    {pluginOverlays.map(overlay => (
      <div key={overlay.id} className={`fixed ${positionClass(overlay.position)}`}>
        <ErrorBoundary>
          {overlay.render()}
        </ErrorBoundary>
      </div>
    ))}
  </div>
);
```

### 7.6 迁移顺序总结

```
Phase 1（1~2 周）：
  ✅ 新建 plugin_api.rs，暴露 DB CRUD
  ✅ 新建 PluginManager.ts，统一加载逻辑
  ✅ 改造 ConfigWindow / ChatUIWindow 使用 PluginManager
  ✅ 现有 AssistantType 插件不受影响（向后兼容）

Phase 2（1~2 周）：
  ✅ 新建 PluginEventBus.ts，桥接 Tauri 事件
  ✅ 实现 PluginContext 和 Capability API
  ✅ 填充 SystemApi
  ✅ 支持 InterfaceType 生命周期

Phase 3（1~2 周）：
  ✅ 在 ChatUI 中添加面板、工具栏、悬浮组件插槽
  ✅ 实现 UICapability (registerPanel, registerToolbarAction, etc.)
  ✅ 实现 StorageCapability (桥接到 plugin_api 的 plugin_data CRUD)

Phase 4（按需）：
  ✅ 上线竞技场插件（第一个 InterfaceType 插件）
  ✅ 验证全链路：安装 -> 加载 -> UI 渲染 -> AI 调用 -> 数据存储
  ✅ 后续逐个上线剧场、伙伴、成就
```

---

## 附录：和原始文档（01~04）的修正对照

| 原始文档说法 | 问题 | 修正 |
|-------------|------|------|
| "src/plugins/ 里有核心逻辑" | 该目录实际为空 | 核心逻辑在 ConfigWindow/ChatUIWindow 内联和两个 hooks 文件中 |
| "PluginManager 统一加载" | 实际不存在 PluginManager | 需要新建，是 Phase 1 交付物 |
| "plugin.db + API 驱动" | DB 有表但无 API 暴露 | 需要新建 plugin_api.rs |
| "能力矩阵已有 chat/ui/window/storage/events/notify" | SystemApi 实际为空接口 | 所有 Capability 需要从零实现 |
| "`InterfaceType/ApplicationType` 已有实战路径" | 实际只有类型定义，无运行时支持 | 需要实现完整生命周期和挂载点 |
| "子任务系统标注'后续计划删除'" | 当前 AssistantType 插件仍依赖子任务 | 短期内保留子任务，新玩法不依赖即可 |

