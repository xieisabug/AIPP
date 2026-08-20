# 和当前 AIPP 怎么结合：不是推倒重来，而是“补中台 + 开插槽”

这份文档重点回答两个问题：

1. 你现在有哪些基础可以直接复用？
2. 新设计要补哪些关键缺口？

---

## 一、当前 AIPP 已有的“可复用资产”

你现在并不是从零开始，已有不少好基础：

1. **助手类型机制已经通了（AssistantType）**  
   `onAssistantTypeInit / onAssistantTypeSelect / onAssistantTypeRun` 已有实战路径。  
   ⚠️ 但仅限 `AssistantType`。`InterfaceType` 和 `ApplicationType` 只有类型枚举定义，无运行时支持。

2. **多窗口架构已经成熟**  
   `ask/chat_ui/config/artifact/...` 都有稳定窗口管理能力，适合承载玩法窗口。

3. **事件与状态基础已存在**  
   对话事件、MCP工具调用状态、activity focus、token 统计都可作为玩法数据源。  
   ⚠️ 但这些事件目前没有桥接到插件——插件无法直接订阅。需要建事件总线。

4. **本地数据库体系完整**  
   已有多库分域设计（assistant/conversation/mcp/plugin/system 等），可承接插件持久化。  
   ⚠️ `plugin.db` 有完整表结构和 Rust CRUD 方法，但**没有 Tauri Command 暴露给前端**。

5. **插件类型概念已定义**  
   `AssistantType/InterfaceType/ApplicationType` 在类型层面已存在，方向是对的。  
   ⚠️ `SystemApi` 接口为空（`interface SystemApi { }`），插件拿不到任何系统能力。

---

## 二、当前关键短板（要正面补齐）

下面是“现状 -> 影响 -> 该补什么”：

| 现状 | 影响 | 需要补齐 |
|---|---|---|
| `ConfigWindow` 和 `ChatUIWindow` 各自硬编码 `pluginLoadList` | 加一个插件要改多处，容易不一致 | 做统一 `PluginManager` 和统一加载入口 |
| 插件清单不是 DB 驱动，还是写死 | 无法形成真正插件市场/安装管理 | 用 `plugin.db + API` 驱动插件发现和状态 |
| `plugin.db` 目前基本只建表，缺少完整 API 接线 | 无法从前端标准化管理插件 | 增加插件 CRUD、启停、配置读写命令 |
| `SystemApi` 基本为空 | 插件开发体验弱，能力不成体系 | 建 `capability api`（chat/ui/window/storage/events） |
| `InterfaceType/ApplicationType` 处于预留状态 | 竞技场/剧场/伙伴/成就难以优雅实现 | 落地界面扩展点与后台插件生命周期 |
| 插件 `id` 管理不完整（sourceId 经常回落到 0） | 子任务归属和鉴权不可靠 | 安装时发放稳定 plugin_id，全链路透传 |
| 子任务系统标注“后续计划删除” | 新玩法若重度依赖子任务会有技术债 | 新玩法主路径尽量走 plugin runtime + capability，不绑 sub_task |

---

## 三、结合策略：保留可用，替换薄弱环节

### 1）保留不动（或小改）

- 现有助手运行主链路（`ask_ai`、会话管理、MCP、Token 统计）
- 现有窗口体系和 UI 风格（黑白灰、Shadcn）
- 现有事件系统（conversation events）

### 2）重点改造

- **改造 A：统一插件运行时中台（前端）**  
  把现在分散在多个窗口里的脚本加载逻辑收敛为一个 `PluginManager`。

- **改造 B：补齐插件后端管理面（Rust API）**  
  给 `plugin.db` 真正接入 `list/install/enable/disable/update-config/...`。

- **改造 C：建立 Capability API（受控能力层）**  
  用“受控 API + 权限弹窗”替代“插件随意访问”。

- **改造 D：建立扩展点（Extension Points）**  
  让 `InterfaceType/ApplicationType` 真正有挂载位，不是只有类型声明。

---

## 四、与四个玩法的映射（直接看结果）

| 玩法 | 主插件类型 | 依赖能力 | 挂载位置 |
|---|---|---|---|
| AI竞技场 | InterfaceType + AssistantType | chat 并发、ui 卡片、storage 评分、events 流式状态 | ChatUI 主区 + 独立窗口（可选） |
| 多模型剧场 | InterfaceType + ApplicationType | chat 编排、events 对话状态、storage 剧本记录 | ChatUI 插件面板 + 剧场窗口 |
| AI伙伴养成 | InterfaceType + ApplicationType | events（消息/活跃度）、ui 悬浮组件、storage 成长数据 | ChatUI 角落组件 + 可选 Sidebar |
| 成就模块 | ApplicationType + InterfaceType | events/token/activity、storage 规则与进度、notify 提醒 | 全局后台 + 成就墙页面 |

---

## 五、对当前代码的“最小扰动接入路径”

### Step 1：先把“插件发现与加载”收口

- 从“窗口内硬编码列表”改为“统一从 plugin registry 读取”
- 暂时兼容老插件产物结构（`dist/main.js`）避免一次性迁移成本

### Step 2：再把“能力供给”收口

- 插件只拿到受控 API（比如 `pluginApi.chat.runCompare(...)`）
- 前端业务代码不直接暴露内部状态对象

### Step 3：最后把“玩法”挂上去

- 先上竞技场（最直观，验证并发和评分链路）
- 再上剧场（验证编排）
- 然后伙伴（验证事件驱动）
- 最后成就（验证长期数据与规则引擎）

---

## 六、你最关心的一句话

你的 AIPP 现状已经够好用，**不需要推倒重建**。  
要做的是：在现有主链路上补一个稳定插件中台，让“玩法”从“写死功能”升级为“可插拔产品能力”。
> 📖 **详细技术实现请看**：`plugin-kernel-05-技术实现细节.md`  
> 包含具体代码示例、Capability API 完整规格、四个玩法的插件源码级设计、以及从现有代码迁移的详细步骤。