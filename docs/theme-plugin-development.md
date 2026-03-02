# Theme Plugin 开发指南（AIPP）

本文档面向 AIPP 插件开发者，详细说明如何开发「主题插件（theme plugin）」：  
- 如何通过 SDK 注册主题  
- 如何覆写全局样式与窗口级样式  
- 如何稳定地定向修改现有组件（如 Chat Send Button、Input Area、Settings 菜单容器）  

> 适用代码基线：当前仓库中 `PluginRuntime + useTheme + windowCss` 机制。

---

## 1. 设计目标与能力边界

Theme Plugin 的目标是：
1. 注册一个可选主题（支持 light/dark/both）；
2. 覆盖系统 CSS 变量（`variables`）；
3. 注入全局附加 CSS（`extraCss`）；
4. 按窗口精细覆写局部样式（`windowCss`）；
5. 可选提供一个插件 UI（通常用于“一键应用主题”按钮）。

Theme Plugin **不直接修改宿主源码**，而是通过宿主提供的 `SystemApi.registerTheme()` 注入样式定义。

---

## 2. 运行机制总览（必读）

### 2.1 注册链路

1. 插件被 `PluginRuntime` 加载并实例化；
2. `onPluginLoad(systemApi)` 被调用；
3. 插件调用 `systemApi.registerTheme(themeDefinition)`；
4. 宿主将主题写入内存 registry，并生成样式 `<style id="aipp-plugin-theme-<themeId>">`；
5. 宿主把主题 registry 持久化到 `localStorage['aipp-plugin-theme-registry']`；
6. 用户应用主题后，宿主设置 `theme-<themeId>` class 并广播 `theme-changed`。

### 2.2 为什么“所有窗口都生效”

即使某个窗口没有加载 `PluginRuntime`，只要使用了 `useTheme(...)`：
- 也会读取 `aipp-plugin-theme-registry`；
- 在本窗口补注入对应主题样式；
- 应用 `theme-*`、`dark` 与 `aipp-window-*` class；
- 因此窗口切换/多窗口场景也能同步主题。

### 2.3 首屏预加载

`index.html` 启动脚本会在 React 挂载前读取 `theme-mode/theme-name` 与 plugin registry，预先应用主题，减少闪烁（FOUC）。

---

## 3. 插件工程结构

推荐结构（参考 `plugin/guofeng-zhusha-theme-plugin`）：

```text
plugin/<your-theme-plugin>/
├── plugin.json
├── package.json
├── tsconfig.json
├── src/
│   ├── main.ts
│   ├── YourThemePlugin.ts
│   └── YourThemePanel.tsx        # 可选：插件设置面板
└── dist/
    └── main.js                   # 构建产物，宿主实际加载它
```

---

## 4. plugin.json 规范

最小示例：

```json
{
  "id": "my-theme-plugin",
  "code": "my-theme-plugin",
  "name": "My Theme Plugin",
  "version": "0.1.0",
  "entry": "dist/main.js",
  "pluginTypes": ["themeType", "interfaceType", "applicationType"],
  "kinds": ["ui", "worker"]
}
```

关键字段：
- `code`：插件唯一 code，目录名建议与 code 一致；
- `entry`：固定使用 `dist/main.js`；
- `pluginTypes`：
  - `themeType`：声明它是主题插件；
  - `interfaceType`：若你要显示插件 UI 面板，需要包含它；
  - `applicationType`：通常与主题插件一起声明（与现有样例一致）。

---

## 5. 构建与导出要求

### 5.1 tsconfig（关键点）

推荐使用：
- `"module": "none"`
- `"outFile": "dist/main.js"`
- `files` 显式包含 SDK 类型声明：`../../docs/plugin-template/shared/aipp-plugin-sdk.d.ts`

### 5.2 全局构造函数导出（关键）

`src/main.ts` 必须把插件类挂到全局，且**至少一个键可被运行时识别**：

```ts
(window as any)["my-theme-plugin"] = MyThemePlugin;
(window as any).MyThemePlugin = MyThemePlugin;
```

建议第一行键名与 `plugin.json.code` 完全一致，否则可能出现：
`[PluginRuntime] No constructor found for plugin '...'`

---

## 6. Theme SDK 接口（核心）

SDK 定义文件：`docs/plugin-template/shared/aipp-plugin-sdk.d.ts`

主题定义接口：

```ts
type AippSystemApiThemeMode = "light" | "dark" | "both";

interface AippSystemApiThemeDefinition {
  id: string;
  label: string;
  mode?: AippSystemApiThemeMode;
  variables: Record<string, string>;
  description?: string;
  extraCss?: string;
  windowCss?: Record<string, string>;
}
```

SystemApi 相关方法：

```ts
registerTheme(theme: AippSystemApiThemeDefinition): void;
unregisterTheme(themeId: string): void;
listThemes(): Promise<AippSystemApiThemeDefinition[]>;
getDisplayConfig(): Promise<AippSystemApiDisplayConfig>;
applyTheme(themeId: string): Promise<void>;
```

---

## 7. 字段语义详解

### 7.1 `id`
- 主题唯一标识，建议全局唯一、全小写短横线；
- 最终会映射为 class：`theme-<id>`。

### 7.2 `mode`
- `light`：仅浅色生效，选择器形如 `.theme-xxx:not(.dark)`；
- `dark`：仅深色生效，选择器形如 `.theme-xxx.dark`；
- `both`：浅/深都生效，选择器形如 `.theme-xxx`。

### 7.3 `variables`
- CSS 变量字典，键可以写 `--background` 或 `background`（宿主会归一化为 `--*`）；
- 通常值写 HSL token，例如 `"357 73% 44%"`。

### 7.4 `extraCss`
- 主题级附加 CSS（跨窗口）；
- 推荐使用 `:scope` 占位，宿主会替换为主题根选择器（例如 `.theme-guofeng-zhusha:not(.dark)`）。

示例：

```css
:scope .rounded-md {
  border-radius: calc(var(--radius) * 0.72);
}
```

### 7.5 `windowCss`

`windowCss` 让你按窗口 label 覆写样式：

```ts
windowCss: {
  chat_ui: `
    :scope [data-theme-slot="input-area-send-button"] { ... }
  `,
  config: `
    :scope [data-theme-slot="settings-menu-container"] { ... }
  `
}
```

规则：
1. key 是窗口 label（会被规范化）；
2. 宿主会自动加窗口作用域 class：`aipp-window-<label>`；
3. 推荐总是写 `:scope`，宿主会替换为：
   `主题选择器 + .aipp-window-<label>`；
4. 如果不写 `:scope`，宿主会按“后代选择器”拼接，可能不如 `:scope` 精确。

---

## 8. 窗口 label 对照表

当前已接入的窗口作用域（来自 `useTheme("<label>")`）：

- `ask`
- `chat_ui`
- `config`
- `sidebar`
- `schedule`
- `artifact_collections`
- `artifact_preview`
- `artifact`

你在 `windowCss` 里写这些 key 即可做窗口定向覆写。

---

## 9. 稳定样式锚点（Style Slots）

为了避免依赖脆弱 DOM 结构，宿主已暴露这些稳定锚点：

### 9.1 Chat 输入区（`InputArea`）
- `data-theme-slot="input-area"`
- `data-theme-slot="input-area-container"`
- `data-theme-slot="input-area-textarea"`
- `data-theme-slot="input-area-add-button"`
- `data-theme-slot="input-area-send-button"`

### 9.2 Settings 菜单容器（`ConfigWindow`）
- `data-theme-slot="settings-menu-container"`（移动端 + 桌面端均有）

建议优先使用这些 `data-theme-slot` 选择器，而不是写深层 class 链。

---

## 10. 完整主题插件示例（精简版）

```ts
const THEME_ID = "my-theme";

const THEME_VARS = {
  "--background": "60 100% 97%",
  "--foreground": "357 45% 20%",
  "--primary": "357 73% 44%",
  "--primary-foreground": "60 100% 97%",
};

const THEME_EXTRA_CSS = `
:scope .rounded-md { border-radius: 1rem; }
`;

const THEME_WINDOW_CSS = {
  chat_ui: `
    :scope [data-theme-slot="input-area-send-button"] {
      box-shadow: 0 12px 24px -14px rgba(192, 30, 37, .72);
    }
  `,
  config: `
    :scope [data-theme-slot="settings-menu-container"] {
      background: linear-gradient(180deg, hsl(48 45% 94%), hsl(48 38% 91%));
    }
  `,
};

class MyThemePlugin {
  private systemApi: SystemApi | null = null;

  config() {
    return {
      name: "My Theme Plugin",
      type: ["themeType", "interfaceType", "applicationType"],
    };
  }

  onPluginLoad(systemApi: SystemApi) {
    this.systemApi = systemApi;
    this.systemApi.registerTheme({
      id: THEME_ID,
      label: "我的主题",
      mode: "light",
      variables: THEME_VARS,
      extraCss: THEME_EXTRA_CSS,
      windowCss: THEME_WINDOW_CSS,
    });
  }

  // 可选：展示插件面板（如“一键应用主题”）
  renderComponent() {
    return null;
  }
}

(window as any)["my-theme-plugin"] = MyThemePlugin;
(window as any).MyThemePlugin = MyThemePlugin;
```

---

## 11. 开发与调试流程（建议）

1. 在插件目录开发 TS 源码；
2. `npm run build --prefix plugin/<your-plugin>` 生成 `dist/main.js`；
3. 打开 AIPP 插件中心，确认插件已启用；
4. 在显示设置或插件 UI 中应用主题；
5. 用 DevTools 检查：
   - `<html>` 是否有 `theme-<id>`、`aipp-window-<label>`；
   - `<style id="aipp-plugin-theme-<id>">`（运行时）或 `aipp-plugin-theme-preload-<id>`（预加载）是否存在；
   - 目标节点是否有预期 `data-theme-slot`；
6. 调整 CSS 优先级（必要时增加选择器权重，尽量少用 `!important`）。

---

## 12. 常见问题与排查

### Q1: No constructor found
检查 `main.ts` 全局导出键是否包含 `plugin.json.code` 对应键。

### Q2: 主题变量生效，但 windowCss 不生效
优先确认：
1. 是否写了正确窗口 label（如 `chat_ui` / `config`）；
2. 当前窗口 `<html>` 是否含 `aipp-window-<label>`；
3. `windowCss` 是否使用 `:scope`；
4. 目标节点是否存在对应 `data-theme-slot`。

### Q3: 多窗口里有的生效有的不生效
确认该窗口已接入 `useTheme("<label>")`；未接入时不会自动加窗口 scope class。

### Q4: 修改后看不到变化
1. 确认重新构建了插件 dist；
2. 在插件中心执行“刷新插件运行时”或重启应用；
3. 检查浏览器缓存/旧 style 是否残留。

---

## 13. 生产实践建议

1. **优先变量化**：先用 `variables`，再用 `extraCss/windowCss` 做细调；
2. **windowCss 做最小覆盖**：只覆盖必要窗口，避免全局污染；
3. **优先 `data-theme-slot`**：减少对 DOM 结构变化的敏感性；
4. **统一命名**：`themeId`、window key、插件 code 全部规范化；
5. **准备降级策略**：找不到目标节点时不应抛异常（CSS 本身天然容错）。

---

## 14. 相关源码入口

- 运行时主题注入：`src/services/PluginRuntime.ts`
- 窗口主题同步与 scope class：`src/hooks/useTheme.ts`
- 启动预加载注入：`index.html`
- SDK 类型声明：`docs/plugin-template/shared/aipp-plugin-sdk.d.ts`
- 示例主题插件：`plugin/guofeng-zhusha-theme-plugin`
- 样式锚点示例：
  - `src/components/conversation/InputArea.tsx`
  - `src/windows/ConfigWindow.tsx`
