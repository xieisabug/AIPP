# 总管家 Super Admin Action API 设计

## 1. 目标

当前总管家已经具备以下基础能力：

- 与用户进行主会话交互
- 派发子任务给其他助手
- 使用 MCP、Skills 和现有工具完成部分工作
- 接收并回复飞书消息

但它仍然不具备“像用户本人操作 AIPP”那样的能力。问题不在于 AIPP 缺少功能，而在于：

- 现有能力分散在大量 Tauri command、数据库 repo、运行时状态和前端 invoke 链路中
- 这些接口主要是给前端和内部代码用的，不是给 AI 直接调用的
- AI 没有浏览器里的 JS 执行能力，也不知道该如何稳定地拼装这些旧接口
- 如果把所有能力都做成 MCP 工具或全量 prompt 注入，信息量会迅速失控

本设计的目标是：

- 让总管家获得接近“用户操作 AIPP”的控制力
- 不要求模型直接理解几百个旧 API
- 不把几百个能力全量注入上下文
- 让能力调用既灵活，又可控、可审计、可逐步扩展
- 尽量把风险限制在 AIPP 应用本身和授权工作区内，而不是整个操作系统


## 2. 结论

最终推荐方案不是“把所有能力都做成独立 MCP 工具”，也不是“把所有 Rust API 直接裸暴露给 AI”。

推荐方案是：

- **以 Skills 负责教学与编排**
- **以 Super Admin Action API 负责执行**
- **以内部 Action Registry 复用现有 Tauri / Rust 能力**
- **以渐进式目录发现与按需检查来避免上下文膨胀**

也就是说：

- 总管家看到的不是几百个离散工具
- 它只需要掌握一组非常小的“总控接口”
- 这些接口背后再路由到 AIPP 现有能力

推荐只对总管家暴露少量核心接口：

- `superadmin_catalog`
- `superadmin_inspect`
- `superadmin_execute`
- `superadmin_batch`

必要时可再补：

- `superadmin_approve`
- `superadmin_rollback`
- `superadmin_audit_query`

这套方案的核心思想是：

- **能力目录是动态可查询的**
- **能力细节是按需展开的**
- **真正执行的是受控 action，而不是底层实现**
- **skills 负责教会总管家如何搜索、挑选、组合这些 action**


## 3. 为什么不再以“MCP 工具化一切”为主

MCP 本身不是问题，但如果把 AIPP 内部几百个能力都平铺成工具，会立刻遇到几个问题：

- prompt 会被工具描述撑爆
- 模型很难稳定记住每个工具的适用范围与参数
- 工具过多会显著降低选择质量
- 很多工具本质只是旧 API 的机械映射，并没有真正做面向 AI 的抽象

如果只是把“旧 API 名字”换成“MCP 工具名字”，本质上并没有解决问题。

因此不再建议沿着“每个能力一个工具”的路径继续扩大。

更好的做法是：

- 让 MCP 或 Tauri command 只承担**很少的总控接口**
- 让“几百个实际能力”隐藏在内部 registry 里
- 让总管家通过搜索目录、查看 schema、再执行 action 的方式工作


## 4. 为什么不能直接暴露旧 API

前端现在确实已经能通过 `invoke` 调很多 Tauri command，但这不意味着 AI 应该直接使用这些旧接口。

旧接口的问题在于：

- 数量多，分布散
- 命名不统一
- 入参和返回值风格不统一
- 很多接口是面向前端页面状态设计的，不是面向 agent 决策设计的
- 有的接口默认信任前端上下文，并没有为 AI 调用场景设计额外边界

如果让总管家直接面对这批旧接口，会出现几个结果：

- 学习成本高
- 提示词冗长
- 调用不稳定
- 风险难分级
- 审计难统一

因此推荐做一层新的 **Super Admin Action API**，把旧接口收编为面向 AI 的能力模型。


## 5. 设计原则

### 5.1 统一入口，小接口面，大能力面

总管家不应该直接看到几百个入口，而应该只看到很少的统一入口。

真正的能力数量可以很多，但对 AI 的入口数量必须很小。

### 5.2 教学与执行分离

`Skills` 很适合承担：

- 解释系统里有哪些能力域
- 教模型遇到什么问题该先搜什么 action
- 教模型如何做规划、审批、回滚和容错

`Action API` 则负责：

- 查目录
- 看 schema
- 执行动作
- 做批处理
- 返回结果

这样做的好处是：

- prompt 中保留的是策略知识，而不是海量能力细节
- 能力细节放在运行时查询

### 5.3 Action 必须是“意图级”的

不要把 action 设计成底层 repo 操作。

不要让总管家调用类似：

- `assistant_repo.update(id, prompt, ...)`
- `conn.execute(sql)`
- `state.feature_config_map.lock()`

而应该设计成：

- `assistant.update_prompt`
- `assistant.set_model`
- `conversation.archive`
- `schedule.run_now`
- `workspace.write_file`
- `ui.open_window`

也就是让 action 反映业务意图，而不是底层实现细节。

### 5.4 风险必须先于执行被表达出来

每个 action 在目录层就应带有风险信息，例如：

- 风险等级
- 影响范围
- 是否需要审批
- 是否可回滚
- 是否会访问外部系统
- 是否会写文件
- 是否会修改配置

这意味着模型在调用前就能知道：

- 这是安全的只读动作
- 这是会改 AIPP 状态的动作
- 这是跨工作区写入
- 这是高风险动作，需要用户确认

### 5.5 高自由应该先局限在 AIPP 内部

最优先开放的不是系统级 root 能力，而是：

- AIPP 应用内部能力
- AIPP 授权工作区能力
- AIPP 可审计的执行能力

也就是说，总管家的自由首先应该体现在“像用户一样操作 AIPP”，而不是“像系统管理员一样操作整台电脑”。


## 6. 总体架构

推荐架构如下：

### 6.1 Skills 层

作用：

- 告诉总管家有哪些能力域
- 告诉总管家什么时候该查目录，什么时候该 inspect，什么时候该执行
- 告诉总管家如何在风险级别较高时走审批
- 告诉总管家如何组合多个 action

Skills 不负责真正执行能力。

### 6.2 Super Admin Action API 层

这是总管家的唯一正式执行入口。

总管家只需要学会调用少量接口：

- `superadmin_catalog`
- `superadmin_inspect`
- `superadmin_execute`
- `superadmin_batch`

必要时扩展：

- `superadmin_approve`
- `superadmin_rollback`
- `superadmin_audit_query`

### 6.3 Action Registry 层

这是系统内部的能力注册中心。

它维护：

- 所有 action 的元数据
- action 与现有 Rust/Tauri 能力的映射
- 参数 schema
- 权限需求
- 审批规则
- 回滚提示
- 结果摘要规则

这一层是“几百个能力”的真正容器，但不会整体暴露给 AI。

### 6.4 Internal Service Layer

这一层继续复用现有实现，例如：

- Tauri commands
- DB repo
- state
- 既有 service
- Butler / scheduler / artifact / skill / plugin / MCP 相关逻辑

也就是说：

- 旧能力不重写
- 但通过新的 registry 组织起来


## 7. 四个核心接口

## 7.1 `superadmin_catalog`

作用：

- 返回当前可用的能力域与 action 摘要
- 支持搜索和分页
- 支持按 domain / tag / risk / writable / connector 筛选

典型输入：

```json
{
  "query": "assistant prompt",
  "domain": "assistant",
  "detail_level": "summary",
  "limit": 20,
  "cursor": null
}
```

典型输出：

```json
{
  "items": [
    {
      "action_id": "assistant.update_prompt",
      "domain": "assistant",
      "summary": "更新指定助手的系统提示词",
      "risk_level": 2,
      "requires_approval": false,
      "allowed_scopes": ["assistant", "app"],
      "tags": ["assistant", "prompt", "write"]
    }
  ],
  "next_cursor": null
}
```

这个接口只返回摘要，不返回全部参数细节。

它的目标是：

- 帮模型先找到正确能力
- 防止一次性注入过多 schema


## 7.2 `superadmin_inspect`

作用：

- 查看某个 action 的详细参数 schema、结果结构、限制条件和风险说明

典型输入：

```json
{
  "action_id": "assistant.update_prompt"
}
```

典型输出：

```json
{
  "action_id": "assistant.update_prompt",
  "summary": "更新指定助手的系统提示词",
  "args_schema": {
    "type": "object",
    "properties": {
      "assistant_id": { "type": "integer" },
      "prompt": { "type": "string" },
      "reason": { "type": "string" }
    },
    "required": ["assistant_id", "prompt"]
  },
  "result_schema": {
    "type": "object",
    "properties": {
      "assistant_id": { "type": "integer" },
      "updated_fields": { "type": "array" }
    }
  },
  "risk_level": 2,
  "requires_approval": false,
  "rollback_hint": "可通过 assistant.update_prompt 恢复旧值"
}
```

这个接口的目标是：

- 在真正执行前，按需加载 action 的细节
- 避免把所有 schema 一次性塞进 prompt


## 7.3 `superadmin_execute`

作用：

- 执行单个 action
- 支持 `dry_run`
- 统一返回结构化结果

典型输入：

```json
{
  "action_id": "assistant.update_prompt",
  "args": {
    "assistant_id": 12,
    "prompt": "新的系统提示词"
  },
  "dry_run": false,
  "reason": "为了让执行助手更适合代码修复任务"
}
```

典型输出：

```json
{
  "success": true,
  "risk_level": 2,
  "approval_used": false,
  "result": {
    "assistant_id": 12,
    "updated_fields": ["prompt"]
  },
  "audit_id": "audit_123"
}
```

这个接口是实际执行面。


## 7.4 `superadmin_batch`

作用：

- 以事务化或半事务化的方式执行一批 action
- 支持前置检查、逐步执行和失败停止

典型输入：

```json
{
  "actions": [
    {
      "action_id": "assistant.create",
      "args": { "name": "新助手" }
    },
    {
      "action_id": "assistant.update_prompt",
      "args": { "assistant_ref": "$prev.result.assistant_id", "prompt": "..." }
    }
  ],
  "dry_run": false,
  "stop_on_error": true
}
```

这个接口适合：

- 多步配置调整
- 一次性创建对象并继续修改
- 总管家的编排式操作


## 8. Action Registry 的设计

每个 action 都应有统一元数据。

建议结构如下：

```json
{
  "action_id": "assistant.update_prompt",
  "domain": "assistant",
  "summary": "更新指定助手的系统提示词",
  "description": "修改 assistant 的 prompt，并写入审计记录",
  "risk_level": 2,
  "requires_approval": false,
  "allowed_scopes": ["assistant", "app"],
  "args_schema": {},
  "result_schema": {},
  "rollback_hint": "可再次调用本 action 恢复原值",
  "executor": "internal_assistant_service.update_prompt"
}
```

这个 registry 的作用不是只做索引，而是成为：

- 权限判断入口
- 参数校验入口
- 审计记录入口
- 能力发现入口


## 9. 能力域设计

不要按“底层 API 文件”分域，而要按业务场景分域。

建议第一版按如下能力域组织。

### 9.1 `assistant`

包括：

- 创建助手
- 复制助手
- 更新 prompt
- 更新 model
- 更新 MCP 设置
- 更新 Skills 设置
- 归档/隐藏助手

### 9.2 `conversation`

包括：

- 创建 conversation
- 读取 conversation
- 写入消息
- 注入 system message
- 归档 conversation
- 重开 Butler 主会话

### 9.3 `task`

包括：

- 派发 Butler 子任务
- 读取任务状态
- 取消任务
- 重试任务
- 获取任务结果

### 9.4 `artifact`

包括：

- 创建 artifact
- 保存 artifact
- 预览 artifact
- 关联 artifact 到 conversation

### 9.5 `workspace`

包括：

- 读写受权工作区文件
- 搜索文件
- 删除/移动文件
- 创建目录

### 9.6 `exec`

包括：

- 在受控工作区执行命令
- 查询执行结果
- 停止命令
- 运行验证任务

### 9.7 `schedule`

包括：

- 创建定时任务
- 更新定时任务
- 立即运行
- 停止运行

### 9.8 `ui`

包括：

- 打开窗口
- 跳转配置页
- 聚焦 conversation
- 打开预览
- 请求用户输入
- 发起审批

### 9.9 `config`

包括：

- 读取实验性配置
- 更新非敏感配置
- 查询运行状态

### 9.10 `connector`

包括：

- 飞书状态读取
- 飞书重连
- 外部连接器状态查询


## 10. Skills 在这套方案里的角色

你的判断是对的：`skills` 更像“教学”和“披露”层。

在这套方案里，skills 不再承担“直接提供所有能力”的职责，而是承担：

- 告诉总管家有哪些 domain
- 告诉它如何用 `catalog -> inspect -> execute`
- 告诉它哪些动作应优先 `dry_run`
- 告诉它哪些高风险动作必须先审批
- 告诉它不同任务类型应优先搜索哪些 domain

因此可以给总管家挂一组专门 skills，例如：

- `superadmin-basics`
- `assistant-management-playbook`
- `workspace-operations-playbook`
- `approval-and-risk-playbook`
- `butler-recovery-playbook`

这些 skills 的内容不需要很长，它们更像“操作手册”和“策略提示”。


## 11. 为什么这套方案不会撑爆上下文

关键原因在于：**AI 默认看不到所有能力细节。**

它只看到：

- 少量总控接口
- 少量 domain 摘要
- skills 中的高层操作规则

真正细节是在运行时按需查询的。

典型流程如下：

1. 总管家先根据任务判断需要哪个 domain
2. 调 `superadmin_catalog` 搜能力
3. 调 `superadmin_inspect` 看具体 action schema
4. 调 `superadmin_execute` 或 `superadmin_batch`

这样做的结果是：

- 上下文中保留的是“方法论”
- 具体 action 细节只在当前回合临时出现

这比全量注入几百个工具定义稳定得多。


## 12. 安全与权限模型

建议把风险控制建立在 action 层，而不是底层 API 层。

### 12.1 风险等级

建议至少四级：

- `0`: 只读安全动作
- `1`: 当前会话/工作区内的低风险写操作
- `2`: 应用内中风险写操作
- `3`: 高风险动作，需要审批

### 12.2 审批策略

高风险动作应支持：

- `auto_allow`
- `allow_in_scope`
- `user_approval_required`
- `deny`

### 12.3 作用域

每个 action 应明确作用域，例如：

- `conversation`
- `assistant`
- `artifact_workspace`
- `project_workspace`
- `app`
- `connector`

### 12.4 敏感数据策略

即使总管家有 superadmin action 能力，也不应默认能读取：

- 密钥原文
- App Secret
- Token
- 安全配置主密钥

它可以得到状态信息，例如：

- 已配置
- 未配置
- 校验失败
- 需要更新

但不应直接得到原文。


## 13. 哪些能力应该开放

推荐优先开放：

- 助手管理
- conversation 管理
- Butler 任务管理
- artifact 管理
- schedule 管理
- 受权工作区文件管理
- 受限命令执行
- UI 打开与跳转
- 外部连接状态控制

这些能力基本都属于：

- AIPP 内部对象
- AIPP 授权工作区
- AIPP 可审计运行面

它们是“像用户操作 AIPP”最核心的部分。


## 14. 哪些能力不应直接开放

不应直接开放：

- 所有旧 Tauri command 原样直出
- 所有 Rust service / repo 原样直出
- 任意 SQL
- 任意系统命令
- 任意注册表 / 系统级配置修改
- 插件安装与启用
- 敏感配置原文读取

原因都一样：

- 这些要么太底层
- 要么太危险
- 要么很难审计
- 要么容易直接突破 AIPP 的边界


## 15. 是否需要一个还是多个 super admin API

建议从“一个逻辑系统、四个正式接口”来设计。

也就是说：

- 逻辑上是一个 **Super Admin Action API 系统**
- 具体上暴露为 4 个主接口

这样兼顾了：

- 概念统一
- 实现清晰
- 调用简单
- 功能完整

不建议只做一个“大一统 execute_everything”接口。

因为那样会失去：

- 目录发现
- schema 检查
- 风险前置表达

所以最合理的形态不是“1 个超大接口”，而是“1 套系统 + 4 个小而稳定的入口”。


## 16. 与现有代码的关系

这套方案不是推翻现有 AIPP，而是重组现有能力。

可以复用的现有部分包括：

- conversation / message / assistant / schedule / feature config 的已有逻辑
- Butler 主会话与子任务机制
- 现有 permission / workspace / artifact 逻辑
- 现有运行时状态与事件广播逻辑
- 现有 Skills 注入机制

需要新增的是：

- action registry
- superadmin command 层
- action schema 定义
- action 风险与审批策略
- action 级审计日志


## 17. 建议新增的系统能力

为了让总管家真正像“用户本人操作 AIPP”，建议新增以下能力。

### 17.1 Action Registry

统一管理所有 action 元数据与执行器。

### 17.2 Action Audit Log

记录：

- 发起源
- 所属 Butler conversation
- action_id
- args 摘要
- 风险等级
- 是否审批
- 执行结果
- 时间

### 17.3 Dry Run 机制

对中高风险动作支持 dry run。

总管家可先查看：

- 将会修改什么
- 将会触达哪些对象
- 是否会越权

### 17.4 Approval Card

在 UI 中为高风险动作生成审批卡片。

### 17.5 Rollback Hint / Undo Hook

对于可以回滚的 action，提供：

- rollback 提示
- 可选 undo action


## 18. 分阶段落地建议

### Phase 1：最小可用 Super Admin Action API

先实现：

- `superadmin_catalog`
- `superadmin_inspect`
- `superadmin_execute`
- `assistant / conversation / task / schedule` 四个 domain

这是总管家最核心的应用内控制面。

### Phase 2：工作区与执行能力

再实现：

- `workspace`
- `exec`
- `artifact`

这一阶段会让总管家真正具备“做事”的能力。

### Phase 3：审批、回滚与 UI 代理

再实现：

- approval
- audit
- rollback
- ui domain

这一阶段会让总管家变成真正可靠的高权限代理。

### Phase 4：外部连接器编排

最后实现：

- connector domain
- 更丰富的飞书控制
- 其他外部连接能力


## 19. 最终建议

如果目标是让总管家像“用户本人”一样操作 AIPP，那么最合理的方向不是：

- 把几百个能力全做成独立 MCP 工具
- 也不是把所有旧 API 裸暴露给它
- 更不是让它长期靠 shell 勉强操作

真正合适的方案是：

- **用 skills 教它如何思考和使用能力**
- **用 Super Admin Action API 给它统一执行入口**
- **用 Action Registry 把现有能力重组为 AI 可调用的 action**
- **用 catalog / inspect / execute / batch 实现渐进式披露**

这样既保留了灵活性，又避免上下文爆炸，还能把风险控制、审批、审计和回滚统一纳入同一套体系。

这套方案的本质是：

**不是让总管家直接理解 AIPP 的所有旧接口，而是为它提供一套面向 AI 的、可发现、可检查、可执行、可审计的高权限控制平面。**
