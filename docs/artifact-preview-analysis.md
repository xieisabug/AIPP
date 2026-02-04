# Artifact 预览流程分析报告

## 现象
- 点击某个 artifact 打开预览窗口后，经常先加载上一次的 artifact，而不是当前点击的。
- 预览页面能显示，但“代码视图”为空或不对应。

## 当前流程梳理

### 1) 主要预览入口（run_artifacts）
来源：聊天窗口/侧边栏等调用 `invoke("run_artifacts")`。

后端流程（`src-tauri/src/artifacts/preview_router.rs`）：
1. `open_artifact_preview_window` 打开/唤起窗口。
2. 发送 `artifact-preview-reset`（含 `request_id`）通知前端清理旧状态。
3. 等待前端 `artifact-preview-ready`（前端会周期发送）。
4. 发送 `artifact-preview-data`（含 type、original_code）。
5. React/Vue 会启动预览服务，后续发送 `artifact-preview-redirect`（含 `request_id`）。
6. 缓存最新 artifact（`LAST_ARTIFACT_CACHE`）供恢复。

前端流程（`src/windows/ArtifactPreviewWindow.tsx` + `src/hooks/useArtifactEvents.ts`）：
1. 挂载后注册事件监听，循环发送 `artifact-preview-ready`。
2. 收到 `artifact-preview-reset` 后清空状态，并重新发送 ready。
3. 收到 `artifact-preview-data` 后设置 `previewType`/`originalCode` 等。
4. 收到 `artifact-preview-redirect` 后设置 `previewUrl` 并标记 `isPreviewReady`。

### 2) 直接向窗口投递数据（非 run_artifacts）
- `ContextList` 的 Markdown 预览（`src/components/chat-sidebar/ContextList.tsx`）
- `ArtifactWindow` 的 “在新窗口打开” Mermaid（`src/windows/ArtifactWindow.tsx`）

流程：
1. `open_artifact_preview_window`
2. `emitTo("artifact_preview", "artifact-preview-data", payload)`
3. 若失败，注册 `once("artifact-preview-ready", ...)` 再次发送

### 3) 缓存恢复
前端在 `ArtifactPreviewWindow` 首次挂载时：
- 若无数据且未收到新数据，会调用 `restore_artifact_preview`（后端触发 `run_artifacts`）。
- 目的：刷新或重启后恢复上一次预览（`localStorage` + `LAST_ARTIFACT_CACHE`）。

## 可能原因（流程漏洞/时序风险）

### 原因 A：缓存恢复与新请求竞争
`ArtifactPreviewWindow` 在挂载时会尝试恢复缓存，但此时可能已有新的 `run_artifacts` 触发。
这会导致**旧 artifact 的 restore 触发 reset/data**，与新请求并发，产生“先看到旧内容”的现象。

### 原因 B：请求关联不完整（仅 redirect 带 request_id）
当前只在 `artifact-preview-redirect` 中带 `request_id`，而 `artifact-preview-data/log/success/error` 不带。
前端也只对 redirect 做 request_id 校验。
结果是**旧请求的数据事件可能晚到并覆盖当前状态**，导致“预览正确但代码为空/不匹配”。

### 原因 C：once(artifact-preview-ready) 的历史回放问题
`ContextList`/`ArtifactWindow` 的 fallback 使用 `once("artifact-preview-ready")`：
- `ready` 是全局事件，不带 request_id。
- 该 once 会在**下一个任意请求 ready**时触发，可能把**旧 payload 再次注入**。
这会造成“点击 A 后切 B，最终又回到 A”的错乱。

### 原因 D：ready 信号被过早停止
`useArtifactEvents` 在收到 redirect 时就标记 `hasReceivedData=true` 并停止 ready 发送。
如果当前请求的 `artifact-preview-data` 未到或丢失，后续依赖 ready 的 fallback 可能失效。

## 解决方案建议

### 1) 统一请求上下文（强烈推荐）
为所有事件统一增加 `request_id` 并做强校验：
- 后端 `run_artifacts`：在 `artifact-preview-data/log/success/error` 中也附带 `request_id`。
- 前端 `useArtifactEvents`：只有当 `request_id` 与当前一致时才处理事件。
- 没有 `request_id` 的事件在“有活跃请求”时直接忽略，避免污染。

### 2) 规范“直接投递”流程
将“直接 emitTo preview”的场景统一走一个**带 request_id 的打开接口**：
- 新增后端命令：`open_artifact_preview_with_payload`（内部发 reset + data + request_id）。
- 或者前端在发送 payload 前先 `emit("artifact-preview-reset", { request_id })`，并将 `request_id` 放入 payload。
- 取消 `once("artifact-preview-ready")`，改成**带 request_id 的 ready 或一次性握手**。

### 3) 缓存恢复仅在“无显式请求”时触发
建议加入“抑制恢复”机制：
- 接收到任何 reset/data/redirect 后，立即取消 restore。
- 或者启动 restore 时加 300~500ms 延迟，若此间收到 reset/data 则中止。
- 亦可由 `open_artifact_preview_window` 增加参数 `skip_restore`。

### 4) UI 兜底策略
若 redirect 到达但 data 未到：
- 保持在 logs 状态并提示“代码加载中”，避免出现“预览有了但代码空”的错觉。
- 等 data 到达后再允许切换到 code/preview。

## 自动化测试方案

### 单元/组件测试（Vitest + React Testing Library）
目标：验证时序/竞态下状态是否正确。
建议覆盖：
1. `reset(A) -> data(A) -> redirect(A)`：应显示 A 预览且代码存在。
2. `reset(A) -> data(A) -> reset(B) -> redirect(A)`：A redirect 不应生效。
3. `reset(A) -> redirect(A)`（无 data）：应保持在 logs 或显示“代码加载中”。
4. `once-ready` 旧监听触发：确保不会覆盖当前请求。

实现思路：
- Mock `@tauri-apps/api/event` 的 `listen/emit/once`。
- 渲染 `ArtifactPreviewWindow`，手动触发事件序列，断言状态与 DOM。

### 事件层集成测试（Vitest）
针对 `useArtifactEvents`：
- 模拟多 request_id 事件并验证过滤逻辑。
- 验证 reset 后 ready 重新发送，收到 data/redirect 后停止。

### 端到端测试（Tauri Driver / Playwright）
目标：真实复现“快速切换 artifact”与“窗口恢复”。
场景：
1. 连续点击 A→B→C，确保最终只显示 C 且 code/preview一致。
2. 打开预览后刷新窗口，确认恢复逻辑不干扰新的点击。
3. React/Vue 预览启动慢时，仍能看到正确 code。

## 结论
当前问题主要是**请求上下文没有贯穿所有事件**，以及**缓存恢复与直接投递流程缺乏隔离**。
优先级最高的修复是：**为所有事件加入 request_id 并在前端严格过滤**，同时规范“直接投递”的入口。
完成后，再通过组件级与端到端测试覆盖上述竞态场景，可显著降低复现概率。
