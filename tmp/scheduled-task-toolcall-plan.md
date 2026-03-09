# 定时任务工具调用收敛方案（V2）

## 1. 问题复盘（基于你给的日志）
- 当前日志出现：`settled` 后，最终“任务输出”仍是 `<!-- MCP_TOOL_CALL ... -->`，说明系统把“工具调用意图”误判成“任务已结束”。
- 本质问题：当前结束判定主要依赖瞬时状态（pending/executing + focus），不能保证**工具执行完成 + 续写完成 + 新一轮工具发现完成**这三件事都结束。
- 结论：结束判定不能只靠字符串和一次快照，必须改成**后端状态机 + 多阶段门禁**。

## 2. 新方案原则
1. **任务是否结束由后端确定，不由 LLM 文本确定**。  
2. notify 仅负责“要不要提醒/怎么摘要”，不负责“任务是否完成”。  
3. 结束判定使用“阶段状态 + 稳定窗口 + 超时/卡死检测”。

## 3. V2 结束判定状态机
`INIT -> ASKING -> TOOL_DISCOVERY -> TOOL_EXECUTION -> CONTINUATION -> QUIET_WINDOW -> READY_FOR_NOTIFY -> DONE/FAILED/TIMEOUT`

### 阶段含义
- **TOOL_DISCOVERY**：等待本轮 assistant 输出中的工具调用全部落库。
- **TOOL_EXECUTION**：等待本轮工具从 pending/executing 进入终态（success/failed/stopped）。
- **CONTINUATION**：若本轮存在可续写工具结果，必须等待“新 response”产生（message_id 增长或 content hash 变化）。
- **QUIET_WINDOW**：短静默窗口内没有新 tool_call、没有 response 更新，才算收敛。

## 4. 结束判定的硬条件（核心）
进入 `READY_FOR_NOTIFY` 需同时满足：
1. 当前轮次不存在 pending/executing 工具；
2. 最近一轮需要续写时，已观察到续写 response 落地；
3. 静默窗口（如 2~4 秒）内：
   - tool_call 总数不再增长；
   - 最新 response `id/hash` 不再变化；
   - ActivityFocus 不处于 streaming/mcp_executing；
4. 未触发卡死判定（见下）。

## 5. 卡死与误判兜底（非字符串）
### 卡死判定
- 存在 pending 且超过阈值（例如 20~30s）；
- 或“关键状态无变化”超过全局阈值（例如 60~90s）；
- 或轮次数超过上限（防无限自调用）。

### 处理策略
- 标记 run 为 `failed/timeout`，写明阶段和最后状态；
- 保留对话与 tool_call（不 cleanup），便于复盘；
- notify 走“失败通知”模板（可选）。

## 6. notify 设计（如果只能字符串，也要结构化）
notify 输入不再只给 `task_result`，而是给结构化执行报告：
- `execution_status`（completed/failed/timeout）
- `phase`
- `round_count`
- `tool_stats`（total/success/failed/stopped）
- `final_response`

输出协议固定为：
```json
{
  "notify": true,
  "summary": "一句话结论",
  "priority": "low|medium|high",
  "reason": "判定依据"
}
```
并继续支持三种解析输入：纯 JSON、```json、```。

## 7. 具体改造清单（先做计划，不直接继续改代码）
1. 在 `scheduled_task_api` 增加 run phase 与 round 状态记录（内存 + 日志）。
2. 增加 `collect_settlement_snapshot`（包含：tool_call增量、latest_response变化、focus、时间戳）。
3. 按状态机重写 `wait_for_task_conversation_settled_v2`，不再使用单条件“稳定轮询”。
4. notify 前只接受 `READY_FOR_NOTIFY`；否则直接失败/超时分支。
5. 更新日志字段：`phase`, `round`, `snapshot_diff`, `block_reason`，便于线上诊断。

## 8. 验证矩阵（必须覆盖）
1. 无工具直接完成；
2. 单轮工具成功并续写；
3. 多轮工具（续写后再发起新工具）；
4. 工具失败但模型继续；
5. 工具失败且不继续；
6. 仅出现 MCP_TOOL_CALL 注释但未真正执行；
7. 长时间 pending 卡死；
8. notify 返回 fenced JSON / 非 fenced JSON / 非法 JSON。

## 9. 说明
- 你这次日志暴露的问题是合理的，现方案确实不够稳。  
- 下一步以 V2 状态机方案为准，先把“结束判定”从文本推断改成“执行过程判定”，再做 notify。  
