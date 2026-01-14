# ACP (Agent Client Protocol) 研究笔记

## 概述

ACP 是一个用于 AI 编码代理与代码编辑器（客户端）之间通信的标准协议。

## 核心类型定义

### 客户端必须实现的 Trait

```rust
#[async_trait::async_trait(?Send)]
pub trait Client {
    // 会话通知（流式输出）
    async fn session_notification(&self, args: SessionNotification) -> Result<()>;

    // 权限请求
    async fn request_permission(&self, args: RequestPermissionRequest) -> Result<RequestPermissionResponse>;

    // 文件操作
    async fn write_text_file(&self, args: WriteTextFileRequest) -> Result<WriteTextFileResponse>;
    async fn read_text_file(&self, args: ReadTextFileRequest) -> Result<ReadTextFileResponse>;

    // 终端操作
    async fn create_terminal(&self, args: CreateTerminalRequest) -> Result<CreateTerminalResponse>;
    async fn terminal_output(&self, args: TerminalOutputRequest) -> Result<TerminalOutputResponse>;
    async fn release_terminal(&self, args: ReleaseTerminalRequest) -> Result<ReleaseTerminalResponse>;
    async fn wait_for_terminal_exit(&self, args: WaitForTerminalExitRequest) -> Result<WaitForTerminalExitResponse>;
    async fn kill_terminal_command(&self, args: KillTerminalCommandRequest) -> Result<KillTerminalCommandResponse>;

    // 扩展方法
    async fn ext_method(&self, args: ExtRequest) -> Result<ExtResponse>;
    async fn ext_notification(&self, args: ExtNotification) -> Result<()>;
}
```

## 请求/响应类型

### ReadTextFileRequest
```rust
pub struct ReadTextFileRequest {
    pub session_id: SessionId,
    pub path: PathBuf,
    pub line: Option<u32>,      // 起始行号（1-based）
    pub limit: Option<u32>,      // 读取行数限制
}
```

### WriteTextFileRequest
```rust
pub struct WriteTextFileRequest {
    pub session_id: SessionId,
    pub path: PathBuf,
    pub content: String,
}
```

### CreateTerminalRequest
```rust
pub struct CreateTerminalRequest {
    pub session_id: SessionId,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<EnvVariable>,
    pub cwd: Option<PathBuf>,
    pub output_byte_limit: Option<u64>,
}
```

### RequestPermissionRequest
```rust
pub struct RequestPermissionRequest {
    pub session_id: SessionId,
    pub tool_call: ToolCallUpdate,
    pub options: Vec<PermissionOption>,
}
```

### 响应类型

#### CreateTerminalResponse
```rust
pub struct CreateTerminalResponse {
    pub terminal_id: TerminalId,  // Arc<str> 包装
}
```

#### TerminalOutputResponse
```rust
pub struct TerminalOutputResponse {
    pub output: String,
    pub truncated: bool,
    pub exit_status: Option<TerminalExitStatus>,
}
```

#### TerminalExitStatus
```rust
pub struct TerminalExitStatus {
    pub exit_code: Option<u32>,
    pub signal: Option<String>,
}
```

## AIPP 项目中的对应实现

### 文件操作
- 位置: `src-tauri/src/mcp/builtin_mcp/operation/file_ops.rs`
- 类型: `ReadFileRequest`, `WriteFileResponse`

### 终端操作
- 位置: `src-tauri/src/mcp/builtin_mcp/operation/bash_ops.rs`
- 类型: `ExecuteBashRequest`, `ExecuteBashResponse`

### 权限管理
- 位置: `src-tauri/src/mcp/builtin_mcp/operation/permission.rs`
- 类型: `PermissionManager`

### 状态管理
- 位置: `src-tauri/src/mcp/builtin_mcp/operation/state.rs`
- 类型: `OperationState`

## 类型转换表

| ACP 类型 | 内部类型 | 转换说明 |
|----------|----------|----------|
| `SessionId` | `i64` (conversation_id) | 直接使用 conversation_id |
| `TerminalId` | `String` (bash_id) | 直接使用 bash_id 作为 TerminalId |
| `ReadTextFileRequest` | `ReadFileRequest` | 字段对应 |
| `WriteTextFileRequest` | `WriteFileRequest` | 字段对应 |
| `CreateTerminalRequest` | `ExecuteBashRequest` | 需要转换 |
| `TerminalOutputRequest` | `GetBashOutputRequest` | 使用 terminal_id -> bash_id |

## 实现要点

1. **终端 ID 管理**: ACP 使用 `TerminalId`（Arc<str>），内部使用 `bash_id`（String），直接使用 `bash_id` 作为 `TerminalId`
2. **会话上下文**: `session_id` 映射到 `conversation_id`
3. **错误转换**: 内部 `String` 错误需要转为 `acp::Error`
4. **状态共享**: 使用 `Arc<OperationState>` 共享状态

## 开发遇到的坑

### 1. `acp::Error::internal_error()` 不接受参数

```rust
// ❌ 错误
Err(acp::Error::internal_error(e))

// ✅ 正确
Err(acp::Error::internal_error().data(e))
```

`internal_error()` 返回基础错误，需要用 `.data()` 方法添加额外信息。

### 2. 退出码类型不匹配

```rust
// 内部 BashProcessStatus 使用 i32，ACP 使用 u32
response.exit_code.map(|code| acp::TerminalExitStatus::new().exit_code(Some(code as u32)))
```

### 3. `exit_status()` 方法接受 `Option<&TerminalExitStatus>`

```rust
// ❌ 错误 - 不能直接传 Option<TerminalExitStatus>
.exit_status(exit_status)

// ✅ 正确 - 需要传引用
.exit_status(exit_status.as_ref())
// 或者直接传 None 或 Some(&status)
.exit_status(Some(&terminal_exit_status))
```

### 4. `ExtResponse::new()` 需要 `Arc<RawValue>`

```rust
// ❌ 错误 - to_raw_value 返回 Box<RawValue>
let response = serde_json::value::to_raw_value(&json)?;
Ok(acp::ExtResponse::new(response))

// ✅ 正确 - 使用 NULL 或手动转换
Ok(acp::ExtResponse::new(serde_json::value::RawValue::NULL.to_owned().into()))

// 或者如果需要自定义 JSON
let json_str = serde_json::to_string(&json_value)?;
// 注意：RawValue::from_string 返回 Box<RawValue>，需要 .into() 转为 Arc
```

### 5. `TerminalId` 实际上就是 `Arc<str>`

```rust
// 创建 TerminalId 很简单，直接用字符串
let terminal_id = acp::TerminalId::new(bash_id);

// 使用时通过 .0 访问内部的 Arc<str>
println!("{}", terminal_id.0);
```

### 6. `TerminalExitStatus` 链式调用

```rust
// 可以链式调用设置字段
acp::TerminalExitStatus::new()
    .exit_code(Some(0))
    .signal(Some("TERM".to_string()))
```

### 7. LocalSet 用于 !Send futures

```rust
// ACP 的 futures 是 !Send，需要使用 LocalSet
let local_set = tokio::task::LocalSet::new();
local_set.run_until(async move {
    // ... ACP 代码
}).await?;
```

### 8. `ContentBlock` 没有 `Unknown` 变体

```rust
// ❌ 错误 - ContentBlock 没有 Unknown 变体
acp::ContentBlock::Unknown(unknown) => ...

// ✅ 正确 - 使用 _ 捕获未来可能的变体
_ => "[Unknown content]".to_string()
```

### 9. `EmbeddedResource` 有嵌套的 `resource` 字段

```rust
// ❌ 错误 - 直接访问 .uri
acp::ContentBlock::Resource(resource) => resource.uri

// ✅ 正确 - 需要访问嵌套的 resource.resource
acp::ContentBlock::Resource(resource) => match &resource.resource {
    acp::EmbeddedResourceResource::TextResourceContents(text) => text.uri.clone(),
    acp::EmbeddedResourceResource::BlobResourceContents(blob) => blob.uri.clone(),
    _ => "[Resource]".to_string(),
}
```

### 10. `ToolCallStatus` 使用 `InProgress` 而不是 `Running`

```rust
// ❌ 错误
acp::ToolCallStatus::Running => "executing".to_string()

// ✅ 正确
acp::ToolCallStatus::InProgress => "executing".to_string()
```

### 11. `ToolCall` 使用 `tool_call_id` 而不是 `id`

```rust
// ❌ 错误
tool_call.id
tool_call.tool

// ✅ 正确
tool_call.tool_call_id
tool_call.title
tool_call.kind
```

### 12. `ToolCallUpdate` 的字段在 `fields` 里面

```rust
// ❌ 错误
update.status
update.result

// ✅ 正确 - 使用 fields
update.fields.status
update.fields.raw_output
```

### 13. `ToolCallId` 是新类型包装器，需要访问 `.0`

```rust
// ❌ 错误 - ToolCallId 不能直接 parse
update.tool_call_id.parse()

// ✅ 正确 - 访问内部的 Arc<str>
update.tool_call_id.0.parse()
```

### 14. `SessionInfoUpdate.title` 是 `MaybeUndefined<String>`

```rust
// ✅ 正确 - 先检查是否 undefined
if !info_update.title.is_undefined() {
    if let Some(title) = info_update.title.as_ref() {
        // 使用 title
    }
}
```

## 最终实现代码结构

```rust
pub struct AcpTauriClient {
    pub app_handle: tauri::AppHandle,
    pub conversation_id: i64,
    pub message_id: i64,
    pub window: tauri::Window,
    operation_state: Arc<OperationState>,
    permission_manager: Arc<PermissionManager>,
}

impl AcpClient for AcpTauriClient {
    async fn write_text_file(&self, args: acp::WriteTextFileRequest) -> acp::Result<...> {
        // 转换请求类型
        let request = WriteFileRequest {
            file_path: args.path.to_string_lossy().to_string(),
            content: args.content,
        };
        // 调用内部实现
        FileOperations::write_file(...)
    }

    async fn create_terminal(&self, args: acp::CreateTerminalRequest) -> acp::Result<...> {
        let full_command = format!("{} {}", args.command, args.args.join(" "));
        let request = ExecuteBashRequest {
            command: full_command,
            run_in_background: Some(true),
            ...
        };
        let response = BashOperations::execute_bash(...).await?;
        let bash_id = response.bash_id.ok_or(...)?;
        let terminal_id = acp::TerminalId::new(bash_id);
        Ok(acp::CreateTerminalResponse::new(terminal_id))
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        match args.update {
            // UserMessageChunk -> message_update (type=user)
            // AgentMessageChunk -> message_update (type=response)
            // AgentThoughtChunk -> message_update (type=reasoning)
            // ToolCall -> mcp_tool_call_update (status=pending)
            // ToolCallUpdate -> mcp_tool_call_update
            // Plan -> 仅日志
            // AvailableCommandsUpdate -> 仅日志
            // CurrentModeUpdate -> 仅日志
            // SessionInfoUpdate -> title_change
            _ => {}
        }
        Ok(())
    }
}
```

## session_notification 事件映射

| ACP SessionUpdate | 前端事件类型 | message_type / status |
|-------------------|-------------|----------------------|
| `UserMessageChunk` | `message_update` | `user` |
| `AgentMessageChunk` | `message_update` | `response` |
| `AgentThoughtChunk` | `message_update` | `reasoning` |
| `ToolCall` | `mcp_tool_call_update` | `pending` |
| `ToolCallUpdate` | `mcp_tool_call_update | `pending/executing/success/failed` |
| `SessionInfoUpdate` | `title_change` | - |
| 其他 | - | 仅日志 |

## 依赖

```toml
[dependencies]
agent-client-protocol = { version = "0.9", git = "https://github.com/agentclientprotocol/rust-sdk" }
```
