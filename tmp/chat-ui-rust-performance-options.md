# Chat UI 使用 Rust 提升滚动性能的改造方向

## 背景

687 这类长对话的主要问题不是消息数量特别多，而是单条消息很重：Markdown 解析、代码高亮、MCP/tool result 渲染、动态高度测量都会压到 WebKit2 的主线程。Virtuoso 可以减少 DOM 数量，但无法消除“挂载重组件”和“行高不断校正”带来的白屏、滚动跳动和掉帧。

Rust 不能直接让 WebKit2 的 layout、paint、DOM 挂载变快；Rust 能做的是把滚动路径上的重活提前处理掉，让前端滚动时尽量只做轻量 DOM 挂载。

## 1. 后端生成消息渲染模型

### 思路

在 Rust 侧把 message content 预解析成结构化 render blocks，例如：

- `paragraph`
- `heading`
- `list`
- `table`
- `code_block`
- `mcp_tool_call`
- `tool_result_preview`
- `preview_code`
- `image`
- `raw_text`

前端不再在每次消息挂载时从 raw Markdown 重新解析全部内容，而是根据 block 类型渲染对应组件。

### 收益

- 减少 React mount 阶段的 Markdown/MCP 标记解析成本。
- 更容易对超大 block 做 preview、折叠、懒加载。
- 便于给每个 block 单独缓存 hash、预估高度和高亮结果。

### 风险

- 需要保持插件自定义 Markdown tag、MCP 卡片、preview_code 等交互能力。
- 不能一次性把所有 Markdown 都替成纯 HTML，否则会损失 React 组件交互。

### 建议

先做混合模型：静态 Markdown block 后端解析，交互 block 仍然交给 React 组件。

## 2. Rust 批量预热代码高亮并缓存

### 思路

当前高亮已经走 Rust/syntect，但如果滚动到某条消息时才触发高亮，WebKit2 仍然会卡。可以在 conversation 加载后，由 Rust 批量生成代码块高亮结果并缓存。

缓存 key 建议包含：

- `message_id`
- `content_hash`
- `block_index`
- `language`
- `theme`
- `mode`: `collapsed_preview` 或 `full`

### 收益

- 滚动时前端直接拿高亮 HTML，不再临时请求和等待高亮。
- collapsed 状态只需要预热 preview slice，避免给大代码块生成完整 DOM。
- full 高亮可以等用户展开时再后台生成。

### 风险

- 主题切换需要重新命中不同 theme cache。
- 高亮 HTML 需要走现有 sanitize/可信边界，避免引入注入风险。

### 建议

优先实现 collapsed preview 的批量缓存。full 模式只在展开时按需生成。

## 3. 行高估算与实测高度持久化

### 思路

Virtuoso 初次打开对话时只能用估算高度，真实高度出来后会校正 scrollHeight，导致滚动条跳动。可以将消息内容 hash 对应的实测高度写入缓存，下次打开同一个 conversation 时作为初始高度。

缓存 key 建议包含：

- `message_id`
- `content_hash`
- `display_config_hash`
- `width_bucket`
- `theme`
- `collapsed_state`

### 收益

- 初始 scrollHeight 更接近真实值。
- 减少滚动时高度修正，降低白屏和滚动条跳动概率。
- 对 687 这种历史内容稳定的对话收益明显。

### 风险

- 宽度变化、字体变化、折叠状态变化会让高度缓存失效。
- 真实高度仍然必须由 WebKit2 测量，Rust 只能保存和复用结果。

### 建议

先做前端测量后回写，Rust/SQLite 保存。读取时只用于初始估算，不作为强约束。

## 4. 超大 tool result / response 的后端 preview 化

### 思路

对超大消息不要默认把完整内容挂载进 DOM。Rust 侧预生成轻量 preview：

- 前 N 行
- 前 N 字符
- JSON/日志/文本统计信息
- 是否截断
- 完整内容引用

用户展开时再加载完整内容。

### 收益

- 直接减少 WebKit2 的 DOM、layout、paint 压力。
- 对长 JSON、长日志、长工具结果、长代码块最有效。
- 可以显著降低首次点开和滚动时的主线程峰值。

### 风险

- 展开行为需要保持可复制完整内容、导出完整内容、搜索完整内容。
- 不能影响 AI 上下文和数据库原始消息，只改变 UI 展示策略。

### 建议

优先针对 `tool_result` 和含 MCP 写文件参数的大消息做 preview。原始内容仍保留在 DB，UI 默认展示摘要。

## 5. Markdown 后端预渲染安全 HTML

### 思路

Rust 使用 `comrak` 或类似 Markdown parser，把静态 Markdown 预渲染成安全 HTML；代码高亮仍然由 syntect 处理。前端只把静态 HTML 插入，交互 block 单独渲染 React 组件。

### 收益

- 避免每次挂载都运行 ReactMarkdown。
- 对历史静态消息尤其有效。
- 可和 block cache、高亮 cache 组合。

### 风险

- HTML sanitize 必须严格。
- ReactMarkdown 现有插件、自定义 tag、组件行为需要拆分兼容。
- 直接全量替换风险较高。

### 建议

不要一开始全量替换。先只对纯静态大段 Markdown response 走 Rust 预渲染，MCP/preview_code/插件 tag 仍使用 React。

## 不建议的方向

### 不建议把虚拟列表算法搬到 Rust

虚拟列表的 start/end index 计算不是主要瓶颈。真正慢的是 WebKit2 挂载节点、计算布局、绘制，以及 React 组件首次渲染。滚动中频繁 IPC 问 Rust 反而可能更慢。

### 不建议只靠更大 overscan

更大 overscan 可以减少白屏，但也会让前端一次挂载更多重消息。它是缓解手段，不是根治方案。

### 不建议关闭高亮

高亮是用户需要保留的功能。正确方向是缓存、preview 高亮、展开后再 full 高亮，而不是直接禁用。

## 推荐实施顺序

1. 增加消息 render block 预处理，但先保持 React 交互组件。
2. Rust 批量预热 collapsed code highlight，并做 hash cache。
3. 对超大 tool_result / response 做 preview UI 模型。
4. 回写并复用实测行高缓存。
5. 逐步把纯静态 Markdown block 迁移到 Rust 预渲染 HTML。

## 验证指标

每步都用 ChatUIWindow desktop harness 验证，不用 `npm run dev` 判断性能。

重点看：

- `blankViewportSampleCount`
- `maxBlankViewportRatio`
- `minVisibleMessageCount`
- `p95FrameMs`
- `worstFrameMs`
- `estimatedDroppedFrameCount`
- `maxScrollTop` 与 `finalMaxScrollTop` 的差值
- `rowHeightDrift`

687 的目标应该先做到：

- `blankViewportSampleCount = 0`
- `minVisibleMessageCount >= 1`
- `p95FrameMs` 稳定低于 50ms
- 滚动过程中 `maxScrollTop` 不出现大幅跳变
