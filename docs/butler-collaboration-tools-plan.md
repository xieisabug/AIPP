# 管家助手协作工具补齐计划

2026-11-05 更新：按用户后续授权，已实现 list_assistants、get_assistant_details、get_assistant_exchange、stop_assistant_exchange，并将 SuperAdmin 直接注入所有桌面管家助手。下文保留调研范围；产物引用、回复目标和运行中即时送达仍是后续项。

2026-11-08 更新：已移除顶栏「协作记录」弹窗及其专用 Tauri 命令（列表、停止、手动重试回执）。下表是历史调研记录；助手协作工具、聊天往来上下文与后台自动回执继续保留。

以现有联系人长期会话、message_bot 和协作回执为基础，先补“发现合适助手、查看真实能力”，再补“查进度、结束协作、传递产物”。工具保持后台执行，主聊天只展示结果和可点击的居中通信提示，往来正文在上下文弹窗查看。

## 范围

- 包含：桌面管家联系人间协作；总管家和普通联系人使用同一套只读发现接口；现有历史检索、协作记录复用。
- 暂不包含：群聊、移动端、新增执行通道、自动换模型或换助手。

## 已核实的现状

| 能力 | AIPP 当前情况 | 结论 |
| --- | --- | --- |
| 助手发现 | 总管家 prompt 注入目录；SuperAdmin 有 assistant.list | 普通联系人仍需可直接调用的统一只读入口 |
| 助手详情 | assistant.get 返回基本信息、prompt、模型及模型参数；目录有画像摘要、MCP/Skills 名称 | 缺少按需读取的完整能力详情，不能把配置列表等同于实际运行时可用工具 |
| 发消息 | message_bot；请求去重、多轮上限和异步回流已存在 | 复用，不另造派工通道 |
| 搜索历史 | search_bot_history、read_bot_message_context 已存在 | 复用，并在详情/状态结果中提供可继续查询的 ID |
| 协作状态与停止 | list_butler_peer_exchanges、stop_butler_peer_exchange 为 Tauri 命令 | 需有归属校验的模型工具封装；现有 stop 更新协作记录，不代表强制终止正在执行的进程 |

主要代码：`src-tauri/src/api/assistant_summary_api.rs`、`src-tauri/src/mcp/builtin_mcp/superadmin/actions/assistant.rs`、`src-tauri/src/api/butler_peer.rs`、`src-tauri/src/mcp/builtin_mcp/templates.rs`。

## 参考实现

- [DS-Bot tools.js](https://github.com/Fei2411/DS-Bot/blob/main/ds-bot/src/host/tools.js)：list_bots 返回角色、简介和实时工作状态；message_bot 支持回复发起者或用户、立即送达或排队、请求去重；还有 stop_bot、create_bot、update_bot、list_bot_models、list_capabilities 及能力管理工具。
- [Grok Bot 官方协作说明](https://docs.x.ai/grok-bot/chat-and-collaboration#let-bots-hand-work-off)：确认异步交接、接收助手唤醒和稍后回信，并建议每阶段明确一个负责人。公开产品文档不等于其内部工具定义，因此以下工具名是 AIPP 的设计建议。

## 建议的接口

### 第一阶段：两个只读工具

1. `list_assistants(query?, after_id?, limit?)`
   - 返回稳定 assistant_id、姓名、职责摘要、标签、是否总管家、联系人 conversation_id，以及空闲/运行/排队/等待授权或用户输入等状态。
   - 搜索支持姓名和能力描述；同名返回多项，由 assistant_id 消歧，不随意选人。
   - 与左侧联系人使用同一过滤规则，包含初始普通聊天助手及总管家；不创建会话、不生成画像、不修改配置。
   - 状态标明观察时间；不把空闲错误地解释为模型可用，不为查询而真实调用模型。
2. `get_assistant_details(assistant_id, sections?, offset?, limit?)`
   - 默认返回短概览；sections 可选择 profile、summary、tools、skills、prompt、runtime。
   - 工具返回名称、用途、来源和已启用/已加载/可按需加载状态；Skills 返回名称、简介与启用状态；区分配置事实和当前会话实际可用能力。
   - summary 是助手画像，不能冒充该助手长期经验记忆。prompt 显式请求时单独分页返回已配置的助手 prompt，不直接复制会话历史或临时系统上下文。
   - 查询只读，排除 API Key、环境变量值和连接凭据；工具详情按需提供，避免默认一次返回所有工具 schema。

### 第二阶段：控制与交付

3. `get_assistant_exchange(exchange_id?, include_context?, before_id?, limit?)`：返回参与助手、状态、轮次、待谁回复、最近结果及消息引用。复用现有上下文读取，校验协作归属；模型按需查询，不通过轮询代替事件回流。
4. `stop_assistant_exchange(exchange_id, reason)`：先明确为“停止这次协作后续轮次”。取消未投递消息、阻止晚到结果触发下一轮，记录原因，重复调用幂等。如果要终止当前执行，单独接原生取消能力并准确报告结果，不能只改数据库就声称进程已停止。
5. 增强 `message_bot`，不另加同义发送工具：支持原消息/产物引用、明确的回复目标；默认答复发起助手。用户明确要求某助手直接交付时，允许该助手在自己的联系人聊天交付；内部讨论仍只进通信弹窗。文件引用验证权限和可访问性，保留去重与轮次限制。
6. DS-Bot 的 `when=now` 可作为后续能力：只有原生通道确实支持运行中送达时才实现；否则明确排队，不宣称已即时打断。暂不为此增加新会话或降级通道。

### 暂缓增加

- 创建/改名/配置助手：复用 SuperAdmin 管理动作；根据用户后续授权，所有管家助手均可直接使用，沿用现有审核与审计。
- 共享能力目录：先由 get_bot_details 满足查询；确有跨助手复用 MCP/Skills 的需求时再引入 list_capabilities，配置写操作沿用现有管理链路。
- 独立记忆工具：需先定义个人记忆/共享知识的归属、来源和失效机制；已有历史检索不重复建设。
- 群聊及组管理：继续暂缓。

## 执行步骤与验收

- [ ] 提取只读助手目录/详情服务，复用画像和配置读取；与联系人列表校对一致性，避免 SuperAdmin 和新工具各自维护一份定义。
- [ ] 注册 list_bots 和 get_bot_details，接入管家联系人执行上下文；默认输出限长、分页稳定，所有身份使用 ID，查询不创建联系人会话或写数据库。
- [ ] 用实际执行通道的能力解析结果提供 tools/skills 状态；对“已配置但不可用”给出准确原因，不自动换模型、provider 或通道。
- [ ] 将现有协作状态/上下文接口封装为工具；补停止协作语义、归属检查和晚到回信处理，防止终止一项任务误伤助手的其他任务。
- [ ] 增强 message_bot 的回复目标和产物引用；验证两位助手“初稿→审稿→修订→交付”多轮链路，主聊天不出现内部正文或工具卡片。
- [ ] 更新总管家和联系人用法说明：先发现再派发、回执不等于完成、收到真实结果后交付。工具 schema 保持稳定，忙闲状态和完整 prompt 按需读取，避免每轮重写 system 前缀；跨压缩边界记录模型实际返回的 cache read/input tokens，不承诺压缩不失缓存。
- [ ] 内存库测试覆盖同名/不存在助手、范围隔离、分页、禁用能力、重复请求、停止后晚到结果；前端连续状态测试覆盖零工具卡片挂载和结果归属。运行 npm build、cargo check 及定向测试，再对已配置模型做实际双助手协作与缓存观测；模拟结果与真实模型结果分开记录。

## 总管家 prompt 位置

- 基础正文：`src-tauri/src/api/butler_api.rs` 的 `BUTLER_SYSTEM_PROMPT_BASE`，约第 97 行。
- 拼装：同文件 `build_butler_system_prompt`，约第 479 行；替换名字，附加助手目录、可信工作区及开发工具规则。
- 初始化：`ensure_butler_system_assistant` 调用 `butler_assistant_profile::sync` 同步到数据库。直接改数据库中的系统助手 prompt 会在下次初始化时被生成值覆盖；应改基础正文并重新编译后重启。

## 尚需决策的后续项

- 暂按“助手画像”解释 summary；如需检索长期经验记忆，应作为独立需求。
- 第一阶段两个只读工具可独立交付；强制终止执行、共享能力写入和运行中即时送达不作为第一阶段前置条件。
