# AIPP 渐进式交互 UI Spec + PRD

## 1. 目标

在 AIPP 中增加一种新的消息内 UI 能力：**非 Artifact、可渐进式展示、可交互、可把用户交互结果回传给工具执行链路**。

这项能力的定位接近 Claude 的 Generative UI：它是回答过程的一部分，不是需要进入 Artifact 工作区管理的交付物。

## 2. 结论

- 新工具名使用 `preview_code`
- 不增加 `read_widget_guide`
- 不新增专用持久化表
- 不走 iframe 路线
- 呈现方式采用**宿主页内联挂载节点 + 渐进式 DOM 更新**
- 第一阶段以 `html` renderer 为主，工具名为未来扩展保留空间

## 3. 产品定义

### 3.1 与现有能力的边界

- `preview_file`：静态或半静态预览，适合文件、文本、图片、PDF、HTML 展示
- `preview_code`：消息内的动态运行时 UI，支持渐进式展示和交互回传
- Artifact：持久产物、工作区内容、可管理预览

`preview_code` 不替代 Artifact，也不改造 `preview_file` 的语义。它是单独的消息内交互 UI 工具。

### 3.2 典型场景

- 解释型模拟器：复利、概率、算法流程、参数实验
- 可探索视图：日志筛选、表格过滤、图表切换
- 交互确认：风险操作确认、选项分流、批量动作预演

## 4. 现状判断

### 4.1 现有 `preview_file` 的可复用点

当前 `preview_file` 已经具备以下基础：

- 消息内卡片展示
- 基于 persisted tool call 的自动恢复
- 本地资源 relay
- 对话切换后的前端重建逻辑

这些能力可以复用到 `preview_code`，但 `preview_file` 本身仍保持只读预览定位。

### 4.2 当前缺口

当前 AIPP 前端拿到的是**完整工具调用**，不是流式 tool-call 参数。

这意味着如果要做渐进式 UI，必须新增：

- `ToolCallChunk` 聚合
- conversation 级别的流式 widget 事件
- 前端对流式片段的实时消费与更新

## 5. 工具设计

### 5.1 工具名

`preview_code`

这个名字比 `show_widget` 更适合长期演进，因为未来不一定只展示 HTML，也可能扩展到：

- `svg`
- `markdown`
- `mermaid`
- `json_view`
- `react`
- `vue`

### 5.2 Phase 1 建议输入

```json
{
  "title": "compound_interest_simulator",
  "renderer": "html",
  "code": "<style>...</style><div>...</div><script>...</script>",
  "loading_messages": [
    "正在生成交互面板",
    "正在填充内容",
    "正在激活交互"
  ],
  "interaction_mode": "submit_once",
  "metadata": {
    "origin": "assistant_response"
  }
}
```

字段说明：

- `title`：组件标识
- `renderer`：首期使用 `html`
- `code`：待渲染代码片段
- `loading_messages`：用于流式阶段的提示文案
- `interaction_mode`：
  - `none`
  - `submit_once`
- `metadata.origin`：来源标识

### 5.3 工具结果

建议结果结构：

```json
{
  "status": "submitted",
  "request_id": "xxx",
  "payload": {
    "principal": 10000,
    "rate": 8,
    "years": 10
  }
}
```

关闭但未提交时：

```json
{
  "status": "dismissed",
  "request_id": "xxx"
}
```

## 6. 呈现与恢复模型

### 6.1 不新增表

`preview_code` 不需要新增专门的持久化表。

原因很简单：AIPP 已经持久化了工具调用本身，前端也已经有基于 tool call 重建 UI 的模式。`preview_code` 应沿用同一套思路。

### 6.2 恢复方式

对话加载时，前端自动读取当前 conversation 的 persisted tool calls：

- 如果发现最近一条 `preview_code` 是 `pending/executing/success`
- 就自动重建对应的消息内 UI
- 如果该工具调用已经有结果，则恢复最终态
- 如果当前会话还在运行，则继续接收流式事件

结论：**渲染动作应随对话加载自动触发，而不是依赖额外持久化表。**

## 7. 核心技术方案

### 7.1 总体方向

采用与 Claude 更接近的路线：

- 不用 iframe
- 不把 UI 作为 Artifact
- 在聊天页面中为每个 `preview_code` 卡片创建一个专用挂载节点
- 由宿主直接把代码片段注入这个挂载节点
- 在同一文档上下文内完成样式、生长式结构更新与交互回传

React 只负责卡片壳和状态，不负责 widget 子树的内部更新。widget 子树由一套独立的 imperative runtime 管理。

### 7.2 前端结构

建议新增：

- `InlineCodePreviewCard`
- `usePreviewCode`
- `previewCodeRuntime.ts`

职责划分：

- `usePreviewCode`：监听事件、恢复历史、管理状态
- `InlineCodePreviewCard`：渲染标题、状态、容器、错误态
- `previewCodeRuntime.ts`：把流式代码应用到宿主 DOM

### 7.3 流式链路

1. 模型开始生成 `preview_code`
2. 后端收到 `ToolCallChunk`
3. 后端按 tool index 聚合参数
4. 提取 `title`、`renderer`、`loading_messages`、`code`
5. 向 conversation 发出 `preview_code_stream_update`
6. 前端首次收到有效片段时创建卡片
7. runtime 将增量代码应用到挂载节点
8. 工具参数完成后进入 final render
9. 用户交互后通过 Tauri command 回传结果

建议事件结构：

```json
{
  "type": "preview_code_stream_update",
  "data": {
    "request_id": "temp-or-stable-id",
    "tool_name": "preview_code",
    "title": "compound_interest_simulator",
    "renderer": "html",
    "loading_messages": ["正在生成交互面板"],
    "partial_code": "<style>...</style><div>...",
    "is_final": false
  }
}
```

## 8. 渐进式渲染

### 8.1 基本策略

要求模型尽量按下面顺序输出：

1. `style`
2. HTML 结构
3. `script`

这样可以先看到稳定的内容骨架，再在最终阶段激活交互。

### 8.2 运行时策略

不使用整块 `innerHTML` 高频全量替换。

建议运行时做法：

- 首次片段到达时创建根容器
- 对增量更新做 100-150ms debounce
- 对代码进行增量解析与最小化更新
- final 阶段再执行 script

### 8.3 推荐的实现方式

首选方案：

- 在宿主侧维护一棵独立 DOM 子树
- 用 diff/morph 思路做局部更新
- 对新增节点做轻量动画

这条路线最接近 Claude 的体验，也避免了 iframe 的交互迟滞和上下文隔离问题。

## 9. Rust 侧可行方案

Tauri 最终还是依赖 WebView2 呈现 DOM，所以**真正的 DOM 应用不可能完全离开前端**。但 Rust 侧可以承担更多流式处理工作，减少 WebView2 的压力。

### 9.1 可用方案

#### `lol_html`

- 低延迟 streaming HTML rewriter
- 适合做流式重写、片段清洗、增量预处理
- 非常适合作为后端的 chunk 归并与标准化层

适合职责：

- 拼接不完整 HTML 片段
- 清理危险节点或属性
- 对脚本执行时机做拆分
- 把大块代码整理成更适合前端消费的更新单元

#### `html5ever`

- 标准兼容度高
- 适合做稳健的 HTML fragment 解析和 tokenizer 级处理

适合职责：

- 严格解析
- 构造更可靠的中间结构
- 需要高正确性时作为核心解析器

#### `tl`

- 速度快，API 轻
- 适合处理“相对正常”的 HTML 片段

适合职责：

- 快速片段分析
- 轻量节点检查
- 性能优先的简单预处理

### 9.2 推荐落地方式

不建议追求“纯 Rust 完成全部渐进式渲染”。

推荐方案是：

- Rust 负责：流式聚合、片段标准化、必要的清洗、可选的 patch 规划
- 前端负责：把 patch 或最小更新操作应用到真实 DOM

这样可以减少 WebView2 反复解析整段 HTML 的成本，同时不和浏览器自身的 DOM 生命周期对抗。

### 9.3 Phase 2 性能优化方向

如果后续确认 WebView2 是瓶颈，可进一步升级为：

- Rust 输出结构化 patch，而不是原始 HTML
- 前端只执行少量命令，例如：
  - `append_html`
  - `replace_children`
  - `set_attr`
  - `remove_node`
  - `run_scripts`

这会比前端持续全量重算更稳。

## 10. 交互桥接

建议提供一个极小的宿主桥接对象：

```js
window.aippPreviewCode.submit(payload)
window.aippPreviewCode.close()
window.aippPreviewCode.emitEvent(name, payload)
```

Phase 1 至少支持：

- `submit`
- `close`

`emitEvent` 可以先保留为扩展点。

## 11. 安全与边界

本方案不走 iframe，但仍要保留最小必要边界。

建议：

- 只允许 `preview_code` 使用受控挂载节点
- 只暴露最小桥接 API
- 外部资源保留 allowlist
- script 默认在 final 阶段执行
- 对超长代码、异常脚本、重复刷新做保护

安全策略的目标不是把它做成高度隔离沙箱，而是在接近 Claude 体验的前提下保持可控。

## 12. 分阶段实施

### Phase 1：可交互最终态

- 增加 `preview_code` 工具
- 增加 `usePreviewCode` 和 `InlineCodePreviewCard`
- 基于 persisted tool call 自动恢复
- 支持 `submit_once`
- `renderer` 先支持 `html`

### Phase 2：渐进式渲染

- 开放 `ToolCallChunk` 到 conversation event
- 增加 `preview_code_stream_update`
- 接入宿主 runtime 的增量更新能力
- final 阶段执行 script

### Phase 3：性能优化与 renderer 扩展

- 引入 Rust 侧片段标准化/patch 规划
- 扩展 `svg`、`markdown`、`mermaid` 等 renderer
- 评估 `react/vue` renderer 是否值得纳入

## 13. 验收标准

1. 模型可以在聊天中调用 `preview_code`
2. UI 在消息流中展示，不进入 Artifact 工作区
3. 对话重新加载后，UI 能随 persisted tool call 自动重建
4. 用户交互结果可以回传工具执行结果
5. Phase 2 完成后，UI 能随着 tool-call 参数流式成形
6. 在长内容和高频 chunk 下，界面仍可接受，不出现明显卡死或全页闪烁

## 14. 最终建议

最终建议是：

**新增 `preview_code`，把它定义为 AIPP 的消息内渐进式交互 UI 工具；持久化直接复用现有 tool call 记录；渲染采用宿主页内联挂载节点；性能优化优先走 Rust 侧流式预处理 + 前端最小 DOM 应用的混合方案。**
