# 应用目录协议 v1

本期覆盖 #290–293 的连接核心。用户明确暂不做账号级权限矩阵，复用助手服务器授权、工具开关及已有执行审核。Skill Server 为独立仓库 `AIPP-helper`，客户端不维护第二份外部应用列表。

## 数据来源与接口

- `GET /api/apps` 返回 `{schema_version:1, version, apps:[...]}`；`GET /api/apps/:id` 返回单项详情。
- 保留 `/api/skills` 与 `/api/plugins`。普通插件、独立 Skills、模型供应商入口不变。
- `apps` 的每项包含 `id/version/name/description/publisher/category/platforms/connection/fields`；可选 `icon/homepage/notice/skill_ids`。
- `connection` 声明固定 HTTPS MCP 地址及 `auth: none | fields | oauth`。OAuth 为授权码 + S256 PKCE 公共客户端；可声明 `client_id/scopes`，未提供 client_id 时要求服务支持动态注册。
- `fields` 支持 text/password/select/boolean/number，默认值、必填、帮助、选项及 `visible_when:{field,equals}`。`header/prefix` 表示受控请求头绑定；`secret:true` 的值不保存到普通配置。v1 不接收远端可执行 JS，也不执行目录下发的命令。
- 新的协议或认证机制需要客户端适配；不能把未知方式标成已支持。
- 客户端缓存通过校验的目录，刷新失败明确显示缓存时间和错误。服务器下线不影响内置工具及已有连接的管理。

## 客户端状态与身份

`mcp_server.id` 仍是连接实例身份。`app_connection` 只记录应用关联、描述快照、配置名称、非秘密字段、凭据引用和状态，不复制工具。相同服务可创建多个独立连接；创建请求 UUID 幂等，不按地址或名称合并。高级自定义 MCP 保留原数据。

状态区分 connecting/available/unchecked/auth_required/disconnected/error；重启后旧 available 不视为当前验证成功。开关启用与服务可用是两个维度。OAuth 只在明确连接操作中启动，正常工具调用不自动弹出授权或换账号。

内置条目由本地 `builtin_templates()` 提供，客户端合并展示；远端不能占用 `builtin:` 命名空间。实际工具定义、平台限制、禁删和管家专用范围以本地能力为准。

## 凭据与执行

目录连接的凭据使用系统钥匙串，MCP headers 仅保存不可直接解出的引用；引用绑定 server ID 与精确服务地址。凭据在 Rust HTTP 传输创建时注入，普通模型和原生 Agent bridge 继续走原有执行链。重定向不携带认证继续请求，回调校验 state、路径、授权码唯一性，刷新序列化。响应对已知认证字符串脱敏，这不代表任意编码数据或同用户恶意进程的强隔离。

Windows Credential Manager / macOS Keychain / Linux Secret Service 使用 keyring 的原生后端。Android 尚未接入这套连接凭据存储，目录中的认证应用明确禁用新建；现有远程自定义 MCP 与内置工具继续按既有平台规则管理。Linux/macOS 后端尚需各自实机验收。

断开先停用连接、取消等待中的授权，再清理本机凭据；不回滚已执行操作。本版不提供服务商授权撤销端点，UI 明示需要用户在服务商账户设置撤销。普通分享、跨端迁移以及任意 shell 凭据隔离仍属于后续凭据需求，不能宣称已覆盖 #298–302。

## 首批描述

- Notion：官方 HTTP MCP，浏览器 OAuth；无客户端注册信息时动态注册。
- GitHub：官方 HTTP MCP，用户主动提供 PAT 的独立接入方式。未配置 AIPP 的 OAuth 应用，因此不展示浏览器登录，也不把 PAT 当 OAuth 失败后的自动降级。

服务端发布前客户端可能收到旧 SPA HTML，会显示目录接口尚未发布，不能显示虚假的外部应用或连接成功。服务端目录与客户端可以分别发布，发布不在本次本地实现的自动操作范围内。

## 验收

1. 打开设置 → 应用与工具：目录包含内置工具；我的应用保留已有 MCP。
2. 通过详情管理旧内置工具，检验原工具/提示词开关及禁删保护；旧 HTTP/stdio 编辑和 JSON 导入仍可达。
3. 发布 `/api/apps` 后出现外部应用；断网刷新显示缓存及错误，现有连接可管理。
4. 用测试账号执行 Notion OAuth 和 GitHub PAT；验证拒绝、过期、重启、重新登录、断开以及两个不同连接，不能用模拟测试替代。
5. 从助手授权页检查原绑定；停用的连接也显示保存的授权，不因全局停用伪装成未授权。
6. 普通模型和原生 Agent 分别读取/调用所选工具；没有助手授权时不发现工具，原执行审核继续生效。
