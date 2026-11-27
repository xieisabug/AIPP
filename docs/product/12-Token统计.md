# Token 统计

Token 统计模块记录和展示 AI 对话的 Token 使用情况，帮助用户了解 Token 消耗。

---

## 统一口径

所有通道写入 message 表时都归一成同一口径，统计和展示都按这个口径计算：

- `input_token_count`：全部输入，**包含缓存读取与缓存写入**
- `output_token_count`：全部输出，**包含思考 / 推理 token**
- `token_count`：总量 = 输入 + 输出
- `metadata_json`：`cached_input_tokens`（= `cached_read_tokens`，缓存命中）、`cached_write_tokens`（缓存写入）、`thought_tokens`（思考）、`usage_source`（`reported` 上报 / `estimated` 估算）、`generation_duration_ms`（流式生成耗时）

各通道的归一方式：

| 通道 | 原始口径 | 归一 |
| --- | --- | --- |
| 普通模型（genai） | 适配器已转成 OpenAI 口径 | 直接写入 |
| Claude Code | Anthropic 口径：`input_tokens` 不含缓存 | 输入 = input + cache_read + cache_creation；原始 usage 存 `claude_usage` |
| Codex | `thread/tokenUsage/updated` 的 `last` 只是最近一次请求 | 本轮用量 = 最后一条通知的累计 `total` − 本轮开始前的累计 `total` |
| ACP | 由各 agent 决定是否含缓存、思考是否计入输出 | 按 `total` 恰好等于哪种组合识别后归一；估算时思考计入输出 |
| Pi | 每次模型回复结束时上报 | 累计各次回复，输入包含缓存读取与写入 |
| OpenCode | 每个完成步骤分别上报输入、输出、缓存与思考 | 同一步骤只计一次，缓存计入输入，思考计入输出 |
| Cursor | SDK 返回本次运行的累计用量 | 缓存读写计入输入，输出已含思考 |
| Kiro、Antigravity | 原生 ACP 可在回合结束时返回用量 | 与通用 ACP 共用回合用量的归一规则 |

Codex 和通用 ACP 会标记按字数估算的用量。Pi、OpenCode、Cursor、Kiro 和 Antigravity 按实际收到的回合或步骤用量统计。会话停止或报错后，已经记录的用量仍计入总量。对于缺少记录的已完成回复，面板显示缺失的轮数，历史记录也适用。

---

## 对话信息窗口

聊天标题栏与管家窗口的“对话信息”按钮打开；管家群聊头部的“群聊用量”按钮按群统计。

### Token 用量
- OpenAI 兼容提供商上的 Gemini 等模型也会请求流式用量。对话按服务端返回的输入、输出和缓存命中数汇总；历史回复缺少这些记录时，无法从正文还原缓存命中数。
- 总量拆成互不重叠的三段：未命中缓存输入、缓存命中输入、输出
- “其中缓存写入”含在未命中缓存输入里，“其中思考”含在输出里，不能再相加
- 缓存命中率 = 缓存命中输入 / 全部输入
- “总量构成”说明总量来自哪里：本会话回复、助手协作、上下文压缩、群聊调度

### 统计范围
- **本会话**：会话自身的回复消息
- **助手协作**（管家）：委派出去的任务会话整段；协作请求只取接收方从投递消息到下一条用户消息之间的回合；接收方转派的下级协作、任务会话再请的协助都沿链递归计入；接收方本身已整段计入时不重复计算
- **辅助模型调用**：上下文压缩的摘要调用、群聊调度模型调用不产生聊天消息，单独记录在 `model_usage_record` 表；调用一返回就记录，摘要随后被丢弃也照样计入
- **群聊**：群会话本身（调度）+ 全部成员（含已移出）的执行会话

### 其他指标
- **模型**：按模型分组（模型已删除时按名称分组）。智能体按通道名称分组，每个带用量的回合计一次调用。
- **消息类型**：只统计本会话自身的消息，系统 / 用户 / AI 回复 / 推理 / 工具结果 / 错误 / 其他之和等于本会话消息数
- **性能指标**：只取带用量的回复（一次完成的请求记一次），含助手协作；首字延迟取平均；生成速度 = 输出总量 ÷ 生成总时长；Agent 回合包含工具执行时间，不计入生成速度；没有样本时显示“—”
- **当前上下文**（普通模型会话）：最近一次请求的输入 / 上下文窗口、自动压缩触发点、已压缩次数
- **最近请求的缓存命中**（管家窗口）：每次请求的缓存命中 / 输入与上下文检查点

---

## 性能指标字段

- `ttft_ms` 首 Token 时间（毫秒），`first_token_time` 首 Token 时间戳
- `start_time` / `finish_time` 开始与完成时间
- `metadata_json.generation_duration_ms` 流式生成耗时（旧数据按时间戳推算）

---

## Token 统计 API

- `get_conversation_token_stats` - 对话统计（含上下文占用）
- `get_butler_group_token_stats` - 管家群聊统计
- `get_message_token_stats` - 单条消息统计
- `get_butler_cache_activity` - 管家会话最近请求的缓存命中

---
相关源码:
- `src-tauri/src/db/token_stats.rs` - 统计口径与汇总（委派递归、辅助调用、按模型、性能、上下文）
- `src-tauri/src/api/token_statistics_api.rs` - Token 统计 API 与辅助调用记录
- `src/components/token-statistics/` - 对话信息窗口
- `src/services/tokenStatisticsService.ts` - Token 统计前端服务
- `src/data/Conversation.tsx` - 对话数据类型
