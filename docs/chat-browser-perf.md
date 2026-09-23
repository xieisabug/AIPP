# Chat 浏览器对照测试指南

用于在本机浏览器中复现长对话、工具卡片、代码和 Preview Code 的渲染负载。
复用真实消息组件、消息过滤和 Virtuoso 列表，使用持久化样本保证跨浏览器数据一致。
这是离线渲染测试，不是完整应用或 Tauri/Electron 性能结论。

## 准备

- 仓库前端依赖已安装；Node.js 22.16 或更新版本。
- 自动化只调用本机已安装的 Chrome 或 Edge，不下载浏览器。
- 自动化库独立配置在 `scripts/chat-perf/package.json`。缺少时执行：

```powershell
npm install --prefix scripts/chat-perf --ignore-scripts --package-lock=false
```

不要运行 `playwright install`。手动使用 Safari/Chrome 测试不需要自动化库。
后续命令均在仓库根目录执行，构建和测试串行运行。

## 导出本地样本

暂停正在生成的对话后执行：

```powershell
node scripts/chat-perf/export.mjs
```

脚本以只读事务查询当前平台的 `conversation.db` 和 `mcp.db`，不修改数据库。
也支持指定数据库目录和导出目录：

```powershell
node scripts/chat-perf/export.mjs "数据库目录" "tmp/chat-perf/fixtures"
```

按预览数、脚本预览数、工具数、可见消息数、文本量、代码标记、图片标记、
表格/公式标记、工具结果体积、思考长度和失败工具记录分别选取高负载样本，
再补长对话与普通基线。标记统计是启发式筛选，不保证对应内容最终可见。
保存完整消息、工具参数与结果、附件元数据，不截断原始内容。

| 本地文件 | 用途 |
| --- | --- |
| `tmp/chat-perf/fixtures/manifest.json` | 入选原因、统计、文件名、SHA-256 |
| `tmp/chat-perf/fixtures/ranking.json` | 全部对话的统计 |
| `tmp/chat-perf/fixtures/conversation-*.json` | 原始测试内容 |

两个数据库是各自独立的读取快照，因此应在应用空闲时导出。

## 构建并启动

```powershell
npx tsc -p scripts/chat-perf/tsconfig.json --noEmit
npx vite build --config scripts/chat-perf/vite.config.ts
npx vite preview --config scripts/chat-perf/vite.config.ts
```

最后一条命令保持运行。访问 `http://127.0.0.1:4179`，使用的是生产构建，
不使用开发模式或 HMR。端口占用时给 preview 加 `--port 4180`。

## Windows 自动化

在另一终端依次执行，两种浏览器不要并行跑：

```powershell
node scripts/chat-perf/verify.mjs --headed --channel=chrome
node scripts/chat-perf/verify.mjs --headed --channel=msedge
```

`--headed` 使用可见窗口；缺少该参数时使用本机浏览器的无头模式。
运行期间不要切换、最小化或操作被测窗口。使用临时浏览器配置，不接管个人标签页。
默认逐个运行 manifest 中的样本，窗口为 1440×1000 CSS 像素，每个方向滚动 5 秒。
每轮输出到独立目录：

```text
tmp/chat-perf/results/<平台>-<浏览器>-<模式>-<时间戳>/
  results.json
  case-<ID>-<轮次>.png
  mobile.png
```

进一步复测可用 `--repeats=3 --seconds=15`；只测部分案例可用
`--cases=123,456`，ID 从自己的 manifest 中选取。
第一轮标记为 first-scroll，后续轮次标记为 warm，比较时分开看。
页面重载只能重置页面缓存，不能当作彻底清空浏览器和系统缓存。
端口变更时，PowerShell 可设置 `$env:CHAT_PERF_URL = 'http://127.0.0.1:4180'`。

## Mac 对照

把同一份 fixtures 复制到 Mac 上相同版本的仓库，按上述步骤构建。
分别在 Safari 和 Chrome 中选择同一案例，保持窗口、缩放、刷新率、
电源模式、思考展开状态和时长一致。关闭开发者工具并保持标签页前台。
先预热一次，再记录至少三次；使用下载按钮保存 JSON。
Safari 需要手动操作，当前自动化脚本只支持本机 Chrome/Edge。

也可以把 `tmp/chat-perf/site` 整个目录与 `scripts/chat-perf/serve.mjs`
一起复制到 Mac，在该目录运行 `node serve.mjs .`。这条路径只需 Node，
不需要 Rust、数据库或 npm 依赖。端口可用 `node serve.mjs . 4180` 指定。

## 比较与判断

```powershell
node scripts/chat-perf/compare.mjs "第一份results.json" "第二份results.json"
```

工具按样本哈希、浏览器、窗口参数、测试模式、思考状态、时长和缓存轮次分组，
排除不完整记录并计算中位数。比较同一机器、相同样本和条件的组：

- `p95FrameMs`：大部分时间是否平稳。
- `worstFrameMs`：是否有孤立长帧；单次最差值不能当作稳定的浏览器差距。
- `rowHeightDrift` 和滚动范围：动态内容是否导致布局变化。
- `previewCoverage`：是否实际挂载预览；同一预览重新挂载可能重复计数。
- 空白指标：基于消息矩形覆盖率，是几何信号，不是截图像素级白屏判断。
- 无滚动区域、后台标签页、未适配命令或运行时错误不能算正常性能通过。

帧指标来自 requestAnimationFrame，丢帧估算参考 60 Hz，不是 GPU 合成器轨迹。
Rust 高亮被统一替换为 Highlight.js；真实 IPC、模型生成、原生 Agent 活动重建、
插件加载和完整应用后台负载未覆盖。Safari 也不完全等于 WKWebView。

## 敏感数据边界

只提交工具代码和说明。`tmp/` 已被 `.gitignore` 忽略，以下均保留在本地：
真实聊天样本、manifest/ranking、截图、结果 JSON、报告、静态站和压缩包。
结果中也包含会话名称或错误信息，不能因为叫“测试结果”就当作已脱敏。
不要使用 `git add -f` 提交这些文件，不要把静态站公开托管。

提交前可检查：

```powershell
git check-ignore -v tmp/chat-perf/fixtures/manifest.json
git diff --cached --name-only
```
