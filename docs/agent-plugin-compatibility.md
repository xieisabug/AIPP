# OpenAI GitHub 插件兼容清单

依据固定提交 [`d420d13051bffe4543ed39e30fa46b87148e47dd`](https://github.com/openai/plugins/tree/d420d13051bffe4543ed39e30fa46b87148e47dd)，共 62 个顶层包。清单由实际 ZIP 内容盘点；它区分运行机制支持和真实服务账号验收。

## 已接入的运行能力

- stdio 使用原始参数数组、插件工作目录、ROOT 变量和环境变量启动；发现与执行走相同配置，环境变量输入进入加密 Vault。Windows npm/npx 使用本机 Node 的 JS CLI，避免 shell 字符串拼接。POSIX 启动脚本需要 Git Bash 等相应解释器。
- HTTP 连接支持公开 OAuth、用户显式提供 Bearer Token 或逐项填写清单请求头。OAuth 可提供自己的 client ID、Secret POST/Basic 和固定 loopback 回调；不会复用上游的 client secret，也不会在授权失败后自行换认证方式。
- `commands/*.md` 进入技能/斜杠索引，用户附加的文本作为命令参数。`agents/*.md` 可按用户选定的普通助手或原生 Agent 模板创建独立助手，继承模型、执行通道、工作区和工具权限。
- `agents/openai.yaml` 中的公开 MCP 依赖加入可连接服务，CLI 依赖显示运行条件。MCP form elicitation 接入现有对话表单确认界面，取消时清理待办。
- 当前固定仓库没有 hooks 文件。没有实现或宣称支持仓库外任意 hook 协议。

## 平台连接器 ID 无法直接复用的部分

下面的包没有为这些依赖提供独立 MCP 地址。不能把 ChatGPT 注册 ID 当 API 地址使用。这证明的是仓库中的接入入口不可移植，并不表示相关服务没有其他公开 API；若另写服务适配器，需要独立的 API、授权和功能对齐。

| 包 | 仓库仅提供平台连接器 ID 的依赖 |
| --- | --- |
| adobe | app-69312da8e4dc81919370cb86fd172b6c |
| data-analytics | teams, outlook_email, outlook_calendar, databricks, bigquery, snowflake, sharepoint, statsig, sites, hex, deepnote, amplitude, mixpanel, omni-analytics, metabase, thoughtspot |
| github | github-enterprise |
| lovable | lovable |
| openai-developers | openai-platform |
| outlook-calendar | outlook-calendar |
| outlook-email | outlook-email |
| public-equity-investing | pitchbook, factset, morningstar, lseg, s-p, third-bridge, daloopa, quartr, alpaca, outlook_email_connector, sharepoint_connector, teams_connector |
| sharepoint | sharepoint |
| teams | teams |

## 环境、授权和界面条件

- Build iOS Apps 的 xcodebuildmcp 需要 npm、macOS 和 Xcode；Build macOS Apps 同样依赖 macOS/Xcode。这些不是 ChatGPT 私有接口。
- Codex Security 的原始启动器和运行时代码依赖 Node、Python、Codex CLI/app-server 及相应账号权限。AIPP 不替换其 Codex 执行通道；尚未执行真实扫描验收。
- GitHub/Gmail/Google Calendar/Google Drive 等清单包含固定或占位 OAuth 客户端配置。需要服务方允许 AIPP 使用的注册信息或用户显式配置服务接受的 Token。已支持公共客户端、Secret POST/Basic 和固定 loopback 回调；服务商仍须接受该注册与授权范围。
- Data Analytics/Creative Production 的 MCP 工具与返回数据可通过标准 MCP 调用。返回内容中的 ChatGPT Apps 专属交互组件，不等于已经拥有相同的 ChatGPT 宿主 UI；需分别确认标准结果和嵌入式交互，不把工具发现成功写成完整界面验收。
- 每个外部 HTTP 服务仍需真实账号验证授权、工具和工作区权限。插件内脚本依赖的解释器、SDK、浏览器或其他宿主工具须在本机具备；缺失时明确报错。

## 逐包盘点

HTTP/stdio 表示已接入的传输机制，不代表该服务已用真实账号验收。命令数与助手数只计顶层对应目录的 Markdown 定义，不计测试夹具和模板。

| 包 | 声明的 MCP 传输 | 命令 | 助手定义 | 仅平台 ID 的依赖 |
| --- | --- | ---: | ---: | --- |
| adobe | 无独立声明 | 0 | 0 | 有，见上表 |
| airtable | http | 0 | 0 | 无 |
| atlassian-rovo | http | 0 | 0 | 无 |
| boltz-api-cli | 无独立声明 | 0 | 0 | 无 |
| build-ios-apps | stdio | 0 | 0 | 无 |
| build-macos-apps | 无独立声明 | 3 | 0 | 无 |
| build-web-apps | 无独立声明 | 0 | 0 | 无 |
| build-web-data-visualization | 无独立声明 | 0 | 0 | 无 |
| canva | http | 0 | 0 | 无 |
| chatcut | 无独立声明 | 0 | 0 | 无 |
| circleci | 无独立声明 | 0 | 0 | 无 |
| clickup | http | 0 | 0 | 无 |
| cloudflare | http | 2 | 0 | 无 |
| coderabbit | 无独立声明 | 0 | 0 | 无 |
| codex-security | stdio | 0 | 0 | 无 |
| consensus | http | 0 | 0 | 无 |
| creative-production | stdio | 0 | 0 | 无 |
| data-analytics | stdio | 0 | 0 | 有，见上表 |
| datadog | http | 0 | 0 | 无 |
| dropbox | http | 0 | 0 | 无 |
| expo | 无独立声明 | 1 | 0 | 无 |
| figma | http | 4 | 4 | 无 |
| game-studio | 无独立声明 | 0 | 0 | 无 |
| github | http | 0 | 0 | 有，见上表 |
| gmail | http | 0 | 0 | 无 |
| google-calendar | http | 0 | 0 | 无 |
| google-drive | http | 0 | 0 | 无 |
| granola | http | 0 | 0 | 无 |
| higgsfield | http | 0 | 0 | 无 |
| hyperframes | 无独立声明 | 0 | 0 | 无 |
| life-science-research | 无独立声明 | 0 | 0 | 无 |
| linear | http | 0 | 0 | 无 |
| lovable | 无独立声明 | 0 | 0 | 有，见上表 |
| magicpath | 无独立声明 | 0 | 0 | 无 |
| mixpanel-headless | 无独立声明 | 0 | 0 | 无 |
| monday-com | http | 0 | 0 | 无 |
| ngs-analysis | 无独立声明 | 0 | 0 | 无 |
| notion | http | 0 | 0 | 无 |
| nvidia | 无独立声明 | 0 | 0 | 无 |
| openai-ads-conversions | 无独立声明 | 0 | 0 | 无 |
| openai-developers | stdio | 0 | 0 | 有，见上表 |
| outlook-calendar | 无独立声明 | 0 | 0 | 有，见上表 |
| outlook-email | 无独立声明 | 0 | 0 | 有，见上表 |
| plugin-eval | 无独立声明 | 0 | 0 | 无 |
| posthog | http | 0 | 0 | 无 |
| product-design | 无独立声明 | 0 | 0 | 无 |
| public-equity-investing | 无独立声明 | 0 | 0 | 有，见上表 |
| remotion | 无独立声明 | 0 | 0 | 无 |
| sentry | http | 0 | 0 | 无 |
| sharepoint | 无独立声明 | 0 | 0 | 有，见上表 |
| shopify | http | 0 | 0 | 无 |
| slack | http | 0 | 0 | 无 |
| stripe | http | 0 | 0 | 无 |
| supabase | http | 0 | 0 | 无 |
| superpowers | 无独立声明 | 0 | 0 | 无 |
| teams | 无独立声明 | 0 | 0 | 有，见上表 |
| temporal | 无独立声明 | 0 | 0 | 无 |
| test-android-apps | 无独立声明 | 0 | 0 | 无 |
| twilio-developer-kit | 无独立声明 | 0 | 0 | 无 |
| vercel | http | 6 | 3 | 无 |
| zoom | http | 26 | 2 | 无 |
| zotero | 无独立声明 | 0 | 0 | 无 |

## 本轮验证记录

- Rust 定向回归 54 项通过，3 项需显式启用；另行启用完整上游 ZIP 与真实 stdio 测试，并验证结构化结果保留，3 项通过。
- 原始 Creative Production、Data Analytics、OpenAI Developers Node 进程实际启动并完成 MCP 初始化、工具发现。Data Analytics `render_table` 返回真实结构化数据；OpenAI Developers 拒绝确认后没有创建密钥。
- 前端 TypeScript/生产构建、5 个文件共 16 项界面测试及最终 cargo check 通过。真实账号 OAuth、应用中的表单交互、原生 Agent 完整工作流、macOS/Linux 和 ChatGPT 嵌入式组件尚未实机验收。
- stdio 沿用每次调用建立进程的生命周期；依赖跨调用进程内状态的服务仍需单独适配，不能据此宣称完整宿主兼容。
