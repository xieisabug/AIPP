# 插件开发说明

> **提示**：以下文档基于当前 `feat/plugin-design-v2` 分支的实现，如有代码变动请同步更新。

## 目录
1. 插件目标与概览
2. 插件分类
3. 文件结构与加载机制
4. 插件入口类与生命周期
5. 公共类型（TypeScript 声明）
6. 创建自定义助手类型插件示例
7. 常见问题
8. 参考源码
9. 子任务图标（iconComponent）

---

## 1. 插件目标与概览
插件机制旨在让开发者以 **最小侵入** 的方式扩展应用能力，例如：
* 新增「助手类型」（如代码生成、学术写作等）
* 向界面注入独立窗口或组件
* 扩展后台逻辑，处理自定义数据或算法

目前（`feat/plugin-design-v2`）阶段已经落地的是 **助手类型插件**，其它类型的生命周期接口已预留。

---

## 2. 插件分类
枚举 `PluginType` 定义了三种插件形态：

| 枚举值 | 名称 | 适用场景 |
| ------- | ---- | -------- |
| `AssistantType` (1) | 助手类型插件 | 在「个人助手配置」页面注册并渲染新的助手类型 |
| `InterfaceType` (2) | 界面插件 | 渲染独立窗口或嵌入式 UI（即将支持） |
| `ApplicationType` (3) | 应用插件 | 脱离 UI、提供后台能力（即将支持） |

> 当前仅实现 `AssistantType`，其余类型请关注后续更新。

---

## 3. 文件结构与加载机制
```
<AppDataDir>/plugin/<plugin_code>/
 └─ dist/
    └─ main.js  # 产物入口，**必须**导出全局插件类
```

* `<AppDataDir>` 由 Tauri 提供，跨平台自动定位。
* `ConfigWindow` 与 `PluginWindow` 会在运行时向 `document` 注入 `<script>`，脚本路径即 `main.js`。
* 加载完成后框架在 `window` 域查找插件类（默认示例为 `SamplePlugin`），随后实例化并触发相应生命周期函数。

---

## 4. 插件入口类与生命周期
一个最小可用的助手类型插件需实现以下方法：

```ts
class MyPlugin {
  /** 可选：插件加载完成后调用 */
  onPluginLoad(systemApi) {
    console.log("插件已加载", systemApi);
  }

  /** 描述信息，框架据此判断插件类型 */
  config() {
    return {
      name: "代码生成",
      type: ["assistantType"] // 数组，可同时声明多种类型
    };
  }

  /** AssistantType 生命周期 ↓ */
  onAssistantTypeInit(assistantTypeApi) {
    // 1. 注册类型（code 请避免与现有重复）
    assistantTypeApi.typeRegist(100, "代码生成助手", this);

    // 2. 新增字段
    assistantTypeApi.addField({
      fieldName: "language",
      label: "目标语言",
      type: "string",
      fieldConfig: { position: "body", tips: "例如 javascript / python" }
    });
  }

  onAssistantTypeSelect(assistantTypeApi) {
    // 用户在下拉框选中该类型时触发，可设置默认值 / 隐藏字段
    assistantTypeApi.forceFieldValue("max_tokens", "2048");
  }

  onAssistantTypeRun(assistantRunApi) {
    // 点击「运行」后触发，可调用 askAI / askAssistant
    const question = assistantRunApi.getUserInput();
    return assistantRunApi.askAI({
      question: question,
      modelId: assistantRunApi.getModelId()
    });
  }

  /** InterfaceType 可实现如下方法 ↓ */
  renderComponent() {
    return <h1>Hello From Plugin UI</h1>;
  }
}

// **务必**挂载到全局，名称不定，但需与加载列表保持一致
window.SamplePlugin = MyPlugin;
```

---

## 5. 公共类型（TypeScript 声明）
`src/types/plugin.d.ts` 暴露了所有可调用接口，常用结构如下：

* `AippPlugin`：基础类，定义 `onPluginLoad` / `renderComponent` / `config`。
* `AippAssistantTypePlugin`：扩展了 `AippPlugin`，增加 **助手类型三大生命周期**：
  * `onAssistantTypeInit`
  * `onAssistantTypeSelect`
  * `onAssistantTypeRun`
* `AssistantTypeApi`：配置阶段可用，支持注册类型、新增/隐藏字段、修改 Label、添加提示等。
* `AssistantRunApi`：运行阶段可用，封装了 `askAI`、`askAssistant`、`appendAiResponse` 等常用方法。

完整签名请直接查阅文件，以获得参数及泛型信息。

---

## 6. 创建自定义助手类型插件示例
### 1) 初始化项目
```bash
# 任选前端技术栈，以下以 Vite + React + TypeScript 为例
npm create vite@latest code-generate-plugin -- --template react-ts
cd code-generate-plugin
npm i
```
### 2) 实现插件入口
在 `src/main.tsx` （或任意入口）追加：
```ts
// 插件核心代码（同上示例），此处略
class CodeGeneratePlugin { /* ... */ }

// 注意全局挂载名称要与主程序加载列表保持一致
(window as any).SamplePlugin = CodeGeneratePlugin;
```
### 3) 构建产物
```bash
npm run build          # 默认产物位于 dist/
```
### 4) 安装到 AI Assistant
将 `dist/` 整体拷贝至：
```
<AppDataDir>/plugin/code-generate/dist/
```
重启应用或切换到「设置 -> 个人助手配置」即可看到「代码生成助手」。

---

## 7. 常见问题
1. **加载路径错误**  
   请检查产物是否位于 `<AppDataDir>/plugin/<plugin_code>/dist/main.js`。
2. **未找到插件类**  
   `window.SamplePlugin` 未挂载或命名不一致，确保加载列表中的 `code` 与挂载名称对应。
3. **API 版本不匹配**  
   更新主程序后应同步对照 `src/types/plugin.d.ts`，以免类型签名变动导致运行异常。

---

## 8. 参考源码
* `src/ConfigWindow.tsx` – 前端插件加载入口
* `src/components/config/AssistantConfig.tsx` – 助手类型插件生命周期注入
* `src/PluginWindow.tsx` – 独立窗口渲染逻辑
* `src/types/plugin.d.ts` – 插件公共类型定义
* `src-tauri/src/db/plugin_db.rs` – 后台插件元数据存储逻辑

如有疑问或改进建议，请提交 Issue 🙏

---

## 9. 子任务图标（iconComponent）

当前仅支持通过前端组件注册子任务图标，无需、也不会将图标持久化到后端数据库。

- 在注册子任务（`subTaskRegist`）时可传入 `iconComponent`，类型兼容：
  - `React.ReactNode`（直接渲染的节点）
  - `React.ComponentType<{ className?: string; size?: number }>`（如 `lucide-react` 图标）
- 框架会将图标按 `code` 注册到运行期前端的注册表，后续 UI 将在子任务选择/详情处显示。

示例（lucide-react）：

```tsx
import { Wrench } from 'lucide-react';

class MyPlugin {
  onAssistantTypeInit(assistantTypeApi) {
    assistantTypeApi.subTaskRegist({
      code: 'fix-bug',
      name: '修复Bug',
      description: '自动定位并修复常见问题',
      systemPrompt: 'You are a helpful bug fixer.',
      iconComponent: Wrench, // 也可传 <Wrench className="h-4 w-4" />
    });
  }
}

// 将插件类挂到 window 以供主程序加载
window.SamplePlugin = MyPlugin;
```

说明：

- `lucide-react` 的组件支持 `size`、`className` 等属性，框架会在渲染时设置合适的 `size`，也可在节点模式自定义样式。
- 仅前端注册，无需后端 API 或数据库调整；若后续需要持久化静态图片/emoji，可另行扩展。