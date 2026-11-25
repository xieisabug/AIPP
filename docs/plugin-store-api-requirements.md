# Plugin 商店 API 需求

## 1. 获取官方推荐插件

### Request

```http
GET /api/plugins
```

### Response

```json
[
  {
    "id": "prompt-optimizer-plugin",
    "code": "prompt-optimizer-plugin",
    "name": "Prompt Optimizer Plugin",
    "description": "Manual assistant prompt optimizer with model-based review and diff preview.",
    "version": "0.1.0",
    "author": "AIPP",
    "tags": ["official", "assistant", "prompt"],
    "pluginTypes": ["interfaceType"],
    "permissions": [
      "assistant.config",
      "conversation.read",
      "assistant.read",
      "assistant.prompt.write"
    ],
    "minAippVersion": "0.4.0",
    "isExperimental": false,
    "source": {
      "type": "zip",
      "url": "https://github.com/xieisabug/AIPP/releases/download/plugins-v0.1.0/prompt-optimizer-plugin-0.1.0.aipp-plugin.zip"
    },
    "dirs": [
      {
        "from": "prompt-optimizer-plugin",
        "to": "prompt-optimizer-plugin"
      }
    ],
    "sourceUrl": "https://github.com/xieisabug/AIPP/tree/main/plugin/prompt-optimizer-plugin",
    "sha256": "sha256:..."
  }
]
```

## 2. 字段要求

### Plugin item

```json
{
  "id": "string",
  "code": "string",
  "name": "string",
  "description": "string",
  "version": "string",
  "author": "string",
  "tags": ["string"],
  "pluginTypes": ["string"],
  "permissions": ["string"],
  "minAippVersion": "string",
  "isExperimental": false,
  "source": {
    "type": "zip",
    "url": "string"
  },
  "dirs": [
    {
      "from": "string",
      "to": "string"
    }
  ],
  "sourceUrl": "string",
  "sha256": "sha256:string",
  "previewImages": ["https://aipp-helper.xiejingyang.com/plugin-previews/<code>-<version>-1.webp"]
}
```

### 必填字段

```json
[
  "id",
  "code",
  "name",
  "description",
  "version",
  "pluginTypes",
  "permissions",
  "source",
  "dirs",
  "sourceUrl",
  "sha256"
]
```

`sha256` 不只是下载校验：客户端安装时会重新拉取本接口，核对来源、`sha256`、`dirs` 与条目一致后才把这次安装记为「官方」。缺少 `sha256` 的条目仍可安装，但不会显示「官方」标记。

### 可选字段

```json
[
  "author",
  "tags",
  "minAippVersion",
  "isExperimental",
  "previewImages"
]
```

`previewImages`：插件市场卡片与详情的效果预览图，第一张做卡片缩略图（约 3:1，建议 1200×400 webp），最多 6 张。客户端只加载 `https://aipp-helper.xiejingyang.com/plugin-previews/` 前缀的地址，其他地址会被丢弃。没有预览图时客户端按插件类型显示通用插画。旧客户端不认识这个字段，会直接忽略。

官方插件的预览图只维护一份：放在 `plugin/<code>/preview/`，在 `plugin.json` 的 `preview` 里列出。打包脚本（`scripts/package-official-plugins.js`）把它们打进 zip，同时导出为 `plugin-previews/<code>-<version>-<n>.<ext>` 并写进 `official-plugins.json` 的 `previewImages`。文件名带版本号，旧图可以长期缓存。已安装的插件包里带图时，客户端只显示包内的图，不再拼接目录图。

### 版本选择与旧客户端

- 同一个 `code` 可以同时保留多个发布版本（例如 plugins-v0.1.0 与 plugins-v0.2.0）。helper 按请求的客户端版本，为每个 `code` 返回 `minAippVersion` 不高于该版本的最新一版；没有任何一版可用时不返回这个插件。这样新版插件依赖新宿主能力时，旧客户端仍拿到它能运行的旧版，而不是丢失插件。
- 客户端从 1.6.2 起在请求头带 `X-AIPP-Version`。不带头的请求来自 1.2.0（首个有官方插件列表的版本）到 1.6.1 之间，无法区分，helper 一律按 1.2.0 处理，保证列给它们的插件在这一段版本里都能运行。
- 新发布只新增条目、不改旧条目：旧条目的 sha256 对应已发布的 zip，改动会让旧客户端的下载校验失败。

### 发布流程

标签只触发 `.github/workflows/package-official-plugins.yml`，不触发应用发版（`release.yml` 只认 `vX.Y.Z`）：

| 标签 | 例子 | 打包范围 |
|---|---|---|
| `plugins-vX.Y.Z` | `plugins-v0.2.0` | `plugin/` 下除 Office 预览外的全部插件 |
| `plugin-<code>-vX.Y.Z` | `plugin-office-preview-plugin-v0.2.0` | 只打这一个插件 |

CI 会建同名 GitHub Release，附带 `<code>-<version>.aipp-plugin.zip`、`official-plugins.json`（含 sha256、previewImages）与预览图。`plugins-v` 后面的号是**批次号**，与各插件自己的版本无关：zip 文件名和客户端判断「可更新」都看 `plugin.json` 的 `version`，没升版本的插件即使重新打包，客户端也不会提示更新。标签推上去不等于发布完成，要完成第 3 步，客户端才会看到新目录。

1. 改 `plugin/<code>/`，提升 `version`；依赖新宿主能力时同时提高 `minAippVersion`。
2. 推 `plugins-vX.Y.Z` 标签（批次号比上一批大），等 Package official plugins 成功。Office 预览走自己的 `plugin-office-preview-plugin-vX.Y.Z` 标签。
3. 在 AIPP-helper 执行 `node scripts/sync-plugin-release.mjs plugins-vX.Y.Z`，把 `official-plugins.json` 原样写入 `src/worker/data/official-plugins.json`、预览图写入 `public/plugin-previews/`；跑 `node --test tests/plugins.test.mjs` 后提交并 `npm run deploy`。单插件标签发布时，在 `src/worker/data/plugins.ts` 手工加条目（参考 Office 预览）。

## 3. source 格式

### Zip source

```json
{
  "type": "zip",
  "url": "https://example.com/plugin.zip"
}
```

### GitHub source

```json
{
  "type": "github",
  "repo": "owner/repo",
  "ref": "main"
}
```

## 4. dirs 格式

```json
[
  {
    "from": "plugin/prompt-optimizer-plugin",
    "to": "prompt-optimizer-plugin"
  }
]
```

`from` 是压缩包里的插件目录相对路径。  
`to` 是安装到本地后的插件目录名，必须等于插件 code。

## 5. 插件包格式

Zip 解压后需要包含：

```text
<plugin_code>/
  plugin.json
  dist/main.js
```

官方推荐插件的 `dirs[].from` 应该指向这个 `<plugin_code>` 目录。

## 6. plugin.json 最小要求

```json
{
  "id": "prompt-optimizer-plugin",
  "code": "prompt-optimizer-plugin",
  "name": "Prompt Optimizer Plugin",
  "version": "0.1.0",
  "description": "Manual assistant prompt optimizer with model-based review and diff preview.",
  "entry": "dist/main.js",
  "runtime": {
    "type": "js",
    "entry": "dist/main.js",
    "checksum": "sha256:..."
  },
  "pluginTypes": ["interfaceType"],
  "permissions": ["assistant.config"],
  "contributions": {}
}
```

## 7. 第一批官方推荐数据

### directory-bang-plugin

```json
{
  "id": "directory-bang-plugin",
  "code": "directory-bang-plugin",
  "name": "Directory Bang Plugin",
  "description": "Adds !directory and !dir bangs backed by the built-in list_directory tool.",
  "version": "0.1.0",
  "author": "AIPP",
  "tags": ["official", "bang", "tool"],
  "pluginTypes": ["toolType", "applicationType"],
  "permissions": ["bang.register"],
  "minAippVersion": "0.4.0",
  "isExperimental": false,
  "source": {
    "type": "zip",
    "url": "https://github.com/xieisabug/AIPP/releases/download/plugins-v0.1.0/directory-bang-plugin-0.1.0.aipp-plugin.zip"
  },
  "dirs": [
    {
      "from": "directory-bang-plugin",
      "to": "directory-bang-plugin"
    }
  ],
  "sourceUrl": "https://github.com/xieisabug/AIPP/tree/main/plugin/directory-bang-plugin",
  "sha256": "sha256:..."
}
```

### run-script-bang-plugin

```json
{
  "id": "run-script-bang-plugin",
  "code": "run-script-bang-plugin",
  "name": "Run Script Bang Plugin",
  "description": "Adds !run_script, !rs and !bash bangs backed by the built-in execute_bash tool.",
  "version": "0.1.0",
  "author": "AIPP",
  "tags": ["official", "bang", "tool", "high-risk"],
  "pluginTypes": ["toolType", "applicationType"],
  "permissions": ["bang.register"],
  "minAippVersion": "0.4.0",
  "isExperimental": false,
  "source": {
    "type": "zip",
    "url": "https://github.com/xieisabug/AIPP/releases/download/plugins-v0.1.0/run-script-bang-plugin-0.1.0.aipp-plugin.zip"
  },
  "dirs": [
    {
      "from": "run-script-bang-plugin",
      "to": "run-script-bang-plugin"
    }
  ],
  "sourceUrl": "https://github.com/xieisabug/AIPP/tree/main/plugin/run-script-bang-plugin",
  "sha256": "sha256:..."
}
```

### think-markdown-plugin

```json
{
  "id": "think-markdown-plugin",
  "code": "think-markdown-plugin",
  "name": "Think Markdown Plugin",
  "description": "Registers a renderer for <think> markdown blocks.",
  "version": "0.1.0",
  "author": "AIPP",
  "tags": ["official", "markdown"],
  "pluginTypes": ["markdownType", "interfaceType"],
  "permissions": ["markdown.register"],
  "minAippVersion": "0.4.0",
  "isExperimental": false,
  "source": {
    "type": "zip",
    "url": "https://github.com/xieisabug/AIPP/releases/download/plugins-v0.1.0/think-markdown-plugin-0.1.0.aipp-plugin.zip"
  },
  "dirs": [
    {
      "from": "think-markdown-plugin",
      "to": "think-markdown-plugin"
    }
  ],
  "sourceUrl": "https://github.com/xieisabug/AIPP/tree/main/plugin/think-markdown-plugin",
  "sha256": "sha256:..."
}
```

### hidden-first-turn-context-plugin

```json
{
  "id": "hidden-first-turn-context-plugin",
  "code": "hidden-first-turn-context-plugin",
  "name": "Hidden First Turn Context Plugin",
  "description": "为助手配置首轮隐藏上下文，并只在首轮模型请求时注入。",
  "version": "0.1.0",
  "author": "AIPP",
  "tags": ["official", "assistant", "hook"],
  "pluginTypes": ["applicationType"],
  "permissions": [
    "assistant.config",
    "data.read.conversation",
    "hook.chat.beforeModelRequest"
  ],
  "minAippVersion": "0.4.0",
  "isExperimental": false,
  "source": {
    "type": "zip",
    "url": "https://github.com/xieisabug/AIPP/releases/download/plugins-v0.1.0/hidden-first-turn-context-plugin-0.1.0.aipp-plugin.zip"
  },
  "dirs": [
    {
      "from": "hidden-first-turn-context-plugin",
      "to": "hidden-first-turn-context-plugin"
    }
  ],
  "sourceUrl": "https://github.com/xieisabug/AIPP/tree/main/plugin/hidden-first-turn-context-plugin",
  "sha256": "sha256:..."
}
```

### prompt-optimizer-plugin

```json
{
  "id": "prompt-optimizer-plugin",
  "code": "prompt-optimizer-plugin",
  "name": "Prompt Optimizer Plugin",
  "description": "Manual assistant prompt optimizer with model-based review and diff preview.",
  "version": "0.1.0",
  "author": "AIPP",
  "tags": ["official", "assistant", "prompt"],
  "pluginTypes": ["interfaceType"],
  "permissions": [
    "assistant.config",
    "conversation.read",
    "assistant.read",
    "assistant.prompt.write"
  ],
  "minAippVersion": "0.4.0",
  "isExperimental": false,
  "source": {
    "type": "zip",
    "url": "https://github.com/xieisabug/AIPP/releases/download/plugins-v0.1.0/prompt-optimizer-plugin-0.1.0.aipp-plugin.zip"
  },
  "dirs": [
    {
      "from": "prompt-optimizer-plugin",
      "to": "prompt-optimizer-plugin"
    }
  ],
  "sourceUrl": "https://github.com/xieisabug/AIPP/tree/main/plugin/prompt-optimizer-plugin",
  "sha256": "sha256:..."
}
```

### usage-dashboard-plugin

```json
{
  "id": "usage-dashboard-plugin",
  "code": "usage-dashboard-plugin",
  "name": "Usage Dashboard Plugin",
  "description": "读取本地 conversation / assistant / llm 数据，生成多维度使用统计与趋势图。",
  "version": "0.1.0",
  "author": "AIPP",
  "tags": ["official", "dashboard", "analytics"],
  "pluginTypes": ["interfaceType", "applicationType"],
  "permissions": [
    "data.read.conversation",
    "data.read.assistant",
    "data.read.llm",
    "plugin.storage"
  ],
  "minAippVersion": "0.4.0",
  "isExperimental": false,
  "source": {
    "type": "zip",
    "url": "https://github.com/xieisabug/AIPP/releases/download/plugins-v0.1.0/usage-dashboard-plugin-0.1.0.aipp-plugin.zip"
  },
  "dirs": [
    {
      "from": "usage-dashboard-plugin",
      "to": "usage-dashboard-plugin"
    }
  ],
  "sourceUrl": "https://github.com/xieisabug/AIPP/tree/main/plugin/usage-dashboard-plugin",
  "sha256": "sha256:..."
}
```

### benchmark-plugin

```json
{
  "id": "benchmark-plugin",
  "code": "benchmark-plugin",
  "name": "Benchmark Plugin",
  "description": "QA benchmark plugin with dataset management, scoring, and leaderboards.",
  "version": "0.1.0",
  "author": "AIPP",
  "tags": ["official", "benchmark", "experimental"],
  "pluginTypes": ["interfaceType", "applicationType"],
  "permissions": [],
  "minAippVersion": "0.4.0",
  "isExperimental": true,
  "source": {
    "type": "zip",
    "url": "https://github.com/xieisabug/AIPP/releases/download/plugins-v0.1.0/benchmark-plugin-0.1.0.aipp-plugin.zip"
  },
  "dirs": [
    {
      "from": "benchmark-plugin",
      "to": "benchmark-plugin"
    }
  ],
  "sourceUrl": "https://github.com/xieisabug/AIPP/tree/main/plugin/benchmark-plugin",
  "sha256": "sha256:..."
}
```

### guofeng-zhusha-theme-plugin

```json
{
  "id": "guofeng-zhusha-theme-plugin",
  "code": "guofeng-zhusha-theme-plugin",
  "name": "国风·朱砂 主题插件",
  "description": "以朱砂红与象牙白为主色的一键国风主题插件。",
  "version": "0.1.0",
  "author": "AIPP",
  "tags": ["official", "theme"],
  "pluginTypes": ["themeType", "interfaceType", "applicationType"],
  "permissions": [],
  "minAippVersion": "0.4.0",
  "isExperimental": false,
  "source": {
    "type": "zip",
    "url": "https://github.com/xieisabug/AIPP/releases/download/plugins-v0.1.0/guofeng-zhusha-theme-plugin-0.1.0.aipp-plugin.zip"
  },
  "dirs": [
    {
      "from": "guofeng-zhusha-theme-plugin",
      "to": "guofeng-zhusha-theme-plugin"
    }
  ],
  "sourceUrl": "https://github.com/xieisabug/AIPP/tree/main/plugin/guofeng-zhusha-theme-plugin",
  "sha256": "sha256:..."
}
```
