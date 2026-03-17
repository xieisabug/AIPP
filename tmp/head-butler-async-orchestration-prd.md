# AIPP 总管家实验窗口 PRD / Spec

## 1. 文档信息

- 文档名称：AIPP 总管家实验窗口 PRD / Spec
- 文档状态：Spec
- 文档范围：实验模式窗口、总管家主会话、任务会话、权限编排、结果回流、飞书机器人接入

---

## 2. 产品定义

AIPP 新增一个独立于普通 Chat 模式的 `总管家（实验）` 窗口。

该窗口的产品定义如下：

1. 它是一个新的 window，与现有 `chat_ui` window 分离。
2. 它不展示普通 Chat 的历史对话列表和多会话入口。
3. 启用实验模式后，默认打开窗口可从 `chat` 切换为 `总管家（实验）`。
4. 用户只与一个长期存在、持续复用的 `butler_main` 主会话交互。
5. 总管家派发任务时，**每个任务都会创建一个新的 `butler_task conversation`**。
6. 任务 conversation 独立运行、独立上下文、独立工具调用，彼此不阻塞。
7. 任务结果、通知、权限请求统一回流到总管家窗口。
8. 任务使用哪个助手、采用什么 handoff 合同、完成后如何处理结果，均由总管家决定。
9. task conversation 的默认权限模板来自其执行助手本身。
10. 高权限能力仅在实验模式下开放，不进入普通 Chat 模式。
11. 总管家窗口可选绑定飞书机器人，实现消息双向同步。

---

## 3. 产品目标

### 3.1 主要目标

1. 提供一个独立的总管家实验工作台。
2. 支持总管家连续接收多个任务请求。
3. 支持总管家将任务派发为独立 conversation。
4. 支持多个 task conversation 并行运行。
5. 支持 task 结果以摘要、结构化结果、证据引用等形式可回读。
6. 支持 task 权限请求在总管家窗口集中处理。
7. 支持总管家长期维护单一主会话的上下文、记忆和历史。
8. 支持总管家与飞书机器人双向同步消息。

### 3.2 非目标

1. 不改造普通 Chat 模式为任务编排模式。
2. 不给普通 Chat 助手开放实验模式高权限。
3. 不在首版提供复杂 DAG 编排器。
4. 不在首版开放无限制自动安装工具或自动执行危险能力。
5. 不在首版支持一个 task conversation 被 finalize 后再自动 reopen 并重复 finalize。

---

## 4. 交互与界面规格

## 4.1 窗口结构

`总管家（实验）` 窗口采用三栏布局：

### 左栏：任务列表

展示当前 `butler_main` 下的任务 conversation：

- Accepted
- Running
- Waiting Approval
- Succeeded
- Failed
- Cancelled

每个任务卡片至少显示：

- 任务标题
- `task_conversation_id`
- 状态
- 执行助手
- 最近更新时间
- 是否存在未处理权限请求
- 是否已 finalize

### 中栏：总管家主会话

用于用户和总管家交互，展示：

- 用户输入
- 总管家即时答复
- 任务创建确认
- 任务结果处理决定
- 汇总结论
- 飞书来源消息标识

中栏中的消息来源至少区分：

- 本地用户输入
- 飞书来源输入
- 总管家回复
- 任务结果卡片 / 任务处理卡片

### 右栏：详情面板

根据当前焦点展示：

- 任务详情
- 结构化结果
- 权限请求
- 审批结果
- 证据与 artifact 引用
- 主会话历史检索结果
- Checkpoint / Memory 信息

## 4.2 默认打开窗口

新增配置项：

- `default_home_window = chat | butler_experiment`

当实验模式启用且配置为 `butler_experiment` 时，默认打开 `总管家（实验）` 窗口。

## 4.3 主交互模型

用户始终对 `butler_main` 发起请求。  
总管家对每条请求有三种处理方式：

1. 直接答复
2. 创建一个或多个 task conversation 执行
3. 先创建或选择助手，再创建 task conversation

总管家决定派发任务时，应调用明确的内部能力：

- `spawn_task_conversation`

该能力由总管家模型决定何时使用，但系统层面必须是显式的内部能力 / API，而不是隐式副作用。

总管家创建任务后，应立即回复任务已受理，用户可继续输入下一条请求。

## 4.4 主会话历史浏览

由于 `butler_main` 是长期唯一主会话，历史查看不采用普通 Chat 的多会话列表模式，而采用同一主会话内的历史浏览模式。

总管家窗口需要提供以下历史浏览能力：

1. 按时间轴浏览主会话历史消息
2. 按日期范围筛选
3. 按来源筛选：
   - 本地输入
   - 飞书输入
   - 总管家回复
   - task 结果相关
4. 按 task 关联筛选
5. 从主会话历史直接跳转到对应 task 详情
6. 从 Checkpoint 快速跳转到对应历史区间

历史浏览属于同一个 `butler_main` 的内部视图，不是新的 conversation。

## 4.5 通知模型

总管家窗口内统一展示通知。

### 轻通知

用于状态变化：

- task 完成
- task 失败
- task 取消
- task 进入等待审批
- task finalize 完成

表现形式：

- badge
- toast
- 任务卡状态变化

### 中通知

用于主会话中的重要摘要：

- “bug 调查已完成”
- “发版风险分析失败”
- “某任务等待权限审批”
- “某任务结果已可供总管家读取”

### 强通知

用于需要立即决策的情况：

- 申请更高工具权限
- 申请创建新助手
- 申请创建新 skill
- 申请访问高风险资源

## 4.6 任务详情视图

用户可在总管家窗口直接查看任务详情，无需进入普通 Chat 列表。

任务详情至少包含：

- 输入目标
- task conversation 基本信息
- 当前状态与时间线
- 执行助手
- handoff 合同
- 一句话摘要
- 结构化结果
- 原始证据
- 工具调用 / artifact 引用
- 审批记录

---

## 5. 运行模型

## 5.1 会话类型

系统需要支持以下 conversation 类型：

- `normal`
- `butler_main`
- `butler_task`

其中：

- `butler_main` 表示总管家主会话
- `butler_task` 表示由总管家派发的任务会话

## 5.2 总管家主会话唯一性

在实验模式下，系统在当前本地用户数据空间内全局只维护一个 `butler_main`。

规则如下：

1. 首次进入总管家窗口时，如果不存在 `butler_main`，系统自动创建一个。
2. 后续再次打开总管家窗口时，始终加载这个已有的 `butler_main`。
3. `butler_main` 不进入普通 Chat 会话列表。
4. `butler_main` 的完整消息历史长期保留，供历史浏览、检索、checkpoint 和记忆提取使用。

## 5.3 总管家主会话的上下文分层

`butler_main` 是长期唯一会话，但模型上下文不应在每次请求时直接回放全部历史。

系统采用分层上下文模型：

### 原始历史层

保存 `butler_main` 的完整消息历史，用于：

- 历史查看
- 搜索
- 回溯
- 审计

### Checkpoint 层

系统定期为 `butler_main` 生成 checkpoint，用于压缩长期历史：

- 最近阶段目标
- 已完成的重要任务
- 未完成事项
- 当前工作假设
- 关键决策

### Task Handoff 层

每个 `butler_task` finalize 后形成可索引的任务结果记录，用于：

- 主会话后续引用
- 多任务汇总
- 历史检索
- 上下文补充

### Memory 层

用于保存需要长期保留、可被重复引用的信息，例如：

- 用户偏好
- 固定协作方式
- 长期项目背景
- 总管家确认过的重要事实

Memory 不等同于全部历史，而是长期有效的信息片段。

### Active Snapshot 层

实时记录：

- 当前运行中的 tasks
- 等待审批的 tasks
- 最近完成但尚未处理的 tasks

## 5.4 总管家主会话的上下文装配顺序

每次 `butler_main` 收到新请求时，系统建议按以下顺序装配上下文：

1. 最近一段主会话原始消息
2. 当前运行中的 task 快照
3. 最近 finalize 且与当前请求相关的 task handoff
4. 最近的主会话 checkpoints
5. 被标记为长期有效的 memory entries
6. 用户或总管家显式点选召回的历史片段

默认不将完整历史全文注入模型上下文。

## 5.5 主会话历史与记忆查看方式

用户查看之前的聊天记录和记忆时，采用以下模型：

1. 在总管家窗口内浏览同一个 `butler_main` 的历史时间轴
2. 可通过日期、来源、task、关键词筛选
3. 可查看主会话 checkpoints
4. 可查看长期 memory entries
5. 可从历史消息跳转到对应 task 详情和结果
6. 可从 task 结果反向跳转到主会话中首次引用它的位置

该方案解决两个问题：

- `butler_main` 长期唯一，但历史仍然可浏览
- 模型上下文不需要每轮都吞下全部历史

## 5.6 任务即会话

在实验模式中：

- `spawn 一个任务 = 创建一个新的 butler_task conversation`

每个 `butler_task conversation` 具备：

- 独立上下文
- 独立消息流
- 独立工具执行
- 独立状态
- 独立取消能力

MVP 阶段中，任务的“继续追加工作”通过创建 follow-up task conversation 实现，不在同一个已 finalize 的 task 内做 reopen。

## 5.7 任务创建合同

总管家创建 task 时，需要明确一份任务创建合同，至少包含：

- `butler_conversation_id`
- `task_title`
- `task_goal`
- `executor_assistant_id`
- `executor_assistant_source`
- `permission_template_source`
- `handoff_contract`
- `result_handling_mode`
- `notification_policy`

其中：

- `executor_assistant_source` 可取：
  - `existing_assistant`
  - `quick_assistant`
  - `new_assistant`
- `permission_template_source` 默认等于执行助手本身
- MVP 阶段中，`permission_template_source` 固定跟随执行助手，不提供独立于助手之外的自定义权限模板

## 5.8 任务助手选择

task 用哪个助手由总管家决定。

总管家可采用以下方式：

1. 选择现有助手
2. 使用快捷助手
3. 先创建一个新助手，再将其作为执行助手

创建新助手或快捷助手属于总管家能力的一部分，不属于普通 Chat 模式能力。

## 5.9 权限模板

task conversation 的默认权限模板来自执行助手本身。

规则如下：

1. 不默认继承 `butler_main` 的全部权限
2. task 的默认权限边界以执行助手配置为准
3. 当 task 需要助手权限之外的额外权限时，必须回到总管家窗口审批

## 5.10 任务生命周期

建议状态：

- `accepted`
- `running`
- `waiting_approval`
- `succeeded`
- `failed`
- `cancelled`

此外，系统需要单独维护：

- `is_finalized`
- `finalized_at`

`is_finalized` 表示该 task 的结果已经被系统确认、落库并可供总管家读取。

## 5.11 任务完成判定与 finalize

对于 `butler_task` conversation，系统需要有明确的“真正结束”判定。

### 完成判定条件

一个 `butler_task` conversation 满足以下条件时，视为任务已完成并应 finalize：

1. conversation 运行态进入 `Idle`
2. 当前不存在执行中的 MCP tool call
3. 当前不存在等待继续执行的后续工具调用链
4. 当前最后一轮 assistant 输出已经结束

### finalize 动作

当 `butler_task` conversation 第一次满足完成判定条件时：

1. 系统将其标记为 `is_finalized = true`
2. 生成任务结果对象
3. 向对应 `butler_main` 发出任务结果可用通知
4. 更新任务列表中的状态、摘要和详情入口

### MVP 规则

MVP 阶段采用单次 finalize 规则：

1. 一个 task 一旦 finalize，不再自动 reopen
2. 若总管家希望继续同一目标，应显式创建 follow-up task conversation
3. follow-up task 可引用上一个 task 的结果作为输入

## 5.12 任务 handoff 合同

task 的 handoff payload 如何生成、生成到什么粒度，由总管家在 `spawn_task_conversation` 时决定。

系统需要支持 handoff 合同概念，至少包含：

- `handoff_mode`
- `summary_requirement`
- `structured_output_schema`
- `evidence_policy`
- `artifact_policy`
- `delivery_mode`

建议支持的 `handoff_mode`：

1. `summary_only`
2. `structured_result`
3. `full_handoff`
4. `manual_pull`

说明如下：

- `summary_only`：生成简要摘要
- `structured_result`：生成摘要 + 结构化结果
- `full_handoff`：生成摘要 + 结构化结果 + 完整证据引用
- `manual_pull`：仅标记任务结果可用，由总管家自行读取 task 详情

## 5.13 任务结果如何回到主会话

task 完成后，系统始终会：

1. 更新 task 状态
2. 写入任务结果记录
3. 触发通知

但 task 结果如何进入 `butler_main`，由总管家自己决定。

总管家可以选择：

1. 立即在主会话中生成处理结论
2. 仅显示通知，不立刻写入主会话正文
3. 稍后统一汇总多个 task 结果
4. 继续读取 task 详情后再回复用户

系统不强制 task 完成后自动把全文或固定摘要灌入主会话。

## 5.14 任务回传对总管家的可见数据

总管家需要能读取以下信息：

- 任务状态
- 执行助手
- 一句话摘要
- 结构化结果
- 证据引用
- artifact 引用
- 是否有后续建议动作
- 是否存在未完成审批
- task conversation 的最终消息引用
- handoff 合同

总管家默认不读取：

- 全量推理过程
- 全量工具调试日志
- 全量网页抓取原文
- 全量中间消息流

需要这些内容时，总管家可显式进入 task 详情查看。

## 5.15 多任务汇总

总管家支持对多个已完成任务做统一汇总。  
汇总默认基于以下材料，而不是直接拼接全文：

1. task 摘要
2. 结构化结果
3. handoff payload
4. 证据引用

---

## 6. 功能需求

## 6.1 总管家主会话

总管家主会话需要支持：

- 接收用户输入
- 直接答复
- 创建 task conversation
- 接收任务结果可用通知
- 汇总多个任务结果
- 展示并处理权限请求
- 长期保存同一个主会话

### 验收标准

- 用户可连续输入多个请求
- 总管家可连续创建多个任务
- 总管家主会话不因单个任务执行而阻塞
- 关闭并重开总管家窗口后仍回到同一个主会话

## 6.2 主会话上下文、记忆与历史

系统需要支持：

- 主会话完整历史持久化
- 主会话 checkpoint
- 长期 memory entries
- task 结果索引
- 历史筛选与检索

### 验收标准

- 用户可查看之前的主会话聊天记录
- 用户可按时间、关键词、来源、task 查看历史
- 模型每次请求不会默认加载完整历史全文
- 系统可基于 checkpoint 与 task handoff 装配后续上下文

## 6.3 任务 conversation 创建

当总管家决定派发任务时，系统必须新建一个 `butler_task conversation`。

任务创建必须明确：

- 执行助手
- 默认权限模板
- handoff 合同
- 结果处理模式

### 验收标准

- 每个任务都有独立 conversation id
- 每个任务 conversation 可独立查看运行状态
- task conversation 不进入普通 Chat 会话列表
- task 的执行助手可被明确追踪

## 6.4 并行执行

多个 task conversation 可并行运行。

### 验收标准

- 创建第二个任务时，第一个任务无需结束
- 总管家窗口可同时展示多个任务状态
- 某个 task 的工具执行不阻塞其他 task 的创建和运行

## 6.5 结果可用与结果处理

task 完成后，结果需在总管家窗口中变为可用。

结果可用内容至少包括：

- `task_conversation_id`
- 状态
- 执行助手
- 一句话摘要
- 结构化结果
- 详情入口
- handoff 合同

结果如何进一步进入主会话，由总管家决定。

### 验收标准

- 任务完成后总管家窗口可见
- 总管家可按需读取任务结果
- 总管家可基于结果生成主区总结
- 用户可查看详情与证据

## 6.6 权限请求中转

task conversation 执行中产生的权限请求必须统一回到总管家窗口。

### 验收标准

- 权限请求可关联到具体 task conversation
- 用户可批准 / 拒绝
- 审批结果可回传给对应 task conversation
- 默认权限模板按执行助手生效

## 6.7 实验模式专属高权限能力

以下能力仅在实验模式开放：

- 创建助手
- 创建 skill
- 调用全部工具能力
- 查看与处理跨 conversation 权限请求
- 通过选择执行助手确定 task 默认权限模板

### 验收标准

- 这些能力不出现在普通 Chat 助手能力面
- 实验窗口中总管家可调用这些能力
- 总管家可选择现有助手、快捷助手或新建助手执行 task

## 6.8 飞书机器人接入

总管家窗口支持绑定飞书机器人通道，实现双向同步。

### 能力范围

1. 总管家可将消息同步发送到飞书机器人所在会话。
2. 任务完成摘要可同步发送到飞书。
3. 权限请求提醒可同步发送到飞书。
4. 飞书中发给机器人的消息可回流为总管家主会话中的新输入。
5. 总管家对飞书来源消息的回复，可同时展示在窗口内并回发到飞书。

### 接入类型

为满足双向收发，飞书接入基于**飞书自建应用机器人**实现。  
仅支持单向发消息的 webhook 机器人不满足本需求。

### 入站规则

飞书机器人收到的用户消息进入 `butler_main`，并保留来源信息：

- 飞书用户标识
- 飞书会话标识
- 来源时间
- 来源渠道 = `feishu`

### 出站规则

支持以下同步策略：

- 仅同步总管家主会话回复
- 同步总管家回复 + 任务完成摘要
- 同步总管家回复 + 任务完成摘要 + 权限请求提醒

### 验收标准

- AIPP 可向飞书机器人所在会话发送消息
- 飞书用户发给机器人的消息可进入总管家主会话
- 总管家可对飞书消息继续派发任务
- 同一条飞书来源消息在 AIPP 中具备可追踪来源

## 6.9 实验配置入口

实验模式相关开关与配置统一放在配置页的**实验性功能**区域，并与当前动态 MCP 实验配置放在同一处。

建议在现有实验配置区域下新增 `总管家实验模式` 分组。

### 配置项

- `butler_experiment_enabled`
- `default_home_window`
- `butler_default_assistant_id`
- `butler_task_notification_level`
- `butler_permission_center_enabled`
- `butler_main_checkpoint_enabled`
- `butler_main_memory_enabled`

其中：

- `butler_default_assistant_id` 用于定义 `butler_main` 的默认助手配置，并作为总管家创建任务时的默认起点

### 飞书配置项

- `butler_feishu_enabled`
- `butler_feishu_app_id`
- `butler_feishu_app_secret`
- `butler_feishu_encrypt_key`
- `butler_feishu_verification_token`
- `butler_feishu_sync_policy`
- `butler_feishu_allowed_chat_ids`

### 配置要求

- 实验模式关闭时，不显示总管家入口
- 飞书配置仅在实验模式启用后显示
- 飞书双向接入需基于飞书自建应用机器人
- 飞书配置保存后需显示连接状态与最近收发状态

---

## 7. 数据与模型规格

## 7.1 Conversation 扩展字段

建议扩展：

- `conversation_kind`
- `parent_butler_conversation_id`
- `source_task_title`
- `is_hidden_from_normal_chat_list`
- `channel_source`

## 7.2 ButlerMainState

用于标识总管家唯一主会话：

- `id`
- `butler_conversation_id`
- `slot = default`
- `last_active_at`
- `last_checkpoint_id`
- `created_at`
- `updated_at`

## 7.3 ButlerMainCheckpoint

用于压缩长期主会话历史：

- `id`
- `butler_conversation_id`
- `anchor_message_id`
- `summary`
- `open_items_json`
- `task_refs_json`
- `created_at`

## 7.4 ButlerMemoryEntry

用于保存长期有效的主会话记忆：

- `id`
- `butler_conversation_id`
- `memory_type`
- `title`
- `content`
- `source_ref`
- `is_pinned`
- `created_at`
- `updated_at`

## 7.5 ButlerTaskDefinition

用于记录任务创建合同：

- `id`
- `butler_conversation_id`
- `task_conversation_id`
- `title`
- `goal`
- `executor_assistant_id`
- `executor_assistant_source`
- `permission_template_source`
- `handoff_contract_json`
- `result_handling_mode`
- `notification_policy`
- `created_at`

## 7.6 ButlerTaskIndex

用于总管家窗口聚合展示：

- `id`
- `butler_conversation_id`
- `task_conversation_id`
- `title`
- `status`
- `is_finalized`
- `executor_assistant_id`
- `has_pending_permission`
- `last_summary`
- `created_at`
- `updated_at`
- `finalized_at`

## 7.7 ButlerPermissionRequest

- `id`
- `task_conversation_id`
- `request_type`
- `reason`
- `status`
- `resolved_by`
- `created_at`
- `resolved_at`

## 7.8 ButlerTaskResult

可采用独立表或 JSON 字段落库，建议至少包含：

- `task_conversation_id`
- `handoff_mode`
- `payload_json`
- `summary`
- `structured_output_json`
- `evidence_json`
- `artifact_refs_json`
- `followup_suggestions_json`
- `final_message_id`
- `created_at`

## 7.9 ButlerNotification

- `id`
- `butler_conversation_id`
- `task_conversation_id`
- `notification_type`
- `title`
- `body`
- `importance`
- `is_read`
- `created_at`

## 7.10 ButlerChannelBinding

用于绑定飞书机器人通道：

- `id`
- `butler_conversation_id`
- `channel_type`
- `channel_target_id`
- `sync_policy`
- `is_enabled`
- `created_at`
- `updated_at`

---

## 8. 架构与集成要求

## 8.1 复用现有基础设施

本方案优先复用 AIPP 已有能力：

- conversation / message 体系
- MCP 工具调用
- conversation 事件总线
- window 管理能力
- assistant / skill 基础设施

## 8.2 总管家窗口

新增独立 window，用于：

- `butler_main` 主会话
- 任务聚合展示
- 结果回流展示
- 权限审批
- 主会话历史与记忆查看
- 飞书通道状态显示

## 8.3 任务执行

每个 `butler_task conversation` 作为独立执行单元运行。  
单个 task conversation 内部的执行约束仅影响该任务本身，不应阻塞总管家窗口继续创建其他任务。

## 8.4 主会话上下文组装器

系统需要新增面向 `butler_main` 的上下文组装器。

职责：

- 读取最近主会话消息
- 读取活动中的 task 快照
- 读取 checkpoints
- 读取 memory entries
- 读取相关 task handoff
- 生成一次对主会话请求所需的上下文输入

该逻辑优先位于后端，避免由前端窗口自行拼接长期上下文。

## 8.5 事件类型

建议新增或聚合以下事件：

- `butler_main_loaded`
- `butler_task_created`
- `butler_task_updated`
- `butler_task_finalized`
- `butler_task_result_available`
- `butler_permission_requested`
- `butler_permission_resolved`
- `butler_notification_created`
- `butler_main_checkpoint_created`
- `butler_channel_message_received`
- `butler_channel_message_sent`

## 8.6 飞书通道服务

飞书接入需要独立的通道服务，负责：

- 飞书应用配置
- 连接状态维护
- 入站消息接收
- 出站消息发送
- 消息与 `butler_main` 的映射

## 8.7 任务结束检测器

系统需要新增一个面向 `butler_task conversation` 的结束检测器。

职责：

- 监听 task conversation 运行态变化
- 判断是否已真正结束
- 触发 `butler_task_finalized`
- 按 handoff 合同生成任务结果
- 写入 `ButlerTaskResult` 与 `ButlerNotification`

该检测逻辑位于后端，不依赖前端窗口是否打开。

---

## 9. 权限模型

## 9.1 普通 Chat 模式

默认不开放：

- 创建助手
- 创建 skill
- 全工具运行权限
- 跨 conversation 权限协调
- 飞书机器人控制面

## 9.2 实验模式总管家

可开放：

- 创建助手
- 创建 skill 草案
- 使用全部工具能力
- 查看并处理 task conversation 权限请求
- 管理飞书通道绑定
- 为 task conversation 选择执行助手，并由执行助手确定默认权限模板

## 9.3 Task Conversation

task conversation 的权限规则如下：

1. 默认权限模板来自执行助手本身
2. 可低于总管家权限
3. 不默认继承总管家的全部权限
4. 需要额外权限时必须回到总管家窗口审批

---

## 10. 现状缺口与需改造能力

## 10.1 当前缺少的关键能力

当前为支持总管家实验窗口，仍缺少以下关键能力：

1. 独立的总管家 window 与入口切换
2. `butler_main / butler_task` conversation 类型定义
3. 唯一主会话的持久化与加载
4. 任务 conversation 对普通 Chat 列表隐藏
5. task conversation 到 `butler_main` 的父子关联
6. 面向 `butler_main` 的上下文组装器
7. 主会话 checkpoint / memory / 历史检索
8. task conversation 真正结束后的后端 finalize 检测
9. 按 handoff 合同生成任务结果
10. task conversation 权限请求向总管家窗口回流
11. 总管家窗口内的任务列表、详情面板、审批面板
12. 飞书双向通道服务与消息映射

## 10.2 需要改造为总管家可用的现有能力

以下现有能力可复用，但需要做总管家模式适配：

### Conversation / Message

- 现有 conversation 与 message 体系可直接复用
- 需要增加 conversation 类型、父子关联、普通列表隐藏能力

### Runtime State / Activity State

- 现有 `runtime_state_snapshot` 与 `Idle` 运行态可复用
- 需要增加面向 `butler_task` 的结束判定与 finalize 触发

### MCP Tool Call

- 现有 MCP tool call 执行和状态广播可复用
- 需要把 task conversation 中的工具执行结果聚合为任务级结果

### Assistant / Skill 管理

- 现有助手和 skill 基础设施可复用
- 需要补一层“仅实验模式总管家可调用”的能力控制
- 需要支持总管家创建助手 / skill 草案或实体

### Experimental Config

- 现有动态 MCP 实验配置入口可复用
- 需要在同一区域增加总管家实验模式配置与飞书配置

### Permission Flow

- 现有权限请求相关能力可复用
- 需要新增跨 conversation 的审批汇聚与回传

### Window Routing

- 现有多窗口架构可复用
- 需要增加默认主页窗口切换到总管家实验窗口

## 10.3 建议新增的总管家专属能力

建议为总管家模式新增或内聚以下能力：

- `load_butler_main_conversation`
- `build_butler_main_context`
- `search_butler_main_history`
- `spawn_task_conversation`
- `list_butler_tasks`
- `get_butler_task_result`
- `resolve_butler_permission_request`
- `create_assistant_draft`
- `create_skill_draft`
- `send_butler_channel_message`
- `bind_feishu_channel`

---

## 11. 分阶段实施

## Phase 1：实验窗口与任务会话 MVP

- 新 window
- 默认打开入口切换
- 唯一 `butler_main`
- 主会话基础历史浏览
- task conversation 创建
- task 执行助手选择
- 助手权限模板继承
- 任务列表与状态展示
- task finalize 与结果可用通知

## Phase 2：结果处理、主会话记忆与权限中心

- 主会话 checkpoint
- 主会话 memory entries
- handoff 合同落地
- task 结果按需处理
- 权限请求中转
- 审批回传
- 多任务汇总

## Phase 3：高权限能力与飞书接入

- 创建助手
- 创建 skill
- 全工具权限模板
- 飞书双向消息同步
- 飞书来源消息到 `butler_main` 映射

---

## 12. 成功标准

满足以下条件即可认为该实验模式成立：

1. 用户进入的是独立总管家窗口，而不是普通 Chat。
2. 用户始终在同一个长期存在的 `butler_main` 中与总管家交互。
3. 用户可查看之前的主会话聊天记录、checkpoint 和记忆信息。
4. 用户可连续给总管家下多个任务。
5. 每个任务都会变成独立 conversation。
6. 任务之间不会阻塞继续派发。
7. task 默认权限模板来自执行助手。
8. 结果可集中回到总管家窗口，并由总管家决定如何处理。
9. 权限请求可集中回到总管家窗口。
10. 高权限能力不会泄漏到普通 Chat 模式。
11. 飞书消息可双向同步到总管家主会话。
