# 插件开发与运行指南（V2 底层）

本文档对应当前实现：后端 `plugin_api` + 前端 `PluginRuntime`。  
目标是让你可以自己写一个插件并在 AIPP 中跑起来。

---

## 1. 当前可用能力

- 已可稳定运行：`assistantType` 插件
- 已具备底层：插件目录自动发现、注册同步、统一运行时加载
- 后续扩展（theme/markdown/tool 等）将基于同一底层继续增加

---

## 2. 插件目录规范

插件根目录在：

```text
<AppDataDir>/plugin
```

每个插件一个子目录：

```text
<AppDataDir>/plugin/<plugin_code>/
├── plugin.json
└── dist/
    └── main.js
```

只要 `dist/main.js` 存在，后端就会扫描并同步到 `plugin.db`，前端运行时会加载启用插件。

---

## 3. plugin.json（最小示例）

```json
{
  "id": "hello-assistant-plugin",
  "name": "Hello Assistant Plugin",
  "version": "0.1.0",
  "entry": "dist/main.js",
  "kinds": ["assistant"],
  "pluginTypes": ["assistantType"]
}
```

说明：

- `id`/`name`/`version`：元数据
- `pluginTypes`：当前前端筛选主要看这个字段（`assistantType`）
- `kinds`：V2 收敛模型字段（assistant/ui/worker），会映射到插件类型

---

## 4. 插件入口要求

运行时会按以下顺序查找全局构造函数：

1. `window[plugin_code]`
2. `window[PascalCase(plugin_code)]`
3. `window.SamplePlugin`

例如插件目录是 `hello-assistant-plugin`，推荐导出：

```js
window.HelloAssistantPlugin = HelloAssistantPlugin;
```

---

## 5. 最小可运行插件（assistantType）

可直接参考仓库模板：

```text
docs/plugin-template/assistant-basic/
├── plugin.json
└── dist/main.js
```

插件 SDK 类型声明（含 `SystemApi` 与 `systemApi.ui`）放在共享模板目录：

```text
docs/plugin-template/shared/aipp-plugin-sdk.d.ts
```

说明：

- 业务插件（如 benchmark）直接复用这份声明，不再在插件内重复维护一份 SDK 类型；
- benchmark 内的 `benchmark-runtime.ts` 是业务运行辅助层（UI 解析 + AI 调用封装），不是 AIPP SDK。

核心代码示意：

```js
class HelloAssistantPlugin {
  config() {
    return { name: "Hello Assistant Plugin", type: ["assistantType"] };
  }

  onAssistantTypeInit(api) {
    api.typeRegist(1, 9901, "Hello Assistant", this);
  }

  onAssistantTypeRun(runApi) {
    return runApi.askAssistant({
      question: `[Hello Plugin] ${runApi.getUserInput()}`,
      assistantId: runApi.getAssistantId(),
      conversationId: runApi.getConversationId()
    });
  }
}

window.HelloAssistantPlugin = HelloAssistantPlugin;
```

---

## 6. 安装与运行步骤

### 方式 A（推荐，自动发现）

1. 把插件目录拷贝到 `<AppDataDir>/plugin/<plugin_code>/`
2. 确保有 `dist/main.js`
3. 重启应用（最稳妥）
4. 在「个人助手」里选择插件注册出的助手类型并测试

### 方式 B（命令安装，支持运行中刷新）

通过 Tauri command 调用：

- `install_plugin`
- `enable_plugin`
- `disable_plugin`
- `uninstall_plugin`

这些命令会触发 `plugin_registry_changed` 事件，`ChatUIWindow` 与 `ConfigWindow` 会自动重载插件列表。

---

## 7. 可用命令（plugin_api）

- `get_plugin_root_dir`
- `list_plugins`
- `get_enabled_plugins`
- `install_plugin`
- `uninstall_plugin`
- `enable_plugin`
- `disable_plugin`
- `get_plugin_config`
- `set_plugin_config`
- `get_plugin_data`
- `set_plugin_data`

---

## 8. 运行时新增能力（Benchmark 所需）

插件 `onPluginLoad(systemApi)` 现在可直接使用：

- `systemApi.listAssistants()`
- `systemApi.listModels()`
- `systemApi.getData(key, sessionId?)`
- `systemApi.getAllData(sessionId?)`
- `systemApi.setData(key, value, sessionId?)`
- `systemApi.runAssistantText({ assistantId, prompt, systemPrompt?, context? })`
- `systemApi.runModelText({ modelId, prompt, systemPrompt?, context? })`
- `systemApi.ui`（宿主 UI 组件集：Button/Input/Textarea/Card/Badge/Alert/...）
- `systemApi.invoke(command, args?)`

说明：

- `runAssistantText` / `runModelText` 走无会话持久化路径，默认不会写入对话列表；
- 需要对话能力时，可继续通过 `systemApi.invoke(...)` 调用现有会话相关命令；
- `systemApi.ui` 让插件可复用宿主 shadcn 风格组件，避免“原生 UI 风格割裂”。

---

## 9. 常见问题

1. **插件不生效**  
   先检查 `dist/main.js` 是否存在，再检查全局构造函数命名是否匹配目录 code。

2. **插件被扫描到但没有运行**  
   检查是否被禁用（`isActive=false`），或 `pluginTypes` 未包含 `assistantType`。

3. **改了插件代码但界面没更新**  
   重启应用，或走命令安装/启用流程触发自动重载。

---

## 10. 相关源码

- 后端注册与扫描：`src-tauri/src/api/plugin_api.rs`
- 运行时加载：`src/services/PluginRuntime.ts`
- 窗口接入：`src/windows/ChatUIWindow.tsx`、`src/windows/ConfigWindow.tsx`
- 类型声明：`src/types/plugin.d.ts`
- 示例插件：`plugin/benchmark-plugin`
