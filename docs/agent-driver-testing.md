# Agent 驱动测试场景对照

本轮按场景参考 t3code，不按测试数量宣称等价。参考版本固定为 [`d5fc29fe5cb62db68bfbf824cfabe0ae94ac6ac4`](https://github.com/pingdotgg/t3code/tree/d5fc29fe5cb62db68bfbf824cfabe0ae94ac6ac4)。没有复制上游代码。

## 新增覆盖

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

既有实现的待核查点：Codex 恢复返回 thread ID 尚无与请求 ID 比对的保护；Claude 提前 EOF 的诊断没有完整退出状态/stderr/run 关联。本轮未为凑通过数改写或掩盖这些行为，也未把格式化诊断的单元测试说成真实退出链路测试。

## 本轮验证结果

在 Linux 容器中实际完成：

- `cargo check --manifest-path src-tauri/Cargo.toml`：通过。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib agent_driver`：13 项通过。
- 同命令分别过滤 `api::ai::acp::`、`api::ai::codex_app_server::`、`api::ai::claude_sdk`、`agent_session_lifecycle_tests`：分别 37、46、31、3 项通过。
- 以上 Rust 范围不重叠，共 130 项，包含已有测试；本轮新增 50 个测试函数，连同初版 7 个，共新增 57 个。函数内部遍历不同 provider/输入不会额外计数。
- `npm run build`：通过；Agent 活动、Plan、会话提示及配置工具相关 6 个前端测试文件：25 项通过。
- 独立 review 与 `git diff --check`：通过。验证没有启动已登录的 Agent CLI，也未运行全仓库测试套件。
