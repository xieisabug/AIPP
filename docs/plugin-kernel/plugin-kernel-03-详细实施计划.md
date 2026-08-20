# 详细实施计划（V2 可落地版）

> 这版计划不是“功能愿景清单”，而是按当前 AIPP 代码现状可直接执行的工程计划。  
> 核心目标：先把插件内核做稳，再做玩法扩展。

---

## 0. 先说结论（执行原则）

1. **只做三类核心插件能力**：`assistant` / `ui` / `worker`  
2. **先统一运行时，再开放扩展点**：先把硬编码加载改掉，再谈玩法  
3. **UI 先走贡献模型（contribution）**：Host 负责渲染，插件提供声明和命令  
4. **向后兼容现有 assistantType 插件**：`code-generate` / `deepresearch` 不中断  
5. **所有新增能力都走权限模型**：不允许隐式高权限

---

## 1. 当前问题（以现有代码为准）

| 问题 | 当前表现 | 风险 |
|---|---|---|
| 插件加载硬编码 | `ConfigWindow.tsx`、`ChatUIWindow.tsx` 各维护一份 `pluginLoadList` | 维护成本高，行为不一致 |
| 后端仅有 DB，无管理 API | `plugin_db.rs` 有 CRUD，`api/mod.rs` 未挂 plugin API | 前端无法真正安装/启停/配置插件 |
| 类型定义与运行时脱节 | `plugin.d.ts` 有 `InterfaceType/ApplicationType`，但运行时无容器 | 设计文档落不了地 |
| 能力边界不清 | `SystemApi` 为空接口 | 插件能力不可治理 |
| 事件模型未收口 | 业务事件存在，但无插件侧统一桥接 | 玩法插件互联困难 |

---

## 2. V2 范围控制（In / Out）

### In Scope（本轮必须完成）

1. 后端插件管理 API（基于现有 `plugin.db`）
2. 前端统一 `PluginRuntime`（替代两处内联加载）
3. 插件 Manifest V2 与基础校验
4. 三类核心能力：assistant / ui / worker
5. 最小扩展点：`chat.toolbar`、`chat.sidebar.panel`、`chat.overlay`
6. 统一事件桥接层（EventBridge）

### Out of Scope（本轮不做）

1. Theme / Markdown / Export / Tool / Message 多类型全面落地
2. 插件市场、远程下载、签名体系
3. 复杂沙箱（多进程隔离）——先做可治理单进程运行时

---

## 3. 统一架构（V2 内核）

```text
plugin.json + dist/main.js
  -> PluginRegistry（Rust + plugin.db）
  -> PluginRuntime（Frontend 单例）
  -> CapabilityBridge（chat/ui/storage/events/window/notify）
  -> ExtensionHost（ChatUI/Config 的插槽渲染）
```

### 关键决策

1. **不要求插件直接返回 ReactNode 给宿主主树**  
   插件通过 contribution 声明“挂在哪里 + 调哪个命令”；宿主负责渲染容器。
2. **命令式交互替代对象直连**  
   插件暴露 `commands`，UI 事件只调用 command，降低耦合。
3. **窗口作用域激活**  
   插件按窗口激活/停用（chat_ui/config），避免无效运行。

---

## 4. 分阶段实施

## Phase 1：注册中心打通（Backend First）

### 目标

让插件从“写死列表”变成“数据库注册 + API 管理”。

### 任务

1. 新增 `src-tauri/src/api/plugin_api.rs`
2. 在 `src-tauri/src/api/mod.rs` 暴露模块
3. 在 `src-tauri/src/lib.rs` 注册 Tauri commands
4. 基于现有 `PluginDatabase` 提供：
   - `list_plugins`
   - `install_plugin`
   - `uninstall_plugin`
   - `enable_plugin`
   - `disable_plugin`
   - `get_plugin_config`
   - `set_plugin_config`
   - `get_plugin_data` / `set_plugin_data`

### 验收

- 不改前端业务页面，也可通过命令完成插件启停与配置读写
- API 错误可见（无 silent fallback）

---

## Phase 2：统一运行时（Frontend Runtime）

### 目标

去掉 `ConfigWindow.tsx` 和 `ChatUIWindow.tsx` 的重复脚本加载逻辑。

### 任务

1. 新建 `src/services/PluginRuntime.ts`
2. 统一脚本注入、构造函数发现、生命周期调用、错误收敛
3. 通过后端 API 获取已启用插件列表
4. 两个窗口都改为调用 `PluginRuntime.getInstance().loadForWindow(...)`

### 验收

- 两个窗口插件行为一致
- 单插件加载失败不影响主流程

---

## Phase 3：Capability + EventBridge

### 目标

把“插件能做什么”变成清晰、可控、可审计的接口。

### 任务

1. 补齐 `plugin.d.ts` 的 `SystemApi/PluginContext`（最小版本）
2. 实现 `capabilities`：
   - `chat`（只暴露必要能力）
   - `storage`（自动按 plugin_id 隔离）
   - `events`（统一命名，不直接暴露原始 Tauri 事件名）
   - `notify`
3. 新建 `PluginEventBridge` 统一映射事件

### 验收

- 插件不再直接依赖内部状态对象
- 事件名稳定，窗口间一致

---

## Phase 4：UI 扩展点（最小集合）

### 目标

让 `ui` 类型插件可见可用，但不破坏现有 ChatUI 结构。

### 任务

1. 落地贡献点：
   - `chat.toolbar`
   - `chat.sidebar.panel`
   - `chat.overlay`
2. 宿主渲染容器 + ErrorBoundary
3. 扩展点注册返回 `dispose`，支持停用后清理

### 验收

- 可以上线一个最小 UI 插件（示例：面板 + 工具栏按钮）
- 禁用插件后 UI 无残留

---

## 5. 风险与对策

| 风险 | 对策 |
|---|---|
| 插件异常影响主界面 | 运行时统一 try/catch + ErrorBoundary + 禁用态回退 |
| 能力扩张过快 | 仅开放最小 capability，按权限增量放开 |
| 文档与代码再度漂移 | 以 `plugin.d.ts` 与 manifest schema 作为单一契约源 |

---

## 6. 里程碑完成定义（DoD）

满足以下 5 条才算 V2 内核完成：

1. 插件列表来源于后端注册中心，不再硬编码
2. 前端只有一个插件运行时入口
3. assistant 插件保持兼容且可运行
4. 至少一个 ui 插件可挂载/卸载
5. 事件与存储 API 可被插件稳定调用

---

## 7. 后续扩展顺序（V2 之后）

1. 先扩 `worker` 玩法（成就/统计）
2. 再扩富 UI 玩法（竞技场/剧场）
3. 最后再评估 Theme/Markdown/Export 等高级类型

这样可以避免“类型先膨胀、内核后补课”的反向建设路线。
