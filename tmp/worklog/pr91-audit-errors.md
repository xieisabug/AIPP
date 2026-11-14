# PR91 审计与模型错误脱敏

- 基于 PR91 最新 head `90100d87a4bcbc75041497d332517834e51278aa`，独立工作区 `aipp_pr91_audit_errors`；不推送、不改 UI。
- SuperAdmin 在统一审计写入边界清理参数、结果、原因、错误和快照。处理在仍有本次凭据作用域时完成，后续无保险箱上下文的查询只读到脱敏后的数据。
- 原参数或撤销快照发生脱敏时不保留撤销快照，避免把遮盖标记作为真实值恢复；普通撤销保持原始数据和行为。
- 回归测试使用内存 SQLite，覆盖原文/JSON/Base64/URL 形式、JSON 键和值、嵌套 JSON 字符串、失败/undo 审计、后续无凭据作用域的原始 SQL 和审计查询、撤销安全性。
- Provider 错误边界修复进行中。
- 验证限制：当前环境没有 cargo/rustc，后端编译与测试尚未执行。

## 完成结果

- Provider 采用非 Deref 的统一 Client 包装，覆盖普通请求、流建立/流中错误、模型目录请求；清理 HTTP/JSON body、headers、结构化错误及流错误 source chain 后才交给现有持久化/返回路径。保留状态码、请求 ID、重试头及诊断原因。
- 复合 API Key 的每个保险箱引用值随内存 guard 交给清理器，不仅遮盖最终拼接 Key。
- 按用户不改变普通短值遮盖行为的要求，审计对短值及本身可解析为 JSON 的凭据采用仅保留动作/时间/成功状态等元数据的保守模式；不保存自由文本/JSON 或撤销快照。此策略仅用于审计，不改变全局响应清理。
- 新增后端回归：审计 4 项，Provider 9 项。全部仍未执行，因为 `cargo check --manifest-path src-tauri/Cargo.toml` 和目标测试命令均报 `cargo: command not found`（exit 127）。
- `npm run build` 通过（现有 chunk 大小提示）；保险箱/MCP/Provider 7 个前端测试文件 52 项全部通过；`git diff --check` 通过。
- 未修改 UI，未改全局短值遮盖策略，未推送。未对旧版本已经写入的历史审计做猜测性清除或迁移。
