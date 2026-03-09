# AIPP Plugin Bang + Markdown 扩展方案（已按当前代码落地修订）

## 1. 当前结论

本轮实现采取两条明确边界：

1. **Bang 仍由 Rust 执行**，因为 prompt 渲染链路在 Rust：`ask_ai`、`scheduled_task_api`、`get_bang_list` 都依赖 `TemplateEngine`。
2. **Markdown 由前端渲染插件扩展**，因为 `UnifiedMarkdown`、`rehype/remark`、React 组件树都在前端窗口里。

因此本次不是把 bang 变成 “TS 回调”，而是：

- Rust 负责解析与执行 bang；
- plugin manifest 负责声明 bang 的路由；
- 前端 runtime 负责注册 markdown tag renderer；
- 两者都纳入现有插件体系，而**不引入 Rust 动态库插件**。

---

## 2. 已实现的能力模型

### 2.1 Bang：manifest 声明 + Rust 执行

插件现在可以在 `plugin.json` 中声明：

- `permissions`
- `contributions.bangs`

Rust 会在构建 `TemplateEngine` 时读取**已安装且启用**的插件 manifest，并将 bang 注册进运行时。

### 2.2 Markdown：前端 registry + 插件注册

前端新增 markdown registry，插件可通过 `systemApi.registerMarkdownTag(...)` 注册自定义标签渲染器。

渲染链路已接入：

- `PluginRuntime`
- `AskWindow`
- `ChatUIWindow`
- `UnifiedMarkdown`
- `useMarkdownConfig`
- 动态 sanitize schema

这意味着 `<think>...</think>` 这类标签，现在不需要硬编码在宿主里，也可以由插件提供。

---

## 3. Bang 的执行架构

### 3.1 运行链路

```text
plugin.json contributions.bangs
    -> backend plugin manifest scan
    -> build_template_engine(app_handle)
    -> TemplateEngine 注册动态 bang
    -> ask_ai / scheduled_task_api / get_bang_list 共用同一套 bang 集合
    -> bang executor 路由到 builtin MCP tool / 已配置 MCP / 插件自带 MCP
```

### 3.2 支持的 executor 类型

#### A. `builtinTool`

路由到 AIPP 内置 builtin MCP tool，例如：

- `aipp:operation::list_directory`
- `aipp:operation::execute_bash`

适合：

- `directory`
- `run_script`
- 未来的 `read_file`、`write_file`、`search_file` 等宿主能力入口

#### B. `mcpTool`

路由到**用户已经配置好的 MCP server**。

适合：

- 让 bang 成为 MCP tool 的语法糖；
- 例如把某个 `web_proxy::fetch_url` 绑定成 `!web_proxy(...)`。

#### C. `pluginMcpTool`

插件 manifest 直接携带 server 定义（当前实现支持 `stdio` / `http`），Rust 在运行时构造临时 MCP server 并执行对应 tool。

适合：

- 插件自己带一套独立能力；
- 不要求开发者先去手动配置 MCP server；
- 这是“新能力 bang”的真正扩展点。

> 当前没有做 Rust 原生动态库加载；插件如果要做“新能力”，推荐走 **TS/Node/Python sidecar + MCP** 这条路。

---

## 4. Bang manifest 结构

```json
{
  "permissions": ["bang.register"],
  "contributions": {
    "bangs": [
      {
        "name": "directory",
        "aliases": ["dir"],
        "complete": "directory(|)",
        "description": "列出指定目录内容",
        "executor": {
          "type": "builtinTool",
          "command": "aipp:operation",
          "toolName": "list_directory",
          "arguments": {
            "path": {
              "source": "firstArg",
              "required": true
            },
            "pattern": {
              "source": "named",
              "name": "pattern"
            },
            "recursive": {
              "source": "named",
              "name": "recursive",
              "valueType": "boolean"
            }
          }
        }
      }
    ]
  }
}
```

### 参数映射说明

`arguments` 支持把 bang 参数映射为 tool 参数，当前支持：

- `raw`：整个参数字符串
- `arg`：按位置取第 N 个参数
- `firstArg`：第一个位置参数
- `named`：取 `key=value`
- `context`：从宿主上下文取值（例如 `selected_text`、`conversation_id`）
- `const`：常量值

支持的 `valueType`：

- `string`
- `number`
- `boolean`
- `json`

这使得 bang 不再被“固定配置项”限制，而是可以成为**任意 tool 的语法入口**。

---

## 5. 为什么这比旧方案更灵活

旧问题在于：

- 如果 bang 只能绑定固定 hostOperation，开发者只能在宿主预设能力里打转；
- 它无法自然承载“全新能力”。

现在的思路变成：

- **bang 不是能力本身，而是能力入口**；
- 真正的能力来自：
  - builtin MCP tool
  - 用户 MCP server
  - 插件自带 MCP server

所以：

- `directory` / `dir` 只是对 `list_directory` 的语法糖；
- `run_script` / `bash` 只是对 `execute_bash` 的语法糖；
- `web_proxy` 可以绑定到已有 MCP tool；
- 未来任何新的插件能力，都可以通过 `pluginMcpTool` 暴露为新的 bang。

---

## 6. Markdown 扩展架构

### 6.1 当前实现

前端新增了 markdown registry，插件可注册：

```ts
systemApi.registerMarkdownTag({
  tagName: "think",
  attributes: ["summary"],
  render({ children, attributes }) {
    // 返回 React 节点
  },
});
```

宿主会自动处理：

- 将 tag 加入 sanitize allowlist
- 在 `UnifiedMarkdown` 中注入组件映射
- 在 Ask / Chat 两个窗口中生效

### 6.2 为什么 markdown 适合前端插件化

因为它天然属于 UI 渲染层：

- 需要 React 组件；
- 需要 window-local UI kit；
- 不应该强绑定 Rust prompt 解析层。

因此 markdown 扩展继续走 `PluginRuntime + SystemApi` 是合理的。

---

## 7. 已实现的示例插件

### 7.1 `directory-bang-plugin`

位置：`plugin/directory-bang-plugin`

提供：

- `!directory(...)`
- `!dir(...)`

底层路由：

- `aipp:operation::list_directory`

示例：

```text
!directory(./src)
!dir(./src, recursive=true)
!dir(./src, pattern="*.rs", recursive=true)
```

### 7.2 `run-script-bang-plugin`

位置：`plugin/run-script-bang-plugin`

提供：

- `!run_script(...)`
- `!rs(...)`
- `!bash(...)`

底层路由：

- `aipp:operation::execute_bash`

示例：

```text
!run_script(pwd)
!bash(ls -la ./src)
!rs(git status --short)
```

### 7.3 `think-markdown-plugin`

位置：`plugin/think-markdown-plugin`

提供：

- `<think>...</think>` 的自定义渲染

默认渲染为可折叠的 `details/summary` 区块。

---

## 8. 权限与运行限制

### 8.1 Bang 注册权限

插件若要声明 bang，必须具备：

```json
"permissions": ["bang.register"]
```

### 8.2 Markdown 注册权限

插件若要注册 markdown tag，必须具备：

```json
"permissions": ["markdown.register"]
```

### 8.3 目录 / Shell 类 bang 仍受宿主权限控制

虽然 bang 可以路由到 `list_directory` / `execute_bash`，但这些底层仍然遵守 AIPP 现有权限模型：

- ChatWindow：有交互式权限弹窗
- AskWindow：本轮已补接 permission dialog
- Scheduled Task：**没有交互授权能力**，仍然只能依赖预授权 / 白名单

所以 bang 的扩展性变强了，但**安全边界仍由宿主控制**。

---

## 9. 关于“plugin 目录下实现，是否真的能跑”

需要区分两件事：

### A. 仓库里的 `plugin/xxx`

这是**示例插件源码工程**，本轮已经补齐并可直接构建出 `dist/main.js`。

### B. 真正运行时加载目录

当前 AIPP 仍从：

```text
appData/plugin/<code>/dist/main.js
```

扫描与加载插件。

也就是说：

- 仓库里的 `plugin/` 现在是“可构建、可作为安装源的示例工程”；
- 真正运行时仍需要把插件安装/复制到 appData 插件目录。

这和现有主题插件机制一致，本轮没有改成“直接从仓库 plugin/ 自动加载”。

---

## 10. 为什么暂时不引入 Rust 原生插件

这轮仍然保持“不引入 Rust 动态库插件”的判断，原因不变：

1. 动态装载跨平台复杂度高；
2. ABI / 版本兼容风险高；
3. 宿主稳定性和崩溃隔离差；
4. 安全边界难做；
5. 对插件开发者门槛过高。

如果未来要进一步增强“新能力插件”，更推荐：

- `pluginMcpTool` + sidecar 进程；
- 或独立 worker / MCP server。

这条路径比 Rust 原生动态插件稳得多。

---

## 11. 本轮代码侧已落地内容

### Backend / Rust

- 扩展插件 manifest 解析：支持 `permissions`、`contributions.bangs`
- 新增动态 bang 构建：`build_template_engine(app_handle)`
- `ask_ai` / `scheduled_task_api` / `get_bang_list` 统一改为使用动态 bang 构建
- bang executor 支持：
  - `builtinTool`
  - `mcpTool`
  - `pluginMcpTool`
- stdio MCP command 解析改为支持引号/转义，便于插件自带 sidecar command

### Frontend / React

- 新增 markdown registry
- 扩展 `PluginRuntime` 与插件 SDK：支持 markdown tag 注册 / 反注册 / 列表
- `useMarkdownConfig` 改为动态生成 sanitize schema 与组件映射
- `AskWindow` 补接插件加载与 operation permission dialog
- 移除宿主内置 `<think>/antthinking` 的硬编码依赖，改为插件提供

### Samples

- `directory-bang-plugin`
- `run-script-bang-plugin`
- `think-markdown-plugin`

---

## 12. 仍然保留的后续增强点

1. 提供更友好的 bang 参数 DSL（例如 shell-like 参数或 schema 驱动补全）；
2. 支持插件声明更细粒度的 bang 权限；
3. 支持 pluginMcpTool 的更多 transport / headers / auth 注入能力；
4. 提供“从仓库示例插件一键安装到 appData”的开发者体验工具；
5. 给 bang 补充更强的 autocomplete 展示（参数说明、来源提示、权限提示）。

---

## 13. 最终结论

这次落地后：

- bang 的执行仍然安全地留在 Rust；
- bang 的扩展性不再被 config-only 限死；
- markdown 真正进入插件注册模型；
- `directory` / `run_script` / `<think>` 三个例子已经把底层通路跑通。

如果后续你要做 `web_proxy`、`dir_tree`、`git_diff`、`repo_search`、`browser_fetch` 之类的 bang，优先建议：

1. 先把真实能力做成 builtin MCP tool 或 plugin MCP tool；
2. 再用 bang 作为语法入口挂上去。

这样 bang 会一直保持“轻语法、重能力底座”的扩展路线，不会再卡死在宿主预设配置里。
