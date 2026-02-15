# MCP 工具系统

MCP (Model Context Protocol) 工具系统是 AIPP 的核心扩展机制，允许 AI 调用各种工具来增强能力。

---

## MCP 服务器管理

### 服务器配置
- 支持添加多个 MCP 服务器
- 服务器配置包括：
  - 服务器名称
  - 命令（command）
  - 工作目录（cwd）
  - 环境变量（env）
  - 附加参数（args）
- 配置持久化到 `mcp_servers` 表

### 服务器 CRUD
- `add_mcp_server` - 添加新服务器
- `get_mcp_servers` - 获取服务器列表
- `update_mcp_server` - 更新服务器配置
- `delete_mcp_server` - 删除服务器
- `test_mcp_server` - 测试服务器连接

### 服务器启动与停止
- 按需启动 MCP 服务器进程
- 使用 rmcp crate 的客户端传输
- 进程生命周期管理
- 错误处理与重试

---

## 工具自动检测

### 工具发现
- 连接服务器后自动调用 `tools/list` 获取工具列表
- 解析工具名称、描述、输入参数 schema
- 工具信息缓存

### 动态 MCP 加载
- 支持按助手配置动态启用/禁用 MCP 服务器
- `is_dynamic_mcp_loading_enabled_for_assistant` 检查
- `collect_mcp_info_for_assistant` 收集可用工具

### 工具检测与处理
- `detect_and_process_mcp_calls` 自动检测工具调用
- 支持原生工具调用和提示词格式化两种模式
- 工具调用结果格式化

---

## 本地工具调用

### use_native_toolcall 模式
- 当 `use_native_toolcall` 为 true 时启用原生调用
- 使用 genai 客户端的原生工具调用能力
- 直接传递工具定义给模型
- 模型返回 ToolCall 后执行

### 原生工具调用执行
- `execute_mcp_tool_call` 执行工具调用
- 调用 MCP 服务器的 `tools/call` 方法
- 处理调用结果
- 超时与错误处理

### 工具结果返回
- 工具执行结果作为助手消息返回
- 支持内容和图片两种结果类型
- 错误信息友好展示

---

## 提示词格式化回退

### 非原生调用模式
- 当 `use_native_toolcall` 为 false 时使用提示词格式化
- 通过提示词让模型输出工具调用指令
- 正则表达式解析工具调用

### 提示词格式化
- `format_mcp_prompt` 构建工具调用提示词
- 包含可用工具列表和使用说明
- 格式化工具调用格式（XML 或 JSON）

### 结果解析
- 解析模型输出中的工具调用
- 提取工具名称和参数
- 执行工具并将结果插回对话

---

## 工具调用 UI

### 工具调用状态展示
- 工具调用卡片 UI
- 状态流转：pending → executing → success/failed

### 状态图标与提示
- pending：等待中
- executing：执行中（动画）
- success：成功（绿色勾）
- failed：失败（红色叉）

### 工具参数与结果展示
- 显示调用的工具名称
- 展示输入参数（JSON 格式化）
- 展示工具执行结果
- 支持展开/收起长内容

---

## 内置 MCP 工具集成

### 内置工具注册
- 内置工具与外部 MCP 服务器统一管理
- Web 搜索、文件操作等作为内置 MCP 工具
- 支持模板动态配置

### 模板管理
- `builtin_mcp/templates.rs` 管理 MCP 模板
- 动态配置 MCP 服务器
- 模板验证

---

## 其他功能

### 目录摘要
- `summarizer.rs` 提供目录摘要功能
- 递归遍历目录结构
- 生成目录树摘要

### 工具名称映射
- `resolve_tool_name`、`sanitize_tool_name` 处理工具名称
- 处理特殊字符和重复名称
- 工具名称规范化

### MCP 工具摘要生成
- 使用 AI 模型自动为 MCP 服务器和工具生成简洁的中文摘要
- 可配置专用的摘要生成模型
- 批量处理所有已启用的 MCP 服务器
- 通过 `mcp-summary-progress` 事件报告生成进度
- 摘要结果持久化到 `mcp_tool_catalog_entry` 表
- 支持重试机制和 JSON 解析回退

### 动态 MCP 加载（实验性）
- 功能开关：`dynamic_mcp_loading_enabled` 特性标志控制
- 减少 prompt 中注入的工具 schema 量，降低上下文浪费
- 仅暴露已生成摘要的工具目录条目
- 未生成摘要的服务器/工具自动隐藏
- 能力纪元（capability epoch）变更时自动重置摘要
- 相关数据库表：`dynamic_mcp_template`、`dynamic_mcp_server`、`conversation_loaded_mcp_tool`

### MCP 工具调用历史
- `mcp_tool_call` 表记录所有 MCP 工具调用历史
- 记录调用的工具名称、参数、结果、状态
- 支持历史调用查询和分析

---
相关源码:
- `src-tauri/src/mcp/mod.rs` - MCP 模块总入口
- `src-tauri/src/mcp/registry_api.rs` - MCP 服务器注册管理
- `src-tauri/src/mcp/execution_api.rs` - 工具执行 API
- `src-tauri/src/mcp/detection.rs` - 工具检测
- `src-tauri/src/mcp/prompt.rs` - 提示词构建
- `src-tauri/src/mcp/summarizer.rs` - MCP 摘要生成
- `src-tauri/src/api/ai/mcp.rs` - AI 中的 MCP 集成
- `src/hooks/useMcpToolCallProcessor.tsx` - 工具调用处理 Hook
