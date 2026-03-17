# `/` Slash 主动调用设计方案（Skills 为首个 namespace）

## 目标

把输入框里的主动调用入口从 `#` 改为统一的 `/`，并先落地 `Skills`：

1. 用户输入 `/` 后，弹出一个**通用入口列表**。
2. 第一阶段先支持 `/skills(...)`，后续可扩展 `/artifacts(...)` 等其他 namespace。
3. 选择 Skill 后，输入框直接补全成 `/skills(<skill name>)`。
4. `skill name` 允许包含空格；对括号、反斜杠等特殊字符使用明确的解析/转义规则。
5. 发送消息时，后端解析 `/skills(...)`，读取对应 `SKILL.md`（以及 `requires_files`），把内容注入到本次请求 prompt。
6. Skill 注入只影响**本次请求**，不等同于“助手绑定 Skill”；它是一次性的主动调用。
7. 输入 `/` 时不能每次都全量扫描 Skills，必须设计缓存、失效和刷新策略。

---

## 当前实现梳理

### 前端

- `src/components/conversation/InputArea.tsx`
  - `!`：Bang 补全
  - `@`：Assistant 补全
  - `#`：当前是 Artifact 补全/打开
- `src/components/conversation/BangCompletionList.tsx`
- `src/components/conversation/AssistantCompletionList.tsx`
- `src/components/conversation/ArtifactCompletionList.tsx`
- `src/utils/pinyinFilter.ts`

### 后端

- `src-tauri/src/template_engine/mod.rs`
  - 当前只解析 `!bang`
- `src-tauri/src/api/skill_api.rs`
  - 已有 `scan_skills`
  - 已有 `get_skill_content`
- `src-tauri/src/skills/parser.rs`
  - 已能解析 `SKILL.md` 和 `requires_files`
- `src-tauri/src/skills/prompt.rs`
  - 当前只把“助手已启用的 Skill 列表摘要”拼到 prompt
  - 还不会把某个 Skill 的完整 `SKILL.md` 作为一次性主动调用内容注入
- `src-tauri/src/mcp/builtin_mcp/agent/handler.rs`
  - 已有 `load_skill`
  - 已具备“按技能名/路径加载完整 Skill 内容”的能力，可复用匹配逻辑

### 当前关键问题

1. `#` 与 Artifact 现有语义冲突。
2. 直接用 `/skill-name` 一类短 token，虽然输入方便，但仍需要处理空格、重名和可读性问题。
3. 如果把 `SKILL.md` 直接展开进用户消息正文，会污染聊天记录并让用户消息暴涨。
4. Slash 入口是高频输入行为，不能在用户每次按下 `/` 时重新扫描全部 Skills。

---

## 核心设计决策

## 1. `/` 作为统一入口，`Skills` 作为第一个 namespace

### 1.1 Slash 总体语法

统一采用：

```text
/namespace(argument)
```

第一阶段先支持：

```text
/skills(React Best Practices)
```

后续可以扩展：

```text
/artifacts(Home Page Draft)
```

### 1.2 为什么改成 `/namespace(argument)`

这个方案相比 `#skill-token` 有几个明显优势：

- **空格天然可支持**：Skill 名称可以直接保留为更接近原始显示名的形式。
- **可扩展**：`/skills(...)`、`/artifacts(...)`、未来其他入口都能挂到同一个 Slash Router 下。
- **冲突更小**：只把 `/name(` 识别为主动调用，普通 `/path/to/file` 不会被误判。
- **可读性更强**：用户消息里直接能看出在调用什么类型的资源。

### 1.3 第一阶段范围

第一阶段只正式实现：

- `/skills(...)` 的补全、解析、注入

但架构按通用 Slash Router 设计，为后续 `/artifacts(...)` 预留接口和缓存位。

---

## 2. `/skills(...)` 的具体语法与解析规则

## 2.1 基本规则

有效调用格式：

```text
/skills(<skill argument>)
```

例如：

```text
/skills(React Best Practices)
/skills(前端性能优化)
/skills(Copilot / React Best Practices)
```

其中：

- `skills` 是 namespace
- `skill argument` 是 Skill 的**规范匹配名**，由后端决定，前端只负责插入，不自行构造

## 2.2 为什么不能让前端直接随便插入 display name

因为会有两个问题：

1. **重名**：不同来源的 Skill 可能 display name 一样。
2. **匹配不一致**：前端和后端各写一套“字符串规整/匹配规则”很容易漂移。

所以建议仍由后端生成一个**规范输入名**，但它不再是短 slug，而是可读的 `invoke_name`：

```rust
SlashSkillCompletionItem {
    identifier: String,         // 真实唯一标识，如 "copilot:react/best-practices"
    display_name: String,       // UI 展示名
    invoke_name: String,        // 插入 /skills(...) 时使用的规范参数
    aliases: Vec<String>,       // 可兼容匹配的别名
    source_type: String,
    source_display_name: String,
    description: Option<String>,
    tags: Vec<String>,
}
```

### 2.2.1 `invoke_name` 生成规则

推荐规则：

1. 默认 `invoke_name = display_name`
2. 如果 `display_name` 冲突，则升级为：
   - `<source_display_name> / <display_name>`
3. 如果仍冲突，再回退到：
   - `identifier`

这样大多数 Skill 都可以呈现为：

```text
/skills(React Best Practices)
```

只有冲突时才会变成：

```text
/skills(Copilot / React Best Practices)
```

前端不要猜测这个名字，必须直接使用后端返回的 `invoke_name`。

---

## 3. 括号、反斜杠与匹配规则

用户已经明确希望保留 `/skills(<skill name>)`，因此括号匹配不能靠简单正则，必须用扫描器。

## 3.1 推荐解析器：逐字符扫描，不用单个 regex

建议新增通用 Slash 解析器，使用状态机/扫描器来解析：

1. 识别 `/`
2. 读取 namespace（如 `skills`）
3. 看到 `(` 后进入 argument 解析
4. 解析 argument 时维护：
   - `depth`（括号嵌套层级）
   - `escaped`（上一字符是否是 `\\`）
5. 当 `depth` 回到 0 时，当前 Slash 调用结束

## 3.2 参数中的括号策略

推荐支持**平衡括号 + 转义字符**，而不是完全禁止括号：

### 可直接支持的情况

```text
/skills(React (Vercel) Best Practices)
```

因为扫描器会计算括号深度，所以这种**成对括号**可以直接解析。

### 必须转义的情况

如果名字里出现“不能靠平衡关系表达”的字符，使用反斜杠：

- `\(` 表示字面量 `(`
- `\)` 表示字面量 `)`
- `\\` 表示字面量反斜杠

例如：

```text
/skills(Skill Name \))
/skills(Skill Name \(beta)
```

## 3.3 发送时的错误策略

### 不完整调用

如果用户发送：

```text
/skills(React Best Practices
```

后端应返回明确错误，而不是静默忽略：

```text
Slash 调用语法错误：/skills(...) 缺少闭合右括号
```

### 未知 namespace

如果用户发送：

```text
/unknown(test)
```

后端也应返回明确错误：

```text
未知 Slash 入口：unknown
```

### 未找到 Skill

如果 `/skills(...)` 的参数找不到匹配项：

```text
未找到 Skill: /skills(React Best Practice)
```

不要静默吞掉。

---

## 4. Skills 的加载策略：元数据缓存 + 按需加载正文

这是本次设计里最重要的性能点。

## 4.1 绝对不要预加载所有 `SKILL.md` 正文

不建议在 Slash 补全阶段就把所有 `SKILL.md` 正文全部读进内存。原因：

- 技能数量可能很多
- `requires_files` 可能附带额外内容
- 输入 `/` 是高频操作，不应伴随重 IO
- 大多数 Skill 在一次会话里根本不会被真正调用

**推荐策略：**

### 层 1：Completion Metadata Cache（必须）

缓存内容只包含：

- `identifier`
- `display_name`
- `invoke_name`
- `source_type`
- `source_display_name`
- `description`
- `tags`
- 用于后端精确匹配的索引

也就是说：**缓存的是索引，不是正文**。

### 层 2：Skill Content On Demand（必须）

只有当用户真正发送消息，且解析出 `/skills(...)` 命中某个 Skill 时，才：

1. 根据 `identifier` 调用 `get_skill_content_internal`
2. 读取 `SKILL.md`
3. 读取 `requires_files`
4. 拼入本次 prompt

### 层 3：正文 LRU Cache（可选优化）

如果后续发现同一 Skill 被频繁主动调用，可以再加一个小型内容缓存：

- Key：`identifier`
- Value：`SkillContent + file mtime fingerprint`

但这不是第一阶段必需项；第一阶段优先把**元数据缓存**和**按需正文加载**做好。

---

## 5. 缓存与更新机制

## 5.1 后端缓存：统一的 Slash Registry Cache

建议新增状态，例如：

```rust
SlashRegistryCacheState {
    skills_index: RwLock<Option<CachedSkillsIndex>>,
}

CachedSkillsIndex {
    built_at: Instant,
    last_check_at: Instant,
    items: Vec<SlashSkillCompletionItem>,
    by_invoke_name: HashMap<String, String>,
    fingerprints: Vec<SourceFingerprint>,
}
```

其中 `SourceFingerprint` 记录每个 source root 的轻量指纹，例如：

- 路径
- `modified_time`
- 是否是目录/文件
- 对 Claude `installed_plugins.json` 场景的文件 mtime

## 5.2 何时触发全量扫描

只在以下情况触发真正的 `scan_skills`：

1. **首次冷启动**：缓存为空
2. **明确的刷新事件**：安装、删除、迁移、手动刷新
3. **轻量指纹检查发现已变化**：source root 的 mtime/fingerprint 变了
4. **发送时解析失败后的一次强制重试**：先 refresh，再 resolve 一次

**不要**在用户输入框里每按一次 `/`、每打一个字符就触发全量扫描。

## 5.3 何时只走缓存

以下场景全部只用缓存：

1. 输入框第一次打开 Slash 列表之后的同一会话输入
2. 用户在 `/skills(...)` 里不断继续输入做过滤
3. 上下移动补全项
4. 点击/Tab/Enter 选择补全项

换句话说：

- **输入中**：只做前端内存过滤
- **发送时**：后端走缓存索引 resolve
- **缓存失效时**：后端才触发 refresh

## 5.4 前端缓存

前端也应有一层轻量缓存，避免每次打开 Slash 面板都跨 Tauri 调用：

### 建议方式

- `InputArea` 挂载时，或第一次使用 Slash 时，调用一次 `get_skills_for_slash_completion()`
- 把结果保存在组件状态或 `useSlashCompletion` hook 中
- 输入 `/skills(` 之后只在前端本地筛选

### 更新来源

前端监听统一事件：

```text
skills-registry-changed
```

收到后：

1. 后台刷新本地缓存
2. 如果当前 Slash 面板是打开状态，则无缝更新列表

## 5.5 外部文件系统变更的处理

Skills 可能来自：

- `~/.agents/skills`
- `~/.copilot/skills`
- `~/.codex/skills`
- Claude 插件目录等

这些目录可能被外部工具直接修改，因此仅靠“应用内事件”不够。

### 推荐策略

#### 第一层：事件驱动（主路径）

应用内安装/删除/迁移完成时，主动：

- 更新后端缓存
- 发出 `skills-registry-changed`

#### 第二层：轻量指纹校验（兜底）

在以下较低频节点做一次 cheap check：

- Slash 面板第一次打开
- 用户手动刷新
- 发送时 resolve Skill 之前

只比较 source root 的 mtime/fingerprint；若未变化，直接用缓存；若变化，再执行 full scan。

这样可以兼顾：

- 高频输入不卡顿
- 外部改动也最终可感知

---

## 6. 前端交互设计

## 6.1 Slash Router 的两阶段补全

### 阶段 A：输入 `/`，显示 namespace 列表

例如：

```text
skills      主动调用已安装 Skills
artifacts   选择/引用 Artifact（后续）
```

第一阶段中，只有 `skills` 真正可执行；`artifacts` 可以先做保留项或灰态项。

### 阶段 B：选择 `skills` 后，插入 `/skills()`

交互建议：

1. 用户输入 `/`
2. 选择 `skills`
3. 输入框变为：

```text
/skills()
```

4. 光标自动落在括号内
5. 立刻弹出 Skill 列表

## 6.2 选择 Skill 后的补全文本

如果用户选中了某个 Skill，前端直接插入：

```text
/skills(<invoke_name>)
```

例如：

```text
/skills(React Best Practices)
```

或重名情况下：

```text
/skills(Copilot / React Best Practices)
```

如果 `invoke_name` 里包含需要转义的字符，前端按后端约定进行 escape 后再插入。

## 6.3 列表过滤策略

Skill 列表的本地过滤建议支持：

- `display_name`
- `invoke_name`
- `description`
- `tags`
- `identifier`
- 拼音搜索

继续复用/扩展 `PinyinFilter`，但数据源来自 Slash 缓存，而不是重新扫描 Skills。

## 6.4 键盘行为建议

### namespace 列表

- `ArrowUp / ArrowDown`：移动选中项
- `Enter / Tab`：选中 namespace，插入 `/skills()`
- `Esc`：关闭面板

### `/skills(...)` 参数列表

- `ArrowUp / ArrowDown`：移动 Skill
- `Enter / Tab`：补全当前 Skill 名
- `Esc`：关闭 Skill 列表，不删除已输入文本

---

## 7. Prompt 注入策略

## 7.1 不建议把 `SKILL.md` 直接塞进用户消息正文

这点保持不变。

如果像 `!bang` 那样直接把 Skill 正文展开进用户文本，会带来两个问题：

1. 聊天记录中的用户消息会被污染成一大段 Skill 文档
2. `SKILL.md` 中的内容还可能被后续模版/命令解析误伤

因此推荐把主动调用 Skill 视为**本次请求的动态系统上下文**。

## 7.2 推荐链路

### 当前链路

1. 提取 `@assistant`
2. 渲染 assistant system prompt
3. 收集 MCP prompt
4. 收集“助手绑定的 Skills 摘要”
5. 解析用户 prompt 的 `!bang`
6. 直接把渲染后的 user prompt 作为消息内容和 LLM 输入

### 改造后链路

1. **提取 `@assistant`**
2. **解析 Slash 调用**
   - 得到：
     - `display_prompt`：给聊天记录显示的用户文本
     - `runtime_user_prompt`：给模型的用户正文（移除 `/skills(...)`）
     - `active_skills`：本次显式调用的 Skill 列表
3. **对 `runtime_user_prompt` 执行 `!bang` 解析**
4. **组装 assistant prompt**
   - 原 assistant prompt
   - MCP prompt
   - 静态启用 Skills 摘要（现有逻辑）
   - 本次主动调用 Skills 的完整内容（新增）
5. **入库/显示时使用 `display_prompt`**
6. **发给模型时使用 runtime prompt + active skills prompt**

## 7.3 主动调用 Skill 的 prompt 结构

建议在 `assistant_prompt_result` 末尾追加：

```markdown
# Active Skills (本次显式调用)

以下技能由用户通过 Slash 主动调用，仅对本次请求生效，优先级高于普通参考信息：

<skill identifier="copilot:react-best-practices" invocation="/skills(React Best Practices)">
## React Best Practices
...SKILL.md 正文...

<skill_file path="checklist.md">
...附加文件内容...
</skill_file>
</skill>
```

这样可以：

- 不污染用户消息正文
- 区分“静态启用 Skill”与“本次显式调用 Skill”
- 把完整 Skill 放到更合适的系统上下文层

---

## 8. 后端模块划分建议

由于 `/` 已经是通用入口，建议不要再把解析器做成 `skills/invocation.rs`，而是做成通用 Slash 模块：

```text
src-tauri/src/slash/
  mod.rs
  parser.rs        // 解析 /namespace(argument)
  router.rs        // namespace -> handler
  cache.rs         // completion index / fingerprint / refresh
  types.rs         // SlashInvocation, SlashSkillCompletionItem ...
```

### Skills 相关职责

- `skills/parser.rs`：继续负责解析 `SKILL.md`
- `api/skill_api.rs`：暴露 Slash Completion API
- `mcp/builtin_mcp/agent/handler.rs`：复用同一套 Skill 匹配逻辑
- `slash/router.rs`：在解析到 `skills` namespace 后调用对应 resolver

### 为什么要抽成通用 slash 模块

因为后续 `/artifacts(...)`、其他 slash namespace 都能复用：

- 统一语法解析
- 统一缓存入口
- 统一错误格式
- 统一输入框交互模型

---

## 9. 建议新增/调整的数据结构

### 9.1 Rust

```rust
pub struct SlashNamespaceItem {
    pub name: String,            // skills
    pub description: String,
    pub is_enabled: bool,
}

pub struct SlashSkillCompletionItem {
    pub identifier: String,
    pub display_name: String,
    pub invoke_name: String,
    pub aliases: Vec<String>,
    pub source_type: String,
    pub source_display_name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
}

pub struct SlashInvocation {
    pub namespace: String,
    pub raw_argument: String,
    pub normalized_argument: String,
    pub raw_text: String,
    pub start: usize,
    pub end: usize,
}

pub struct ActiveSkillInvocation {
    pub raw_argument: String,
    pub invoke_name: String,
    pub identifier: String,
    pub display_name: String,
    pub content: String,
    pub additional_files: Vec<SkillFile>,
}

pub struct SlashParseResult {
    pub display_prompt: String,
    pub runtime_user_prompt: String,
    pub active_skills: Vec<ActiveSkillInvocation>,
}
```

### 9.2 TypeScript

建议新增单独的 `src/data/Slash.ts`，避免把 slash router 概念全部塞进 `Skill.ts`：

```ts
export interface SlashNamespaceItem {
  name: string;
  description: string;
  is_enabled: boolean;
}

export interface SlashSkillCompletionItem {
  identifier: string;
  display_name: string;
  invoke_name: string;
  aliases: string[];
  source_type: string;
  source_display_name: string;
  description?: string | null;
  tags: string[];
}
```

---

## 10. 具体实现步骤

## 第 1 步：新增通用 Slash 模块

目标：

- 解析 `/namespace(argument)`
- 支持括号深度和反斜杠转义
- 把 `skills` 注册为第一个 namespace handler

建议：

- 新增 `src-tauri/src/slash/`
- 在 `lib.rs` 中注册相关 state 和命令

---

## 第 2 步：新增 Skills Completion Cache

目标：

- 首次构建 Skills 元数据索引
- 生成 `invoke_name`
- 维护 `invoke_name -> identifier` 精确映射
- 支持 fingerprint-based refresh

建议暴露：

```rust
get_skills_for_slash_completion(force_refresh?: bool)
```

这个接口应尽量返回缓存结果，而不是每次都 full scan。

---

## 第 3 步：前端改造 Slash Router

改造文件：

- `src/components/conversation/InputArea.tsx`
- `src/components/conversation/SlashCompletionList.tsx`（新增）
- `src/utils/pinyinFilter.ts`
- `src/data/Slash.ts`（新增）

完成后应支持：

1. 输入 `/` 显示 namespace 列表
2. 选择 `skills` 后自动插入 `/skills()`
3. 光标落在括号内，立刻显示 Skill 列表
4. 过滤完全在前端本地完成
5. `Tab / Enter / Click` 补全 `invoke_name`

---

## 第 4 步：后端请求预处理接入 Slash 解析

在 `ask_ai` 增加：

1. `extract_assistant_from_message`
2. `parse_slash_invocations`
3. `template_engine.parse(runtime_user_prompt, context)`
4. `format_active_skills_prompt(...)`

同时把 `initialize_conversation` 改造成支持区分：

- 用户看到/数据库保存的 prompt
- 真正发给模型的 runtime prompt

这是整个方案里最关键的一步。

---

## 第 5 步：让 scheduled task 复用同一逻辑

`scheduled_task_api.rs` 也有独立 prompt 渲染链路。

建议同步接入同一套 Slash 解析与 Skill 注入逻辑，保证：

- 聊天输入框里的 `/skills(...)`
- 定时任务 prompt 里的 `/skills(...)`

行为一致。

---

## 第 6 步：后续接入 `/artifacts(...)`

本次先不定义 Artifact 的最终业务语义，只做架构预留：

- Slash Router 支持多 namespace
- 前端 namespace 面板先展示 `artifacts`
- 后续再决定 `/artifacts(...)` 是“打开窗口”“引用内容”“附加上下文”还是其他行为

这样可以避免这次把 Skill 和 Artifact 语义再次硬绑死在一起。

---

## 第 7 步：测试与验证

### Rust 单测

1. `/skills(...)` 基本解析
2. 嵌套括号解析
3. `\(` `\)` `\\` 转义
4. 未闭合括号报错
5. `invoke_name` 冲突去重
6. Slash parser 与 Skill resolver 集成
7. 发送时命中缓存索引并按需加载正文
8. 解析失败后触发一次强制 refresh 并重试

### 前端测试

1. 输入 `/` 显示 namespace 列表
2. 选择 `skills` 后插入 `/skills()`
3. 光标落在括号内
4. Skill 列表来自缓存，不重复发请求
5. 拼音/关键字过滤正常
6. `Enter / Tab / Click` 补全正常
7. 收到 `skills-registry-changed` 后列表自动刷新

### 联调验收

1. 输入 `/skills(React Best Practices)` 并发送
2. 后端成功定位 Skill
3. 仅对命中的 Skill 按需加载 `SKILL.md`
4. 完整 Skill 内容被注入本次 prompt
5. 用户聊天记录仍保持简洁，不被 Skill 正文污染
6. 外部新增/删除 Skill 后，refresh 或下一次 cheap check 能感知变化

---

## 最终建议

正式方案建议如下：

1. 用 `/` 作为统一主动调用入口
2. 第一阶段实现 `/skills(...)`
3. 前端插入的是后端给定的 `invoke_name`，不是前端自己猜出来的字符串
4. 高频交互阶段只使用**元数据缓存**，不预加载所有 `SKILL.md`
5. 真正发送时，才按需加载命中 Skill 的完整内容
6. 缓存更新采用“事件驱动 + 轻量指纹校验”双层策略
7. `ask_ai` 与 `scheduled_task` 共用同一套 Slash 解析器和 Skill 注入链路

这个设计相比原来的 `#skill-token` 更适合长期演进，因为它把：

- 入口语法
- 名称匹配
- 高性能补全
- 运行时 prompt 注入
- 多 namespace 扩展

全部放到了同一个统一模型里。

---

## 建议涉及文件清单

### 前端

- `src/components/conversation/InputArea.tsx`
- `src/components/conversation/SlashCompletionList.tsx`（新增）
- `src/utils/pinyinFilter.ts`
- `src/data/Slash.ts`（新增）

### 后端

- `src-tauri/src/slash/mod.rs`（新增）
- `src-tauri/src/slash/parser.rs`（新增）
- `src-tauri/src/slash/router.rs`（新增）
- `src-tauri/src/slash/cache.rs`（新增）
- `src-tauri/src/slash/types.rs`（新增）
- `src-tauri/src/api/skill_api.rs`
- `src-tauri/src/api/ai_api.rs`
- `src-tauri/src/api/scheduled_task_api.rs`
- `src-tauri/src/mcp/builtin_mcp/agent/handler.rs`
- `src-tauri/src/lib.rs`
