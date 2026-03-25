# OpenAI / Copilot `Responses` / `Chat Completions` 模型级请求方式切换方案

## 背景

当前 AIPP 已接入自定义 fork 的 `rust-genai`：

- OpenAI 已有 `AdapterKind::OpenAI` 与 `AdapterKind::OpenAIResp`
- AIPP 当前却仍把 `openai` / `openai_api` 固定映射到 `AdapterKind::OpenAI`
- GitHub Copilot 当前映射到 `AdapterKind::Copilot`

新的产品诉求有两部分：

- 不仅 OpenAI，GitHub Copilot 也要支持“模型级”的请求方式切换
- 交互上希望直接在模型 tag 里切换，用紧凑的 `c` / `r` 图标表达当前请求方式，并放在删除按钮左侧

这次需要把方案写得更准确，特别是：

- 当前 `llm_model` 到底是不是稳定可持久化的载体
- 如果把请求方式拆到独立表，究竟解决了什么、又没有解决什么
- Copilot 端是否真的能像 OpenAI 一样直接切到 `responses`


## 先说结论

### 一、当前 `llm_model` 不是稳定的“模型偏好存储位”

不是“只有改动的模型会删改”，而是存在两个明确的 **provider 级整批删除再重建** 路径：

1. `fetch_model_list`
   - 先 `delete_llm_model_by_provider(llm_provider_id)`
   - 再把远程返回的模型整批重新插入 `llm_model`

2. `update_selected_models`
   - 先 `delete_llm_model_by_provider(llm_provider_id)`
   - 再把用户勾选的模型重新插入 `llm_model`

因此，**在当前实现不改的前提下**，如果把 `request_mode` 直接放进 `llm_model`，它会在“刷新模型列表”和“保存模型选择”这两个核心流程里被覆盖掉。

### 二、拆到独立表并不是“没有问题”，但它确实解决了当前最核心的问题

如果改成独立表，例如 `llm_model_request_mode_preference`：

- 它**可以解决**当前 `llm_model` 被整批删光重建时，模型级请求方式被覆盖的问题
- 但它**不会自动解决**以下问题：
  - provider 删除时的偏好清理
  - provider 导入导出时的偏好迁移
  - 长期积累的“未选中模型偏好”是否要清理

也就是说，拆表不是银弹；但在 **当前 `llm_model` 生命周期没有重构** 的前提下，它仍然是更稳的落地方式。

### 三、Copilot 不能直接照搬 OpenAI 的实现

OpenAI 之所以能切换，是因为 `genai` 已经有：

- `AdapterKind::OpenAI`
- `AdapterKind::OpenAIResp`

但当前 `genai` 对 Copilot 只有：

- `AdapterKind::Copilot`

而且 Copilot adapter 当前 service URL 是写死到：

- `/chat/completions`

这意味着：

- **OpenAI 的模型级切换可以直接接 AIPP 运行时 adapter 选择逻辑**
- **Copilot 想支持 `responses`，需要先补 `genai` 层能力**，不能只改 AIPP 文档或 AIPP UI


## 核心目标

本方案的目标是：

- 不再依赖模型名称自动推断请求方式
- 由用户在“模型级”显式指定请求方式
- 首期目标 provider 范围为：
  - `openai`
  - `openai_api`
  - `github_copilot`
- 前端用统一的 `c` / `r` 紧凑图标表达请求方式
- 配置在应用重启、刷新模型列表、重新保存模型选择后仍保留
- 业务层仍继续统一使用 `exec_chat` / `exec_chat_stream`
- 请求方式不兼容时返回明确错误，不做静默回退


## 非目标

本次不做以下事情：

- 不引入按模型名猜测 `responses` 的逻辑
- 不做 provider 级总开关
- 不做失败后自动从 `responses` 回退到 `chat_completions`
- 不为了这个功能顺手重写所有模型同步逻辑（除非明确决定走“把字段放进 `llm_model` 并顺带重构同步”那条更大方案）
- 不把这个能力扩展到所有 provider


## 当前实现现状

## 一、`llm_model` 的真实生命周期

### 1. 远程获取模型列表时

`fetch_model_list` 当前行为是：

1. 根据 provider 创建 client
2. 远程调用 `client.all_models(...)`
3. 删除该 provider 下所有 `llm_model`
4. 重新插入远程返回的模型

这说明：

- `llm_model` 不是“稳定 upsert”
- 而是“远程列表快照”

### 2. 模型选择弹窗确认保存时

`update_selected_models` 当前行为也是：

1. 删除该 provider 下所有 `llm_model`
2. 仅把 `is_selected = true` 的模型重新写回

这说明：

- `llm_model` 同时还承担了“当前已启用模型集合”的角色
- 用户一旦重新保存选择结果，所有旧行都会被替换

### 3. 手动添加 / 删除模型时

这部分是单行增删：

- `add_llm_model`：直接插入一条模型记录
- `delete_llm_model`：按 `llm_provider_id + code` 删除一条模型记录

所以当前 `llm_model` 的状态是三种写入路径并存：

- 整批重建（远程拉取）
- 整批重建（模型选择确认）
- 单项增删（手动输入）

这也是为什么它并不适合作为“稳定保存用户偏好”的唯一载体。


## 二、前端模型列表现状

### 1. `TagInput`

当前真正带删除按钮的 tag UI 在 `TagInput`：

- tag 文本
- 右侧一个 `X` 删除按钮

因此你提到的“在 model tag 的删除按钮左侧放 `c` 或 `r` 按钮”，从现有 UI 结构上看，最自然的落点就是这套 tag 组件。

### 2. `ReadOnlyModelList`

当前 `ReadOnlyModelList` 只是展示 badge 文本，没有删除按钮，也没有请求方式入口。

如果这次要统一体验，建议不要继续维持两套不同的 tag 表现，而是抽成一个共享的 tag 子组件：

- 模型名
- `c/r` 请求方式按钮
- 可选的删除按钮（有删除能力时显示，无删除能力时隐藏）

### 3. `ModelSelectionDialog`

当前弹窗里每一行只有：

- checkbox
- 模型名
- 多模态能力图标

还没有请求方式切换入口。


## 三、当前运行时路由现状

### 1. OpenAI

AIPP 现在把：

- `openai`
- `openai_api`

都固定映射到：

- `AdapterKind::OpenAI`

所以虽然 `genai` 已支持 `OpenAIResp`，AIPP 运行时并没有把用户配置接进去。

### 2. Copilot

AIPP 当前把：

- `github_copilot`

映射到：

- `AdapterKind::Copilot`

但当前 `genai` 的 Copilot adapter 只有一个 `Copilot` 适配器，并且 URL 仍然走：

- `/chat/completions`

因此 Copilot 的 `responses` 支持不能只在 AIPP 里“切个枚举”就完成。


## 产品设计

## 一、术语调整：用 `request_mode`，不再强行叫 protocol

这里更准确的叫法建议改为：

- `request_mode`

枚举值仍保持清晰可读：

- `chat_completions`
- `responses`

UI 上为了紧凑展示，可以只显示：

- `c`
- `r`

也就是说：

- **存储值** 用完整枚举
- **界面缩写** 用 `c/r`
- **Tooltip** 再把完整含义说明白

这样既满足交互紧凑，也避免概念过于模糊。


## 二、核心交互原则

- 请求方式入口必须跟着“模型”走，而不是藏到 provider 高级配置里
- 请求方式切换和“是否选中该模型”是两个并列维度
- tag 场景下优先使用最小可点击单元，不额外弹二级菜单
- `c/r` 按钮与删除按钮尺寸一致，避免视觉失衡
- Hover 时必须给出完整 Tooltip，避免只看 `c/r` 看不懂
- 不支持该能力的 provider 不展示该入口


## 三、模型 tag 交互（重点按你的要求调整）

### 1. 统一 tag 结构

建议所有“已保存模型”最终都复用同一个 tag 组件，内部结构为：

- 模型名
- `c/r` 按钮
- 删除按钮（如果当前场景允许删除）

排列顺序：

- `模型名` → `c/r` → `X`

也就是 **`c/r` 在删除按钮左侧**。

### 2. `c/r` 按钮样式

建议和当前删除按钮保持一致的点击体积：

- 同级小按钮
- 与 `X` 按钮同高同宽
- 视觉上不抢主标签文字

例如视觉目标可以理解为：

- 跟 `X` 一样是一个小圆形 ghost 按钮
- 中间不是图标，而是 `c` / `r`

### 3. 点击行为

你希望的是“点击后就在 `c` 和 `r` 里切换”，因此这里不建议再弹 `Popover` 或菜单，直接做成 toggle：

- 当前是 `c`，点击后切到 `r`
- 当前是 `r`，点击后切到 `c`

优点是：

- 操作路径最短
- 和 tag 的轻量交互风格一致
- 用户能快速批量调整多个模型

### 4. Tooltip 文案

Hover `c/r` 时，Tooltip 需要明确当前含义，例如：

- `当前请求方式：Chat Completions`
- `当前请求方式：Responses`
- 也可以补充次级提示：`点击切换为 Responses` / `点击切换为 Chat Completions`

### 5. 删除与切换相互独立

需要明确交互约束：

- 点击 `c/r` 只切换请求方式
- 点击 `X` 只删除模型
- 二者都不应触发外层 tag 的其他行为


## 四、`ModelSelectionDialog` 交互

虽然你重点提的是 tag，但首次批量配置仍然发生在 `ModelSelectionDialog`，所以这里也要补入口。

建议每一行模型右侧加入同样语义的 `c/r` 小按钮：

- checkbox 控制是否选中
- `c/r` 控制请求方式
- 两者互不影响

要求：

- 点击 `c/r` 不触发行点击，不影响 checkbox
- 全选 / 取消全选只修改 `is_selected`
- 搜索过滤不丢失 `request_mode` 本地状态


## 五、手动添加模型

手动输入添加模型时，建议如下：

- 新增模型默认 `chat_completions`
- 如果本地已经存在该 `model_code` 的历史 `request_mode` 偏好，则优先恢复历史值
- 添加成功后，tag 立即显示 `c/r` 按钮

这样用户不用先去额外打开配置面板。


## 数据设计：重新评估“放 `llm_model` 还是拆表”

## 一、方案 A：直接把 `request_mode` 放进 `llm_model`

### 优点

- 语义上最直观：请求方式确实是模型级属性
- 查询结构简单，不需要额外 join / merge
- 如果未来模型导入导出本来就连 `llm_model` 一起走，这个字段天然也能跟着走

### 当前阻塞点

但要注意，**这些优点成立的前提，是 `llm_model` 本身必须是稳定 upsert，而不是整批重建**。

如果坚持放进 `llm_model`，那么本次实现不能只加字段，至少还要同时做下面这些改造：

1. 给 `llm_model` 增加 `request_mode`
2. 给 `llm_model` 建立稳定唯一键（至少 `llm_provider_id + code`）
3. 把 `fetch_model_list` 从“先删后插”改为“按 code 做 diff/upsert”
4. 把 `update_selected_models` 从“先删后插”改为“按 code 保留已有行并更新选中状态/模型元数据”
5. 明确远程模型消失、用户取消勾选、手动删除模型时，`request_mode` 应该保留还是清理

### 结论

所以：

- **不是说永远不能放在 `llm_model`**
- 而是 **在当前实现不重构的前提下，不适合直接放进去**
- 如果产品愿意把“模型同步策略重构”为本次需求的一部分，那放进 `llm_model` 就重新变得合理

换句话说：

> 把 `request_mode` 放进 `llm_model` 是一条“更整洁但范围更大”的方案，前提是同步改掉当前 destructive sync。


## 二、方案 B：拆到独立偏好表

建议表名：

- `llm_model_request_mode_preference`

字段建议：

- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `llm_provider_id INTEGER NOT NULL`
- `model_code TEXT NOT NULL`
- `request_mode TEXT NOT NULL DEFAULT 'chat_completions'`
- `created_time DATETIME DEFAULT CURRENT_TIMESTAMP`
- `updated_time DATETIME DEFAULT CURRENT_TIMESTAMP`

唯一索引：

- `(llm_provider_id, model_code)`

### 它真正解决了什么

它解决的是：

- `fetch_model_list` 删除重建 `llm_model` 时，请求方式不被带走
- `update_selected_models` 删除重建 `llm_model` 时，请求方式不被带走
- 远程拉取模型元数据变化时，不会顺手覆盖用户的本地请求方式偏好

### 它没有自动解决什么

它不会天然解决：

1. **provider 删除清理**
   - 需要在删除 provider 时一起删掉这张表里对应 provider 的数据

2. **导入导出迁移**
   - 当前 provider 导出只导出 `name / api_type / endpoint / api_key`
   - 连模型列表本身都没有导出
   - 所以不论 `request_mode` 放在 `llm_model` 还是独立表，当前导入导出都不会保留这部分信息

3. **陈旧偏好回收**
   - 某些模型长期未再出现时，是否需要 GC，要另定策略

### 结论

因此，更准确的结论应该是：

> 独立表并不是“没有问题”，但它确实能解决当前架构下最核心的覆盖问题，而且改动面明显小于“顺带重构整个 `llm_model` 生命周期”。

也就是说，**在当前版本里，独立表仍然是推荐方案**。


## 三、推荐决策

本次推荐采用：

- **短中期：独立偏好表**
- **长期：如果未来把 `llm_model` 同步逻辑彻底改为 diff/upsert，再评估是否回收合并**

这比原文直接写“不要放到 `llm_model` 里”更准确。

更准确的表达应该是：

- **在当前 destructive sync 架构下，不建议放进 `llm_model`**
- **如果愿意把 `llm_model` 的同步策略一起重构，那放进去也可以成立**


## 后端改造方案

## 一、数据库层

### 1. 新增偏好表

在 `src-tauri/src/db/llm_db.rs` 中新增：

- `llm_model_request_mode_preference`

### 2. 新增数据库访问方法

建议新增：

- `get_model_request_mode(llm_provider_id, model_code) -> Option<String>`
- `list_model_request_modes(llm_provider_id) -> Vec<...>`
- `upsert_model_request_mode(llm_provider_id, model_code, request_mode)`
- `delete_model_request_modes_by_provider(llm_provider_id)`

### 3. provider 删除联动清理

如果走独立表，必须把下面动作补齐：

- `delete_llm_provider` 时，同时删除该 provider 的 request mode 偏好

否则就会留下孤儿数据。

### 4. 历史兼容

历史模型没有该配置时：

- 默认按 `chat_completions`

这保持现有行为不变。


## 二、API 层

### 1. 扩展模型结构

以下返回结构都要补充：

- `LlmModel`
- `ModelForSelection`
- `ModelSelectionResponse.available_models[*]`

新增字段：

- `request_mode`

### 2. 批量保存模型选择时

`update_selected_models` 应当：

1. 继续按当前逻辑更新 `llm_model`
2. 对传入的每个模型 `upsert request_mode`

注意这里建议：

- **不要因为模型暂时未选中就删除其 request mode 偏好**

因为 request mode 更像“该 provider 下该 model_code 的用户偏好”，而不是“当前是否启用”的状态。

### 3. 单模型快速切换接口

建议新增 Tauri command：

- `update_llm_model_request_mode`

入参：

- `llm_provider_id`
- `model_code`
- `request_mode`

用于：

- tag 中点击 `c/r` 后立即切换并持久化

### 4. 获取模型列表时合并偏好

以下接口返回模型时都要 merge 本地偏好：

- `get_llm_models`
- `preview_model_list`

如果命中偏好表：

- 返回已保存 `request_mode`

如果没命中：

- 返回默认 `chat_completions`


## 三、运行时请求方式决策

## OpenAI 路径

对 `openai` / `openai_api`：

- `chat_completions` -> `AdapterKind::OpenAI`
- `responses` -> `AdapterKind::OpenAIResp`

这部分可以直接落地。

## Copilot 路径

对 `github_copilot`：

- `chat_completions` -> 当前 `AdapterKind::Copilot`
- `responses` -> **需要先补 `genai` 支持**

### 为什么 Copilot 这里需要额外前置改造

因为当前 `genai`：

- 没有 `AdapterKind::CopilotResp`
- 现有 `Copilot` adapter 的 service URL 仍然走 `/chat/completions`

所以这里有两个可行实现方向：

### 方案 1：在 `genai` 新增 `AdapterKind::CopilotResp`

优点：

- 与 OpenAI 的 `OpenAI / OpenAIResp` 结构对称
- AIPP 运行时映射更直观

缺点：

- `genai` 需要新增 adapter kind 与对应实现

### 方案 2：保留 `AdapterKind::Copilot`，但让 Copilot adapter 内部支持 `request_mode`

优点：

- 不一定需要新增 enum variant
- Copilot 的认证逻辑和公共行为可以复用更多

缺点：

- `genai` 需要把“请求方式”从 AIPP 透传进 adapter，侵入点未必比新增 variant 更小

### 本方案建议

文档层不强绑其中一种，但必须明确写清楚：

> **Copilot 的 `responses` 支持需要包含 `genai` 依赖改造，这不是单纯的 AIPP UI/DB 改造。**


## 前端改造方案

## 一、不要再只传 `string[] tags`

当前 tag 组件基本都是：

- `tags: string[]`

但这次要在 tag 上展示和切换请求方式，仅靠字符串已经不够了，因为至少还需要：

- `model_code`
- `display_name`
- `request_mode`
- 是否允许删除 / 是否允许切换

因此建议引入一个共享前端类型，例如：

- `ModelTagItem`

字段建议：

- `code`
- `name`
- `request_mode`
- `removable`
- `switchable`

这样才能稳定地以 `model_code` 为 key 和更新目标，而不是拿显示名硬操作。

## 二、抽共享组件

建议抽两个共享组件：

### 1. `ModelRequestModeToggle`

职责：

- 显示 `c` 或 `r`
- Tooltip 展示完整含义
- 点击切换
- 仅在支持的 provider 下启用

### 2. `ModelTagBadge`

职责：

- 展示模型名
- 内嵌 `ModelRequestModeToggle`
- 根据场景决定是否显示删除按钮

这样可以同时复用于：

- `TagInput`
- `ReadOnlyModelList`
- 甚至未来其他模型选择面板

## 三、`TagInput`

这是最贴近你要求的位置。

改造后每个 tag 结构应为：

- 文本：模型名
- `c/r` 小按钮
- `X` 小按钮

其中：

- `c/r` 在 `X` 左侧
- 二者尺寸一致
- Tooltip 说明当前请求方式
- 点击 `c/r` 后立即保存

## 四、`ReadOnlyModelList`

当前 Copilot provider 使用的是 `ReadOnlyModelList`，所以这里也必须支持 `c/r`。

建议：

- 不再只是纯 badge 文本
- 改用共享 `ModelTagBadge`
- 如果当前列表不支持删除，就隐藏 `X`，但保留 `c/r`

这样 Copilot 的模型列表也能直接切换请求方式。

## 五、`ModelSelectionDialog`

在每一行模型中加入：

- checkbox
- 模型名
- 多模态图标
- `c/r` 切换按钮

并保证：

- 切换请求方式不会影响 checkbox
- 勾选状态不会重置请求方式


## 用户体验与策略细节

## 一、默认值

未配置时默认：

- `chat_completions`

原因：

- 与当前行为一致
- 升级风险最低

## 二、为什么不用 provider 级开关

因为同一个 provider 下，不同模型完全可能需要不同请求方式。

这点对：

- OpenAI 兼容站点
- Copilot 下不同能力模型

都成立。

## 三、为什么不能按名称猜

同一个模型名在不同 endpoint / 网关 / 中转站上，支持的请求方式未必一致。

因此：

- 名称启发式不应成为产品行为依据

## 四、请求方式错误时如何报错

如果用户把模型切到 `responses`，但 provider 实际不支持：

- 直接报错
- 错误中尽量带上：
  - provider
  - model_code
  - request_mode
  - endpoint

不要自动回退。


## 导入导出：这里要把问题讲清楚

当前 provider 导出只包含：

- `name`
- `api_type`
- `endpoint`
- `api_key`

并**不包含**：

- 模型列表
- 模型级 request mode

所以这里要明确：

- 这不是“拆表才有的问题”
- 就算把 `request_mode` 放进 `llm_model`，当前导入导出一样带不走

因此文档里更准确的结论应该是：

- 如果首期不扩展导入导出，可以接受
- 但应明确标注这是已知缺口
- 后续若要支持 provider 迁移完整体验，应把模型列表与 request mode 一起纳入导出格式


## 推荐实施顺序

### 第一阶段：先补能力判断与数据结构

1. 明确引入 `request_mode`
2. 新增偏好表与读写接口
3. 扩展后端模型结构
4. 扩展前端共享类型

### 第二阶段：OpenAI 端先打通

1. 运行时把 OpenAI 模型映射到 `OpenAI / OpenAIResp`
2. 在 tag 与选择弹窗里完成 `c/r` 切换
3. 验证持久化与刷新后回填

### 第三阶段：补 Copilot 底层能力

1. 在 `genai` 中补齐 Copilot `responses` 支持
2. 更新 AIPP 运行时映射
3. 打通 Copilot 的 `r` 模式真实调用

### 第四阶段：补导入导出与收尾文案

1. 决定是否导出模型列表与 request mode
2. 补充 Tooltip / 错误文案
3. 评估是否需要陈旧偏好清理策略


## 验收标准

满足以下条件即可认为功能完成：

- 在支持的 provider（`openai` / `openai_api` / `github_copilot`）下，模型 UI 中可看到 `c/r` 请求方式按钮
- 在 tag 场景中，`c/r` 按钮位于删除按钮左侧，且尺寸与删除按钮一致
- Hover `c/r` 时能看到完整 Tooltip，明确当前请求方式
- 点击 `c/r` 后可以直接在两种请求方式间切换
- OpenAI 模型能按配置真实走 `Chat Completions` 或 `Responses`
- Copilot 模型在底层能力补齐后，也能按配置真实切换请求方式
- 刷新模型列表、重新保存模型选择、应用重启后，请求方式设置仍然保留
- 未配置请求方式的历史模型保持默认 `chat_completions`
- 配置错误时返回明确错误，不做静默回退


## 最终结论

这次文档更新后，核心结论应该改成下面这版：

- **模型级显式配置仍然是对的**
- **交互上用 tag 内 `c/r` 直切，放在删除按钮左侧，是最符合当前界面的方案**
- **当前 `llm_model` 确实存在 provider 级整批删除重建，不是“只有改动模型才删改”**
- **因此在不重构同步逻辑的前提下，`request_mode` 仍更适合拆到独立偏好表**
- **但拆表并不是没有问题，它只是更精确地解决了“当前 destructive sync 会覆盖模型级偏好”这个核心问题**
- **Copilot 要支持 `responses`，必须把 `genai` 底层能力一并纳入方案，不能假设 AIPP 侧单独改造就够了**

相比原文，这个版本更准确地区分了：

- 当前代码事实
- 请求方式存储设计的边界
- OpenAI 与 Copilot 在技术实现上的差异
