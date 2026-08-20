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
  "sha256": "sha256:string"
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

### 可选字段

```json
[
  "author",
  "tags",
  "minAippVersion",
  "isExperimental"
]
```

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
