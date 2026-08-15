# 移动端优化 PRD

> 状态：草案
> 分支：`optimize/mobile`
> 工程规范见根目录 `AGENTS-mobile.md`，本文档聚焦需求与优先级。

## 1. 背景与问题陈述

AIPP 移动端（Android）目前处于"能跑而非能用"的阶段：

- 唯一完整可用的路径是启动 → 单 `chat_ui` webview → `ChatUIWindow` 窄屏分支（顶栏 + Sheet 抽屉会话列表 + 移动模式输入框），会话与 AI 流式对话基本可用。
- 移动端检测基于 `window.innerWidth < 768`（`src/hooks/use-mobile.ts`），混淆了"窄屏"与"移动平台"两个维度：桌面窗口拖窄会误触发移动端布局，Android 平板横屏会误判为桌面。
- 导航依赖 `window.__setAppWindow` 全局函数 hack（`src/App.tsx`），仅 2 处使用；Schedule / Butler / Artifact / Sidebar / Plugin 等窗口没有移动端分支。
- 触控体验零投入：无 safe-area（`index.html` 无 `viewport-fit=cover`）、无 `100dvh`（全部窗口用 `h-screen`）、无虚拟键盘适配、无手势导航；移动端专属样式仅 `InputArea.css` 约 34 行。
- 桌面功能在移动端静默失败：脚本/Shell 执行（`bash_ops.rs` 无 Android guard）、浏览器模式搜索（chromiumoxide 仅桌面依赖）、剪贴板图片、文件对话框等运行期报错或静默空返回，前端无禁用 UI。
- 原生层零定制：AndroidManifest 仅 `INTERNET` 权限，无 `windowSoftInputMode`、无分享 intent；`capabilities/mobile.json` 只授权 `chat_ui` 窗口且 fs scope 为 `**`。

## 2. 目标

- **短期**：消除最痛的体验问题——刘海/手势条遮挡、虚拟键盘遮挡、桌面窄窗口误触移动端布局。
- **中期**：移动端从 hack 变成产品——单页导航架构、不支持的功能明确禁用/提示，权限收敛。
- **长期**：触控体验达标（触控目标、手势导航、对话页移动细节），按需接入 Android 原生能力。

### 非目标

- 不支持 iOS（本期不做）。
- 不做移动端脚本执行能力的替代方案（云端/远程执行为独立产品决策，不在本期）。
- 不为桌面组件大面积补响应式样式——移动端问题优先在移动端专用分支/组件内解决。

## 3. 需求详述

### P0 — 地基：修正抽象与视口

| # | 需求 | 说明 | 验收标准 |
|---|------|------|---------|
| P0-1 | 分离"平台"与"宽度"检测 | 新增平台检测能力（Tauri `platform()` 或 Rust 命令返回 `cfg!(target_os)`），用于禁用桌面功能；现有 `useIsMobile()` 明确为窄屏布局用途 | 代码中"是否移动平台"的判断不再依赖屏幕宽度；桌面拖窄窗口不再触发平台级行为差异 |
| P0-2 | 视口三件套 | `index.html` 加 `viewport-fit=cover`；全局 safe-area padding（至少覆盖顶栏与输入框）；移动端全屏容器从 `h-screen` 换为 `100dvh`（带 fallback） | 刘海屏/手势条区域无内容遮挡；键盘弹起时布局不跳变 |
| P0-3 | 虚拟键盘适配 | AndroidManifest 配 `windowSoftInputMode="adjustResize"`；输入区监听 `visualViewport` resize | 键盘弹起时输入框不被遮挡，消息列表自动滚到底部 |

### P1 — 架构：移动端单 App 导航

| # | 需求 | 说明 | 验收标准 |
|---|------|------|---------|
| P1-4 | MobileShell 单页导航 | 在 `src/mobile/` 实现移动端 Shell（底部 Tab 或抽屉：对话 / 设置 / 任务），React 状态切换视图，替代 `__setAppWindow`；移动端只保留 `chat_ui` 一个 webview，其余窗口不创建、入口隐藏 | `__setAppWindow` 被移除；移动端各功能页在单 webview 内可达且为移动布局 |
| P1-5 | 功能降级清单 | 维护"移动端不支持"功能表（脚本执行、浏览器搜索、自动更新、托盘/快捷键、多窗口预览等）；Rust 侧统一结构化错误码；前端按平台隐藏入口 | 移动端界面上不存在点了才报错的入口；错误信息统一且明确 |
| P1-6 | 收紧 `mobile.json` | fs scope 从 `**` 收敛到应用数据目录；补授权实际需要的能力 | 权限最小化且移动端功能不回归 |

### P2 — 体验：触控与布局

| # | 需求 | 说明 | 验收标准 |
|---|------|------|---------|
| P2-7 | 触控目标基线 | 可点目标 ≥44px，优先覆盖输入区、消息操作按钮、会话列表项 | 主要操作无不跟手/误触反馈 |
| P2-8 | 手势导航 | 会话列表抽屉支持边缘右滑打开 / 返回手势（基于现有 Sheet，不引入手势库） | 手势可用且不与横向滚动冲突 |
| P2-9 | 对话页移动细节 | 流式输出时"回到底部"按钮、代码块横向滚动、消息长按菜单替代 hover 操作 | 长对话在移动端可完整操作 |

### P3 — 原生能力与集成

| # | 需求 | 说明 | 验收标准 |
|---|------|------|---------|
| P3-10 | Android 集成 | 分享接收 intent（分享到 AIPP 发起对话）、deep link、按需补权限（通知、存储） | 系统分享可直达 AIPP 对话 |

## 4. 落地计划

- **第一批（约一周，纯前端）**：P0-1、P0-2、P0-3 + P1-5 的前端隐藏部分。不动架构，先消除三类最痛体验问题。
- **第二批**：P1-4 MobileShell 重构 + P1-5 错误码统一 + P1-6 权限收敛。
- **第三批及以后**：P2 体验项、P3 原生集成，按反馈排期。

## 5. 风险与依赖

- `use-mobile.ts` 语义变更影响面：现有 6+ 处消费方（`ChatUIWindow`、`ConfigWindow`、`ConversationUI`、`InputArea`、`NewChatComponent`、`sidebar.tsx`）需要逐一核对语义是"窄屏"还是"平台"。
- MobileShell 重构涉及窗口创建逻辑（`window.rs`）与前端入口（`App.tsx`），需保证桌面端零回归。
- 虚拟键盘行为依赖 Android WebView 版本，需在真机验证（minSdk 24 覆盖范围内的旧设备尤其）。
