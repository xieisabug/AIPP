# Agent 驱动测试场景对照

本轮按场景参考 t3code，不按测试数量宣称等价。参考版本固定为 [`d5fc29fe5cb62db68bfbf824cfabe0ae94ac6ac4`](https://github.com/pingdotgg/t3code/tree/d5fc29fe5cb62db68bfbf824cfabe0ae94ac6ac4)。没有复制上游代码。

## 范围与统计口径

本次按用户最新要求，重点是 **AIPP 的统一 Agent 使用层**：provider/config 选择、统一 prompt/cancel/config 入口、共享落库与事件顺序，以及这些操作依赖的现有协议边界。不为对齐数字移植 t3 的 fork/rollback、原生子代理、goal、共享进程池或 MCP-app 等专属功能。

上游三个 adapter 文件有 **356 个静态测试注册点**（Claude 198、Codex 81、ACP 77）。这不是实测运行 case 数；`each`、内部循环和条件执行不能与 Rust 测试函数数量直接比较。此次没有执行 t3 测试。以下逐项表保留源标题、固定版本位置、参数化说明和 AIPP 证据，明确区分已证明的边界、适用但尚缺完整验证、以及范围外功能：

- [Claude 198 项](testing/claude-t3-scenarios.md)
- [Codex 81 项](testing/codex-t3-scenarios.md)
- [ACP 77 项](testing/acp-t3-scenarios.md)

表中部分覆盖不代表整个上游测试场景等价通过；新测试也不声称运行了完整 Tauri 桌面 actor。

## 本次追加：真实统一入口和协议边界

- 配置准备：`agent_driver/turn_tests.rs` 调用生产 provider 加载/选路、原生配置提取、MCP payload 签名收尾。非空非法 override 在查询 provider 前明确失败，不悄悄回默认；保留 None/空白兼容。验证合法跨 provider/model 覆盖、权限/模式限制、Claude sentinel、环境/proxy 与 MCP 签名。
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

## 保留并重新运行的已有覆盖

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

既有实现的待核查点：Codex 恢复返回 thread ID 尚无与请求 ID 比对的保护；Claude 提前 EOF 的诊断没有完整退出状态/stderr/run 关联。配置准备失败时前端 catch 会清理局部等待，但后台 user_pending 活动状态可能残留；ACP 启动失败的消息终态、resume/load 失败后新建与历史回填、旧 run 按 conversation 清审批等也未由本轮改写。它们是已存在的原生/桌面生命周期边界，不能用统一 dispatch 或 helper 测试充当已验证。本轮不为凑数字扩大会话架构改造。

## 首轮场景补强的历史验证（提交 e3aa0b7）

在 Linux 容器中实际完成：

- `cargo check --manifest-path src-tauri/Cargo.toml`：通过。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib agent_driver`：13 项通过。
- 同命令分别过滤 `api::ai::acp::`、`api::ai::codex_app_server::`、`api::ai::claude_sdk`、`agent_session_lifecycle_tests`：分别 37、46、31、3 项通过。
- 以上 Rust 范围不重叠，共 130 项，包含已有测试；本轮新增 50 个测试函数，连同初版 7 个，共新增 57 个。函数内部遍历不同 provider/输入不会额外计数。
- `npm run build`：通过；Agent 活动、Plan、会话提示及配置工具相关 6 个前端测试文件：25 项通过。
- 独立 review 与 `git diff --check`：通过。验证没有启动已登录的 Agent CLI，也未运行全仓库测试套件。

## 最终追加验证

最终源码在 Linux 容器中运行，范围互不重叠：

- `cargo check --manifest-path src-tauri/Cargo.toml`：通过（仍有编译警告，未执行无关清理）。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib agent_driver`：28 项通过。
- 同命令过滤 `api::ai::acp::`：41 项；`api::ai::codex_app_server::`：58 项；`api::ai::claude_sdk`：39 项；`agent_session_lifecycle_tests`：3 项，全部通过。
- 共 **169 项 Rust 测试通过**，包含已有回归。本次追加 39 个测试函数；加上前两轮，PR 累计新增 96 个。函数内部的参数循环不另计运行函数数量。
- `npm run build` 通过；相同 6 个 Agent 前端测试文件的 25 项通过。
- 独立 review、356 项来源登记核对与 `git diff --check` 通过。

Codex 权限等待取消的回归先用旧 production wait 实际执行，因 500ms 内不能解除等待而失败，再在修复后通过。最终 resume happy-path fixture保持原 native thread ID，不把不同ID恢复成功固定为契约。

这些是精确范围结果，不是全仓库、真实登录 Agent、完整桌面 UI、打包或跨平台 E2E 的通过声明。Codex Python/Claude shell 子进程 fixture 仅在 Unix 运行；没有为测试安装或登录 Agent CLI。
