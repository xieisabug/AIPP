# AIPP 移动端开发指南

> 本文档是 AIPP 移动端（当前仅 Android）开发的专项指南，补充根目录 `AGENTS.md`。
> 涉及移动端相关的开发任务时，必须先阅读本文档。

## 当前状态定位

移动端目前处于 **"能跑而非能用"** 的阶段：

- 唯一可用的完整路径是：启动 → 单 `chat_ui` webview → `ChatUIWindow` 的窄屏分支布局（顶栏 + Sheet 抽屉会话列表 + 移动模式输入框）。
- 其余窗口（Schedule / Butler / Artifact / Sidebar / Plugin 等）**没有移动端分支**，打开即为桌面布局。
- 移动端导航依赖 `window.__setAppWindow` 全局函数 hack，属于过渡方案。
- 移动端优化规划见 `docs/mobile-optimization-prd.md`。

## 平台与构建

- **目标平台**：Android（`src-tauri/gen/android`），applicationId `com.xieisabug.aipp`，minSdk 24 / targetSdk 36。
- **开发命令**：`npm run android`（即 `scripts/android-dev.ps1`，自动选局域网 IP 设 `TAURI_DEV_HOST` 后执行 `tauri android dev`，支持 HMR）。
- **启动差异**：移动端启动时 Rust 侧只创建 `chat_ui` 一个窗口（`src-tauri/src/lib.rs` 中移动端分支），桌面端会预创建多个隐藏窗口。
- **能力配置**：移动端权限在 `src-tauri/capabilities/mobile.json`（与桌面端 `migrated.json` 分离）。修改移动端可用的 Tauri API 时必须更新此文件，且注意 fs scope 不要无限制放开。
- **Android 原生配置**：`src-tauri/gen/android/app/src/main/AndroidManifest.xml` 目前仅声明 `INTERNET` 权限，无 deep link、无分享 intent、无 `windowSoftInputMode` 配置。

## 核心机制与已知坑

### 1. 移动端检测（重要，抽象有问题）

- `src/hooks/use-mobile.ts` 用 `window.innerWidth < 768` 判定——**它检测的是"窄屏"而非"移动平台"**。
- 后果：桌面端把窗口拖窄到 <768px 也会触发移动端布局；Android 平板横屏会被误判为桌面。
- **使用规范**：
  - 需要"窄屏响应式布局"时，用 `useIsMobile()`。
  - 需要"是否移动平台"（禁用桌面功能、平台差异化行为）时，**不要**用宽度判断；用 `src/hooks/use-platform.ts` 的 `usePlatform()` / `useIsMobilePlatform()`（基于 Rust 命令 `get_platform`，P0-1 已落地）。
- **功能降级清单**：移动端不支持的功能集中在 `src/lib/mobileUnsupported.ts`（`useFeatureAvailableOnPlatform`），新增移动端限制时在此登记并隐藏入口。后端统一错误前缀 `MOBILE_UNSUPPORTED`（`system_api::mobile_unsupported_error`），前端用 `isMobileUnsupportedError` 识别。

### 2. 移动端导航（过渡方案）

- `src/App.tsx` 暴露全局 `window.__setAppWindow(label)`，用于移动端在单 webview 内切换视图。当前仅 `ConfigWindow.tsx` 和 `ChatUIInfomation.tsx` 消费。
- 长期方向是 `src/mobile/` 下的 `MobileShell` 单页导航架构（目前目录为空），见 PRD P1-4。**新功能不要再扩展 `__setAppWindow` 的使用面**。

### 3. 视口与触控（P0-2/P0-3 已落地基础设施，改动时注意）

- `index.html` 已加 `viewport-fit=cover`；`src/App.css` 提供 safe-area 工具类（`.safe-top` / `.safe-bottom` / `.safe-x` / `.safe-bottom-bar`），并全局将 `.h-screen` 覆盖为 `100vh; 100dvh`（不支持 dvh 的环境自动回退）。
- 已应用安全区的位置：`ChatUIWindow.tsx` 移动端顶栏（`safe-top`）、`src/styles/InputArea.css` 移动端输入框与按钮（bottom 偏移叠加 `env(safe-area-inset-bottom)`）。
- 虚拟键盘：AndroidManifest 已配 `windowSoftInputMode="adjustResize"`；`ConversationUI.tsx` 移动端监听 `visualViewport`，键盘弹起时强制滚到底部。
- 仍缺失：手势导航；触控目标尺寸只有 `src/styles/InputArea.css`（`.input-area.mobile`）做过系统适配。
- **新写移动端 UI 时**：可点目标 ≥44px；全屏容器直接用 `h-screen`（已自动走 dvh）；顶部/底部固定栏必须加 `safe-top` / `safe-bottom` 类。

### 4. 桌面功能在移动端的行为

Rust 侧对桌面功能做了编译期/运行期分支；移动端不支持的错误统一带 `MOBILE_UNSUPPORTED` 前缀（`system_api::mobile_unsupported_error`），前端用 `isMobileUnsupportedError` 识别：

| 功能 | 移动端行为 | 位置 |
|---|---|---|
| 全局快捷键 / 系统托盘 | `#[cfg(desktop)]` 不编译；前端设置页已隐藏全局快捷键区块 | `lib.rs` / `ShortcutsConfigForm.tsx` |
| 开机自启 | 返回 `MOBILE_UNSUPPORTED` 错误；前端已隐藏设置项 | `system_api.rs` / `OtherConfigForm.tsx` |
| 自动更新 | 返回 `MOBILE_UNSUPPORTED` 错误；前端已隐藏更新 UI | `updater_api.rs` / `AboutConfigForm.tsx` |
| PDF 导出 | 返回 `MOBILE_UNSUPPORTED` 错误（AppError 包装） | `export_api.rs` |
| 剪贴板复制图片 | 返回 `MOBILE_UNSUPPORTED` 错误 | `system_api.rs` |
| 获取选中文本 | 返回空字符串 | `lib.rs` |
| 脚本/Shell 执行 | **无任何 guard，运行期必失败**；前端已隐藏代码块"运行"按钮 | `mcp/builtin_mcp/operation/bash_ops.rs` / `CodeBlock.tsx` |
| 浏览器模式搜索（chromiumoxide） | 仅桌面依赖；前端已隐藏浏览器专属配置字段 | `Cargo.toml` / `BuiltinToolDialog.tsx` |
| 文件对话框 | 前端无平台判断，直接调用 | `conversationExportService.ts` 等 |

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
