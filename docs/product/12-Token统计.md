# Token 统计

Token 统计模块记录和展示 AI 对话的 Token 使用情况，帮助用户了解 Token 消耗。

---

## Token 使用记录

### 消息 Token 计数
- `message_token.rs` 消息 Token 管理
- 每条消息记录 Token 使用量
- Token 计数在消息生成时进行
- 实时更新 Token 数据

### Input/Output Token 分别统计
- `input_token_count` - 输入 Token 数
- `output_token_count` - 输出 Token 数
- `token_count` - 总 Token 数（兼容旧数据）
- 分别统计便于成本分析

### Token 数据持久化
- Token 数据存储在 message 表中
- 消息数据库包含 Token 字段
- 历史 Token 数据永久保存
- 支持 Token 数据查询和统计

---

## 性能指标

### TTFT (Time To First Token)
- `ttft_ms` 首 Token 时间（毫秒）
- `first_token_time` 首 Token 时间戳
- 记录模型响应速度
- 性能监控和优化

### 消息时间记录
- `start_time` - 开始时间
- `finish_time` - 完成时间
- 计算响应耗时
- 性能分析数据

---

## Token 统计 API

### 统计查询 API
- `token_statistics_api.rs` Token 统计 API
- `get_conversation_token_stats` - 获取对话 Token 统计
- `get_message_token_stats` - 获取消息 Token 统计

### 按模型维度聚合统计
- `tokenStatisticsService.ts` 提供按模型维度的聚合分析
- 按模型名称分组统计 Token 使用量
- 计算每个模型的 Token 占比百分比
- 支持对话级别和消息级别的统计视图

---
相关源码:
- `src-tauri/src/api/token_statistics_api.rs` - Token 统计 API
- `src-tauri/src/state/message_token.rs` - 消息 Token 管理
- `src/services/tokenStatisticsService.ts` - Token 统计前端服务
- `src/data/Conversation.tsx` - 对话数据类型
