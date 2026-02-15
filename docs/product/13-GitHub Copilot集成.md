# GitHub Copilot 集成

GitHub Copilot 集成模块允许用户连接 GitHub Copilot 服务，通过设备流程进行认证。

---

## Copilot 认证

### GitHub 设备流程认证
- `copilot_api.rs` Copilot API
- `start_github_copilot_device_flow` - 开始 GitHub Copilot 设备流程
- `poll_github_copilot_token` - 轮询 GitHub Copilot 令牌
- 显示用户验证 URL 和设备码
- 用户在浏览器中输入设备码完成验证

### 认证状态管理
- `check_copilot_status` - 检查 Copilot 状态
- `get_copilot_lsp_status` - 获取 Copilot LSP 状态
- `get_copilot_oauth_token_from_config` - 从配置获取 Copilot OAuth 令牌
- 认证状态查询

### 登录与退出
- `sign_in_initiate` - 开始登录
- `sign_in_confirm` - 确认登录
- `sign_out_copilot` - 退出 Copilot
- `stop_copilot_lsp` - 停止 Copilot LSP

---

## Copilot LSP 集成

### Copilot LSP 服务管理
- `copilot_lsp.rs` Copilot LSP 集成
- Copilot LSP 服务进程管理
- LSP 客户端初始化
- 服务生命周期管理

---
相关源码:
- `src-tauri/src/api/copilot_api.rs` - Copilot API
- `src-tauri/src/api/copilot_lsp.rs` - Copilot LSP 集成
- `src/hooks/useCopilot.ts` - Copilot Hook
