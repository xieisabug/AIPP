# Chat UI 使用 Rust 提升滚动性能的改造方案

## 结论

原来的方向总体成立，但如果要直接按它开工，还需要把范围再收紧一点。

不建议第一阶段就做“Rust 生成完整消息渲染模型”。这个方向有价值，但单独收益有限，而且最容易影响自定义 Markdown 组件、插件 tag、MCP 卡片和 `preview_code` 这类 React 交互能力。

更合理的路线是：

1. 先减少 WebKit2 需要挂载和绘制的内容。
2. 再减少代码高亮和 Markdown 解析在滚动路径上的成本。
3. 再减少虚拟列表高度校正带来的滚动条跳动。
4. 最后再引入 Rust render block 作为统一基础设施。

Rust 不能直接让 WebKit2 的 DOM layout、paint、React mount 变快。Rust 能做的是提前解析、截断、缓存、预热，让前端滚动时少做重活。

如果目标是先把 687 这种对话救回来，第一阶段不应该追求“后端架构一次到位”，而应该先用最小改动验证 preview 化本身是否有效，再把已经验证有效的 preview 逻辑下沉到 Rust。

这里最需要避免的误区是：不要把“用 Rust 提升性能”理解成“Rust 接管消息渲染”。对当前 Chat UI 来说，Rust 更适合做消息内容的辅助索引层和缓存层，React 仍然负责最终交互渲染。

## 当前问题判断

687 这类对话不是“消息条数太多”，而是“单条消息太重”：

- 长 `response`
- 长 `tool_result`
- MCP tool call 标记和参数很大
- Markdown 解析成本高
- 代码高亮和代码块 DOM 成本高
- Virtuoso 初始高度不准，滚动中不断校正

所以优化重点不是把虚拟列表算法搬到 Rust，而是降低每个 message row 的首屏挂载成本和高度漂移。

## 第一阶段：超大消息 Preview 化

### 优先级

最高。

### 思路

对历史重消息，不要默认把完整内容挂载到 DOM。

第一批建议只处理：

- 超大 `tool_result`
- 主要由 `MCP_TOOL_CALL` 包装、并且参数 payload 很大的 `response`

普通长 Markdown `response` 不建议第一批就全部折叠。它们虽然也可能很重，但用户通常直接阅读正文，贸然统一 preview 化更容易影响可用性。可以把这类消息放到第二小步，等前一批收益被 harness 证明后再决定是否纳入。

Rust 侧生成 UI preview 数据：

- 前 N 行
- 前 N 字符
- 总字符数
- 总行数
- 是否截断
- 主要结构摘要，比如 JSON key 数量、数组长度、文件路径、工具名
- 内容标识，比如 `message_id`、`tool_call_id`、`content_hash`

前端默认只挂载 preview 的 DOM。用户点击展开后，仍然进入现有完整 React/Markdown/MCP/插件渲染链路。

第一版不建议同时改成“完整内容按需读取”。为了降低风险，可以先继续让现有消息 API 返回完整 `content`，只是不在折叠态挂载完整 DOM。这样复制、导出、AI 上下文和已有组件输入语义都不变。等 preview 化被 harness 证明有效后，再单独评估是否增加 `get_message_content(message_id)` 这类懒加载接口。

建议把这一阶段拆成两步：

1. 先用前端临时规则做 preview shell，验证阈值、交互和 harness 收益。
2. 验证有效后，再把统计、摘要、hash、完整内容引用迁到 Rust，避免长期让前端反复扫描大字符串。
3. 如果还存在内存或 IPC 传输压力，再考虑完整内容 lazy load；这不是第一批必须项。

### 为什么收益最大

WebKit2 最怕大量 DOM、长文本 layout、长代码块 paint。Preview 化是直接减少 DOM 和 layout 工作，不只是把工作从 JS 挪到 Rust。

### 兼容边界

- 不修改 DB 原始消息。
- 不影响 AI 上下文。
- 不影响导出完整内容。
- 不影响复制完整内容。
- UI 默认展示摘要，但必须保留“查看完整内容”的入口。
- preview 只作用于历史消息；最新 assistant 组、流式消息、仍在交互中的消息默认保持完整渲染。
- 折叠态下，被隐藏内容里的插件、自定义 tag、MCP 卡片和 `preview_code` 不会立即可交互；用户展开后必须回到现有完整渲染链路。这是可接受但必须显式承认的取舍。
- 第一版不要折叠普通长 Markdown 正文。普通长正文是用户直接阅读的主体内容，误折叠会比折叠工具 payload 更明显地影响体验。

### 适用对象

建议先只处理：

- `message_type = tool_result`
- 以 `MCP_TOOL_CALL` 为主、且隐藏参数 payload 明显过大的 `response`

第二批再评估：

- 纯 Markdown 但正文极长的历史 `response`
- `system` 这类通常不该频繁出现、但一旦很长也会拖慢挂载的消息

## 第二阶段：Rust 代码高亮预热与缓存

### 优先级

高。

### 思路

当前代码高亮已经走 Rust/syntect，但如果滚动到某条消息时才开始高亮，仍然会卡。应优先确认现有高亮链路是否已经有 LRU cache、in-flight 去重、collapsed preview 只高亮片段这些能力。

如果这些能力已经存在，第二阶段不应该重复造一套缓存，而是补齐缺口：让预热、缓存 key 和展开态/折叠态的边界更明确。

缓存 key 建议包含：

- `message_id`
- `content_hash`
- `block_index`
- `language`
- `theme`
- `mode`: `collapsed_preview` 或 `full`

### 策略

- 默认只预热 collapsed preview。
- full 高亮只在用户展开代码块时按需生成。
- plain text 类语言继续走纯文本，不需要 syntect。
- 不要为了性能关闭高亮；应该减少默认高亮输入规模。

### 收益

- 减少滚动过程中 `highlight_code` 请求和 HTML 注入抖动。
- 大代码块默认只挂载 preview 高亮 DOM。
- 保留高亮能力，不用关闭高亮。

### 风险

- 主题切换要按 theme 区分缓存。
- 高亮 HTML 需要继续走 sanitize 或可信输出边界。

## 第三阶段：行高缓存

### 优先级

高。

### 思路

Virtuoso 初次打开对话时只能按估算高度构建 scrollHeight。真实 DOM 高度出来后会不断校正，导致滚动条跳动和白屏。

可以把前端测得的行高回写到 Rust/SQLite，下次打开同一条消息时复用为初始高度。

但这一阶段比 preview 化更容易引入复杂失效逻辑，不建议在第一批和 preview 同时做。只有当 harness 仍然显示 `maxScrollTop` 与 `finalMaxScrollTop` 差异明显、`rowHeightDrift` 仍然很大时，再进入这一阶段。

缓存 key 建议包含：

- `message_id`
- `content_hash`
- `display_config_hash`
- `width_bucket`
- `theme`
- `collapsed_state`

### 收益

- 初始 scrollHeight 更接近真实值。
- 降低 `maxScrollTop` 和 `finalMaxScrollTop` 的差异。
- 减少滚动中高度修正。

### 边界

Rust 不能自己算出最终真实高度。真实高度仍然要 WebKit2 测量。Rust 做的是持久化和复用。

### 失效条件

- 消息内容变了。
- 展示宽度变化明显。
- 字体或主题变化。
- 代码块折叠状态变化。
- display config 影响 Markdown 展示。

### 第一版实现建议

先做会话内的轻量缓存或前端 estimate 修正，证明高度稳定性收益；确认有效后再落 SQLite。持久化行高需要节流写入，避免滚动过程中频繁 IPC/DB 写。

## 第四阶段：Rust Render Block Manifest

### 优先级

中。它是基础设施，不是第一阶段直接性能收益最大项。

### 正确定位

Rust 不应该直接接管完整 UI 渲染，而是生成 block manifest，帮助前端知道内容由哪些部分组成。

示例：

```json
[
  {
    "type": "markdown_static",
    "raw_range": [0, 1200],
    "content_hash": "..."
  },
  {
    "type": "code_block",
    "language": "ts",
    "block_index": 0,
    "content_hash": "...",
    "line_count": 240,
    "preview_line_count": 120
  },
  {
    "type": "mcp_tool_call",
    "call_id": 1751,
    "raw_range": [1201, 4500]
  },
  {
    "type": "custom_or_unknown",
    "raw_range": [4501, 5200]
  }
]
```

### 兼容原则

- 插件自定义 tag 默认走旧 ReactMarkdown 链路。
- MCP/tool call 默认走现有 React 组件。
- `preview_code` 默认走现有 React 组件。
- Rust 只做识别、hash、统计、索引、缓存 key。
- 无法确定安全的 block 保持原始 raw 内容给前端旧链路。

### 收益

- 后续 preview、高亮缓存、行高缓存都有统一 block key。
- 避免前端反复扫描整条 message content。
- 为静态 Markdown 预渲染打基础。

### 风险

如果把它做成“Rust 输出最终 HTML”，会破坏自定义组件和插件。所以第一版应该叫 `render_manifest`，不要叫 `rendered_html`。

## 第五阶段：静态 Markdown 后端预渲染

### 优先级

中低。等前几步稳定后再做。

### 思路

只对纯静态 Markdown block 做 Rust 预渲染安全 HTML。交互 block 继续使用 React。

可以考虑：

- 普通段落
- 标题
- 列表
- 表格
- 引用
- 内联代码

不建议第一版处理：

- 插件自定义 tag
- MCP tool call
- `preview_code`
- 附件
- 交互卡片
- 需要 React state 的组件

### 收益

- 减少 ReactMarkdown mount 成本。
- 对历史静态消息有帮助。

### 风险

- sanitize 必须严格。
- 需要和现有 Markdown 插件体系保持一致。
- 全量替换风险高。

## 不建议做的方向

### 不建议把虚拟列表算法搬到 Rust

虚拟列表计算 start/end index 不是主要瓶颈。真正慢的是 WebKit2 挂载节点、计算布局、绘制，以及 React 组件首次渲染。滚动中频繁 IPC 问 Rust 还可能更慢。

### 不建议第一步做完整 Rust Markdown HTML 渲染

这会绕过 ReactMarkdown、自定义组件、插件 tag、MCP 卡片和 `preview_code`。风险高，收益不一定比 preview 化更大。

### 不建议只靠增大 overscan

更大 overscan 可以减少白屏，但也会一次挂载更多重消息。它是缓解手段，不是根治方案。

### 不建议关闭高亮

高亮是需要保留的能力。正确方向是 collapsed preview 高亮缓存、展开后 full 高亮，而不是禁用。

## 推荐实施顺序

### Phase 1A：前端 Preview Shell 验证

目标：先确认“少挂载内容”这件事本身能否明显改善 687。

建议范围：

- 历史超大 `tool_result`
- 历史大 payload `MCP_TOOL_CALL response`

要求：

- 不改 DB
- 不改完整内容的复制、导出和 AI 上下文
- 折叠态只展示摘要
- 展开后仍走现有 React/Markdown/MCP 组件链路

### Phase 1B：Rust Preview Metadata

目标：把已经验证有效的 preview 统计、摘要、hash、完整内容引用下沉到 Rust，避免前端继续为判断 preview 反复扫描重内容。

建议范围：

- Rust 生成字符数、行数、payload 摘要、content hash
- 前端优先消费 metadata 和 preview text
- 保持 Phase 1A 的交互语义不变
- 仍保留完整 `content` 返回，避免第一版同时修改复制、导出、展开渲染和历史接口语义

### Phase 1C：完整内容 Lazy Load

目标：如果 Phase 1A/1B 后仍存在明显内存、IPC 或序列化压力，再减少初始 API 返回体。

建议范围：

- `get_conversation_with_messages` 对被 preview 的历史重消息只返回 preview metadata 和内容引用
- 新增按 `message_id` 读取完整内容的命令
- 展开、复制、导出时能够拿到完整内容

注意：

- 这是兼容风险更高的一步，不应和 Phase 1A 同时做。
- 需要逐项验证复制、导出、搜索、侧边栏、Artifact/插件引用是否仍能拿到完整内容。

### Phase 2：代码高亮缓存

目标：保留高亮，同时避免滚动时临时高亮大代码块。

建议范围：

- collapsed preview 高亮预热
- full 高亮按需生成
- 按 theme/content hash 缓存

### Phase 3：行高缓存

目标：降低 Virtuoso 高度校正、滚动条跳动和空窗概率。

建议范围：

- 前端测量后回写
- 下次加载复用
- 按宽度 bucket/display config/theme 失效
- 写入需要节流，不能在滚动高频路径上同步写 DB

### Phase 4：Render Block Manifest

目标：给前 3 个优化提供稳定 block key 和解析结果。

注意：

- 只做 manifest。
- 不直接替换 React 组件。
- 自定义组件和插件默认走旧链路。

### Phase 5：静态 Markdown 预渲染

目标：进一步减少 ReactMarkdown 成本。

注意：

- 只处理明确静态、安全的 Markdown block。
- 交互 block 不迁移。

## 暂不建议第一批做的内容

- 不做完整 Rust Markdown renderer。
- 不做完整内容 lazy load，除非 Phase 1A/1B 后证明确实还有传输或内存瓶颈。
- 不折叠普通长 Markdown 正文。
- 不把插件 tag、MCP 卡片、`preview_code` 转成 Rust HTML。
- 不把 Virtuoso 的 start/end index 计算迁到 Rust。
- 不在滚动时高频同步写 SQLite 行高。

## 验证方式

每一步都用 ChatUIWindow desktop harness 验证，不用 `npm run dev` 判断最终性能。

每次先确认 JSON 里的 `conversationName`，不要只凭列表序号认定自己测到了目标会话。

重点指标：

- `blankViewportSampleCount`
- `maxBlankViewportRatio`
- `minVisibleMessageCount`
- `p95FrameMs`
- `worstFrameMs`
- `estimatedDroppedFrameCount`
- `maxScrollTop` 与 `finalMaxScrollTop` 的差值
- `rowHeightDrift`

687 的阶段性目标：

- `blankViewportSampleCount = 0`
- `minVisibleMessageCount >= 1`
- `p95FrameMs` 稳定低于 50ms
- 滚动过程中 `maxScrollTop` 不出现大幅跳变

## 开工建议

如果按这个文档开干，第一张任务单不应该是“Rust render block 全量改造”，而应该是：

> 先为历史超大 `tool_result` 和大 payload `MCP_TOOL_CALL response` 增加前端 preview shell，验证 687 的白屏和帧耗时是否明显改善；验证成立后，再把 preview metadata 下沉到 Rust。

这个切入点比直接做完整 Rust manifest 更稳，也比一开始折叠所有长 Markdown 更容易守住现有交互能力。

开工时建议把任务边界写死：

- 第一批只影响历史重工具内容。
- 展开后必须回到旧完整渲染链路。
- 复制、导出、AI 上下文必须使用完整原文。
- harness 指标必须证明 `blankViewportSampleCount` 和滚动条跳动有改善，否则不要继续扩大折叠范围。
