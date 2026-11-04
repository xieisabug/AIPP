# AIPP 移动端开发指南

> 本文档是 AIPP 移动端（当前仅 Android）开发的专项指南，补充根目录 `AGENTS.md`。
> 涉及移动端相关的开发任务时，必须先阅读本文档。

## 当前状态定位

移动端正在按功能逐项适配：

- 启动进入单 `chat_ui` WebView，聊天使用顶栏、Sheet 会话抽屉和移动模式输入框。
- 未接入 MobileShell 的独立窗口仍须逐项评估移动兼容性，不能直接开放桌面窗口入口。
- 移动端使用 `src/mobile/MobileShell.tsx` 的单 WebView 导航，已接入聊天、设置、收藏与受限预览；页面保留挂载以保留聊天订阅和输入。
- 移动端优化规划见 `docs/mobile-optimization-prd.md`。

## 平台与构建

- **目标平台**：Android（`src-tauri/gen/android`），applicationId `com.xieisabug.aipp`，minSdk 24 / targetSdk 36。
- **开发命令**：`pnpm run android`（即 `scripts/android-dev.ps1`，自动选局域网 IP 并通过 `--host` 显式传给 `tauri android dev`，支持 HMR；可追加 CLI 列出的设备名称）。直接运行 `pnpm run tauri android dev` 时，多网卡环境应显式传 `--host <局域网 IP>`，避免选到 VPN/TUN 网卡。启动前关闭同项目旧 dev 进程以释放 3000 端口。
- **启动差异**：移动端启动时 Rust 侧只创建 `chat_ui` 一个窗口（`src-tauri/src/lib.rs` 中移动端分支），桌面端会预创建多个隐藏窗口。
- **能力配置**：移动端权限在 `src-tauri/capabilities/mobile.json`（与桌面端 `migrated.json` 分离）。修改移动端可用的 Tauri API 时必须更新此文件，且注意 fs scope 不要无限制放开。
- **Android 原生配置**：`src-tauri/gen/android/app/src/main/AndroidManifest.xml` 当前声明 `INTERNET` 权限，Activity 已配置 `windowSoftInputMode="adjustResize"`；尚无 deep link、系统分享接收（ACTION_SEND）和拍照（ACTION_IMAGE_CAPTURE）intent-filter。`POST_NOTIFICATIONS` 由 `tauri-plugin-notification` 自带 Manifest 合并进入最终清单（见 `app/build/intermediates/merged_manifest/**/AndroidManifest.xml`），但仓库内没有运行时请求权限的代码。应用为 `singleTask`，任何新增原生入口都必须同时处理冷启动 intent 与 `MainActivity.onNewIntent`，不能只接一头。修改原生权限与入口时需同步检查 Manifest、Tauri capability 和 Android 生命周期行为。
- **HTTPS 初始化**：reqwest 0.13 的 Android 系统证书校验依赖 `MainActivity.onWebViewCreate` 调用 JNI 初始化，以及 Gradle 引入 Cargo 锁定版本附带的 `rustls-platform-verifier-android` AAR；缺少初始化会使请求线程 panic。更新依赖时须同步检查这条链路及 ProGuard 保留规则。

## 核心机制与已知坑

### 1. 移动端检测（重要，抽象有问题）

- `src/hooks/use-mobile.ts` 用 `window.innerWidth < 768` 判定——**它检测的是"窄屏"而非"移动平台"**。
- 后果：桌面端把窗口拖窄到 <768px 也会触发移动端布局；Android 平板横屏会被误判为桌面。
- **使用规范**：
  - 需要"窄屏响应式布局"时，用 `useIsMobile()`。
  - 需要"是否移动平台"（禁用桌面功能、平台差异化行为）时，**不要**用宽度判断；用 `src/hooks/use-platform.ts` 的 `usePlatform()` / `useIsMobilePlatform()`（基于 Rust 命令 `get_platform`，P0-1 已落地）。
- **功能降级清单**：移动端不支持的功能集中在 `src/lib/mobileUnsupported.ts`（`useFeatureAvailableOnPlatform`），新增移动端限制时在此登记并隐藏入口。后端统一错误前缀 `MOBILE_UNSUPPORTED`（`system_api::mobile_unsupported_error`），前端用 `isMobileUnsupportedError` 识别。

### 2. 移动端导航

- 使用 `src/mobile/navigation.ts` 的 `navigateMobile` / `previewMobile`，仅注册已适配的页面；不要创建额外 WebView。
- Android `MainActivity` 先收起 IME，再调用 `__aippMobileBack` 关闭面板或弹出返回栈，根页退后台。禁止调用 WebView.goBack。
- 返回顺序（`window.__aippMobileBack`）：先对最上层打开的面板（`role=dialog/alertdialog/menu/listbox` 且 `data-state=open`，或 `[data-mobile-panel]`）派发 Escape；再调用当前页通过 `registerMobileBackHandler(route, fn)` 注册的处理器（返回 true 即消费）；最后弹出页面栈。设置页不记录分区历史，返回（系统返回与顶栏图标一致）直接退出设置页，离开前经过 `configLeaveGuard`（确认后用 `popMobile()` 出栈）。
- `handleBackNavigation=false` 禁用 Wry 后注册的默认返回回调。原生 IPC 在 `setContentView` 边界替换为校验主框架与来源的 WebMessageListener：Wry 的 `onWebViewCreate` 早于旧 IPC 注册，不能在该早期回调中完成替换。
- 受限预览契约见 `docs/product/20-移动导航与预览.md`。预览 runtime 以独立 bundle 运行在不含 allow-same-origin 的 iframe 中，不得添加 Tauri 或宿主凭据桥接。

### 3. 视口与触控（P0-2/P0-3 已落地基础设施，改动时注意）

- `index.html` 已加 `viewport-fit=cover`；`src/App.css` 提供 safe-area 工具类（`.safe-top` / `.safe-bottom` / `.safe-x` / `.safe-bottom-bar`），并全局将 `.h-screen` 覆盖为 `100vh; 100dvh`（不支持 dvh 的环境自动回退）。
- 已应用安全区的位置：`ChatUIWindow.tsx` 移动端顶栏（`safe-top`）、`src/styles/InputArea.css` 移动端输入框与按钮（bottom 偏移叠加 `env(safe-area-inset-bottom)`）。
- 虚拟键盘：AndroidManifest 已配 `windowSoftInputMode="adjustResize"`；`ConversationUI.tsx` 移动端监听 `visualViewport`，仅跟随底部阅读状态，键盘弹起不抢占历史阅读位置。
- 消息操作、编辑、版本选择和移动预览使用至少 44px 触控目标；新增页面仍需逐项检查。
- 设置表单分组卡片带 `data-aipp-config-section`，移动端由 `src/mobile/mobile.css` 去掉卡片外框；列表项卡片（插件、技能）不加该属性，保留卡片。
- 设置页窄屏滚动容器由 `ConfigPageLayout` 提供（全宽、内边距在内部，滚动条贴边），页面内容不要再自建带外边距的滚动容器。
- 设置页首次读取完成前统一渲染 `ConfigPageLoading`（`@/components/common`），不要先渲染空状态；新增设置页沿用此约定。
- 移动平台对话框由 `mobile.css` 全屏化并收窄左右内边距、DialogHeader 右侧预留关闭按钮位置；对话框内的行布局用 `flex-wrap` + `min-w-0`，不要依赖固定宽度或单行并排多个开关。
- **新写移动端 UI 时**：可点目标 ≥44px；全屏容器直接用 `h-screen`（已自动走 dvh）；顶部/底部固定栏必须加 `safe-top` / `safe-bottom` 类。

### 4. 桌面功能在移动端的行为

Rust 侧对桌面功能做了编译期/运行期分支；移动端不支持的错误统一带 `MOBILE_UNSUPPORTED` 前缀（`system_api::mobile_unsupported_error`），前端用 `isMobileUnsupportedError` 识别：

| 功能 | 移动端行为 | 位置 |
|---|---|---|
| 全局快捷键 / 系统托盘 | `#[cfg(desktop)]` 不编译；前端设置页已隐藏全局快捷键区块 | `lib.rs` / `ShortcutsConfigForm.tsx` |
| 开机自启 | 返回 `MOBILE_UNSUPPORTED` 错误；前端已隐藏设置项 | `system_api.rs` / `OtherConfigForm.tsx` |
| 自动更新 | 返回 `MOBILE_UNSUPPORTED` 错误；前端已隐藏更新 UI | `updater_api.rs` / `AboutConfigForm.tsx` |
| PDF 导出 | Rust `export_api` 仍返回 `MOBILE_UNSUPPORTED`；Android 前端改走系统打印（`printMobilePdf` → `mobileFileAction("print")`，返回 `print_launched` 不代表已生成 PDF） | `export_api.rs` / `conversationExportService.ts` |
| 剪贴板复制图片 | Rust `copy_image_to_clipboard` 返回 `MOBILE_UNSUPPORTED`；Android 前端走 `mobileFileAction("copyImage")` 原生 ClipData | `system_api.rs` / `LazyImage.tsx` |
| 获取选中文本 | 返回空字符串 | `lib.rs` |
| 脚本/Shell 执行 | **无任何 guard，运行期必失败**；前端已隐藏代码块"运行"按钮 | `mcp/builtin_mcp/operation/bash_ops.rs` / `CodeBlock.tsx` |
| 浏览器模式搜索（chromiumoxide） | 仅桌面依赖；前端已隐藏浏览器专属配置字段 | `Cargo.toml` / `BuiltinToolDialog.tsx` |
| 文件对话框 | Android 走 `mobileFileService.ts`（SAF 导入/保存、分享、打开）；桌面保留原对话框路径 | `mobileFileService.ts` / `conversationExportService.ts` |

- 前端功能降级清单集中在 `src/lib/mobileUnsupported.ts`（`useFeatureAvailableOnPlatform`），新增移动端限制时在此登记。
- **原则**：移动端不支持的功能，前端应根据平台隐藏入口，而不是点了才报错；Rust 侧错误用 `mobile_unsupported_error` 保持统一（禁止静默降级，也不要新增 fallback）。
- 多窗口创建函数（`window.rs`）在 `#[cfg(mobile)]` 下是裸 builder（无尺寸无标题），实际移动端不应创建除 `chat_ui` 外的窗口。

## 移动端改动规范

1. **改动 UI 时同步检查窄屏分支**：`ChatUIWindow.tsx`、`ConfigWindow.tsx`、`ConversationUI.tsx`、`InputArea.tsx` 是仅有的有移动端分支的组件，改动它们的桌面布局时不要破坏移动端分支。
2. **响应式样式**：项目响应式 Tailwind 前缀使用极稀疏（`md:`/`sm:` 合计约百处），不要为了移动端给所有桌面组件加响应式——优先在移动端专用分支/组件里处理。
3. **图标与样式风格**与桌面一致：shadcn/ui + Tailwind，主色调黑白灰，遵循根 `AGENTS.md` 的图标规范。
4. **Android 清单/权限变更**：修改 `AndroidManifest.xml` 或 `mobile.json` 时，说明用途并在 PR 中标注；fs scope 按需收敛，不要 `**`。
5. **测试验证**：移动端相关改动至少验证 `npm run build` + `cargo check --manifest-path src-tauri/Cargo.toml`；涉及 Android 行为的改动需说明是否已在真机/模拟器验证。

## 验证命令

```bash
# 前端（含 TypeScript 检查）
npm run build

# Rust
cargo check --manifest-path src-tauri/Cargo.toml

# Android 开发调试（需设备/模拟器）
npm run android
```

## 能力缺口与需求编号（2026-11-02）

移动端「应该有但目前没有」的功能已登记为 leaderai AIPP 需求 #250–#258，对应 PRD `docs/mobile-optimization-prd.md` 的 MOB-14 至 MOB-19：

| 条目 | leaderai | 落地时的主要触点 |
|---|---|---|
| MOB-14 离线与网络失败可解释 | #250 | `useMobileResume.ts`、`ChatUIWindow.tsx`、输入区 |
| MOB-15 回复期保活与通知闭环 | #251 | `agent_completion.rs`、`MainActivity.onNewIntent`、通知权限请求 |
| MOB-16 会话详情面板 | #252 | `ConversationUI.tsx:2126`、`ChatSidebarContent.tsx`、移动路由 |
| MOB-17.1 接收系统分享 | #253 | Manifest ACTION_SEND、`MobileFileCopy`、草稿态 |
| MOB-17.2 拍照添加附件 | #254 | ACTION_IMAGE_CAPTURE、`file_paths.xml`、`mobileFileService.ts` |
| MOB-17.3 交互打磨 | #257 | `MainActivity` 返回回调、`Theme.aipp`、shortcuts |
| MOB-18 任务创建与编辑 | #255 | `mobile/navigation.ts`、`MobileShell.tsx`、定时任务表单 |
| MOB-19 会话手势与轮次跳转 | #256 #258 | 会话抽屉；轮次跳转已由 `MobileTurnScrubber`（复用 `useTurnRailTracking`）承担 |

新增移动端原生入口时遵守：不创建额外 WebView；intent 只能进入草稿，不能直接执行工具或跳过审批；手势不得与系统返回手势（屏幕边缘约 20dp）冲突。

## B 类文件服务接入（2026-10-31）

- `mobile_files::mobile_file_action` 注册 `MobileFilesPlugin` 原生桥接；系统 SAF 导入/保存、FileProvider 分享/打开、系统打印及图片 ClipData 均复用此服务。外部 URI 只能从系统选择结果获得，Rust 附件命令另行验证应用导入目录。
- `mobileFileService.ts` 分离生成与输出；Android 导出取消不返回成功，打印返回 `print_launched` 不能报告 PDF 已生成，分享返回 `share_launched` 不能报告接收端完成。桌面导出保留原路径。
- `tauri.android.conf.json` 将移动 asset 协议限制在私有附件及技能目录；`mobile.json` 不再开放全盘 fs。修改路径时同步 FileProvider XML，禁止恢复 `**`。
- Skills 移动安装目录统一走 `skill_api::skills_directory` 的 app_data/skills；扫描器移动分支不扫描 desktop home、不执行旧目录迁移。`MobileSkillsManager` 提供应用内安装/查看/绑定，不创建新 WebView。
- 原生行为及接收应用兼容性验证记录在 `tmp/worklog/mobile-files-output-device.md`；未有设备证据的项目不能视为已验收。
