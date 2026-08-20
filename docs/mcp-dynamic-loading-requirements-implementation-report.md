# MCP 动态加载（实验性）功能需求分析与实施报告

## 1. 文档目的

本文档用于定义 **MCP 工具动态加载** 的实验性方案，目标是在不推翻现有设计的前提下，减少上下文浪费，并确保：

- 开关关闭：行为与当前版本完全一致（全量注入）。
- 开关开启：采用"目录摘要 + 按需加载"的新行为。

---

## 2. 背景与问题陈述

当前 MCP 的主要问题是：工具元信息（工具描述、参数 schema）被大规模注入上下文，导致 token 占用高，尤其在多 MCP Server、多工具场景下更明显。

### 2.1 现状（代码层面）

- 非原生模式会把所有可用工具写入 prompt 文本（`src-tauri/src/mcp/prompt.rs`）。
- 原生 toolcall 模式会在请求时注入所有启用工具 schema（`src-tauri/src/api/ai_api.rs` 的 `build_tools_with_mapping` + `build_tool_config`）。

### 2.2 直接影响

- 上下文成本增大（输入 token 增长）。
- 冷启动推理噪音增大（模型先看很多不相关工具）。
- 工具越多，效率越差，扩展性下降。

---

## 3. 目标与非目标

## 3.1 目标

1. 在对话初始阶段只提供极简工具描述（工具目录摘要）。
2. 提供 `load_mcp_server` 和 `load_mcp_tool` 作为内置 MCP 工具，模型在需要时动态加载目标工具的完整 schema。
3. 动态加载结果可持久化到会话维度，保证下一轮请求可复用。
4. 在工具变更（改名/删除/参数变更）时自动失效，避免脏缓存。
5. 以实验开关控制新行为，关闭后完全回到旧行为。
6. 提供 MCP 总结 AI 配置，用于自动生成 server 和 tool 的摘要。

---

## 4. 功能需求（Functional Requirements）

### FR-1 实验开关

- 新增全局配置 `dynamic_mcp_loading_enabled`（默认 `false`），存储在 `system_db.feature_config` 表。
- 在配置中增加一个"实验性"菜单，该功能开关名为"MCP 动态加载"，并提供功能说明和风险提示。
- **助手级覆盖**：全局开关开启后，助手配置界面中出现实验性配置项，默认开启，用户可以关闭。
- **助手界面 MCP 配置隐藏**：当助手的动态加载开关开启时，助手配置界面中的 MCP 服务器选择区域不再展示（因为无需手动配置，默认会加载所有 MCP server 的简介）。
- 关闭时走当前代码的原始逻辑，不应有行为差异。

### FR-2 MCP 总结 AI 配置

- **仅在全局实验开关开启时显示**：在辅助 AI 配置区域增加"MCP 总结 AI"选择下拉框。
- **用途**：用于在刷新 MCP server 能力时，调用该 AI 对 server 和 tools 进行总结，生成简洁的摘要。
- **提示词不可修改**：系统内置固定的提示词用于生成摘要，用户不可自定义。
- **配置存储**：选择的模型 ID 存储在 `system_db.feature_config` 表中，key 为 `mcp_summarizer_model_id`。注意存储的格式和反显到界面的格式，可参考其他配置。

### FR-3 工具目录摘要

- server级别提供整个server的整体描述，使用非常简短的语句概括该 server 的核心能力（由 MCP 总结 AI 生成）。
- server级加载后，提供server下的工具级别摘要：工具名、概括用途（由 MCP 总结 AI 生成）。不提供完整 schema。
- 工具级别加载后，提供完整的 mcp schema。

### FR-4 动态加载工具（内置 MCP 工具）

- `load_mcp_server` 和 `load_mcp_tool` 作为**内置 MCP 工具**（类似 `aipp:agent`），在 `builtin_mcp/templates.rs` 中定义。
- **仅当实验开关开启时**（全局+助手级都开启），这两个工具才会被注入到模型可用工具列表中。
- **强制启用**：这两个工具不受用户的 MCP 配置影响，总是启用。

**`load_mcp_server` 工具参数**：
- `name`: 需求描述或关键字（用于搜索匹配的 server）

**`load_mcp_tool` 工具参数**：
- `names`: 需求描述或关键字，数组形式，可一次加载多个工具
- `server_name`：（可选）限定在某个 server 下加载

### FR-5 会话级持久化

- 记录"当前 conversation 已加载工具集合"，在侧边栏的上下文中展示完整加载的工具名称。
- 后续请求自动从该集合注入 native tools（实验开关开启时）。

### FR-6 自动注入提示词

- 当开启动态加载后，系统在 prompt 中增加提示词，告知模型如何使用 `load_mcp_server` 和 `load_mcp_tool` 加载更多工具。
- 如果使用的是native工具调用，记得修改工具schema。

### FR-7 工具变更自动失效

- 工具删除、禁用、schema 变化、服务器禁用时，已加载状态自动标记失效。

### FR-8 安全与回退

- 若模型直接调用未加载工具，系统返回可解释错误并引导先调用 `load_mcp_tool`。
- 在关键任务可配置"自动尝试补加载一次"。

---

## 5. 非功能需求（Non-Functional Requirements）

### NFR-1 兼容性

- 旧路径必须零侵入：开关关闭时，输出请求结构与当前一致。

### NFR-2 性能

- `load_mcp_server` / `load_mcp_tool` 查询应在本地索引中完成（不依赖实时远程调用）。
- MCP 总结 AI 调用应在后台异步执行，不阻塞 MCP server 刷新流程。

### NFR-3 可观测性

- 记录关键指标：加载命中率、失效率、上下文 token 降幅、额外轮次开销。

---

## 6. 总体设计

## 6.1 双模式行为矩阵

### A. 开关关闭（默认）

- 维持当前实现：
  - native：注入所有已启用工具 schema。
  - non-native：拼接全量 MCP 工具说明。

### B. 开关开启（实验）

- 初始注入：
  - 内置工具 `load_mcp_server` + `load_mcp_tool` + 所有 servers 摘要。
- 运行中：
  - 模型按需调用 `load_mcp_server` / `load_mcp_tool`。
  - native 工具 schema 随着加载的工具动态注入。
  - non-native 无需修改system，工具调用的记录会随着聊天记录发送给模型，相当于模型能够从聊天记录里获取到工具调用方法。

---

## 7. 持久化与数据模型设计

> 目标：既能跨轮次复用，又能在工具变化时可靠失效。

## 7.1 mcp.db 新增表（建议）

### 7.1.1 `mcp_server_capability_epoch_catalog`

- 用途：记录每个 server 能力版本，用于批量失效判断。
- 字段建议：
  - `server_id INTEGER PRIMARY KEY`
  - `epoch INTEGER NOT NULL DEFAULT 1`
  - `last_refresh_at DATETIME NOT NULL`
  - `summary TEXT NOT NULL`（由 MCP 总结 AI 生成的 server 摘要）
  - `summary_generated_at DATETIME`（摘要生成时间）

### 7.1.2 `mcp_tool_catalog`

- 用途：工具摘要索引与版本信息。
- 字段建议：
  - `tool_id INTEGER PRIMARY KEY`（对应 `mcp_server_tool.id`）
  - `server_id INTEGER NOT NULL`
  - `tool_name TEXT NOT NULL`
  - `summary TEXT NOT NULL`（由 MCP 总结 AI 生成的 tool 摘要）
  - `keywords_json TEXT NOT NULL`
  - `schema_hash TEXT NOT NULL`
  - `capability_epoch INTEGER NOT NULL`
  - `updated_at DATETIME NOT NULL`
  - `summary_generated_at DATETIME`（摘要生成时间）
- 索引建议：
  - `(server_id, tool_name)`
  - `(schema_hash)`

### 7.1.3 `conversation_mcp_loaded_tool`

- 用途：会话级已加载工具持久化。
- 字段建议：
  - `id INTEGER PRIMARY KEY AUTOINCREMENT`
  - `conversation_id INTEGER NOT NULL`
  - `tool_id INTEGER NOT NULL`
  - `loaded_schema_hash TEXT NOT NULL`
  - `loaded_epoch INTEGER NOT NULL`
  - `status TEXT NOT NULL`
    - `valid | invalid_changed | invalid_deleted | invalid_disabled | invalid_server_disabled | invalid_renamed`
  - `invalid_reason TEXT`
  - `source TEXT`（`manual|auto|policy_preload`）
  - `loaded_at DATETIME NOT NULL`
  - `updated_at DATETIME NOT NULL`
- 唯一约束建议：
  - `UNIQUE(conversation_id, tool_id)`

---

## 8. 失效处理（改名/删除/修改）详解

## 8.1 核心原则

会话缓存不是永真数据，每次构建请求前都要做"有效性判定"。

判定条件（必须全部满足）：

1. 该工具仍存在且启用。
2. 所属 server 仍启用。
3. `loaded_schema_hash == current_schema_hash`。
4. （可选）`loaded_epoch == current_epoch`。

## 8.2 各类变化的处理策略

### 删除

- 现象：`conversation_mcp_loaded_tool.tool_id` 在当前 `mcp_server_tool` 不存在。
- 处理：标记 `invalid_deleted`，并从注入集合剔除。

### 禁用

- 工具禁用：`invalid_disabled`。
- server 禁用：`invalid_server_disabled`。

### 参数/schema 修改

- 通过 `schema_hash` 检测。
- 不一致时标记 `invalid_changed`，要求重新 load。

### 改名

- 当前系统工具主标识依赖 `server_id + tool_name`，改名通常等价"旧删新建"。
- MVP 建议：按"删除+新增"处理（旧记录 `invalid_renamed` 或 `invalid_deleted`）。
- 增强版：引入 `tool_alias` 或稳定 remote id 后做 rename 关联。

## 8.3 触发时机

在 `refresh_mcp_server_capabilities`、`update_mcp_server_tool`、`toggle_mcp_server` 后执行：

1. 更新 catalog 与 `schema_hash`。
2. 增加 server `epoch`。
3. 批量标记受影响会话缓存失效。
4. （异步）触发 MCP 总结 AI 重新生成摘要。

---

## 9. 实验开关设计

## 9.1 开关层级

建议优先级（高 -> 低）：

1. 助手级配置（仅在全局开关开启时可见，默认开启）
2. 全局配置（默认 false）

## 9.2 兼容要求（关键）

- 仅当**全局开关 AND 助手级开关**都为 true 时启用动态逻辑。
- false 时：
  - 不读取会话 loaded 集参与注入。
  - 不改变现有 prompt 生成逻辑。
  - 不改变现有工具执行链路。

---

## 10. 代码改造建议（按模块）

## 10.1 后端（Rust）

1. `src-tauri/src/db/mcp_db.rs`
   - 新增 3 张表迁移与 CRUD。
   - 增加 `schema_hash` 计算函数（可复用 `ai_api.rs` 的 `short_hash`）。
   - 增加摘要存储字段 `summary` 和 `summary_generated_at`。
2. `src-tauri/src/mcp/builtin_mcp/templates.rs`
   - 增加 `aipp:dynamic_mcp` 内置模板，包含 `load_mcp_server` 和 `load_mcp_tool` 工具定义。
3. `src-tauri/src/mcp/builtin_mcp/mod.rs`
   - 增加 `load_mcp_server` 和 `load_mcp_tool` 执行逻辑。
4. `src-tauri/src/api/ai_api.rs`
   - 工具注入分支：legacy 与 dynamic 双路径。
   - 动态模式下，只注入已加载工具 + `load_mcp_server` + `load_mcp_tool`。
5. `src-tauri/src/mcp/prompt.rs`
   - non-native 模式下，改为目录摘要 + load 规范（开关开启时）。
6. `src-tauri/src/mcp/registry_api.rs`
   - 能力刷新后触发 catalog 与 epoch 更新。
   - 能力刷新后异步触发 MCP 总结 AI 生成摘要。
7. `src-tauri/src/lib.rs`
   - 导出新增 tauri commands。
8. `src-tauri/src/db/mod.rs`
   - 新增 `special_logic_0_0_10` 迁移函数。
9. `src-tauri/src/api/ai/title.rs` 或新建 `mcp_summarizer.rs`
   - 实现 MCP 总结 AI 调用逻辑。
   - 内置固定提示词，不可修改。

## 10.2 前端

1. 整体实验开关可视化（在实验性菜单中）。
2. 助手配置界面：
   - 全局开关开启后，显示助手级实验开关（默认开启）。
   - 当助手的动态加载开关开启时，隐藏 MCP 服务器选择区域。
3. 辅助 AI 配置区域：
   - 全局开关开启后，显示"MCP 总结 AI"下拉选择框。
4. 侧边栏详情视图上下文显示当前会话已加载工具及失效原因。

---

## 11. 实施阶段计划

## Phase 1（MVP，后端可用）

1. 引入实验开关（全局 + 助手级）。
2. 增加 `load_mcp_server` 和 `load_mcp_tool` 作为内置 MCP 工具。
3. 增加会话 loaded 持久化。
4. native 注入改为动态加载集合（仅开关开启时）。
5. 支持失效判定（删除/禁用/schema 变化）。

## Phase 2（MCP 总结 AI）

1. 增加 MCP 总结 AI 配置（前端 + 后端）。
2. 实现 MCP 总结 AI 调用逻辑（内置固定提示词）。
3. 在 `refresh_mcp_server_capabilities` 后异步触发摘要生成。
4. 将生成的摘要存储到 catalog 表。

## Phase 3（优化）

1. auto 策略基于历史使用统计。
2. rename 关联增强（alias/stable id）。
3. 观测指标与报表。

---

## 12. 测试方案

## 12.1 单元测试

1. `schema_hash` 计算一致性。
2. loaded 判定逻辑（valid/invalid_*）。
3. 开关分支行为测试（全局 on/off + 助手级 on/off 组合）。
4. `load_mcp_server` / `load_mcp_tool` 检索与返回格式。

## 12.2 集成测试

1. 开关关闭时请求 payload 与当前版本一致。
2. 开关开启后首次仅注入最小集合。
3. load 后下一轮可调用对应工具。
4. refresh 后缓存自动失效。
5. MCP 总结 AI 异步生成摘要并正确存储。

## 12.3 回归测试

1. 现有 MCP 创建/执行/续写流程不退化。

---

## 13. 验收标准（Definition of Done）

1. 开关关闭：与当前版本行为一致。
2. 开关开启：初始上下文显著缩小（可观测到 token 降低）。
3. 可通过 `load_mcp_server` / `load_mcp_tool` 按需加载并持久化会话状态。
4. 工具删除/禁用/schema 变化后，不会继续使用陈旧缓存。
5. MCP 总结 AI 能够在刷新后异步生成摘要并存储。
6. 关键日志与错误提示清晰可追踪。

---

## 14. 风险与应对

1. **多一轮调用导致时延上升**
   - 应对：关键路径减少补加载。
2. **改名识别不准**
   - 应对：MVP 先按删除+新增处理，后续引入 alias/stable id。
3. **模型未遵循"先 load 再调用"**
   - 应对：增加 deterministic 错误引导 + 可选自动补加载一次。
4. **缓存与配置不一致**
   - 应对：每次构建请求前统一做 validity gate。
5. **MCP 总结 AI 调用失败/超时**
   - 应对：摘要生成失败时使用原始 description 作为降级方案，不阻塞刷新流程。

---

## 15. 结论

该方案满足"**实验性启用、可回滚、不推翻现有实现**"的要求：

- 通过开关实现新旧行为并存；
- 通过会话级持久化提升动态加载复用率；
- 通过 `schema_hash + epoch + 状态机` 保证缓存在工具变化时可控失效；
- 通过 MCP 总结 AI 自动生成高质量摘要；
- 可先低风险 MVP 落地，再逐步演进。
