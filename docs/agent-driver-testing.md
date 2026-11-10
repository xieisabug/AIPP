# Agent 驱动测试场景对照

## Pi / Cursor / OpenCode 补充（2026-11-10）

新增协议参考 T3 Code `e2aa223229e42dd742b6dd05a9fad61775231d88`，独立于下文早期三通道的统计口径。

追加阅读了这三个 provider 的上游测试，并按适用场景补充回归；具体来源、已验证边界和未覆盖项见 [Pi / Cursor / OpenCode 场景对照](testing/native-agents-t3-scenarios.md)。Node 测试现按 common、Pi、Cursor、OpenCode 拆分，统一入口保持不变。

- `node --test src-tauri/src/api/ai/agent_driver/native_agents/native-agents.test.mjs`：直接测试生产适配器，通过可替换进程/SDK/HTTP 边界验证 NDJSON Unicode、精确恢复失败不重建、异步取消、模型 ID、SSE 文本与思考分离、用户消息过滤、权限与错误终态。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib api::ai::agent_driver::`：统一层回归，含新驱动注册、环境变量、控制错误、活动事件序号与正文位置。资源不足时仅可调整测试包调试信息，不切换 target 目录。
- `npm run test -- src/hooks/assistant/useAssistantFormConfig.test.ts src/components/NewChatComponent.test.tsx src/utils/agentCapabilities.test.ts`：模型选择与既有会话/能力界面回归。
- 本机 Pi 1.5.1 与 OpenCode 1.3.15 已通过启动和模型发现；该冒烟没有发送模型 prompt。Cursor 官方 SDK 1.2.0 API 已核对并由替身测试验证；真实账号运行未验证。
- 应用验收仍需分别执行：选定模型发消息、工具审批/拒绝、MCP 工具调用、等待提问时停止、继续同一会话、重启恢复、删除活跃会话。依赖或模型错误应保留原始原因，不切换协议或创建替代会话。此处不把协议单元测试视为完整桌面端到端验收。

本轮按场景参考 t3code，不按测试数量宣称等价。参考版本固定为 [`d5fc29fe5cb62db68bfbf824cfabe0ae94ac6ac4`](https://github.com/pingdotgg/t3code/tree/d5fc29fe5cb62db68bfbf824cfabe0ae94ac6ac4)。没有复制上游代码。

## 范围与统计口径

本次按用户最新要求，重点是 **AIPP 的统一 Agent 使用层**：可注册驱动与提供商实例、provider/config 选择、统一 prompt/cancel/config 入口、能力声明与会话发现、审批回复归属、共享事件与错误边界，以及这些操作依赖的现有原生协议。不为对齐数字移植 t3 的 fork/rollback、原生子代理、goal、共享进程池或 MCP-app 等专属功能。

上游三个 adapter 文件有 **356 个静态测试注册点**（Claude 198、Codex 81、ACP 77）。这不是实测运行 case 数；`each`、内部循环和条件执行不能与 Rust 测试函数数量直接比较。此次没有执行 t3 测试。以下逐项表保留源标题、固定版本位置、参数化说明和 AIPP 证据，明确区分已证明的边界、适用但尚缺完整验证、以及范围外功能：

- [Claude 198 项](testing/claude-t3-scenarios.md)
- [Codex 81 项](testing/codex-t3-scenarios.md)
- [ACP 77 项](testing/acp-t3-scenarios.md)

表中部分覆盖不代表整个上游测试场景等价通过；新测试也不声称运行了完整 Tauri 桌面 actor。

## 本轮追加：注册、能力、审批与事件边界

本节描述当前源码中的测试场景与生产消费点，不以测试函数数量换算上游等价覆盖。此前 356 个上游注册点及逐项映射保持原口径；新增的统一层场景补充原映射，不重新计入上游数字。

| 场景 | 当前测试证据与生产边界 | 不作出的承诺 |
| --- | --- | --- |
| 第四种驱动扩展 | `agent_driver/registry_tests.rs` 注册测试驱动，经过生产 provider resolve、prepare、`dispatch_prepared`、内存 `MessageRepository`、自有 acquire/prompt/cancel/config 实现；验证自定义模型标识及不透明配置值。重复/未知注册报错，声明不支持的控制操作不调用 actor。 | 不是新增第四个真实 CLI 集成，也不是运行时加载动态库或前端安装插件。 |
| 同驱动多实例与配置 schema | 同文件验证两个 provider ID 的配置、fingerprint、模型覆盖与 run 独立；错误所属配置、重复配置、非法必填值/枚举值拒绝，失败不覆盖上次有效配置快照。原生准备回归已从 `turn_tests.rs` 移至 `native_tests.rs`。 | 注册表不新建第二套进程池、会话存储或 provider 数据库。 |
| 运行时实例绑定 | `lifecycle.rs` 的生产 lookup 使用 conversation/provider/driver/run 绑定，取消、配置和状态读取共用选路。旧未绑定会话只能在候选唯一时采用；绑定 actor 退出后保留身份，不改用其他 map 的残留 actor。`lifecycle_tests.rs` 对选择算法替换 native map read 进行测试。 | 无桌面窗口的选择算法测试不等价于完整并发进程启动、重连和崩溃恢复。 |
| 持久化 session 的实例归属 | `db/tests/conversation_db_tests.rs` 中 `test_agent_session_instance_*` 使用内存数据库验证同 owner 恢复、跨 owner 不复用、实例切换覆盖最新映射、未归属写入不能抹除已知 owner，以及旧/复合主键迁移。读取 NULL owner 不会自行认领；成功 scoped save 才写入实例。 | 旧 NULL owner 的首次恢复保留兼容行为，无法证明历史实例隔离；同一 conversation/driver 只保留最新映射，不保留每个 provider 的恢复历史。 |
| 真实输入准备与附件 | 新 `turn_tests.rs` 调用生产输入选择：非法 override 在 DB 查询前拒绝、指定 provider 读失败不改查默认、跨实例配置拒绝、Codex/Claude 读取已初始化的文本附件上下文且仅一次，不能表示的非文本附件明确报错。 | 不是图片解码、CLI 文件读取或桌面拖放附件 E2E。 |
| 能力声明与实时发现 | `capabilities_tests.rs` 验证 adapter baseline 与 session 证据、supported/unsupported/unknown、动态目录替换、同驱动实例隔离、有效 opaque ID/空值/空白值、部分目录、旧快照及扩展驱动自定义发现。状态命令和三种原生 snapshot event 实际附带 `agent_capabilities`。 | 不从配置缺失推断服务端不支持；不声明未实现的音频编码或 Codex/Claude 结构化图片输入。 |
| 配置前端消费与兼容 | `useConversationEvents.test.tsx` 和 `agentCapabilities.test.ts` 验证旧字段/命名兼容、未知状态保留、实时能力替换及明确 unsupported 时禁用配置。公共命令仅检查稳定 adapter 操作支持，ID/值原样交由 native actor 校验。 | 不用读取时的旧模型目录拒绝稍后可能有效的值，也不删除 Codex 既有 `thought_level` 别名。 |
| 统一审批、elicitation 与提问回复 | `approval_tests.rs` 经过生产 `resolve_reply` 和真实 pending stores/oneshot，覆盖三种原生驱动及自定义 driver、driver/instance/conversation/run 精确匹配、advertised opaque option、非法输入不消费请求、重复/迟到/未知回复、取消与回复竞争、waiter 丢失及分 scope 清理。普通 MCP 和旧前端 payload 保持兼容。 | 测试不是桌面审批卡片、Feishu 交付或已登录 Agent 的端到端授权验证。 |
| 统一事件与旧前端兼容 | `events_tests.rs` 经过生产 envelope、分类与 publish sink，验证原生类型和 JSON payload 保留、未知事件/字段 roundtrip、调用顺序、身份字段不凭空补全，以及旧 channel 失败时新 channel 仍只尝试一次。桌面发送仅被受控 sink 替代。 | 保留原生事件顺序，不宣称全局排序、全事件去重或所有迟到流式帧隔离。 |
| 统一失败终态与诊断 | `failure_tests.rs` 使用真实内存 SQLite 和生产持久化/发布函数，验证先落库后终态、底层 cause/身份/诊断保留、已完成行不被重复失败覆盖、不跨 conversation 覆写、数据库失败仍显示原始原因，以及缺失诊断的明确说明。 | 已完成消息的条件更新不等价于所有 actor/run 的 exactly-once 保证；缺少的 stderr/退出状态不能伪造。 |

### 扩展与前端迁移的实际范围

- 扩展点是 Rust 代码注册：实现 `AgentDriver`、`PreparedAgentSession`、`AgentSessionControl`，通过 `AgentDriverRegistry::register` 加入。实例使用已有 `llm_provider.id`，经过 `register_instance` 或 provider resolve 的 schema 验证；驱动可提供自己的能力发现。没有新增“配置页面即可装载任意 adapter”的功能。
- conversation、消息仓库、原生 actor、MCP bridge 与原有 session map 保持原有职责；注册表与绑定只做选择、能力描述和归属检查。
- 新后端命令 `confirm_agent_interaction` 与旧确认命令共用 scoped resolver。当前前端仍调用 `confirm_acp_permission`、`confirm_codex_permission`、`confirm_acp_elicitation` 等兼容入口，不宣称 UI 已全面改用新命令。
- 新 `agent_event_{conversation_id}` 与旧 `conversation_event_{conversation_id}` 并行发布。现有前端继续消费旧事件；新增 envelope 保留扩展字段，不能把它视作完整的新前端协议迁移。

### 持久化归属的兼容限制

- 现有 `acp_session` 增加可空 `provider_instance_id`，主键仍为 `(conversation_id, agent_kind)`。带 owner 的记录仅供匹配实例恢复；不同 owner 或未提供实例的调用不会复用它。
- 历史 NULL owner 仍允许首次恢复，保留原有兼容行为并记录警告。单纯读取、失败或中断的恢复不写入归属；只有成功建立后的 scoped save 才记录当前 provider。首次成功保存前无法证明历史会话来自哪个实例。
- 一个 conversation/driver 只保存最新映射。切换 A→B→A 不会保留并自动找回 A 的旧 provider 会话；不把这次增量 owner 字段描述成完整多实例历史仓库。
- 未带实例身份的写入不能覆盖已建立 owner 的记录。删除与兼容查询保留旧通用读取入口；此限制与运行时 binding 的代次检查是不同层次。

### 本轮验证状态

- Agent 相关前端回归：9 个文件、39 项测试通过，范围为 `AgentPlanCard`、`agentSessionNotice`、`AgentActivityCards`、`activitySegments`、`AgentActivityList`、`agentDefaults`、`agentPlanMode`、`agentCapabilities`、`useConversationEvents`。该范围包含此前 2 个文件的 7 项结果，不重复累加。
- `pnpm run build`：TypeScript 与 Vite 构建通过，仍有 chunk-size 警告。
- Rust 冻结源码的非重叠相关范围合计 371 项通过、1 项 opt-in 忽略，具体命令见文末。本节不重复累加；下文 130/169 项仅为历史记录。
- 本轮没有执行 t3 测试；仍未运行真实登录 Agent、完整桌面 UI、安装包或跨平台 E2E。

## 前轮补强：真实统一入口和协议边界

- 配置准备：`agent_driver/native_tests.rs`（原 `turn_tests.rs`）保留生产 provider 加载/选路、原生配置提取、MCP payload 签名收尾回归。非空非法 override 在查询 provider 前明确失败，不悄悄回默认；保留 None/空白兼容。验证合法跨 provider/model 覆盖、权限/模式限制、Claude sentinel、环境/proxy 与 MCP 签名。
- 配置透传：统一 facade 保留空默认值、不透明 config ID 和带空白值，由原生通道判断支持性。
- ACP：真实 SDK duplex 两端经过生产 request dispatch 和 permission reply，验证挂起审批不阻塞通知/另一 RPC、精确 option 回复、取消后迟到选择无效、两个连接隔离、waiter 丢失 fail-closed。不是 OS 子进程或完整 ACP actor。
- Claude：真实本地假 stream-json 子进程，统一 handle 经过生产 read/select 边界写出 interrupt；验证配置失败不损坏控制通道、审批等待取消、迟到回复隔离、关闭/EOF/nonzero 和多会话隔离。审批测试组合真实生产 wait 与 reader，不包含桌面 card 发送与 DB 终态。
- Codex：审批等待取消回归先在旧生产 wait 上实际运行失败（500ms 超时），再修复；取消仅解除本 run 的请求，保留替代 run 和其他会话。协议帧归属与读写边界覆盖见逐项表。

### 测试发现并修复

1. Claude/Codex 主循环内等待审批时无法读取 Cancel 命令。使用每个 native handle 的取消 generation 解除等待，让原控制循环继续处理已排队的 Cancel；活动 turn 仍在时发送 interrupt。不新增 actor/进程池。旧 idle cancel 不应取消下一轮，run-scoped 请求 ID 避免误撤销替代 run。
2. 非空无效的模型/provider override 原会被忽略，落回默认。现在在统一准备入口明确报错，正常空值兼容不变。
3. Codex 带明确标识的迟到通知有 turn/thread 归属保护，避免已知旧 turn 完成通知结束新轮；无标识通知和 server request 仍遵循既有兼容路径，不宣称全部迟到帧隔离。具体协议兼容边界以 Codex 场景表为准。

## 已有统一层回归基础

| 场景 | AIPP 测试与实际边界 | t3 参考 |
| --- | --- | --- |
| 分发顺序与存储失败 | `agent_driver/dispatch_tests.rs` 调用生产 `dispatch_prepared`；真实内存 `MessageRepository`，验证响应落库和 message_add 在获取 session/提交之前；写入失败无 session/event 副作用。桌面窗口和 session I/O 用受控替身。 | `CodexAdapterV2.test.ts` 的事件先后关系、初始化失败场景 |
| prompt / 附件身份 | 同一生产分发函数验证运行时 prompt 原样传递、ACP 只取最近 user 附件、原生 Codex/Claude 不再重复传结构化附件。不是附件解析/CLI 读取文件的 E2E。 | `packages/provider-core/src/server/attachmentPrompt.test.ts`、adapter 输入场景 |
| 提交失败不重试 | 注入关闭 actor 错误后，真实 SQLite 行写成 error/finish_time；生产失败事件构造器产生一次 message_update 和一次 stream_complete，保留底层原因与标识；没有替代会话/重发。 | `packages/provider-core/src/server/failure.test.ts`、adapter 失败终态场景 |
| 并发分发隔离 | acquisition barrier 强制两个分发同时在途，验证 conversation/message/attachment 对应关系。只覆盖共享分发边界，不等价于两个真实 Agent 进程并发。 | `ProviderSessionManager.test.ts` 独立会话并发打开；`ProviderAdapterRegistry.test.ts` 多实例路由 |
| 原生 session 复用 | 三种生产 `reuse_handle` 方法：签名相同 touch 并返回原命令通道，签名不同不修改 entry；不同 conversation 的活动时间、run 与命令通道隔离。 | `CodexDriver.test.ts` 实例环境/continuation 隔离 |
| 取消/配置控制失败 | 原生 mpsc/oneshot 通道验证成功、明确错误、receiver 关闭、reply 丢失、不隐式重试；测试有 timeout 防止回归时无限等待。 | `CodexTextGeneration.test.ts` 中断/进程生命周期；adapter 控制失败场景 |
| 旧 run 清理 | 生产 run-ID guard 验证迟到清理不能删掉替代 run 或别的 conversation。仅清理 guard，不代表所有迟到流式事件都已隔离。 | `AcpAdapterV2.test.ts` Stop 后迟到更新的隔离目标 |
| 权限撤销隔离 | 真实 `AcpPermissionState`：取消本会话全部请求、保留其他/无所属会话请求、重复取消、旧/未知 request ID 不能解决新请求、明确 deny 保留、断开的等待方清理；未知 option ID 在同一锁内拒绝且不消费请求，合法 advertised 扩展 ID 仍可选择。 | `AcpAdapterV2.test.ts` 未知权限不能成为授权、中断撤销待审批 |
| 恢复与错误识别 | Claude resume ID / run 事件标识与失败帧分类；Codex thread ID 解析和有界 stderr 诊断。属于生产 helper 测试，不是完整恢复协议重放。 | `OrchestratorReplayRecovery.integration.test.ts` 恢复身份；`ClaudeTextGeneration.test.ts` 原始错误处理 |
| 异常子进程 | Unix 下生产 spawn helper 启动系统 `false`，验证非零退出及 stdout EOF，带超时；不要求安装或登录 Agent CLI。 | 上游 fake-process/异常退出测试方法 |

## 本轮测试暴露并修复的权限边界

- ACP 选择项必须是本次请求 advertised options 中的精确 ID。未知 ID 返回具体错误，既不消费 pending request，也不唤醒等待方；随后仍可合法选择或取消。ID 是不透明字符串，不限定标准名称，因此合法供应商扩展 ID 保持兼容。ACP/Codex 确认 API 在校验错误时不发送已解决事件。
- Codex command/file 审批仅转发当前原生协议支持的四种字符串决定：accept、acceptForSession、decline、cancel。未知字符串或伪装成字符串的 amendment JSON 会安全取消；permissions grant 未知值保持空权限。合法的结构化 amendment 对象不等同于字符串，当前 UI/decision 通道此前就未支持，本轮不虚构此能力。

## 保留的原生回归范围

- ACP：配置/签名、MCP bridge、启动参数、协议配置更新、权限/elicitation、空闲判定、文本缓冲、内存 duplex fake-agent 通知流和用量。
- Codex：模型/配置、审批映射、Plan、事件映射、MCP、stderr 诊断和 shutdown 原因。
- Claude：既有原生模块测试、独立 resume-ID helper 测试。
- 通用 idle predicate，以及前端 Agent 活动/Plan/会话提示与配置工具测试。

这些已有测试不能算作本轮新增，也不能把 Rust 循环中的每个断言分支算成独立运行 case。实际执行结果以 PR 中各命令的 test summary 为准。

## 仍未覆盖，不宣称与 t3 等价

- 三种完整原生 session loop 的握手→prompt→审批→取消→终态→下一轮全过程。
- 原生取消尚未完成时下一轮排队、所有迟到 delta/后台事件隔离、终态精确一次的全链路验证。
- 原生恢复协议 replay、进程崩溃后的完整持久化/重连路径、真实多进程运行及进程树终止。
- 真实 CLI 登录、MCP server 交互、桌面 Wry 窗口、Windows/macOS/Linux 安装包 E2E。

仍需端到端核查的路径包括：恢复协议中的 thread/session ID 一致性、配置准备失败后的活动标记、启动失败或提前 EOF 时的完整进程诊断、ACP resume/load 失败后的原生恢复策略，以及取消后开始下一轮的全过程。本轮统一失败发布与 scoped 审批清理已经接入原生路径，但生产 helper、内存数据库和真实 pending store 的测试不能替代上述完整 actor/桌面生命周期验证。

## 首轮场景补强的历史验证（提交 e3aa0b7）

在 Linux 容器中实际完成：

- `cargo check --manifest-path src-tauri/Cargo.toml`：通过。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib agent_driver`：13 项通过。
- 同命令分别过滤 `api::ai::acp::`、`api::ai::codex_app_server::`、`api::ai::claude_sdk`、`agent_session_lifecycle_tests`：分别 37、46、31、3 项通过。
- 以上 Rust 范围不重叠，共 130 项，包含已有测试；本轮新增 50 个测试函数，连同初版 7 个，共新增 57 个。函数内部遍历不同 provider/输入不会额外计数。
- `npm run build`：通过；Agent 活动、Plan、会话提示及配置工具相关 6 个前端测试文件：25 项通过。
- 独立 review 与 `git diff --check`：通过。验证没有启动已登录的 Agent CLI，也未运行全仓库测试套件。

## 前轮追加验证记录（本轮扩展改动前）

以下为前轮完成时源码的历史验证，不覆盖上文当前注册/能力/审批/事件改动。在 Linux 容器中运行，范围互不重叠：

- `cargo check --manifest-path src-tauri/Cargo.toml`：通过（仍有编译警告，未执行无关清理）。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib agent_driver`：28 项通过。
- 同命令过滤 `api::ai::acp::`：41 项；`api::ai::codex_app_server::`：58 项；`api::ai::claude_sdk`：39 项；`agent_session_lifecycle_tests`：3 项，全部通过。
- 共 **169 项 Rust 测试通过**，包含已有回归。本次追加 39 个测试函数；加上前两轮，PR 累计新增 96 个。函数内部的参数循环不另计运行函数数量。
- `npm run build` 通过；相同 6 个 Agent 前端测试文件的 25 项通过。
- 独立 review、356 项来源登记核对与 `git diff --check` 通过。

Codex 权限等待取消的回归先用旧 production wait 实际执行，因 500ms 内不能解除等待而失败，再在修复后通过。当时的 resume happy-path fixture 保持原 native thread ID，不把不同 ID 恢复成功固定为契约。

这些是精确范围结果，不是全仓库、真实登录 Agent、完整桌面 UI、打包或跨平台 E2E 的通过声明。Codex Python/Claude shell 子进程 fixture 仅在 Unix 运行；没有为测试安装或登录 Agent CLI。


## 本轮统一层扩展的最终验证

以下为本轮冻结源码在 Linux 容器中的实际结果，范围互不重叠：

- `cargo check --manifest-path src-tauri/Cargo.toml` 与 `cargo build --manifest-path src-tauri/Cargo.toml`：均通过，保留编译警告，未进行无关清理。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib api::ai:: -- --test-threads=2`：315 通过，1 忽略。忽略项是已有的、需要真实付费模型凭证的 opt-in 测试。
- 同命令过滤 `db::tests::conversation_db_tests`：33 通过；`state::activity_state`：13 通过；`mcp::builtin_mcp::interaction`：10 通过。
- 合计 **371 项 Rust 测试通过，1 项忽略**，包括既有相关回归，不是新增测试数量，也不与上游 356 个静态注册点作等价比较。
- 上述范围内 `agent_driver` 模块实际运行 **100 项**：审批 16、能力 16、分发 8、事件 8、错误 7、生命周期 7、原生准备 18、注册表 8、会话控制 7、turn 准备 5。
- 9 个相关前端文件的 **39 项测试通过**；`pnpm run build` 的 TypeScript 与生产构建通过，仅有 bundle 大小提示。
- 独立复审与 `git diff --check` 通过；未合并 PR、未发布安装包、未登录真实 Agent CLI。

本轮实际修正了旧 run 的快照/完成事件、活动更新与清理、session ID 保存的归属校验，以及 ACP 实际 resume/load 出错后不得自动新建的策略。三种完整桌面 actor 流程仍未由这些测试证明。尤其同一 run 内重叠 turn/epoch 的全链路隔离、所有后台工具活动以及数据库写入失败时的持久化去重保证不在“已验证完整”的范围内。旧 NULL 实例归属的首次恢复兼容，以及跨实例新上下文不自动重放完整历史的限制保持明确。
