# Kiro / Antigravity：T3 参考与验收

参考时间：2026-11-10，T3 `main`。采用协议行为与回归场景，不照搬 T3 的 Effect 运行框架。

| 上游来源 | AIPP 对应覆盖 |
| --- | --- |
| [Kiro driver.test.ts](https://github.com/pingdotgg/t3code/blob/main/packages/provider-kiro/src/server/driver.test.ts) | V3 启动参数、cli 认证、自定义命令；`acp-native.test.mjs` 检查实际传给 launcher 的参数 |
| [Kiro adapter.ts](https://github.com/pingdotgg/t3code/blob/main/packages/provider-kiro/src/server/adapter.ts) 与 [acpSupport.ts](https://github.com/pingdotgg/t3code/blob/main/packages/provider-kiro/src/server/acpSupport.ts)（实现参考） | Agent → model → effort 顺序、晚到模型选项、提前取消、allow_once 与持久化权限区别。AIPP 对不支持的显式设置直接报错，不沿用上游部分忽略/保留默认的行为 |
| [AntigravityAcpSupport.test.ts](https://github.com/pingdotgg/t3code/blob/main/apps/server/src/provider/acp/AntigravityAcpSupport.test.ts) | 不可用模型报错、保留显式模型、分组原生模型选项；本次未接入其多媒体场景 |
| [AntigravityProtocol.test.ts](https://github.com/pingdotgg/t3code/blob/main/apps/server/src/provider/acp/AntigravityProtocol.test.ts) | `interaction_` 映射用户问题、原样返回 opaque optionId、不把选项中的拒绝当作取消；权限提示保留原生安全提示 |
| [antigravityAuthSupport.test.ts](https://github.com/pingdotgg/t3code/blob/main/apps/server/src/provider/antigravityAuthSupport.test.ts) | `agy acp` 与显式 standalone/harness、独立账号目录、清除继承的 Google 凭据、仅注入选定 API key、运行目录归属；正常工作拒绝 OAuth，显式登录才转发校验后的 URL |

`acp-native.test.mjs` 还覆盖两者恢复失败不新建、不跨协议重试，文本/工具流与真实 prompt 终态、进程退出状态、权限失效和非法回答。测试使用内存流/模拟启动器，不访问真实账号、模型或工作目录。环境隔离测试注入虚拟文件系统，不创建凭据文件。

`AntigravityLogin.test.tsx` 覆盖：等待期间不提前宣称登录成功、只有用户点击才打开浏览器、认证失败原始错误、取消及离开提供商时关闭登录。助手表单参数化覆盖两种新提供商的模型选择和配置隔离；Rust 现有 native_agents tests 自动覆盖新增驱动身份、能力和错误传播。

## 本地验证

- Node 原生协议回归：54 项通过（其中本次新增 19 项）。
- Rust `native_agents`：7 项通过，包含 5 种独立驱动的注册检查与内存数据库提供商/模型过滤；使用默认 target，仅关闭 Aipp 测试包调试符号以控制磁盘占用。
- 前端登录、提供商表单、助手表单：21 项通过。首次沙箱缓存重命名 EPERM，沙箱外同命令重跑通过。
- `npm run build` 与 `cargo check --manifest-path src-tauri/Cargo.toml` 通过。

## 用户验收

1. 安装官方 Kiro CLI，运行 `kiro-cli login`；创建 Kiro 提供商和智能体助手，刷新模型，指定测试工作目录。
2. 发送文本任务，检查思考/工具活动、审批允许和拒绝；测试停止后继续发言、关闭应用后恢复原会话。
3. 分别验证有效/无效 Agent、模型和思考强度。无效设置应明确失败；删掉或失效原生 session 后应报恢复错误，不自动新建。
4. 安装官方 `agy`，创建 Antigravity 提供商，点击登录、浏览器完成账号认证，再刷新助手模型。验证取消登录、过期重新登录及第二个提供商不复用第一个账号。
5. Antigravity 测试文本、文件/命令审批、原生选择题、停止与原会话恢复；选一个 AIPP MCP 工具验证桥接及权限归属。
6. 如使用独立 ACP 程序，显式切换运行程序类型并配置同版本 harness；缺失或不匹配应报具体启动错误。

本机未安装 Kiro/Antigravity，以上真实登录、模型、工作区操作、MCP、Tauri UI 验收尚未执行。未承诺 Windows/macOS/Linux 上游运行程序均可用；AIPP 启动路径跨平台，实际安装支持以官方发布为准。Kiro steer/fork/compact、Antigravity 企业认证/Agent Platform、多媒体、原生子代理执行树不在本次实现范围。ACP Registry 管理已另建 LeaderAI AIPP 需求 #305，未分配版本。
