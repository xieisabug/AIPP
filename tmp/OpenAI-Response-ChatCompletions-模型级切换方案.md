# OpenAI `Responses` / `Chat Completions` 模型级切换方案

## 背景

当前 AIPP 已经接入自定义 fork 的 `rust-genai`，而该 fork 实际已经支持 OpenAI `Responses API` 对应的 `OpenAIResp` 适配器。

但 AIPP 现有实现中，`src-tauri/src/api/genai_client.rs` 会把 `openai` / `openai_api` 固定映射到 `AdapterKind::OpenAI`，导致运行时始终走 `Chat Completions` 路径，无法让用户显式指定某个模型走 `Responses`。

这会带来两个问题：

- 不能利用 `genai` 已有的 `OpenAIResp` 能力。
- 如果依赖模型名规则自动推断协议，容易误判，因为不同站点、不同兼容网关下，同名模型未必都应该走同一协议。


## 核心目标

本方案的目标是：

- 不再依赖模型名称自动判断是否走 `Responses`。
- 在“提供商配置页”的模型列表中，为每个模型提供一个可见、可改、可持久化的协议配置入口。
- 配置粒度为“模型级”，而不是“提供商级”。
- 保持业务调用入口不变，继续统一使用 `exec_chat` / `exec_chat_stream`。
- 未配置的历史模型保持现有行为，不破坏线上兼容性。


## 非目标

本次不做以下事情：

- 不引入自动按模型名猜测协议的逻辑。
- 不把协议切换扩展到所有 provider；首期仅对 `openai` / `openai_api` 生效。
- 不做失败后自动从 `responses` 静默回退到 `chat completions`。
- 不改 `genai` 的外部调用方式，不新增 `exec_responses` 业务层接口。


## 当前现状

### 1. `genai` 能力

`genai` 当前支持：

- `Client::exec_chat(...)`
- `Client::exec_chat_stream(...)`
- `AdapterKind::OpenAI`
- `AdapterKind::OpenAIResp`

也就是说，业务层不需要改成“分别调用两个 API”；只需要在运行时正确选择 adapter。

### 2. AIPP 当前问题

当前 AIPP 的协议选择是 provider 导向的，而不是 model 导向的。

在 `genai_client.rs` 中：

- `openai` / `openai_api` 会固定映射到 `AdapterKind::OpenAI`
- 后续 `ServiceTargetResolver` 又会用这个固定 adapter 构造 `ModelIden`

因此，即使 `genai` 支持 `OpenAIResp`，AIPP 运行时也不会选中它。

### 3. 模型列表存储现状

当前 `llm_model` 表保存的是：

- `name`
- `llm_provider_id`
- `code`
- `description`
- 多模态支持字段

但不保存“该模型请求协议”。

同时，当前 `update_selected_models` 的实现是：

- 先删掉该 provider 下所有模型
- 再重建选中的模型

因此如果直接把协议字段硬塞进 `llm_model` 表，在批量更新时很容易丢失用户配置。


## 产品设计

## 一、交互原则

- 协议配置必须在“模型”旁边，而不是藏到 provider 高级配置里。
- 用户在看到模型列表时，应该能直观看到每个模型当前使用的是哪种协议。
- 修改协议不应影响“是否选中该模型”这个动作，二者是并列维度。
- 手动添加的模型与远程拉取到的模型，都必须支持配置协议。


## 二、协议枚举

定义统一的协议值：

- `chat_completions`
- `responses`

首期只在 OpenAI 类 provider 上展示这个能力：

- `openai`
- `openai_api`

其他 provider 暂不展示协议图标，也不写入协议配置。


## 三、模型选择弹窗（`ModelSelectionDialog`）

### 交互改造

在每一行模型条目的右侧，新增一个协议图标入口。

建议表现形式：

- 图标按钮 + Tooltip
- 点击图标后弹出一个轻量菜单或 `Popover`
- 可选项只有两个：
  - `Chat Completions`
  - `Responses`

### 展示要求

每个模型行应同时包含：

- 复选框：控制该模型是否被保存/启用
- 模型名
- 模态能力图标（视觉/音频/视频）
- 协议图标

### 协议图标建议

建议使用直观但不喧宾夺主的图标语义，例如：

- `MessagesSquare` / `MessageCircle` 表示 `chat_completions`
- `Workflow` / `Sparkles` / `Waypoints` 表示 `responses`

也可以用统一图标配合不同颜色/Badge 文案，例如：

- `Chat` Badge
- `Resp` Badge

建议以 Tooltip 明确文案：

- `当前协议：Chat Completions`
- `当前协议：Responses`

### 默认行为

如果模型还没有显式配置协议，默认显示：

- `Chat Completions`

注意：这个默认值只是“默认展示与默认保存值”，不是名称推断结果。


## 四、已保存模型列表（`ReadOnlyModelList`）

### 交互改造

当前已保存模型列表只展示 badge 文本，不足以体现协议配置。

建议把每个模型 badge 升级为：

- 模型名
- 协议小图标或协议缩写
- 可点击后直接切换协议

### 目的

这样用户不必每次都打开“获取模型列表/选择模型”弹窗才能修改协议。

尤其对于：

- 手动输入添加的模型
- 已经保存到本地但远程列表暂时拉不到的模型

都能保留直接修改入口。


## 五、手动添加模型（`TagInputContainer`）

当前支持用户手动输入 model code。

这部分也必须支持协议配置，建议如下：

- 新增的手动模型默认 `chat_completions`
- 保存后在 `ReadOnlyModelList` 中立即显示协议图标
- 用户可在保存后直接改协议

这样可以避免在输入时增加复杂表单，降低交互阻力。


## 数据设计

## 一、为什么不直接改 `llm_model`

虽然从语义上看，协议偏好属于模型属性，但当前实现中 `llm_model` 会被“整批删除再重建”，直接把协议存进去会有两个风险：

- 批量更新模型时丢失用户协议选择
- 未来如果远程拉取字段更新，也更容易误覆盖用户本地偏好

因此建议将协议配置单独存储。


## 二、推荐表结构

新增一张独立表，例如：

`llm_model_protocol_preference`

字段建议：

- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `llm_provider_id INTEGER NOT NULL`
- `model_code TEXT NOT NULL`
- `request_protocol TEXT NOT NULL DEFAULT 'chat_completions'`
- `created_time DATETIME DEFAULT CURRENT_TIMESTAMP`
- `updated_time DATETIME DEFAULT CURRENT_TIMESTAMP`

唯一索引：

- `(llm_provider_id, model_code)`


## 三、设计理由

这样做的好处：

- 协议偏好独立于远程模型同步结果
- 即使 `update_selected_models` 删除并重建 `llm_model`，协议偏好也不会丢失
- 手动添加模型、远程获取模型、本地历史模型都能统一处理
- 后续如果要增加更多模型级网络偏好，也可以继续扩展这张表


## 后端改造方案

## 一、数据库层

### 1. 新增表

在 `src-tauri/src/db/llm_db.rs` 中新增建表逻辑。

### 2. 新增数据库访问方法

建议新增以下方法：

- `get_model_protocol_preference(llm_provider_id, model_code) -> Option<String>`
- `set_model_protocol_preference(llm_provider_id, model_code, request_protocol)`
- `list_model_protocol_preferences(llm_provider_id) -> Vec<...>`

### 3. 数据库升级

在 `src-tauri/src/db/mod.rs` 增加新的版本升级逻辑：

- 提升 `CURRENT_VERSION`
- 增加对应 `special_logic_xxx`
- 负责创建 `llm_model_protocol_preference`

### 4. 历史兼容

不需要为历史模型回填复杂规则。

默认即可：

- 未命中配置时，按 `chat_completions` 处理


## 二、API 层

### 1. 扩展模型返回结构

当前 `LlmModel` / `ModelForSelection` 需要补充一个字段：

- `request_protocol: String`

适用接口：

- `get_llm_models`
- `preview_model_list`
- `update_selected_models`

### 2. 新增单模型更新接口

建议增加 Tauri command：

- `update_llm_model_protocol`

入参：

- `llm_provider_id`
- `model_code`
- `request_protocol`

用途：

- 在 `ReadOnlyModelList` 中快速切换单个模型协议

### 3. 批量保存接口

`update_selected_models` 在保存模型选择结果时，也要顺带保存每个模型的协议。

注意：

- 先删除/重建 `llm_model`
- 再逐条 upsert `llm_model_protocol_preference`

不要因为模型未选中就删除其协议偏好，除非产品上明确要这么做。

建议首期策略：

- 对未选中的模型，保留其协议偏好
- 用户再次选回时，自动恢复之前的协议设置


## 三、运行时协议选择

### 1. 改造原则

运行时仍保持统一调用：

- `exec_chat`
- `exec_chat_stream`

只改“如何决定 adapter kind”。

### 2. 优先级设计

对于 `openai` / `openai_api`：

1. 先读取当前模型的 `request_protocol`
2. 若为 `chat_completions`，使用 `AdapterKind::OpenAI`
3. 若为 `responses`，使用 `AdapterKind::OpenAIResp`
4. 若无配置，默认 `AdapterKind::OpenAI`

对于其他 provider：

- 保持现有逻辑

### 3. 落点

建议在 `src-tauri/src/api/genai_client.rs` 收口。

即新增一个“基于 provider + model + explicit protocol”的 adapter 决策函数，供：

- `create_client_with_config`
- `infer_adapter_kind_simple` 的相关调用链

统一使用。

### 4. 明确不做静默回退

如果用户把某模型配置成 `responses`，但目标站点不支持：

- 应直接返回明确错误
- 错误中尽量带上 endpoint / provider / model / protocol

不要偷偷再试一次 `chat_completions`，否则会让用户难以理解真实行为。


## 前端改造方案

## 一、类型定义

以下前端类型要同步补充 `request_protocol`：

- `ModelForSelection`
- `ModelSelectionResponse.available_models[*]`
- `LLMModel`


## 二、组件拆分建议

### 1. 新增可复用协议选择组件

建议抽一个小组件，例如：

- `ModelRequestProtocolBadge`
- 或 `ModelRequestProtocolToggle`

能力：

- 展示当前协议
- Tooltip 说明
- 点击切换
- 仅在 OpenAI provider 下启用

这样可以复用于：

- `ModelSelectionDialog`
- `ReadOnlyModelList`

### 2. `ModelSelectionDialog`

在每个模型行中增加协议组件。

交互上要注意：

- 点击协议图标时不触发行点击，不影响 checkbox 勾选
- 搜索过滤不影响协议状态
- 全选/取消全选只改 `is_selected`，不改协议值

### 3. `ReadOnlyModelList`

把只读 badge 变为“轻交互 badge”。

建议表现：

- badge 主体显示模型名
- 尾部带一个协议标识
- 点击协议标识弹菜单切换

### 4. `TagInputContainer`

无需在“输入时”增加协议字段。

只需保证：

- 手动添加后，刷新列表时能带回默认协议
- 在已保存模型列表中可修改协议


## 用户体验细节

## 一、为什么不用 provider 级开关

因为同一个 provider 下可能存在多个模型，而这些模型未必都适合同一协议。

例如：

- 某些模型需要 `responses`
- 某些模型继续用 `chat_completions`

如果放在 provider 级，会导致：

- 误伤其他模型
- 用户难以理解为什么切了一个 provider，所有模型行为都变了


## 二、为什么不能按名字猜

因为用户可能接的是：

- OpenAI 官方
- OpenAI 兼容中转
- 第三方站点
- 私有网关

同样的模型名在不同站点上，支持的协议未必一致。

所以“名称启发式”最多只能作为内部参考，不能作为产品行为的真实依据。


## 三、为什么默认保守

默认 `chat_completions` 的原因：

- 与当前行为保持一致
- 降低升级风险
- 历史已有 provider / 模型不会突然改变请求路径


## 四、图标显示范围

首期只对 `openai` / `openai_api` 显示协议图标。

原因：

- 其他 provider 当前没有对应的双协议切换语义
- 提前展示会制造误导
- UI 上也更清晰，不会让所有模型列表都出现一个暂时无意义的开关


## 风险与注意事项

## 一、模型列表刷新

当前刷新模型列表会重新拉远程模型并更新本地模型表。

要确保：

- 重新获取模型列表后，已有协议偏好能正确 merge 回 `available_models`
- 不会因为远程返回结果变化而覆盖本地协议配置


## 二、手动模型与远程模型并存

有些用户会：

- 手动输入模型
- 同时使用远程自动获取模型

因此协议配置的主键必须使用：

- `llm_provider_id + model_code`

而不是显示名。


## 三、错误处理

当协议与实际站点不兼容时，要给出清晰错误提示，例如：

- 当前模型配置为 `Responses`
- 当前 endpoint 不支持该协议
- 请在模型列表中切换为 `Chat Completions`

这样用户能理解问题来源，不会以为是 API Key 或模型名错了。


## 四、导入导出

当前 provider 支持分享与导入。

这次方案若首期不扩展导入导出，也可以接受，但需要明确：

- 导出 provider 时，是否同时导出模型协议偏好

建议最终纳入导出，否则用户迁移配置时会丢失这部分设置。

如果首期先不做，也应在文档和代码里标记为后续补充项。


## 推荐实施顺序

### 第一阶段：数据与运行时

1. 新增协议偏好表
2. 新增 DB 读写方法
3. 扩展后端模型结构
4. 在运行时接入协议选择

目标：

- 即使前端还没完全完成，后端能力已闭环


### 第二阶段：模型选择弹窗

1. 在 `ModelSelectionDialog` 增加协议图标
2. 支持批量保存模型选择 + 协议

目标：

- 用户能从“获取模型列表”入口完成首次配置


### 第三阶段：已保存模型快速修改

1. 改造 `ReadOnlyModelList`
2. 增加单模型协议更新接口

目标：

- 用户无需反复进入弹窗即可调协议


### 第四阶段：补充导出导入与文案

1. 评估 provider 分享/导入是否带出协议偏好
2. 补充 Tooltip、错误文案、空态文案


## 验收标准

满足以下条件即可认为功能完成：

- 在 OpenAI provider 的模型列表里，每个模型都能看到协议标识。
- 用户可以为同一个 provider 下的不同模型分别设置 `chat_completions` 或 `responses`。
- 模型协议设置在应用重启后仍然保留。
- 刷新模型列表、重新获取远程模型时，不会丢失已有协议配置。
- 运行时会严格按模型配置选择 `OpenAI` 或 `OpenAIResp`。
- 未配置协议的历史模型继续保持当前行为，即默认 `chat_completions`。
- 配置错误协议时，用户能收到明确错误，而不是静默回退。


## 最终结论

本方案的核心是：

- **不再用名称猜协议**
- **把协议选择权交给用户**
- **以模型级显式配置作为唯一可信来源**

从工程实现角度看，这也是对现有架构侵入最小、可解释性最强、后续可维护性最高的一种落地方式。
