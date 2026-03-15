# 总管家（实验）飞书机器人双向接入设计

## 1. 目标

在现有 AIPP `总管家（实验）` 的基础上，增加飞书机器人的双向接入能力，使其能够：

- 接收飞书中发给机器人的消息，并把消息路由给总管家主会话处理。
- 将总管家的最终回复自动发回飞书原聊天上下文。
- 在 `实验性` 配置中，随总管家功能一起提供飞书相关设置，配置正确后即可连接。
- 复用现有 AIPP 的 `conversation/message`、`ask_ai`、Butler 主会话、事件系统和 feature config 机制，不另起一套独立 agent runtime。


## 2. 总体结论

要实现“能收飞书消息，也能把消息发回飞书”，推荐采用：

- **飞书自建应用**
- **机器人能力（Bot）**
- **事件订阅**
- **长连接模式（Long Connection / WebSocket）**
- **消息发送 / 回复 API**

不推荐使用“自定义群机器人 webhook”方案，原因如下：

- 该方案更偏向向某个群单向推送消息，不适合稳定接收用户消息并参与完整对话。
- AIPP 是桌面应用，长连接模式不需要公网回调地址，更贴合本地运行形态。
- Butler 需要把飞书消息纳入现有主会话，长连接模式更容易在本地进程内直接消费事件并立即路由到 `ask_ai`。


## 3. 方案边界

### 3.1 本期要解决

- 飞书单聊机器人发送文本消息，AIPP 中的 Butler 能收到。
- 飞书群聊中 `@机器人` 或回复机器人消息时，Butler 能收到。
- Butler 给出最终答复后，AIPP 自动把答复回到飞书。
- 配置入口放在 `实验性 -> 总管家（实验）` 下。
- 可以配置“谁能够和 bot 对话”，并且规则可靠。

### 3.2 本期暂不做

- 多租户、多飞书应用并存。
- Store 应用 / Marketplace 分发。
- 24x7 云端常驻收消息。
- 飞书卡片、多媒体富交互的完整实现。

### 3.3 现实约束

该方案是**桌面端本地长连接**。因此：

- 只有 AIPP 正在运行、且飞书连接已建立时，才能实时收消息。
- 如果应用退出，飞书消息不会被本地实例继续接收。
- 如果后续需要 24x7 持续在线收消息，需要再补中继服务或常驻服务进程。


## 4. 飞书侧接入方式

### 4.1 应用类型

使用 **飞书自建应用**，开启 **机器人能力**。

### 4.2 事件接收方式

推荐使用 **长连接模式（Long Connection / WebSocket）** 接收事件。

推荐原因：

- 不需要公网回调地址。
- 更适合 AIPP 本地桌面程序。
- 可以在 Rust 后端直接维护连接、断线重连和状态管理。

### 4.3 关键事件

至少订阅：

- `im.message.receive_v1`

这是 Butler 接收飞书消息的核心入口。

### 4.4 关键 API

至少会用到：

- 获取 `tenant_access_token`
- 发送消息 API
- 回复消息 API

实现上建议优先使用“**回复消息**”能力，把 Butler 回复挂回原消息上下文；若遇到无法 reply 的场景，再退回“发送消息到 chat”。

### 4.5 建议权限

以飞书控制台最新权限命名为准，本方案至少需要以下类别的权限：

- 机器人发送消息相关权限
- 机器人读取消息 / 接收消息事件相关权限
- 单聊消息读取权限
- 群聊 `@机器人` 消息读取权限

调研中常见会涉及：

- `im:message`
- `im:message:send_as_bot`
- `im:message:readonly`
- `im:message.p2p_msg:readonly`
- `im:message.group_at_msg:readonly`

最终以上线时飞书开放平台控制台中的实际权限名称与审批要求为准。


## 5. 谁可以和 Bot 对话

这个能力建议采用**双层控制**，而不是只放在单一侧。

### 5.1 飞书侧控制

飞书侧作为**第一层边界**，负责限制应用可被哪些人或范围使用。

推荐利用飞书应用本身的可用范围能力，例如：

- 全员可用
- 指定用户可用
- 指定部门可用

这一层的好处是：

- 控制点更靠前，未被授权的人根本接触不到应用。
- 组织管理员可以直接在飞书侧管理，不依赖 AIPP 本地实例。
- 对部门级、人群级范围控制更自然。

### 5.2 AIPP 侧控制

AIPP 侧作为**第二层运行时策略**，负责精确决定收到事件后是否真正交给 Butler 处理。

这一层建议支持：

- 按 `sender_open_id` 白名单
- 按 `chat_id` 白名单
- 按会话类型控制：单聊 / 群聊
- 群聊仅响应 `@机器人` 或回复机器人

这一层的好处是：

- 可以做更细粒度的策略，尤其适合实验阶段。
- 可以在不改飞书管理后台的情况下快速调整本地运行策略。
- 可以作为兜底保护，避免飞书侧范围放宽后本地实例无差别接单。

### 5.3 最终建议

最终建议是：

- **飞书侧做粗粒度范围控制**
- **AIPP 侧做精粒度 allowlist 和运行时过滤**

这样最稳妥。

### 5.4 AIPP 侧不建议怎么做

不建议基于以下信息做权限判断：

- 昵称
- 展示名
- 可变文本标识

因为这些字段不稳定，容易误判。AIPP 侧权限判断应基于稳定 ID：

- `sender_open_id`
- `chat_id`


## 6. 用户体验设计

### 6.1 配置入口

在现有 `实验性功能` 中，`总管家模式（实验）` 打开后，出现新的飞书分组：

- `飞书接入（实验）`

### 6.2 配置项

建议提供以下配置项：

- `butler_feishu_enabled`
  - 是否开启 Butler 的飞书接入

- `butler_feishu_connection_mode`
  - 默认且仅支持：`long_connection`
  - 先保留字段，便于后续扩展其他接入方式

- `butler_feishu_app_id`
  - 飞书应用 App ID

- `butler_feishu_app_secret`
  - 飞书应用 App Secret
  - 前端可编辑，但不应以普通配置明文长期存储

- `butler_feishu_receive_p2p_enabled`
  - 是否接收飞书单聊消息
  - 默认：`true`

- `butler_feishu_receive_group_enabled`
  - 是否接收飞书群聊消息
  - 默认：`true`

- `butler_feishu_group_only_mention`
  - 群聊中是否仅处理 `@机器人` 或回复机器人的消息
  - 默认：`true`

- `butler_feishu_access_mode`
  - 访问控制模式
  - 建议值：
    - `all_in_scope`
    - `allowlist_only`

- `butler_feishu_allowed_user_open_ids`
  - 允许与 bot 对话的用户 `open_id` 列表
  - 多个值用换行或逗号分隔

- `butler_feishu_allowed_chat_ids`
  - 允许与 bot 对话的群聊或会话 `chat_id` 列表
  - 多个值用换行或逗号分隔

- `butler_feishu_reply_in_thread`
  - 是否优先以“回复原消息”的方式回发
  - 默认：`true`

- `butler_feishu_status`
  - 只读状态字段
  - 用于展示：`未配置 / 已断开 / 连接中 / 已连接 / 配置错误 / 权限不足`

- `butler_feishu_last_error`
  - 只读，用于展示最近一次连接或发送失败原因

### 6.3 配置界面行为

建议界面行为如下：

- 只有在 `总管家模式（实验）` 打开时，才显示飞书分组。
- 只有在 `飞书接入（实验）` 打开时，才显示 App ID / App Secret 等详细配置。
- 保存后自动尝试：
  - 验证凭据
  - 启动或重启飞书长连接
- 提供两个按钮：
  - `测试连接`
  - `重连`

其中：

- `测试连接`：只验证 token 获取和事件连接初始化，不直接往飞书发消息。
- `重连`：强制关闭当前连接并重建。


## 7. 交互规则

### 7.1 单聊规则

- 用户给机器人发单聊消息。
- AIPP 收到后，先做权限判断与去重判断。
- 通过后路由给 Butler 主会话。
- Butler 最终回复自动回发到该单聊。

### 7.2 群聊规则

默认只处理以下两种消息：

- 用户在群里 `@机器人`
- 用户回复了机器人之前的消息

这样可以避免机器人监听整个群的所有自然聊天。

### 7.3 权限判断顺序

建议按以下顺序判断：

1. 是否为机器人自己发出的消息
2. 是否为重复事件
3. 是否为启用的会话类型
4. 是否满足群聊触发条件
5. 是否在 AIPP allowlist 中

不通过则直接忽略，不进入 Butler 主会话。


## 8. OpenClaw 对照结论

OpenClaw 这类项目与飞书的对接，本质上也是：

- 飞书自建应用
- 长连接接收事件
- 每个外部 chat 映射到内部 session
- 把平台事件转成统一消息，再交给内部 agent/runtime

它的思路是**渠道适配器**，而不是单独在飞书侧搞一套特殊逻辑。

### 8.1 OpenClaw 的共性做法

从公开资料看，OpenClaw 更偏向以下结构：

- 使用 Feishu 长连接收消息
- 按 chat/session 做内部会话映射
- 通过统一 channel adapter 把消息交给 agent
- 用自身的会话、消息和 session 持久化能力承载状态

### 8.2 是否一定要多个表

不一定。

OpenClaw 这种项目通常有自己的通用会话模型，因此很多状态是复用已有的：

- conversations
- messages
- session / pairing / channel mapping

是否拆成多张专门表，取决于宿主系统本身已经有什么数据层。

### 8.3 AIPP 的取舍

对 AIPP 来说，已经有：

- `conversation`
- `message`
- Butler 主会话 / 子任务会话

因此本期不需要为了飞书对接而新造很多表。MVP 只需要在现有模型外增加**一张最小映射表**就足够：

- 用于去重
- 用于记录外部 message_id 与内部 message_id 的对应关系
- 用于支撑 reply / 回溯 / 问题排查

如果后续要做更强的主动推送、多渠道会话绑定、群线程映射，再增加第二张会话绑定表即可。


## 9. Butler 侧行为设计

### 9.1 仍然使用现有 Butler 主会话

飞书消息不新造一套聊天系统，而是进入现有 Butler 主会话体系：

- Butler 主会话仍然是唯一 `butler_main`
- 执行工作仍然靠 `spawn_task_conversation`
- 子任务结果仍然自动回流 Butler

飞书只是**新增一个外部输入 / 输出通道**。

### 9.2 外部消息注入方式

建议不要把飞书元数据直接污染用户正文。推荐做法：

1. 先写入一条隐藏的 `system` 消息，记录飞书上下文
2. 再写入一条正常的 `user` 消息，正文只保存用户真实文本

隐藏 system 消息建议结构类似：

```xml
<external_channel_message channel="feishu"
  chat_id="oc_xxx"
  chat_type="p2p"
  message_id="om_xxx"
  sender_open_id="ou_xxx"
  sender_name="张三"
  is_mention="true"
  payload_type="text">
</external_channel_message>
```

这样做的好处：

- 对话正文保持干净，方便搜索、摘要和后续复盘。
- Butler 仍能获得完整上下文元数据。
- 后续扩展到文件、图片时，可以继续沿用同一套外部渠道消息包装方式。

### 9.3 Butler prompt 约束

在 Butler 固定系统提示词中，新增一段渠道规则，仅在飞书接入启用时拼装：

- 来自飞书的消息属于外部渠道消息。
- 对外回复应尽量简洁、明确、面向业务结果。
- 默认不要把中间推理、内部调度过程、原始 tool 细节直接发到飞书。
- 对飞书只发送最终可读结论。
- 如果任务仍在处理中，可回一条短状态，如“已接收，正在处理”。

这部分仍然应由 `build_butler_system_prompt()` 统一拼接，而不是做成可编辑 assistant prompt。


## 10. 系统架构设计

### 10.1 总体结构

建议新增一个独立的飞书运行时层，挂在 Rust 后端：

```text
Experimental Config
    -> FeishuButlerConfig
    -> FeishuButlerRuntime
        -> TokenManager
        -> LongConnectionWorker
        -> InboundRouter
        -> OutboundSender
        -> StatusEmitter
```

### 10.2 建议新增模块

建议新增目录：

```text
src-tauri\src\feishu\
  mod.rs
  config.rs
  types.rs
  auth.rs
  client.rs
  long_connection.rs
  inbound.rs
  outbound.rs
  runtime.rs
```

各模块职责：

- `config.rs`
  - 读取和解析实验性配置、访问控制配置

- `types.rs`
  - 飞书事件、消息、连接状态、内部路由结构体

- `auth.rs`
  - 获取并缓存 `tenant_access_token`

- `client.rs`
  - 统一封装飞书 HTTP API

- `long_connection.rs`
  - 维护长连接生命周期、重连、心跳、事件分发

- `inbound.rs`
  - 处理飞书事件到 AIPP 会话的路由

- `outbound.rs`
  - 处理 AIPP 消息到飞书回复/发送

- `runtime.rs`
  - 启停运行时、维护全局状态、响应配置变更


## 11. 配置存储设计

### 11.1 非敏感配置

非敏感项继续放在现有 `feature_config(feature_code = "experimental")` 中，例如：

- `butler_feishu_enabled`
- `butler_feishu_connection_mode`
- `butler_feishu_receive_p2p_enabled`
- `butler_feishu_receive_group_enabled`
- `butler_feishu_group_only_mention`
- `butler_feishu_access_mode`
- `butler_feishu_allowed_user_open_ids`
- `butler_feishu_allowed_chat_ids`
- `butler_feishu_reply_in_thread`

### 11.2 敏感配置

`app_secret` 不应直接明文存入当前通用 `feature_config`。

理论上，把加密后的密文继续存回原来的 `feature_config` 也可以做到“不是明文”。但本方案仍建议把敏感配置放到独立的 `secure_config` 中。

### 11.3 为什么建议 `secure_config`

建议新增一套轻量的本地安全存储：

```sql
CREATE TABLE secure_config (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  scope TEXT NOT NULL,
  key TEXT NOT NULL,
  ciphertext TEXT NOT NULL,
  nonce TEXT NOT NULL,
  updated_time DATETIME DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(scope, key)
);
```

保存：

- `scope = experimental`
- `key = butler_feishu_app_secret`

这样做的意义是：

- **把普通配置和敏感配置分开管理**
  - 普通 experimental 配置会整组读写，敏感值不应和普通表单数据混在一起

- **避免敏感值进入普通配置读取链路**
  - 当前 `get_all_feature_config()` 会把整个 feature config 读回前端，敏感值不适合走同一条链路

- **避免被整组覆盖**
  - 当前 `save_feature_config()` 是按 feature_code 整组替换，敏感配置和普通配置混存更容易被误覆盖

- **为后续扩展留空间**
  - 以后如果还要加企业微信、Telegram、Slack、Webhook token，也可以共用同一套安全配置存储

### 11.4 加密策略

MVP 建议使用本地对称加密：

- 第一次启动时生成一个随机本地主密钥
- 主密钥保存在本地系统配置中
- `app_secret` 用 AES-GCM 加密后保存到 `secure_config`

这不是最高级别的系统级凭据保护，但比明文落库安全得多，且与当前项目依赖和实现复杂度匹配。后续如果需要更强安全性，再切系统钥匙串/凭据管理器。


## 12. 数据模型设计

### 12.1 MVP 只增加一张映射表

MVP 建议只增加一张表：

```sql
CREATE TABLE IF NOT EXISTS external_channel_message_link (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  channel TEXT NOT NULL,
  external_message_id TEXT NOT NULL,
  external_chat_id TEXT NOT NULL,
  external_user_id TEXT,
  conversation_id INTEGER NOT NULL,
  message_id INTEGER NOT NULL,
  direction TEXT NOT NULL,
  payload_type TEXT NOT NULL,
  created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(channel, external_message_id, direction)
);
```

用途：

- 做去重
- 保留 AIPP 消息与飞书消息之间的映射
- 支撑 reply / 审计 / 问题排查
- 为后续文件、图片等更丰富 payload 做类型预留

### 12.2 为什么本期只要这一张表

因为当前 AIPP 已经有：

- `conversation`
- `message`
- Butler 主会话与子任务体系

因此本期不需要再单独为飞书做完整会话表和绑定表。只要解决：

- 外部 message_id 去重
- reply 时能找到对应内部消息
- 能回溯外部消息来源

就足够支撑 MVP。

### 12.3 后续扩展表

如果后续需要以下能力，再补第二张会话绑定表：

- Butler 主动给某个飞书会话推送消息
- 多渠道长期会话绑定
- 群线程 / topic 级映射
- 跨会话恢复与主动通知

这张会话绑定表不是本期前置条件。


## 13. 文本优先，但为多模态留好扩展口

### 13.1 本期

第一阶段只接受：

- 文本输入
- 文本输出

### 13.2 后续需要支持的方向

后续要支持：

- 飞书上传文件给 Butler
- 飞书发送图片给 Butler
- Butler 向飞书回传图片
- Butler 向飞书回传文件

### 13.3 现在就要预留的设计

虽然本期只做文本，但设计上应从一开始就预留统一消息部件模型。内部建议把外部渠道消息抽象为：

- `text`
- `image`
- `file`
- `audio`
- `unknown`

隐藏 system 消息里也应预留：

- `payload_type`
- `attachments`
- `mime_type`
- `file_name`
- `file_size`

即使第一阶段只真正处理 `text`，也不要把数据结构写死成“只有纯文本”。

### 13.4 后续多模态扩展方向

后续可按以下方式扩展：

- **文件输入**
  - 飞书事件拿到 file token / file key
  - 通过飞书文件接口下载到本地临时目录
  - 再按 AIPP 现有附件或上下文模式交给 Butler / 子任务

- **图片输入**
  - 下载图片到本地临时目录
  - 以附件或上下文方式注入

- **图片输出**
  - Butler 或子任务产出图片文件
  - 走飞书图片上传与发送能力回传

- **文件输出**
  - Butler 或子任务产出文档、表格、压缩包等
  - 走飞书文件上传与发送能力回传

本期不实现这些能力，但数据结构、映射表和隐藏 system 消息格式都应兼容未来扩展。


## 14. 运行时状态设计

建议新增全局状态：

```rust
struct FeishuButlerState {
    runtime: Arc<TokioMutex<Option<FeishuButlerRuntimeHandle>>>,
    status: Arc<TokioMutex<FeishuButlerConnectionStatus>>,
}
```

连接状态建议包含：

- `disabled`
- `not_configured`
- `connecting`
- `connected`
- `reconnecting`
- `error`

并通过事件广播给前端，例如：

- `butler-feishu-status-changed`

前端据此展示状态文案和错误提示。


## 15. 入站消息处理流程

```text
飞书事件 -> LongConnectionWorker -> 去重 -> 解析消息
        -> 权限判断
        -> 写入 external metadata system message
        -> 写入 user message
        -> 调用 Butler ask_ai
```

详细流程：

1. 长连接收到 `im.message.receive_v1`
2. 解析出：
   - `message_id`
   - `chat_id`
   - `chat_type`
   - `sender_open_id`
   - `sender_name`
   - 文本内容
3. 过滤不需要处理的消息：
   - 机器人自己发出的消息
   - 重复投递事件
   - 非文本消息
   - 不在 allowlist 中的用户或 chat
   - 群聊但未 `@机器人`、也不是回复机器人的消息
4. 确保 Butler 主会话存在
5. 写入飞书元数据 system 消息
6. 写入真实用户 user 消息
7. 调用 `ask_ai` 推动 Butler 主会话运行


## 16. 出站消息处理流程

```text
Butler 最终 assistant 消息 -> Feishu outbound sender
                      -> 优先 reply 原消息
                      -> 失败时 fallback 为 send chat message
```

详细流程：

1. 识别本轮用户输入是否来自飞书
2. 等待 Butler 本轮最终 assistant 回复完成
3. 提取“适合对外发送”的最终文本
4. 若 `butler_feishu_reply_in_thread = true`
   - 优先调用回复消息 API
5. 如果 reply 失败且允许降级
   - 使用发送消息 API，发到原 chat_id
6. 记录发送结果与飞书 message_id

### 重要规则

- **只发送最终 assistant 文本**
- 不发送中间 streaming 片段
- 不发送内部 MCP 卡片状态
- 不发送隐藏的 system 消息

这能保证飞书侧看到的是干净结果，而不是 Butler 的内部运行过程。


## 17. 去重与防回环

必须处理两个问题：

### 17.1 重复事件

飞书事件可能重投，因此入站处理要基于 `external_message_id` 去重。

### 17.2 自己回复自己

机器人发出的消息如果再次以事件形式回来，必须识别并忽略。

过滤条件建议包括：

- sender 是否为当前 bot 自己
- `external_channel_message_link` 中是否已有该 `external_message_id` 的入站记录


## 18. 与现有 AIPP 代码的结合点

### 18.1 前端

重点修改：

- `src\components\config\feature\forms\ExperimentalConfigForm.tsx`
- `src\components\config\FeatureAssistantConfig.tsx`

要做的事：

- 在 Butler 配置块下新增飞书配置区
- 增加访问控制配置区
- 增加状态展示、测试连接、重连按钮
- 保存 experimental 配置时，非敏感配置仍走 `save_feature_config("experimental", ...)`
- 敏感配置单独调用新命令保存

### 18.2 后端

重点修改：

- `src-tauri\src\api\system_api.rs`
  - 增加安全配置保存 / 读取命令

- `src-tauri\src\api\butler_api.rs`
  - 增加 Feishu 渠道上下文注入
  - 增加 Butler prompt 的飞书渠道规则拼接
  - 增加 Butler 回复完成后对飞书回发的触发

- `src-tauri\src\lib.rs`
  - 注册 `FeishuButlerState`
  - 应用启动时根据 experimental 配置决定是否启动连接
  - 注册相关 tauri command

- `src-tauri\src\db\system_db.rs`
  - 增加安全配置相关表或访问接口

- `src-tauri\src\db\conversation_db.rs`
  - 增加 external channel mapping 表

### 18.3 复用点

本方案应尽量复用：

- 现有 `FeatureConfigState`
- 现有 Butler 主会话加载逻辑
- 现有 `add_message` / `ask_ai`
- 现有 runtime state / completion 事件
- 现有事件广播给前端的模式

核心原则是：**飞书只是 Butler 的一个外部输入输出通道，不是另一套聊天系统。**


## 19. 前端配置草图

当 `总管家模式（实验）` 打开后，在现有 Butler 配置块下新增：

```text
[x] 飞书接入（实验）
    连接方式：长连接
    App ID: [____________]
    App Secret: [____________]
    [x] 接收单聊
    [x] 接收群聊
    [x] 群聊仅响应 @我 / 回复我

    访问控制模式：
    ( ) 飞书范围内全部可用
    ( ) 仅允许白名单

    允许的 User Open ID:
    [____________]

    允许的 Chat ID:
    [____________]

    状态：已连接 / 连接中 / 错误
    最近错误：权限不足

    [测试连接] [重连]
```


## 20. 错误处理

至少要覆盖以下错误：

- App ID / App Secret 错误
- 无法获取 `tenant_access_token`
- 未开启机器人能力
- 未订阅 `im.message.receive_v1`
- 权限不足
- 长连接断开
- reply 失败
- bot 不在目标群
- 用户或 chat 不在 AIPP allowlist 中

产品行为建议：

- 保存配置失败：直接 toast 报错
- 长连接运行失败：状态切为 `error`，界面展示最近错误
- reply 失败：记录日志，可选在 Butler 主会话中补一条隐藏 system 消息


## 21. MVP 实现顺序

### Phase 1：配置、权限与连接打通

- Experimental UI 增加飞书配置区
- Experimental UI 增加访问控制配置区
- 增加安全存储 `app_secret`
- 实现 token 获取
- 实现长连接启动、断线重连、状态展示

### Phase 2：文本入站打通

- 接入 `im.message.receive_v1`
- 文本消息解析
- 去重、自消息过滤、权限过滤、群聊规则过滤
- 将飞书消息写入 Butler 主会话并触发 `ask_ai`

### Phase 3：文本出站打通

- 监听 Butler 最终回复
- 将最终文本回发飞书
- 优先 reply，失败再 send
- 建立最小消息映射表

### Phase 4：多模态扩展

- 文件输入下载与临时落地
- 图片输入下载与注入
- 图片输出上传与发送
- 文件输出上传与发送
- 针对附件类型优化隐藏 system 消息结构


## 22. 验收标准

满足以下条件即可认为本期完成：

1. 在 `实验性` 中启用 `总管家模式（实验）` 后，能看到飞书配置。
2. 可以配置允许哪些用户或 chat 与 bot 对话。
3. 填入正确的飞书应用配置后，AIPP 能显示“已连接”。
4. 飞书单聊机器人发送文本消息，Butler 能收到并回复。
5. 飞书群聊中 `@机器人` 发送文本消息，Butler 能收到并回复。
6. Butler 回复会自动回到原飞书上下文。
7. 机器人自己的回复不会被再次当成入站消息重复处理。
8. 配置错误、权限不足、连接失败时，界面能给出明确状态或错误信息。


## 23. 最终设计决策

本项目的飞书双向接入，采用以下最终决策：

- **接入类型**：飞书自建应用机器人
- **收消息方式**：长连接
- **发消息方式**：优先 reply，必要时 send
- **承载会话**：现有 Butler 主会话
- **权限控制**：飞书范围控制 + AIPP allowlist 双层控制
- **配置位置**：`实验性 -> 总管家模式（实验）`
- **消息注入方式**：隐藏 system 元数据 + 普通 user 正文
- **敏感配置策略**：`app_secret` 使用独立安全存储，不走普通 feature config
- **MVP 数据层**：只增加一张外部消息映射表
- **MVP 范围**：文本消息双向收发
- **扩展方向**：未来兼容文件、图片等多模态输入输出

这套方案与当前 AIPP 的 Butler 架构、桌面形态和 feature config 机制兼容，能在不破坏主逻辑的前提下逐步落地。
