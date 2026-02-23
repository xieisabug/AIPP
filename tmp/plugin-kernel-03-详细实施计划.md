# 详细实施计划（分阶段、可落地、可验收）

这份计划是为了避免“大改焦虑”，把复杂目标拆成可执行阶段。  
核心原则：**先建中台，再做玩法；先保稳定，再追炫技。**

---

## 阶段总览

| 阶段 | 目标 | 主要产出 |
|---|---|---|
| Phase 0 | 统一契约与范围 | 插件 Manifest 规范、能力矩阵、扩展点清单 |
| Phase 1 | 打通安装与注册 | 插件注册表、后端插件管理 API、安装/启停流程 |
| Phase 2 | 统一运行时 | 前端 `PluginManager`、统一生命周期、异常隔离 |
| Phase 3 | 建立能力层 | 受控 API + 权限模型 + 审计日志 |
| Phase 4 | 开放扩展点 | ChatUI/Config/Window 的插件插槽 |
| Phase 5 | 上四个玩法 MVP | 竞技场、剧场、伙伴、成就最小闭环 |
| Phase 6 | 联动优化与稳定性 | 性能、容错、可观测、开发者体验 |

---

## Phase 0：契约先行（先立规矩）

### 要做什么

1. 定义 `plugin.json`（manifest）最小字段：
   - `id/name/version`
   - `entry`
   - `pluginTypes`
   - `permissions`
   - `compat`（AIPP 版本范围）
2. 定义生命周期钩子：
   - `onInstall/onEnable/onActivate/onDeactivate/onUnload`
3. 定义能力矩阵：
   - `chat/ui/window/storage/events/notify/schedule`
4. 定义扩展点命名：
   - `chat.toolbar.right`
   - `chat.sidebar.panel`
   - `config.page`
   - `window.route`

### 验收标准

- 团队对“什么叫插件、插件能做什么、不能做什么”达成统一
- 后续开发不再靠口头约定

---

## Phase 1：安装与注册层（Manifest/安装层）

### 要做什么

1. **后端补齐插件管理 API（Rust）**
   - `list_plugins`
   - `install_plugin`
   - `uninstall_plugin`
   - `enable_plugin / disable_plugin`
   - `get_plugin_config / set_plugin_config`
2. **plugin.db 真正成为插件注册中心**
   - 元数据（版本、作者、目录、状态）
   - 配置（key-value）
   - 运行数据（按 plugin_id + session_key）
3. **安装流程标准化**
   - 校验 manifest
   - 校验入口文件存在
   - 记录安装日志

### 验收标准

- 不改前端代码，也能通过 API 完整管理插件生命周期（安装、启停、卸载）
- 同一个插件可稳定升级，状态可追踪

---

## Phase 2：统一运行时（Runtime层）

### 要做什么

1. 新建前端统一 `PluginManager`（单例）
   - 统一加载脚本
   - 统一实例化
   - 统一错误捕获
2. 把 `ConfigWindow` 和 `ChatUIWindow` 的硬编码 loader 替换为 `PluginManager`
3. 生命周期执行顺序固定化
   - install -> enable -> activate
4. 异常防护
   - 单插件超时
   - 单插件崩溃隔离
   - 故障插件自动降级为 disabled（并提示）

### 验收标准

- 两个窗口不再各写一份插件加载逻辑
- 某插件抛异常时，主聊天流程不崩

---

## Phase 3：能力层（Capability层）

### 要做什么

1. 定义并实现受控 API：
   - `pluginApi.chat`
   - `pluginApi.ui`
   - `pluginApi.window`
   - `pluginApi.storage`
   - `pluginApi.events`
   - `pluginApi.notify`
2. 权限模型
   - 插件声明权限
   - 首次使用弹窗确认（可记住选择）
3. 审计日志
   - 记录插件何时调用了哪些能力
   - 失败时可回放问题

### 验收标准

- 插件可以做事，但做什么是受控、可见、可追踪的
- 用户可清楚看到权限与行为

---

## Phase 4：扩展点落地（Extension层）

### 要做什么

1. 在 ChatUI 开放扩展点：
   - 工具栏按钮位
   - 右侧插件面板位
   - 消息下方插件卡片位（可选）
2. 在 Config 开放扩展点：
   - 插件设置页面
3. 在 Window 系统开放扩展点：
   - 插件可注册自己的路由窗口（受权限控制）
4. 在全局状态开放事件总线：
   - 消息事件、Token事件、MCP事件、活动焦点事件

### 验收标准

- `InterfaceType/ApplicationType` 不再只是概念，能实际挂载并展示

---

## Phase 5：四个玩法 MVP（先做可用版）

### 目标

每个玩法都实现一个“可演示、可体验、可迭代”的最小版本。

### 顺序建议

1. **AI竞技场**（最快体现价值）
2. **多模型剧场**（验证编排引擎）
3. **AI伙伴养成**（验证事件驱动 UI）
4. **成就模块**（验证长期数据与规则）

### 验收标准

- 四个玩法都能以插件形态启停
- 卸载插件后主系统无残留故障

---

## Phase 6：稳定性与体验优化

### 要做什么

1. 性能优化
   - 懒加载插件
   - 插件按窗口激活
2. 可观测性
   - 插件日志面板
   - 插件健康状态（最近错误、最近耗时）
3. 开发体验
   - 插件开发模板
   - 类型声明和示例代码
4. 兼容策略
   - 老插件兼容过渡期
   - API 版本号与弃用公告

### 验收标准

- 插件数量增长后，应用依然稳定、可维护

---

## 关键风险与应对

### 风险 1：插件把主界面卡死

- 应对：运行时超时 + 错误隔离 + 独立调度队列

### 风险 2：权限弹窗太多影响体验

- 应对：按能力分组授权，支持“本插件记住选择”

### 风险 3：玩法插件互相耦合

- 应对：统一事件总线，不允许插件互相直接引用内部对象

### 风险 4：早期过度依赖即将删除模块（sub_task）

- 应对：新玩法核心链路不绑定 sub_task，只把它当可选桥接

---

## 交付视角（给你做决策用）

当你推进到 Phase 4 完成后，你就拥有了“插件平台”；  
Phase 5 完成后，你才拥有“插件生态玩法”。  
这两者要分开看：先平台，后玩法，成功率会高很多。
