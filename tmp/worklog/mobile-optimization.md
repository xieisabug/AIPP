# 移动端优化工作日志

> 分支：`optimize/mobile`　PRD：`docs/mobile-optimization-prd.md`　工程规范：`AGENTS-mobile.md`

## 2026-08-15

### 已完成

- 创建移动端专项文档：
  - `AGENTS-mobile.md`：移动端现状、检测机制、导航方案、桌面功能兼容对照表、改动规范。
  - `docs/mobile-optimization-prd.md`：P0–P3 分级优化需求与验收标准。
  - `AGENTS.md`：新增移动端指引（指向 AGENTS-mobile.md）与"边干活边写工作日志"规范（Development Guidelines 第 9 条，日志放 `tmp/worklog/`）。
- 新分支 `optimize/mobile`（基于 main）。

## 2026-08-15（P0 地基 + P1-5 功能降级）

### P0-1 平台与宽度检测分离

- Rust：新增 `get_platform` 命令（`src-tauri/src/api/system_api.rs`，返回 windows/macos/linux/android/ios），已注册到 `lib.rs` invoke_handler。自定义 command 不受 capabilities 约束，未动 mobile.json。
- 前端：新建 `src/hooks/use-platform.ts`——`usePlatform()` / `useIsMobilePlatform()`，模块级缓存只请求一次。
- `src/hooks/use-mobile.ts` 补注释明确语义：`useIsMobile()` 仅表示窄屏布局（<768px），不是平台判断。

### P0-2 视口与安全区

- `index.html`：viewport 加 `viewport-fit=cover`。
- `src/App.css`：新增移动端基础设施——全局覆盖 `.h-screen` 为 `100vh; 100dvh`（桌面 dvh≡vh，无副作用），新增 `.safe-top` / `.safe-bottom` / `.safe-x` / `.safe-bottom-bar` 工具类。
- 应用安全区：`ChatUIWindow.tsx` 移动端顶栏加 `safe-top`；`src/styles/InputArea.css` 移动端输入框/发送按钮/添加按钮的 bottom 偏移叠加 `env(safe-area-inset-bottom)`。

### P0-3 虚拟键盘适配

- `src-tauri/gen/android/app/src/main/AndroidManifest.xml`：MainActivity 加 `android:windowSoftInputMode="adjustResize"`。
- `src/components/ConversationUI.tsx`：移动端监听 `visualViewport` resize，高度收缩 >150px 视为键盘弹起，强制 `smartScroll(true, "auto")` 滚到底。

### P1-5 功能降级清单 + 前端隐藏入口

- 新建 `src/lib/mobileUnsupported.ts`：`MobileUnsupportedFeature` 中央清单（autostart / global_shortcut / app_updater / script_execution / browser_search）、`MOBILE_UNSUPPORTED_FEATURES`、`BROWSER_SEARCH_ENV_KEYS`（内置 search 工具中依赖 chromiumoxide 的环境变量 key）、hook `useFeatureAvailableOnPlatform()`（桌面一律 true，移动端对清单内功能返回 false）。
- 按平台隐藏入口（桌面零变化）：
  - `OtherConfigForm.tsx`：移动端隐藏"开机自启动"设置项，并跳过 `get_autostart_state` 加载（避免报错后卡在加载态）。
  - `AboutConfigForm.tsx`：移动端只保留版本展示，隐藏更新状态徽章 / 更新说明 / 检查更新按钮。
  - `ShortcutsConfigForm.tsx`：移动端隐藏"全局快捷键"区块，应用内快捷键保留。
  - `CodeBlock.tsx` / `RustCodeBlock.tsx`：移动端隐藏代码块"运行"按钮（SquareTerminal → run_artifacts）。
  - `BuiltinToolDialog.tsx`：移动端渲染内置工具环境变量时过滤 `BROWSER_SEARCH_ENV_KEYS`（USER_DATA_DIR、HEADLESS、WAIT_*、POOL_*、DEBUG_* 等浏览器专属字段）。
- Rust 侧错误码统一（P1-5 验收要求）：
  - `system_api.rs` 新增 `MOBILE_UNSUPPORTED_PREFIX = "MOBILE_UNSUPPORTED"` 与 `mobile_unsupported_error(feature)`。
  - 统一改写：剪贴板图片复制、开机自启动（system_api.rs）、自动更新三处（updater_api.rs）、PDF 导出（export_api.rs，AppError 包装保留）。
  - 前端 `mobileUnsupported.ts` 导出 `MOBILE_UNSUPPORTED_ERROR_PREFIX` 与 `isMobileUnsupportedError()` 用于统一识别。

### 验证

- `npx tsc --noEmit` 通过（exit 0）。
- `npm run build` 通过（exit 0，4.27s）。
- `cargo check --manifest-path src-tauri/Cargo.toml` 通过（exit 0，2m28s；102 个 warning 均为既有警告，与本次改动无关）。
- `AGENTS-mobile.md` 已同步更新：检测机制、视口/触控、桌面功能对照表改为落地后的现状。

### 发现但未动的可疑桌面专属入口

- `DisplayConfigForm.tsx` "默认主页窗口"（tooltip 提到托盘点击唤醒，移动端无托盘/多窗口）。
- `PreviewConfigForm.tsx` / `LLMProviderConfigForm.tsx` 的 Bun 运行时安装（React/Vue 组件预览依赖 Bun，Android 无此环境，但属"预览"而非脚本执行，未动）。
- `DisplayConfigForm.tsx` "消息完成时发送系统通知"（Android 通知能力未确认，未动）。
- 内置 MCP "操作工具"（execute_bash 等）与 "搜索工具"整体在移动端基本不可用，但属 MCP 配置区而非独立开关，本次只隐藏了浏览器专属字段。
- `SidebarWindow.tsx` / `AskWindow.tsx` 里的 `run_artifacts` 预览入口——属多窗口/预览范畴，留给 P1-4 MobileShell 整体处理。

### 未完成 / 下一步

- P1-4 MobileShell 单页导航（移除 `__setAppWindow` hack）、P1-6 权限收敛（mobile.json fs scope）、P2 触控体验、P3 Android 原生集成。
- P0/P1-5 改动未在 Android 真机验证，仅通过编译级检查。

## 2026-08-16（设置页打开慢：前端加载/挂载优化）

### 背景

调研结论：设置页慢的根因在前端静态产物图谱，不在 Rust 侧。ConfigWindow 静态 import 六个设置页打出 330KB chunk；DisplayConfigForm 为一个下拉选项静态拖入整个聊天消息渲染树（26 模块/248KB 源码）；每次打开设置都 `loadPlugins(true)` 强制重载插件；pinyin-pro 292KB 被静态拉入。

### 已完成

- `src/windows/ConfigWindow.tsx`：
  - 六个设置页改为 `React.lazy` 动态 import，内容区包 `Suspense`（黑白灰"加载中..."占位）。产物 ConfigWindow chunk 330KB → 10.8KB，LLMProviderConfig 57.6KB / AssistantConfig 107.5KB / FeatureAssistantConfig 58.9KB / MCPConfig 46KB / SkillsConfig 12.1KB / PluginCenterConfig 29.1KB 各自独立按需加载。
  - 首次 `loadPlugins(true)` 改为 `loadPlugins()`（非 forceReload 时插件已加载则直接返回、`<script>` 用稳定 cacheKey 可缓存）；仅 `plugin_registry_changed` 事件才 forceReload。
- `src/components/config/feature/forms/DisplayConfigForm.tsx`：`ensureBuiltinMcpToolComponentsRegistered` 改为 `import("@/services/builtinMcpToolComponents")` 动态加载，注册完成后 registry 的 `useSyncExternalStore` 自动刷新下拉选项。产物中 builtinMcpToolComponents 独立 chunk 197.2KB，不再进设置页静态图谱。
- `src/utils/pinyinFilter.ts`：移除顶层 `import { pinyin } from "pinyin-pro"`，新增 `loadPinyin()`（动态 import + 单例缓存）与 `usePinyinReady(active)` hook；`getPinyinIndex` 在未加载时触发加载并返回 null（本次调用仅精确匹配），加载完成后由 hook 触发重渲染补上拼音结果。产物 pinyinFilter chunk 6.2KB，pinyin-pro 本体独立 chunk（esm-*.js 279.7KB）仅动态引用。
- 调用点接入 `usePinyinReady`（搜索框有输入才激活，加入过滤 useMemo 依赖）：`LLMProviderConfig` / `AssistantConfig` / `SkillsConfig` / `MCPConfig` / `ModelSelectionDialog` / `AssistantMCPConfigDialog` / `AssistantSkillsConfigDialog`；`InputArea`（补全弹窗可见时激活，slash 重过滤 effect 加依赖）；`AssistantCompletionList` 改 `import type`。
- `src/utils/pinyinFilter.test.ts`：`beforeAll` 中 `await loadPinyin()` 适配异步加载。

### 验证

- `npm run build` 通过（exit 0，5.94s），上述 chunk 拆分结果来自 dist 产物实测。
- `npx vitest run src/utils/pinyinFilter.test.ts` 4 个测试全部通过。

### 未完成 / 下一步

- 未在 Android 真机验证设置页打开耗时的实际改善幅度。
- ChatUI 冷启动静态图谱仍有 2.06MB（含 katex 253KB、useArtifactEvents 651KB 等），后续可同样按入口拆包。
- `LLMProviderConfig` 的 sidebar useMemo 在移动端下拉模式下仍构造整棵元素树（未动，收益小）。
