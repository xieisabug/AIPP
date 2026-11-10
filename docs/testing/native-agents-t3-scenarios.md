# Pi / Cursor / OpenCode：T3 测试参考与 AIPP 证据

参考版本固定为 T3 Code [`e2aa223229e42dd742b6dd05a9fad61775231d88`](https://github.com/pingdotgg/t3code/tree/e2aa223229e42dd742b6dd05a9fad61775231d88)，与本轮协议实现参考版本一致。测试独立编写，未复制上游测试实现，也未运行上游测试套件。下述“通过”只指 AIPP 的测试，不能换算为 T3 场景等价全覆盖。

## 上游确有专项测试

三个 `src/server` 目录的直接子文件中，分别有 Pi 7、Cursor 15、OpenCode 14 份 `*.test.ts`；此处不递归统计子目录，也不把文件数作为用例数。

- [Pi 测试目录](https://github.com/pingdotgg/t3code/tree/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-pi/src/server)：`adapter.test.ts`，以及 commands、driver、MCP extension/injection、status、textGeneration 测试。
- [Cursor 测试目录](https://github.com/pingdotgg/t3code/tree/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-cursor/src/server)：`adapter.test.ts`，以及 CursorAgentSdk、CursorSdk、catalog、transportFailure、auth、credentials、account、skills、driver、status 等专项测试。
- [OpenCode 测试目录](https://github.com/pingdotgg/t3code/tree/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-opencode/src/server)：`adapter.test.ts`，以及 runtime、server owner/ledger、catalog、driver、status、tools、usage、versionProbe 等专项测试。

本轮重点阅读三个 adapter 文件，并补读 Cursor SDK/transportFailure、Pi MCP extension、OpenCode environment 测试。未深入验证的专项文件不会列作已覆盖证据。

## 已落实的适用场景

本地文件均位于 `src-tauri/src/api/ai/agent_driver/`；JavaScript 用例在 `native_agents/` 下按生产适配器拆分，共享替身在 `test-helpers.mjs`。测试直接调用生产适配器，只替换进程、SDK、HTTP 边界。

| 上游依据 | AIPP 生产行为与回归证据 | 验证边界 |
| --- | --- | --- |
| Pi adapter [替换身份检查](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-pi/src/server/adapter.test.ts#L2131) | `pi.test.mjs`：恢复失败、恢复到不同 sessionFile 都报错，关闭自有进程，不发送 new_session。 | 未验证真实文件丢失后的桌面恢复流程。 |
| Pi adapter [command-only admission](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-pi/src/server/adapter.test.ts#L3614) | 原生 prompt receipt 明确为 handled 后检查 get_state；空闲才结束本轮，避免一直显示执行中。 | 不自动重试 prompt，也不增加 `/compact` 专用路由。 |
| Pi adapter [extension error/status](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-pi/src/server/adapter.test.ts#L2006)、[进程退出](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-pi/src/server/adapter.test.ts#L5344) | `pi.test.mjs`：extension_error 带原始错误、扩展路径和阶段；进程非零退出保留 code/signal。 | 上游扩展状态栏、notify 专属展示未移植；本轮只验证错误边界。 |
| Pi adapter [JSONL 分帧](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-pi/src/server/adapter.test.ts#L5255) | `common.test.mjs`：跨字节 UTF-8、多个记录、CRLF、Unicode separator、非法 JSON、过大的未结束记录。 | AIPP 对超限明确失败，不照搬上游丢弃记录后继续运行的策略。 |
| Pi adapter [MCP 注入](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-pi/src/server/adapter.test.ts#L2054) | `pi.test.mjs`：扩展入口与 scoped MCP 配置注入，token 不进入 CLI 参数。 | 只验证启动配置，未证明实际 MCP handshake/tools/call 或截图结果转换。 |
| Cursor adapter [工具生命周期](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-cursor/src/server/adapter.test.ts#L42) | `cursor.test.mjs`：tool-call-completed 中的失败 result 不能标成功；旧回调不能串到下一轮。`native_agents_tests.rs`：未返回终态的工具不会因本轮结束而标成功，失败/取消结束活动，保留已完成结果。 | 子代理树和专属工具呈现没有迁移；工具仍显示为 AIPP 通用活动。 |
| Cursor adapter [wrapped abort](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-cursor/src/server/adapter.test.ts#L1793) | `cursor.test.mjs`：用户停止后的嵌套 AbortError 识别为取消；停止时认证错误仍为错误；run admission 未完成时仍可取消。 | 未用真实 SDK 网络断开来强制制造竞态。 |
| Cursor [transportFailure](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-cursor/src/server/transportFailure.test.ts#L14)、adapter [SDK diagnostics](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-cursor/src/server/adapter.test.ts#L514) | `cursor.test.mjs`：SDK finished 中独立的已知传输诊断视为失败；说明文字和代码块里的相同文本保持正文。只有 final result、没有 delta 时仍交付结果。 | 使用有限的已知诊断模式，不根据任意自然语言推测运行失败。 |
| Cursor adapter [MCP credentials](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-cursor/src/server/adapter.test.ts#L1749) | `cursor.test.mjs`：scoped stdio MCP 配置原样交给 SDK，保持本机自动审核和禁止 agent retry。已有恢复测试验证 exact agent ID 与 model 对象。 | 未覆盖 SDK 内部日志的完整脱敏，也未验证真实账户工具调用。 |
| OpenCode adapter [迟到元数据、工具终态](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-opencode/src/server/adapter.test.ts#L741) | `opencode.test.mjs`：先到 part 后到 assistant 身份时有界缓存并只回放一次；用户正文不回显，text/reasoning 分离；非零退出为失败，明确中止为取消。 | 2 MiB 等待身份缓冲超限会失败；不引入后台子代理执行树。 |
| OpenCode adapter [native question flags](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-opencode/src/server/adapter.test.ts#L663) | `opencode.test.mjs`：问题正文作为答案键，多选保留含逗号标签；custom=false 时拒绝不在原生选项中的答案且不消费请求；非法权限选项同样不消费。 | AIPP 卡片始终允许用户输入“其他”；后端明确拒绝不被原生允许的值。重复题目/歧义多选、超出卡片容量会报错。 |
| OpenCode adapter [pending prompt cancellation](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-opencode/src/server/adapter.test.ts#L2165)、[stream completion](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-opencode/src/server/adapter.test.ts#L2333) | `opencode.test.mjs`：HTTP prompt pending 时可发 abort；SSE 意外正常关闭也必须报错；无正文前的 session.error 立即结束为错误。 | 没有执行真实网络断流或真实 OpenCode 长任务。 |
| OpenCode adapter [stale idle](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-opencode/src/server/adapter.test.ts#L1754) | 现有 `opencode.test.mjs`：忽略尚无本轮活动时的 idle 和其他 session 的事件，按原生文本快照去重。 | 部分覆盖：尚未移植上游基于新 user receipt 的完整 admission 状态机，不能宣称所有跨轮迟到事件已验证。 |
| OpenCode adapter [authenticated session](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-opencode/src/server/adapter.test.ts#L2915)、runtime [environment](https://github.com/pingdotgg/t3code/blob/e2aa223229e42dd742b6dd05a9fad61775231d88/packages/provider-opencode/src/server/OpenCodeRuntime.environment.test.ts#L9) | `opencode.test.mjs`：自有服务随机密码与 Basic header 一致，MCP 合并保留既有 permission 和其他 MCP 配置；恢复身份错误不新建替代 session。 | AIPP 只启动自有 loopback 服务，不实现 T3 外部服务复用或服务 ledger。 |

## 明确未移植或仍需验证

- **与 AIPP 约束不同**：Pi 恢复失败后新建会话、停止后自动重启；Cursor 清理 abandoned run 后自动重发。这些上游成功用例不改变 AIPP 的“当前失败直接报错、不自动重建或重试”规则。
- **当前接入范围之外**：原生 wake 收养、steer、fork/revert、后台子代理树、服务池/ledger、usage/计费、账号 OAuth、沙箱策略切换、完整原生 skills 与图片资产流程。
- **仍需独立验证**：真实账号对话、实际 MCP 协议与工具执行、Pi startup/editor 等复杂交互、跨轮晚到的原生 SSE 事件、Tauri actor 与审批卡片端到端运行。Pi/OpenCode 已有启动及模型发现冒烟不等于这些场景通过。

## 验证命令

```powershell
node --test src-tauri/src/api/ai/agent_driver/native_agents/native-agents.test.mjs
cargo test --manifest-path src-tauri/Cargo.toml --lib api::ai::agent_driver::native_agents::tests
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

协议测试从 10 项增加到 35 项；新增场景先验证失败再修复。Rust 新增两个终态测试。环境内验证按 AGENTS.md 使用异步 shell，每个 shell 一条验证命令并读取完成标记；Rust 继续默认 target，磁盘空间有限时仅关闭 Aipp 测试包调试符号。
