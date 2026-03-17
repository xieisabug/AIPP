# 上下文详情预览设计

## 目标

补齐 Chat UI 详情侧边栏中“上下文”详情框的预览能力，让 `ContextItem` 的每一种类型都有稳定、可理解、可操作的预览方式，同时保持现有 Artifact 预览体验不受影响。

## 当前问题

- `search` 之外的大多数上下文预览仍然依赖零散字段兜底，信息不完整。
- `read_file` / `list_directory` 目前只能看到工具名或路径碎片，没有真正的内容预览。
- `skill`、`other`、部分 `user_file` 类型没有清晰的详情态设计。
- 详情框的展示逻辑现在混合在 `SidebarWindow.tsx` 中，分支越来越多，后续继续补类型会更难维护。

## 设计原则

1. 每个上下文类型都必须有“首选预览方式”。
2. 首选预览不可用时，必须有明确的降级展示，不允许只显示“暂无可预览的内容”。
3. 预览需要同时覆盖“信息展示”和“操作入口”，例如复制路径、外部打开、跳转预览窗口。
4. 搜索类上下文允许重复展示；详情预览按单次搜索实例展开。
5. 将类型判断和 UI 分发从 `SidebarWindow` 抽离为独立的 context preview renderer，避免主窗口组件继续膨胀。

## 建议的数据模型补充

建议在 `ContextItem` 上补一个统一的 `previewData` 字段，避免继续复用 `details` / `name` / `attachmentData` 做隐式约定。

```ts
interface ContextPreviewData {
  title?: string;
  subtitle?: string;
  rawValue?: string;
  contentType?: 'text' | 'code' | 'markdown' | 'image' | 'json' | 'directory' | 'file-meta';
  content?: string;
  language?: string;
  path?: string;
  url?: string;
  items?: Array<{ label: string; value?: string; description?: string }>;
  metadata?: Record<string, string>;
}
```

其中：

- `rawValue` 用于去重和“复制原始值”。
- `content` 用于文本、代码、Markdown、JSON 预览。
- `items` 用于目录列表、搜索结果列表、技能元数据列表。
- `metadata` 用于状态、来源、工具名、附件类型等。

建议同时在 `ContextItem` 上增加一个可选的加载状态字段，便于详情框处理懒加载：

```ts
interface ContextItem {
  // 现有字段...
  previewData?: ContextPreviewData;
  previewStatus?: 'ready' | 'needs_load' | 'loading' | 'error';
}
```

## `previewData` 的获取与传递链路

这里建议把上下文预览拆成两层数据：

1. **轻量同步数据**：随 `contextItems` 一起生成并发送，足够渲染列表和部分详情。
2. **重型详情数据**：仅在用户点击某个上下文项后按需加载，避免在侧边栏同步时塞入过大的内容。

### 1. 轻量同步数据的生成位置

当前代码里，侧边栏上下文数据是在前端 `ConversationUI` 中通过 `useContextList` 聚合生成的：

- `ConversationUI.tsx` 调用 `useContextList(...)`
- `useContextList.ts` 读取以下来源并产出 `contextItems`
  - `messages[].attachment_list`：用户已发送附件
  - `fileInfoList`：输入框里尚未发送的用户文件
  - `mcpToolCallStates`：MCP 工具调用状态、参数、结果
  - `acpWorkingDirectory`：ACP 工作目录
  - `get_conversation_loaded_mcp_tools`：已加载 MCP 工具说明

建议就在 `useContextList` 这一层把**轻量版 `previewData`** 填好，因为这里已经拿到了列表页所需的大多数上下文元数据。

示意：

```ts
const contextItems = useMemo(() => {
  return rawSources.map((source) => ({
    id,
    type,
    name,
    details,
    source,
    previewStatus: canRenderImmediately ? 'ready' : 'needs_load',
    previewData: {
      title: name,
      subtitle: details,
      rawValue,
      contentType,
      path,
      url,
      metadata,
      items,
    },
  }));
}, [...deps]);
```

### 2. 轻量同步数据如何传给详情侧边栏

当前链路已经存在，只需要把 `previewData` 放进 `contextItems` 一起传过去：

1. `useContextList` 返回 `contextItems`
2. `ConversationUI.tsx` 通过 `emit("sidebar-data-sync", { todos, artifacts, contextItems, conversationId })` 发给侧边栏窗口
3. `SidebarWindow.tsx` 监听 `"sidebar-data-sync"`，把 `contextItems` 存进 `sidebarData`
4. 用户点击某个上下文项时，`SidebarWindow` 读取当前项的 `previewData` 渲染详情

也就是说，`previewData` **不是单独走一条 IPC**，而是作为 `ContextItem` 的一部分，随现有的 `sidebar-data-sync` 一起送到前端详情窗口。

### 3. 哪些类型应该直接在前端同步时填充 `previewData`

这些类型的数据量通常可控，建议在 `useContextList` 中直接构造 `previewData`：

- `user_file`
  - 图片：`attachmentData.content` / `attachmentData.url`
  - 文本：附件内容、路径、类型
- `search`
  - `searchMarkdown`
  - `searchResults`
  - 查询词、工具名、显示域名
- `loaded_mcp_tool`
  - 服务名、工具名、状态、参数、描述
- `list_directory`
  - 目录路径、工具名、来源
  - 若 `toolCall.result` 中本身带有目录条目，可直接解析填入 `items`
- `read_file`
  - 文件路径、工具名、来源
  - 若 `toolCall.result` 里已经是文本内容，可直接写入 `previewData.content`

### 4. 哪些类型应该按需懒加载

这些类型可能比较重，或当前列表构建阶段没有完整正文，建议只在点击详情时加载：

- `skill`
  - 必须走后端读取完整 `SKILL.md`
- `read_file`
  - 如果当前 `toolCall.result` 没有缓存文件正文，只能先展示路径卡片，再按需补正文
- `list_directory`
  - 如果当前只有路径没有目录结果，可后续补“再次查询目录详情”的能力

### 5. 懒加载的推荐实现

建议在 `SidebarWindow` 的 `handleContextClick` 中增加一个 `loadContextPreview(item)` 流程：

```ts
const handleContextClick = useCallback(async (item: ContextItem) => {
  setContextPreview({ context: item, loading: item.previewStatus === 'needs_load' });

  if (item.previewStatus !== 'needs_load') {
    return;
  }

  const previewData = await loadContextPreviewData(item);
  setContextPreview({ context: { ...item, previewData, previewStatus: 'ready' }, loading: false });
}, []);
```

其中 `loadContextPreviewData(item)` 按类型分发：

- `skill` -> `invoke("get_skill_content", { identifier })`
- `read_file` -> 优先用已存的 tool result；若没有，再补独立的读取命令
- `list_directory` -> 优先用已存的 tool result；若没有，再补独立的列目录命令

### 6. `skill` 的后端获取方式必须落地

这个类型不能再写成“后续可选”。现有后端已经具备能力：

- `src-tauri/src/api/skill_api.rs`
  - `get_skill_content`
  - `get_skill_content_internal`
- `get_skill_content_internal` 会找到 skill 对应的 `SKILL.md`
- `SkillParser::parse_full(...)` 会读取并解析正文
- 返回 `SkillContent`
  - `identifier`
  - `content`：`SKILL.md` 正文
  - `additional_files`：`requires_files` 声明的附加文件

因此 `skill` 详情预览应当明确采用如下流程：

1. 列表态仅保存 `identifier` / `displayName` / source / 基础 metadata
2. 用户点击 skill 上下文项
3. `SidebarWindow` 调用 `invoke("get_skill_content", { identifier })`
4. 将返回的 `SkillContent.content` 作为 Markdown 正文写入 `previewData.content`
5. 将 `SkillContent.additional_files` 作为附加文件列表写入 `previewData.items`
6. 详情框使用 Markdown renderer 展示完整 `SKILL.md`

也就是说，**Skill 详情预览不是“能做最好”，而是必须直接展示后端返回的完整 `SKILL.md` 内容。**

## 各类型预览方案

### 1. `user_file`

#### Image

- 顶部显示文件名、来源、原始路径。
- 主区域显示图片预览。
- 底部操作：`在系统中打开`、`复制路径`。

#### Text

- 如果已有文本内容，使用代码/纯文本查看器展示。
- 如果能识别扩展名，自动推断语言高亮。
- 如果只有路径没有内容，展示文件元数据卡片，并提供 `在系统中打开`。

#### PDF / Word / PowerPoint / Excel

- 当前阶段不强求内嵌渲染。
- 详情框展示文件名、类型、路径、来源。
- 若后端后续能提供提取文本，优先展示“抽取摘要 + 元信息”。
- 始终提供 `在系统中打开`。

### 2. `skill`

- 顶部显示 `displayName` 与 `identifier`。
- 主区域必须展示后端返回的完整 `SKILL.md` Markdown 正文，而不是只展示摘要。
- 详情加载时调用 `get_skill_content(identifier)`，并使用返回的：
  - `content` 作为主 Markdown 正文
  - `additional_files` 作为附加文件列表/折叠区
- 同时展示技能基础信息：
  - 标识符
  - 来源（用户附件 / 官方技能 / 本地技能）
  - 若存在附加文件数量，也要显示
- 操作：`复制技能标识`。

### 3. `read_file`

- 这是最需要补齐的类型。
- 顶部显示文件路径，副标题显示触发工具名。
- 主区域优先展示文件内容：
  - 文本/代码：代码查看器，支持换行、只读高亮。
  - Markdown：渲染 Markdown。
  - JSON：格式化 JSON。
- 如果当前仅有路径，没有内容：
  - 显示文件路径卡片
  - 明确提示“当前只记录了读取操作，未缓存文件内容”
  - 提供 `复制路径`

建议实现分层：

- 若 `toolCall.result` 已包含正文，在 `useContextList` 中直接生成 `previewData.content`
- 若当前只拿到了路径，则 `previewStatus = 'needs_load'`，详情框点击后再走独立读取命令补正文

### 4. `search`

- 搜索预览保留“按实例展开”的策略，不去重。
- 顶部显示查询词，副标题显示触发工具。
- 主区域分两种：
  - 有 `searchMarkdown`：使用 Markdown 渲染器展示搜索摘要。
  - 有 `searchResults`：展示结果列表，包含标题、摘要、显示域名、序号。
- 操作：
  - `在预览窗口打开`（针对 Markdown 摘要）
  - 单条结果 `打开链接`
  - `复制查询词`

### 5. `list_directory`

- 顶部显示目录路径，副标题显示来源（ACP 工作目录 / list_directory 工具）。
- 主区域展示目录结构摘要：
  - 若有列表结果，展示文件/子目录列表
  - 若只有路径，展示目录信息卡片
- 操作：
  - `复制路径`
  - 若是本地可访问目录，后续可补 `在系统文件管理器中打开`

建议实现分层：

- 若 `toolCall.result` 已包含目录条目，则直接解析为 `previewData.items`
- 若当前只有路径，则 `previewStatus = 'needs_load'`，点击时再懒加载目录内容

目录类上下文保存结构化结果建议如下：

```ts
items: [
  { label: 'src', description: 'directory' },
  { label: 'App.tsx', description: 'file' }
]
```

### 6. `loaded_mcp_tool`

- 现有实现基本可用，应保留。
- 建议补充：
  - 状态徽标（已加载 / 失效）
  - 服务名、工具名分行展示
  - 参数区支持 JSON 高亮
  - `invalidReason` 单独告警块，不要混在状态字符串里

### 7. `other`

- 作为兜底类型，不能只有空白提示。
- 统一展示：
  - 标题：上下文名称
  - 副标题：来源
  - 原始字段：`details`、`name`、可用元数据
- 若未来有未识别 payload，可直接 JSON pretty-print 展示。

## UI 结构建议

建议将当前 `SidebarWindow` 里的上下文预览主体抽为：

- `ContextPreviewPanel`
- `ContextPreviewHeader`
- `ContextPreviewBody`
- `ContextPreviewActions`
- 各类型 renderer：
  - `UserFilePreview`
  - `SkillPreview`
  - `ReadFilePreview`
  - `SearchPreview`
  - `DirectoryPreview`
  - `LoadedMcpToolPreview`
  - `FallbackContextPreview`

这样每种类型只关心自己的渲染逻辑，`SidebarWindow` 只负责状态切换。

## 交互与降级策略

- 无数据时显示空态，但要告诉用户“缺少哪类数据”，例如“未缓存文件内容”。
- 所有类型统一支持：
  - 复制主值（路径、标识符、查询词）
  - 外部打开（仅在有 URL / 文件路径时）
- 对长文本使用滚动容器和等宽字体。
- 图片/Markdown/JSON/代码使用明确的专用渲染器，不混用纯 `pre` 标签作为长期方案。

## 推荐实施顺序

1. 为 `ContextItem` 增补统一 `previewData` + `previewStatus` 结构。
2. 在 `useContextList` 中补齐轻量 `previewData`，并继续通过现有 `sidebar-data-sync` 发送到 `SidebarWindow`。
3. 在 `SidebarWindow` 中增加 `loadContextPreviewData(item)`，处理懒加载详情。
4. 先补齐 `skill`（直接接后端 `get_skill_content`）、再补 `read_file`、`list_directory`。
5. 抽离 `SidebarWindow` 的上下文详情渲染。
6. 再统一补测试：类型分发、空态、降级态、操作按钮展示。

## 测试建议

- Hook 测试：验证各类型 `ContextItem` 会生成正确 `previewData` / `previewStatus`。
- 组件测试：点击不同上下文项后，详情框显示对应 renderer。
- 组件测试：`skill` 点击后会调用 `get_skill_content`，并渲染返回的 Markdown 内容与附加文件。
- 回归测试：
  - 搜索项允许重复
  - 读文件/目录项去重
  - 图片附件仍可正常预览
  - 已加载 MCP 工具详情不回退
