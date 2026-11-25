# ACP 集成

ACP (Agent Client Protocol) 集成模块允许 AIPP 与 ACP 代理进行交互，每个对话运行一个独立的 ACP 进程。

## 统一 Agent 调用

Codex app-server、Claude Code、Pi、Cursor、OpenCode、Kiro、Antigravity 与 ACP 共用 AIPP 的 Agent 驱动入口。模型、权限、MCP 与可用配置由对应 Agent 决定，各通道的能力并不完全相同。点击智能体历史消息的「重新生成」，会在原会话追加「重新检查并完成：」及该条正文，由同一助手继续执行。回复只有工具活动时，改用该轮用户原话。

Pi、Cursor、OpenCode、Kiro、Antigravity 首次返回正文、推理或工具活动时，聊天边框从用户消息移到承载输出的助手消息，管家状态同步结束“投递中”。Codex 的推理和计划有独立消息，边框会跟随当前收到内容的消息；正在执行的 MCP 工具仍优先高亮。

点击会话右上角的「对话信息」可查看累计 Token、输入、输出和缓存读写。各智能体的上报方式见 [Token 统计](./12-Token统计.md)。

### 管家会话的角色和工具说明

管家主会话、联系人、独立任务和群成员共用 AIPP 的角色构造逻辑。模型按本次 MCP 目录里的实际注册名和参数定义调用工具。动态工具先加载，将回执条目的 `server`、`tool` 分别填入 `server_name`、`tool_name`，再用 `call_mcp_tool` 传对象类型的 `parameters`，无参数时传 `{}`。角色与工具说明在原生驱动发送时注入；用户消息、附件、引用和长期记忆仍走原有本轮上下文链路。

| 驱动 | 管家说明的投递位置 |
| --- | --- |
| Codex app-server | `turn/start.additionalContext` 中的应用条目，每条最多 4,000 个 UTF-8 字节。收到原生压缩完成事件即在同一回合补回当前指令；同轮再次压缩会再次补回。 |
| Claude Code | CLI 的 `--append-system-prompt-file`。新建和恢复进程都追加到 Claude 自带系统提示。 |
| Pi | 扩展在 `before_agent_start` 填充系统提示 section，旧版扩展接口使用追加的系统提示文本。 |
| Cursor | SDK 发送的首个普通回合附带管家说明，成功后该运行实例后续回合省略重复内容。 |
| OpenCode | AIPP 当前支持的本机 HTTP 服务协议在 `prompt_async.system` 接收说明，每次随回合发送。 |
| 通用 ACP、Kiro V3 | 首个普通 `session/prompt` 追加独立文本块。会话恢复到新的驱动实例时重新投递；手动压缩后在下一普通回合补发。 |
| Antigravity | 原生端支持时，在新建或恢复的 `_meta.antigravity.systemPrompt.append` 发送。其余受支持版本使用 ACP 文本块。 |

上下文说明不授予工具权限。工具启停、审批、凭据保险箱和会话归属仍由原有 AIPP 执行路径检查。

### 原生智能体的环境检测与更新

Pi、Cursor、OpenCode、Kiro、Antigravity 的提供商表单打开时会自动检测运行环境，不需要手动填写命令：

- **Node.js 运行时**：默认「自动」从 PATH、版本管理器（如 nvm、fnm、Volta）及常见安装目录中选择本机可运行的最高版本，最低要求为 22.19.0。同版本优先采用 PATH 中的安装。新启动的桥接进程使用该可执行文件，其目录排在子进程 PATH 首位。需要固定版本时，可以选择列表中的安装或填写自定义路径。固定版本低于要求时，设置页会标注并提示可改用的版本。
- **智能体程序**：自动模式先查找会话的 PATH，然后查找官方安装器、npm / pnpm / Bun 全局目录以及其他 Node 安装目录。添加向导和设置页显示所选程序运行 `--version` 的结果。聊天、模型列表、登录和更新都按相同顺序定位程序。固定到某个文件后，如果文件缺失或无法启动，会显示该程序的错误。未安装时设置页提供官方安装命令和文档链接。
- **更新**：检测到版本后自动对照最新发布（Pi / OpenCode / Cursor SDK 查 npm，Kiro 查官方 manifest，Antigravity 查官方 CHANGELOG，结果缓存 30 分钟），有新版本时显示「更新」。更新使用智能体自带的更新命令（`pi update self`、`opencode upgrade`、`kiro-cli update`、`agy update`），输出实时显示；失败时保留退出码与输出，可使用全局代理重试。有会话正在回复时拒绝更新；空闲会话会先关闭，下次发送消息时按原会话恢复。Antigravity「独立 ACP 程序」由用户自行管理，不提供一键更新。
- **工作目录**使用文件夹选择器（也可手动输入），目录不存在时直接提示。

### Codex、Claude Code 与 ACP 的环境状态

这几类提供商表单的「环境状态」与原生智能体使用同样的展示：一行「版本 · 路径」，下方是「检查更新」「重新检测」。提供商设置里的「CLI 命令」同时用于环境检测、模型探测和会话启动。安装了多份 CLI 时，可以直接填写目标程序的绝对路径，点击「重新检测」查看实际路径和版本。Windows 会按 PATH 顺序查找可执行文件或启动脚本，并检查用户目录下的常用安装位置。探测失败时会显示搜索目录或版本命令的具体错误。已安装的 Codex 可以直接使用，选择通过 AIPP 一键安装时才需要 Bun。 Windows 上保留默认命令 `codex` 时，AIPP 会检查本机候选安装，优先使用 npm/pnpm 启动脚本所对应的同版本 Windows 原生程序；候选版本检测失败会继续查找，全部失败时显示各自原因。手填路径始终只使用指定程序。 ACP 适配器仍检测所选命令。

- **Codex** 支持通过 Bun 一键安装 `@openai/codex`。已有安装使用当前选中的启动器。AIPP 核对 npm、pnpm 或 Bun 的全局目录及包入口，确认来源后使用对应的包管理器更新，并重新检测启动器和实际版本。无法确认来源时，设置页提示使用原安装方式手动更新。
- **Claude Code**：按官方说明自行安装；检查更新查 npm 上的 `@anthropic-ai/claude-code`，更新运行 Claude Code 自带的 `claude update`，更新的是实际检测到的那份安装（原生安装或 npm 安装）。失败时保留输出，可使用全局代理重试。
- **Gemini、Kimi Code、DeepSeek Harness**：由用户自行安装和更新，只提供重新检测。

### 原生智能体的审批

审批设置在「智能体助手」中（与 Codex 的审批策略同一位置），每个助手可以不同：

| 智能体 | 原生审批控制 | 选项 |
| --- | --- | --- |
| Pi | Pi 自身没有权限系统，由 AIPP 扩展在工具调用前请求确认 | 「需要审批的操作」：所有工具调用（默认）/ 只读工具（read、grep、find、ls）直接执行 / 全部直接执行 |
| Kiro | Autopilot | 关闭：逐项审批（默认）/ 开启：Kiro 自动执行 |
| Antigravity | 原生执行模式与工具审批 | 可选默认、自动接受编辑、自动审核（`auto`）、规划（`plan`）或全部自动执行。全部自动执行沿用已保存的 `yolo` 设置，CLI 通过 `agy acp --dangerously-skip-permissions` 启动，独立 ACP 程序须公布 `yolo` 模式。会话缺少所选模式时会列出可用模式并停止。 |
| OpenCode | 本机配置的 `permissions` 规则 | AIPP 不改写（新旧版本的规则格式不同），默认几乎全部放行 |
| Claude Code | 提供商可设默认权限，聊天输入框可选默认（default）、接受编辑（acceptEdits）、自动审核（auto）、跳过审核（bypassPermissions）、不问询（dontAsk），Plan 有独立开关 | 发送时记录本条消息的选择，包括排队消息。已启动的 CLI 通过 `set_permission_mode` 确认后再接收正文，同一进程继续运行。CLI 拒绝时本条消息报错。Claude Code 发给 AIPP 的 `can_use_tool` 请求再按 AIPP 的审批设置处理；`AskUserQuestion` 仍交给用户回答 |
| 通用 ACP | 原生 `session/request_permission` 请求 | 安全时只回传 Agent 提供的一次性允许选项；转人工时仍可通过已有飞书通道处理 |
| Cursor | SDK 自带的自动审核（Auto-review） | SDK 不支持逐项人工审批，不会弹出确认 |

Claude Code、通用 ACP、Pi、OpenCode、Kiro、Antigravity 的「审批请求的处理」可选人工审批或 **AIPP 模型自动审核**。在「设置 → 辅助AI」配置审核模型后，尚未指定审批方式的助手默认使用模型自动审核，未配置审核模型时默认人工审批。已保存的助手选择优先，Pi、OpenCode、Kiro、Antigravity 也可继承提供商已指定的审批方式。

交互会话调用已注册的 AIPP 工具时，Pi、Claude Code、Codex 和能提供结构化工具身份的 ACP 适配器会先按本次实参检查免审核规则。匹配的操作直接执行。OpenCode 的权限事件、仅含标题的申请和智能体自身的命令或文件操作继续按原审批设置处理。

使用模型自动审核时，需要审核的权限请求交给审核模型，使用与 MCP 工具审核相同的规则，并告诉审核模型智能体的工作目录。判定安全时自动选择智能体提供的「本次允许」（从不替用户选择「始终允许」这类持久化选项），不弹窗也不发提醒；有风险、审核失败、超时、未配置审核模型或智能体没有提供「本次允许」时才弹出权限确认，并在弹窗中说明原因。审核期间（最长 20 秒）停止会话会撤销这次请求。修改 Pi、OpenCode、Kiro、Antigravity 的审批设置后，下一次发送消息时会按新设置重启并恢复原会话。Claude Code 和通用 ACP 在管家会话中使用模型审核，普通会话按助手设置处理。

### Kiro 与 Antigravity

在设置中新增「Kiro」或「Antigravity」提供商，再在智能体助手中选择该提供商。两者使用官方原生 ACP 协议，拥有独立驱动身份、会话和权限归属；需要本机 Node.js 22.19 或更新版本，仅桌面支持。支持文本流、思考与工具活动、原生权限确认、选中的 AIPP MCP 工具、模型发现、停止和原会话恢复。恢复、模型或配置失败直接报错，不创建替代会话。

**Kiro**：安装支持 V3 的官方 `kiro-cli`，先在终端运行 `kiro-cli login`。AIPP 固定启动 `kiro-cli acp --agent-engine=v3 --auth-method=cli`，每次新建或恢复会话都按助手设置写入 Autopilot（默认 `off`，见下方「原生智能体的审批」）。提供商可以填写 Kiro Agent（原生 `mode` ID）和思考强度（`effortLevel`）；留空使用原生当前值，指定的值不受支持时直接报错。选择 Agent 后再应用模型和思考强度。Kiro 在新会话后才公布模型选项时，发现过程会短暂等待。

Kiro 提供「本次允许」时，AIPP 不显示其持久化允许选项；只有原生没有本次允许时才提供持久化允许，并标注会保存权限规则。停止以原生 prompt 结束为准；刚发送 prompt 时的取消会等到开始信号或短暂等待后发送，避免 Kiro 忽略过早的取消。

**Agent 网络代理**：在全局网络设置填写代理地址，再在提供商的「高级配置」开启「使用网络代理进行请求」。此开关作用于该实例的聊天、模型探测和内置登录流程的后台请求。外部浏览器沿用系统网络设置。修改代理后，下一次启动或复用会话时按新配置建立连接；已启动的请求沿用原设置。显式配置的代理环境变量优先，回环地址加入直连列表以供 OpenCode 本地服务与登录回调使用。Cursor 通过 Node 的环境代理和 SDK 的 HTTP/1.1 流式模式发起请求，需要 Node 22.21+ LTS 或 24.5+ 和支持该选项的 Cursor SDK。HTTP 代理已在 Windows 官方 Antigravity CLI 1.5.12 的 Google 账号会话验证，其他代理协议的支持取决于各 Agent 的网络实现。

**Antigravity**：默认使用官方 `agy acp`，建议使用 1.5.1 或更新版本（低于时表单会提示并可一键更新）。也可显式选择「独立 ACP 程序」，选择其可执行文件和相同版本的 `localharness_external`。独立程序由用户自行安装，本次不包含 Registry 安装管理。

认证支持 Google 个人账号和 Gemini API Key。在提供商设置填写当前配置后点击「登录 Antigravity」；个人账号出现「在浏览器中继续登录」按钮，完成 Google 认证后等待 AIPP 显示原生认证成功。可以取消登录，最多等待 5 分钟。API Key 使用专用密码字段；认证错误保留原生原因。普通聊天与模型发现不会自动开启登录，过期时提示返回设置登录。

助手设置中的模型选择框从当前提供商账号读取模型。读取时只请求模型目录，助手已保存的执行模式不可用时仍可查看和选择模型。

Claude Code 的助手设置、新建对话和会话控制共用模型目录。选择带版本的「Claude Opus 5.5 · claude-opus-5-5」时，AIPP 保存完整 ID，并用它启动或恢复会话。选择「CLI 别名 · opus」会沿用 Claude 设置中的映射。CLI 报告的目标与别名不同时，选择器会显示两者，例如「CLI 路由 sonnet → opus」。以前保存的短别名继续按别名执行。「使用 CLI 默认模型」随本机配置生效。目录同时显示当前提供商登记的模型和 CLI 公布的其他选项。助手设置保存默认值，新建对话可单次覆盖并选择其他 Anthropic 提供商登记的模型；会话内切换只使用当前连接的模型，从下一轮生效。

Claude Code 助手的「默认思考强度」位于默认模型下方，可跟随提供商、使用 CLI 默认值，或指定档位。新建对话会带入生效的默认强度，可为本次对话调整；会话中的「思考强度」从下一轮生效。CLI 公布模型支持的强度时，选项按模型显示，包括支持时的极高（xhigh）。

Antigravity 在登录成功、建立会话和收到模型选项更新时，将账号返回的模型保存在本机。重启 AIPP 后可直接选择；首次使用且没有记录时会等待模型探测。在提供商设置可点「刷新模型」，刷新失败会显示原因并保留同一账号上次成功的列表。使用 Google 账号时可从这里退出登录，需运行程序支持原生退出。切换认证配置或重新登录后会重新获取对应账号的列表。

Antigravity 的账号保存在应用数据目录 `agent-profiles/antigravity/<provider-id>`，与 IDE 和其他提供商实例分离，不继承主机的 Google 凭据变量。运行时临时文件归属当前桥接进程并在关闭或取消后清理。使用真实 AIPP 客户端身份，只展示该账号和运行程序实际公布的模型。会话启动会设为助手选择的原生权限模式（默认 `default`），不继承之前保存的 `yolo` 或 `auto_edit`。原生 `interaction_` 选择题显示为 AIPP 问题卡片，答案必须匹配选项（当前卡片支持 2–4 项，超出时明确报错）；其他请求显示为权限卡片。

当前未接入：Kiro steer/fork/compact、Antigravity 企业认证和 Agent Platform、多媒体输入、完整原生子代理执行树。真实登录、真实模型、工作区工具和 MCP 执行仍需在已安装运行程序的机器验收；协议回放测试不代表真实账号验证通过。

### Pi、Cursor 与 OpenCode

Pi 遇到临时连接错误时，会按 Pi 本机的重试设置在当前会话继续。重试期间可以停止。重试成功后，管家聊天显示最终答复。重试耗尽时，聊天显示最后一次失败的具体原因，重新打开会话仍可查看。

在设置中新增对应类型的提供商，再在「智能体助手」中选择它。三者都需要本机 Node.js 22.19 或更新版本，只在桌面执行。

| 类型 | 本机依赖 | 通道和恢复身份 |
| --- | --- | --- |
| Pi | 已安装并完成登录的 `pi` | 原生 RPC；保存 `sessionFile` 路径，按原路径恢复 |
| Cursor | `@cursor/sdk`、Cursor 账号登录或 API Key | 官方 SDK 的本机执行模式；保存并恢复 agent ID |
| OpenCode | 已安装并完成登录的 `opencode` | 本机 HTTP/SSE 服务；保存并恢复 session ID |

Cursor SDK 目录留空时，AIPP 先从工作目录向上查找 `node_modules/@cursor/sdk`，然后查找应用数据目录 `agent-runtimes/cursor-sdk` 中的托管安装。填写目录后，从指定位置加载 SDK。设置页用所选 Node.js 自带的 npm 更新当前安装。首次一键安装使用填写的目录，留空则装入托管目录，安装完成后自动填入目录。

账号有两种方式，不需要 Cursor 编辑器或 CLI 的登录（SDK 不读取它们）：
- **登录 Cursor（推荐）**：点击「登录 Cursor」，在浏览器完成 Cursor 登录。AIPP 通过 SDK 官方的 `Cursor.auth.login()` 生成一个名为「AIPP - <提供商名称>」、90 天有效的密钥，只保存在应用数据目录 `agent-profiles/cursor/<provider-id>/auth.json`（每个提供商实例独立、不同步、界面不显示密钥），会话和模型发现自动使用。登录页只允许打开 `cursor.com` 域名；可取消，最多等待 5 分钟。「退出登录」会关闭空闲的 Cursor 会话并删除本地密钥；密钥在到期前仍有效，可在 Cursor 后台的 API Keys 中撤销。过期后表单提示重新登录。账号需要 Cursor 付费计划。
- **API Key**：填在「Cursor API Key（可选）」字段；环境变量中的 `CURSOR_API_KEY` 会覆盖它。配置了 API Key 时优先使用 API Key，浏览器登录不可用。

两者都没有时表单会提示，会话与模型发现直接报「Cursor 未登录」。Cursor 使用 SDK 自动审核；不提供 AIPP 人工审批卡片或额外承诺文件系统沙箱能力。SDK 初始化或权限错误会直接显示，不会切换执行方式。

Pi 和 OpenCode 默认读取本机登录及配置；可单独设置 CLI 入口、工作目录和环境变量。Windows 会解析常见 npm/pnpm PowerShell shim 的 JavaScript 入口，避免交互协议被 shell 管道缓冲；不识别的 shim 会提示填写实际入口。环境变量接受 `KEY=VALUE` 多行文本或 JSON 字符串对象，助手中的同名变量覆盖提供商变量。

模型列表从原生目录发现。「使用智能体本机默认模型」是显式选项；指定模型错误不会改用其他模型。Pi/OpenCode 的完整模型标识为 `provider/model`，Cursor 使用账号模型目录中的 ID。OpenCode 智能体与「OpenCode Go」API 提供商是不同入口。

文本回复、思考和工具活动复用现有会话展示；工具活动随响应保存。Pi 通过扩展注入选中的 AIPP MCP 工具，并在工具调用前请求确认；OpenCode 的原生审批进入 AIPP 权限卡片，执行规则仍由本机 OpenCode 配置决定。Pi/OpenCode 的提问复用 AIPP 交互卡片；超出当前卡片支持范围的原生交互会明确报错。Cursor/OpenCode 通过各自原生 MCP 配置接入 AIPP bridge。

活动来源显示实际执行的智能体名称；MCP 工具另外保留服务名。Pi 按实际操作展示命令、读取、搜索/列目录、编辑和写入文件，MCP 与扩展工具保留工具卡片。思考复用普通消息的思考样式和展开交互，受「展示思考过程」设置控制：关闭后只在思考进行时显示加载提示，结束后隐藏，不作为工具卡片展示。操作标题直接显示命令或文件路径，详情默认折叠（失败时展开），可展开查看文本结果、原生返回的文件 diff，以及参数与原始结果。原生 Agent 活动以首次出现的位置排序，进度更新不改变位置，并按该顺序保存以供回放；旧记录未保存顺序时保留已有排列，无法完整还原缺失的先后关系。未收到执行终态的操作不会因为整轮结束就标为成功。工具审批仍按助手的审批设置执行。

Claude Code、Cursor、OpenCode、Kiro 与 Antigravity 同样按原生操作类型或已知内置工具标识展示命令、文件读取、搜索、编辑、写入与删除；MCP 和未知扩展继续保留工具身份，不能只根据标题猜测操作。文本结果与原始参数/结果分开展示，只有原生返回真实 diff 时才展示 diff，不把写入推断为新增文件。Cursor、Kiro 和 Antigravity 的多段思考在转入工具或正文时分别结束，新思考保留自己的位置；OpenCode 使用其原生思考段标识和终态。

Claude Code 的流式思考和完整思考消息合并为同一段，工具完成后保留名称、参数和结果，并与回复一起保存，刷新或重新打开可回放。旧版本没有保存的 Claude 工具记录无法补回。Codex 也保存固定首次顺序，其思考只使用原有独立思考消息，不再重复生成空白活动项。以上思考展示都受同一显示开关控制。

停止会向原生运行发送取消，撤销该 run 的待审批和提问；20 秒内未收到终态则终止所拥有的进程树并显示原因。恢复失败、进程退出或协议错误均不会切换 ACP、模型或新会话。Pi、Cursor 和 OpenCode 均可接收图片，可访问的本机文件随用户消息提供原路径。

发送时若会话控制通道已经关闭，当前响应会显示失败并保留错误原因，不会自动新建会话后重新发送。

### 驱动扩展与提供商实例

- 现有 conversation 和原生会话继续使用原来的存储与 actor。注册表负责按提供商配置选择驱动，并按 conversation、provider ID、driver 和 run 记录活动实例归属。
- 新驱动通过 Rust 代码实现 `AgentDriver`、`PreparedAgentSession`、`AgentSessionControl` 并注册，提供自己的配置 schema、能力发现和原生运行逻辑。扩展不要求向公共 prompt/cancel/config 分发代码增加新的类型分支。
- 实例复用已有提供商 ID，同一种驱动可以有多份独立配置。未知驱动、无效配置、歧义选路或跨实例控制会报错，不自动改用其他提供商。
- 当前没有新增运行时插件装载器或“在配置 UI 安装任意驱动”的入口；代码扩展仍需随应用编译。现有配置界面继续管理已支持的提供商与模型。

### 能力声明与发现

- 每个注册驱动声明本地实现支持的操作；实际会话快照另提供 `agent_capabilities`，区分 `supported`、`unsupported` 和 `unknown`，并标记依据来自本地适配器还是当前会话。尚未连接不等于不支持。
- 取消、配置修改、审批响应、附件输入和原生会话恢复分别声明，不把 ACP、Codex、Claude Code 视为能力相同的通道。扩展驱动可覆盖自己的会话能力发现，不需要增加核心分发分支。
- 模型、思考强度、审批策略等选项继续使用当前原生会话的 `config_options`。ID 和值原样保留，包括厂商扩展值；不同提供商实例不共享缓存的配置目录。
- ACP 从实际协商结果读取图片、嵌入上下文、load/resume 能力。未收到配置目录，或目录含客户端尚不能表示的扩展类型时，目录保持“不完整”，不会凭空推断扩展项不受支持。
- Codex、Claude Code、Pi、Cursor、OpenCode、Kiro、Antigravity 和通用 ACP 共用本机文件路径。Codex、Claude Code、Pi、Cursor、OpenCode 按各自协议传图片。Kiro、Antigravity 和通用 ACP 按会话协商发送图片；对方未声明图片能力时需要可读的本机路径。
- Claude Code 在原生 stream-json 消息中发送图片及大小预算内的 PDF 文档，OpenCode 使用本机 PDF 文件 URI，Kiro、Antigravity 和通用 ACP 在支持嵌入上下文时追加 PDF 资源链接。其他格式使用本机路径；实际能否解析由 Agent 的工具和访问权限决定。
- 会话状态读取与实时事件使用同一份能力结构；旧会话字段保持兼容。配置 UI 仅在明确不支持修改时禁用，未知状态仍交由原生协议返回具体结果。

---

## ACP 会话管理

### 每个对话一个 ACP 进程
- 采用 per-conversation 进程模型
- 每个对话启动一个长期运行的 ACP 进程
- 会话存储在 `AcpSessionState` 中，以 `conversation_id` 为键
- 进程隔离，不同对话互不干扰

### 会话句柄路由
- `AcpSessionHandle` 会话句柄
- 后台任务保持单个 `ConnectionTo<Agent>` 连接活跃（agent-client-protocol v2 SDK）
- ACP 启动连接、提示、取消、配置项更新都通过会话句柄路由到后台任务
- 用户打开 ACP 对话时会自动建立 session 连接，用于提前获取 `configOptions`、Plan 和可用命令
- 自动连接不会创建空消息，也不会向 Agent 发送 prompt
- ACP session 空闲 15 分钟且没有活跃 prompt 时会自动释放；已绑定的实例释放后，下一次发送按显式选择重新建立连接，自动打开页面不会用助手默认 provider 覆盖之前的实例选择

### 会话任务运行时
- ACP 会话任务是普通的 `tokio::spawn` 任务
- agent-client-protocol v2 SDK（crates.io `agent-client-protocol` v2.0.0，schema 类型位于 `agent_client_protocol::schema::v1 as acp`）不绑定具体异步运行时，所有 future 均为 `Send`
- 不再需要专用单线程运行时和 `LocalSet`
- 客户端通过 `Client.builder()` + `on_receive_request` / `on_receive_notification` 回调构建；请求通过 `cx.send_request(...).block_task().await` 在 `ConnectionTo<Agent>` 上发送
- v2 `on_receive_*` 回调内联运行在连接调度循环上，因此每个客户端请求处理器（权限/文件/终端/elicitation）必须立即通过 `AcpTauriClient::spawn_request_task` 转发到 `cx.spawn` 任务，并在该任务里经由 `Responder` 响应——禁止在处理器内 `block_task`（死锁）
- 未知扩展方法/通知由 SDK 内建 fallback 处理（method-not-found / 忽略），不再保留旧的 `ext_method` / `ext_notification` 桩

---

## 会话持久化

### session_id 存储
- `session_id` 存储在 `conversation.db` 的 `acp_session` 表中
- 按 `(conversation_id, agent_kind)` 保存每种驱动的最新 session 映射，并记录可空的 `provider_instance_id`
- 已记录实例归属的 session 只供同一提供商实例恢复；切换实例后保存新的最新映射，不保留各 provider 的完整恢复历史
- 旧记录没有可追溯的实例归属时，首次恢复保留兼容行为；单纯读取不会认领归属，成功建立会话并保存后才补写当前 provider 实例
- 会话创建/加载时更新。应用聊天记录保持不变；跨 provider/驱动的新原生上下文不等于自动重放所有旧历史，Codex/Claude 本轮未新增跨实例历史迁移。

### 会话加载逻辑
- ACP 启动时检查 `initialize` capabilities
- 如果 `sessionCapabilities.resume` 能力支持且存在存储的 `session_id`，优先调用 `session/resume`
- 如果不支持 `session/resume`，但支持 `loadSession`，调用 `session/load`
- 实际 `session/resume` 或 `session/load` 请求失败时立即报错并保留原因，不尝试替代恢复或新会话。只有未声明两种恢复能力的既有兼容路径，才创建 `session/new` 并用 AIPP 本地 conversation 历史构建一次 history fallback prompt
- `claude-code-acp` 不支持旧的 `loadSession`，但可通过 `session/resume` 恢复已有 session
- 恢复成功后，前端会提示本次使用的是 `session/resume` 还是 `session/load`

### 会话重放抑制
- 在 `session/load` / `session/resume` 期间，抑制 ACP `session/update` 通知
- 避免重放内容污染 UI/数据库
- 加载完成后恢复正常事件通知

### 会话元数据同步
- `AcpSessionState` 除了保存 session handle，还保存当前 session 的前端快照
- 同步的内容包括：`session_id`、title、`updated_at`、config options、Plan、可用命令、当前是否有活跃 prompt
- ACP 的 `SessionInfoUpdate` 会同步更新本地 conversation 标题，并发出统一的 `title_change` 事件

### 会话配置
- ACP 会话控制 UI 以 `session_config_options` / `configOptions` 为唯一配置来源
- `configOptions` 来自 `session/new`、`session/load`、`session/resume` 或后续配置更新，不来自 `initialize`
- `category=model`、`category=mode`、`category=thought_level` 会优先显示为模型、运行模式、思考强度配置
- Agent 助手配置与新对话界面会启动一次短生命周期 ACP 探测会话，从 `session/new` 的 `category=model` / `category=thought_level` 选项读取真实可用模型和思考强度；探测会话会按 Agent 能力调用 `session/delete` 或 `session/close` 清理
- 新对话选中的模型和思考强度会在首条 prompt 前通过 `session/set_config_option` 应用；Agent 未返回对应配置、拒绝该值或更新失败时会显示底层协议错误并停止，不会静默使用其他模型
- Agent 未提供协议模型目录时，仍可显示该提供商已录入的数据库模型；协议探测和数据库都没有模型时，界面会报告包含命令、工作目录及 stderr 的发现错误
- 用户修改配置时调用 `session/set_config_option`
- 成功后使用 Agent 返回的完整 `configOptions` 替换本地状态，避免只在本地猜测单个选项的新值
- 不再暴露旧的 `session/set_mode` UI/命令入口

---

## 工具调用映射

### ACP 工具调用转 MCP 工具调用 UI
- ACP 工具调用转换为 MCP 工具调用 UI 事件
- 统一的工具调用展示界面
- 用户体验一致

### 工具状态映射
- ACP 工具状态映射到：pending/executing/success/failed
- 状态流转与 MCP 工具一致
- 状态图标和提示复用
- ACP `Pending` 只有在明确需要确认时映射为 AIPP 的 pending，否则按 executing 展示，以便运行中的工具显示闪亮边框
- ACP 后续状态更新没有携带新结果时，会保留已保存的工具结果，避免成功后自动收起再展开时结果丢失

### 工具调用事件
- 工具调用事件发送到前端
- UI 实时更新工具状态
- 工具参数和结果展示

---

## 文件/终端操作桥接

### 文件读写操作桥接
- ACP 文件读/写操作桥接到内置操作
- 通过权限管理器进行权限控制
- 操作请求转发到内置文件操作模块

### 终端命令执行桥接
- ACP 终端命令按结构化字段执行：`command` 作为程序名，`args` 作为参数数组
- 支持 `cwd`、`env`、`outputByteLimit`
- 输出超过 `outputByteLimit` 时从开头截断，并保持 UTF-8 字符边界合法

### 权限管理器集成
- 与现有的权限管理器集成
- 权限请求对话框
- 权限决策持久化

### 权限请求审批
- ACP 权限请求会通过现有审批流发到前端
- 后端 `confirm_agent_interaction` 统一处理 Agent 权限、elicitation 和提问回复，并检查请求创建时的 driver、provider 实例、conversation 与 run 归属；已有 pending store 仍是请求的唯一来源
- 当前前端继续使用 `confirm_acp_permission`、`confirm_codex_permission`、`confirm_acp_elicitation` 等兼容命令，这些入口在后端转到同一回复校验流程；审批卡片格式保持兼容
- 打开申请所属会话时，输入框旁的权限面板默认展开，可收起后继续查看聊天。群成员的申请归到对应群聊。查看其他助手时，待处理申请使用窗口弹窗；两处共用审批详情与选项。
- 普通聊天回复中按 Enter 默认排队，管家私聊默认插队。按 Ctrl+Enter（macOS 也可用 ⌘+Enter）使用另一种方式。排队的消息保存在 AIPP，原生回合成功结束后发送下一条。Pi 和 Codex 可以将插队消息直接发给正在执行的回合。其他原生驱动在同一会话中断当前回合后优先发送；工具可能随中断停止。手动停止或执行失败后，待发消息可从队列手动发送。

- Butler/Feishu 场景下也会同步转发
- 当用户取消当前 ACP prompt 时，未完成的 ACP 权限请求会被一起取消

---

## 会话生命周期清理

### 原生 Codex / Claude Code

- 打开或切换到历史对话不会仅因浏览而启动 Codex / Claude Code 进程；首次发送消息时才连接，已有热会话继续复用
- 切换离开对话不会中断正在生成、等待权限或执行工具的会话
- ACP、Codex、Claude Code 的本地运行实例统一在无活跃请求且空闲 15 分钟后释放；持久化的 session/thread ID 保留，下次请求通过原生恢复能力继续
- 普通聊天、当前管家私聊和总管家主会话会保留仍在执行后台任务的 Claude Code 进程。后台命令或子代理结束、Claude 主动继续作答时，AIPP 会在原聊天中追加一条助手回复。期间的工具授权沿用该会话的审批规则。
- Codex、Pi 和 OpenCode 的原生会话仍连接时，如果运行时在原线程或会话主动开始下一轮，AIPP 会在上述聊天中另起一条助手回复，沿用该会话的工具审批。终端输出或子会话完成本身不会触发模型请求。通用 ACP 的正文与工具更新只写入当前 `session/prompt` 的回复。
- Codex 空闲释放只关闭本地 app-server 进程，不调用 `thread/archive` 或 `thread/delete`；Claude Code 空闲释放只关闭本地 stream-json 进程
- 恢复成功提示按连接代次去重：Codex 在 `thread/resume` 成功响应后确认，Claude Code 在 CLI 返回并核对原 session ID 后确认
- 删除 AIPP 对话时会停止三类本地运行实例并清理本地 session 映射；Codex / Claude Code 的 CLI 侧历史不会被隐式归档或删除

### session/close
- Agent 声明 `session_capabilities.close` 时，会话任务退出前（空闲释放/切换对话等）会先发送 `session/close`（5 秒超时）
- 失败只记录日志，进程仍按原逻辑退出

### session/delete
- 删除 ACP 对话时，`schedule_acp_session_delete` 会结束本地活跃会话、启动一次性 agent 进程按能力调用 `session/delete`，并删除本地 `acp_session` 记录
- Agent 不支持 `session/delete` 时跳过 agent 侧清理；任何失败都不影响本地对话删除

## 结构化提问（Elicitation）

- 客户端通过 `elicitation.form` 能力声明支持表单式结构化提问（依赖 SDK `unstable_elicitation` feature）
- Agent 发起 `elicitation/create` 时，前端复用 ask_user_question 的内联卡片（`AskUserQuestionCard`）渲染：JSON Schema 的文本/数字/整数/布尔/单选/多选字段转换为问题列表，提交时还原为类型化键值
- 选填字段（不在 schema `required` 中）可留空跳过；卡片取消按钮对应拒绝（decline）
- URL 模式、request 作用域和未知模式统一回复 decline 并记录原因
- 取消 prompt 或会话任务退出时，挂起的 elicitation 请求会随权限请求一起取消

---

## 配置输入

### 支持的 ACP CLI
- `claude-code-acp`（`@zed-industries/claude-code-acp`）支持 Bun 一键安装。环境检测实际启动适配器，通过 ACP `initialize` 读取版本，无需新建会话。npm、pnpm 和 Bun 安装都可检测，自动更新与 Codex 使用相同的安装来源核验。
- `gemini`：Gemini CLI 原生支持 ACP，需用户自行安装
- `kimi`：在「ACP」中选择 Kimi Code CLI。检测和会话启动按相同顺序查找程序，先查 PATH，再查用户安装目录（含 `~/.kimi-code/bin`）。首次使用需在 Kimi 中完成 `/login`。AIPP 自动运行 `kimi acp`，可在助手或新对话中选择 Kimi 通过 `session/new` 返回的模型与 Thinking 选项。
- `dsh-acp-server`：DeepSeek Harness 的 ACP 插件，需 Node.js >= 22 并手动安装 `@deepseek-ai/dsh` 与 `dsh-acp-server`，模型与密钥在 dsh 中配置
- node-shebang 脚本类 CLI（如 npm/bun 全局 bin）统一通过显式 node 运行时启动，保证 Windows 可用
- 启动失败时会按 CLI 给出对应的安装提示

### ACP CLI 命令配置
- 从 `llm_provider_config` 读取 ACP CLI 命令
- 从 `assistant_model_config` 读取助手覆盖配置
- 提供商默认配置 + 助手覆盖配置

### 工作目录配置
- 可配置 ACP 工作目录
- 助手配置界面支持通过文件夹选择器选择工作目录，也可手动编辑路径
- 助手设置或继承的工作目录也是 AIPP 内置文件工具的可信工作区。读取、编辑、列目录以及方案比选读取本地候选时，目录内的文件通过路径检查。两处均留空时，启动默认使用的用户主目录不会自动获得文件授权。
- 影响相对路径解析
- 工作目录查询与启动共用解析：助手覆盖值优先，其次继承提供商配置，未设置时使用系统用户主目录。Claude Code、ACP 与其他原生 Agent 使用路径库展开 `~`、`~/project`（Windows 也支持 `~\project`），不展开环境变量或执行 shell；`~其他用户名` 按原样路径处理。Codex 保持原样路径语义。主目录不可用且配置需要它时返回明确错误，不改用当前目录。

### 环境变量配置
- 可配置环境变量
- 环境变量传递给 ACP 进程
- 支持多环境变量配置

### 额外参数配置
- 可配置额外 CLI 参数
- 参数追加到 ACP 命令
- 灵活的命令定制

### CLI 路径解析
- ACP CLI 按以下顺序解析：
  1. 绝对路径
  2. `~/.bun/bin` 目录
  3. `PATH` 环境变量查找
  4. 原始命令直接使用

---

## 其他功能

### 提示流
- 每个新用户请求创建新响应消息
- ACP 流式输出内容到该消息
- 发出 `message_update` 事件
- 内容持久化到数据库
- 初始化时会显式声明客户端能力：`fs/read_text_file`、`fs/write_text_file`、`terminal/*`
- ACP prompt 会按 Agent 的 prompt capabilities 转换内容块；图片能力不支持时会阻止发送并提示用户，文档内容在支持 `embeddedContext` 时以 Resource 发送

### 取消行为
- `cancel_ai` 对 ACP 会话发送 `session/cancel`
- 正常取消不会 tear down 整个 ACP 进程
- 只有 session 真正退出时才会清理 `AcpSessionState`

### 会话控制 UI
- Chat/Butler 的对话标题栏会显示 ACP 会话入口
- 用户可以查看当前 session 状态、工作目录、config options 和可用命令数量
- Codex、Claude Code 与支持 mode config option 的 ACP Agent 会在输入框下方提供「Plan 模式」开关（迷你 Switch + 文字，悬停提示含义，不额外显示说明文字）；普通助手和不支持 Plan 的 Agent 不显示这一行。回复进行中或切换中开关不可用。新对话页在发送首条消息前即可切换；已有对话在会话连上后也会显示；切换到普通助手或其他会话时立即隐藏，迟到的上一个 Agent 运行时信息/自动连接结果会被丢弃。切换失败时保留原模式并展示底层错误，不降级到其他通道
- Agent 返回明确的完整 Plan 正文后，聊天流以独立 Plan 消息展示 Markdown 内容，并在最新已完成 Plan 上提供“继续完善”“开始执行”操作；开始执行会先切回执行模式，再发送确认后的执行请求。右侧“规划”仅展示运行步骤（Todo），不再重复 Plan 正文
- 右侧栏统一命名为“规划”：Agent 对话只投影当前 Agent Plan 的步骤，普通对话只展示 AIPP Todo，不拼接两种来源，也不显示来源标签
- 聊天窗口里的 Agent 对话，右侧“详情”最顶上有一块工作区面板（普通助手、管家不显示），下方「规划」标题上方有一条分隔线把它和面板分开。Git 相关行只在 Agent 工作目录是 Git 仓库时出现，读取失败时显示具体原因并可重试：
  - 当前分支（领先/落后上游时显示 ↑N ↓M）：点开是可筛选的分支下拉（本地/远程分组），点选即切换；远程分支没有同名本地分支时自动建立跟踪分支。回复进行中不可切换，git 拒绝（如未提交改动冲突）时原样展示 git 的报错
  - 提交：有未提交改动时展开内嵌表单，列出待提交文件与行数、提交信息输入框，可点「让 Agent 生成」——在对话里发一轮请 Agent 起草提交信息，结束后把回复填进输入框（可修改）；再选「提交」或「提交并推送」（`git add -A` 后提交）。只剩本地提交时显示「推送 N 个提交」。提交成功但推送失败会单独提示，不把提交当成失败
  - 改动：对比上游（没有上游时对比 HEAD）的增删行数，含未提交与未跟踪文件
  - 压缩：短按钮「压缩」，和以后的同类操作并排，上面不加分隔线。Claude Code（CLI 自带 `/compact`）、Codex（app-server `thread/compact/start`）以及在可用命令里声明了 `compact` 的 ACP Agent 可用，其它 Agent 显示“不支持”。在输入框直接发送 `/compact` 效果相同；压缩作为一轮对话显示，完成后回复“上下文已压缩（压缩前约 N tokens）”
- 支持直接在标题栏更新 ACP config option
- 可用 ACP 命令会进入输入框 `/` 建议，并以 ACP 标识区分 AIPP 自有命令

### 已知限制
- `loadSession` 支持因代理而异
- 部分代理只支持 `session/resume`，不能通过 `session/load` 回放历史消息
- 会话持久化仅在代理支持 `loadSession` 或 `session/resume` 时有效
- 聊天弹窗和会话详情只在当前驱动明确不支持 load 和 resume 时提示无法恢复；能力未知时不提示。Kiro 以当前连接的握手结果为准。通用 ACP 在两种方式都未声明时可使用本地历史构建提示词，已声明的恢复操作失败时会报告实际错误。

---

## Codex MCP 桥接注入

- Codex app-server 通道通过 `thread/start` / `thread/resume` 的 `config` 覆盖（`mcp_servers.aipp.*`）把 AIPP 桥（`--aipp-acp-mcp-bridge`）注册为 `aipp` MCP server，以此挂载助手选中的 MCP 工具
- 工具执行经由与 ACP 桥相同的 TCP 代理和权限路径回流
- Codex 的审批人优先使用助手或提供商的显式配置，两者均留空时遵循 Codex 自身配置。恢复旧会话时，AIPP 向本次启动的 Codex 读取当前工作目录的生效配置，再将审批人传给恢复请求；旧回合保存的审批人不会覆盖当前设置。Codex 发给 AIPP 的命令、文件、权限和 MCP 工具审批，在管家会话或已开启模型审核的普通会话中先交给 AIPP 审核。自动批准只使用原生请求提供的“本次允许”。需要人工决定时，窗口会显示理由、命令和目录，可展开原始请求。MCP 工具审批直接展示工具名和本次实参；管家历史搜索显示搜索词和传入的助手编号。审核超时时显示等待时间，模型和错误信息可在“审核详情”中查看。重新打开管家窗口可以恢复当前进程中尚待处理的 Codex 审批。
- Codex 发出的 MCP 工具审批（`mcpServer/elicitation/request`）显示在现有权限卡片中，可本次允许、拒绝或取消；不会因为协议未处理而误报为“用户拒绝”，也不会隐式保存长期允许。停止会撤销当前运行的请求。
- 同一协议的普通 form 表单复用现有提问卡片，保留字段类型和选项值。URL 等未支持的交互模式明确报出协议、模式和请求标识，不自动接受或切换通道。
- 选中工具负载是 session 签名的一部分：绑定变更会重建 session，同时恢复同一线程

---
相关源码:
- `src-tauri/src/api/ai/acp.rs` - ACP 集成主模块
- `src-tauri/src/state/activity_state.rs` - 活动状态管理


新建或通过总管家显式设为动态 MCP 的 Agent 助手，通过原生桥接提供目录加载与 `call_mcp_tool` 入口，外部工具按需加载并遵循现有助手启停、连接与审核约束。旧助手保留手选 MCP 的行为，切换为动态模式后后续请求按新工具快照建立或复用会话；不会更换执行通道。新助手默认启用创建时从 `.agents` 发现的 Skills。总管家可查询及修改当前通道支持的 cwd、Codex 沙箱、审批策略/审批人及适用的工具审核参数，并能清除助手覆盖恢复 provider/通道默认，详见 [配置管理](./18-Butler任务系统.md#通过总管家管理助手配置)。
