# UI 性能问题调试方法（以 Chat 滚动为例）

## 目的

这份文档只记录**调试方法**，不记录某一次问题的复盘。  
目标是把性能问题固定成一套可重复动作：

`复现 -> 采样 -> 分流 -> 单点修改 -> 回归`

这套方法以后排查别的 UI 性能问题也可以直接复用。

---

## 核心原则

### 1. 先让问题能稳定自跑

不要先靠手工拖动、主观感受、临时猜测。  
先把问题变成可以自动执行、自动出结果的流程。

### 2. 一次只改一类东西

每次只改下面的一类：

- 高度估算
- mount 初始化成本
- 滚动时的每帧工作量
- 异步内容导致的二次布局

不要一次混改很多点，不然很难知道哪一项真正有效。

### 3. 永远保留两组场景

- 一个**普通场景**，用于防回归
- 一个**重场景**，用于稳定复现问题

以后排查别的性能问题，也建议保留这种“双样本”方式。

### 4. 先看信号，再改代码

先判断问题属于哪一类，再去看对应代码。  
不要一上来就改虚拟列表、改 overscan、改缓存。

---

## 标准流程

### 第 1 步：先确认当前代码能构建

```bash
npm run build
npm run test
cargo check --manifest-path src-tauri/Cargo.toml
```

### 第 2 步：跑自带基准

当前 Chat 滚动问题已经有现成自跑入口：

```bash
AIPP_CHAT_SCROLL_PERF_AUTORUN=1 \
AIPP_CHAT_SCROLL_PERF_INDEX=<对话索引> \
AIPP_CHAT_SCROLL_PERF_TIMEOUT_SECS=60 \
AIPP_CHAT_SCROLL_PERF_RESULT_PATH=tmp/chat-scroll-perf-result.json \
cargo run --manifest-path src-tauri/Cargo.toml --features custom-protocol
```

说明：

- `AIPP_CHAT_SCROLL_PERF_INDEX` 从 `0` 开始
- 结果会写到 `tmp/*.json`
- 如果以后排查别的 UI 问题，没有现成入口，就按同样模式补一个：
  - 前端暴露测试 API
  - 前端上报 progress
  - Rust 侧 autorun 触发并写结果

### 第 3 步：按平台选择自动化方式

#### Windows

- **界面自动化**：优先用官方桌面 WebDriver 路线：
  - `tauri-driver`
  - `msedgedriver.exe`
- **适合做**：
  - 点击、输入、切窗口、截图
  - 端到端功能回归
- **数据采集**：
  - 用 WebDriver 驱动操作
  - 用应用内 API / `execute script` 读状态
  - 更稳定的性能数据仍建议由应用自己写 JSON

#### Linux

- **界面自动化**：优先用官方桌面 WebDriver 路线：
  - `tauri-driver`
  - `WebKitWebDriver` / `webkit2gtk-driver`
- **适合做**：
  - 功能回归
  - WebKitGTK 平台专项交互测试
- **数据采集**：
  - 和 Windows 一样，操作可以走 WebDriver
  - 性能指标、阶段进度、结构化结果最好仍由应用自己输出

#### macOS

- **界面自动化**：
  - 官方 Tauri 桌面 WebDriver 路线当前不适用 `WKWebView`
  - 默认优先用**应用内自跑 harness + Rust autorun**
- **适合做**：
  - 性能问题复现
  - 自动切换窗口 / 对话 / 场景
  - 稳定采集结构化指标
- **数据采集**：
  - 前端暴露测试入口
  - 前端上报 progress / 自定义指标
  - Rust 侧触发并把结果写到 `tmp/*.json`
- **可选方案**：
  - 如果以后需要 macOS 全量 UI 自动化，可评估 debug/test-only 的嵌入式 WebDriver 方案，如 `tauri-plugin-webdriver`
  - 或 CrabNebula 的 `tauri-plugin-automation` + `@crabnebula/tauri-driver`
  - 这类能力只能用于测试构建，不能进入生产包

#### 平台选择建议

- **功能正确性 UI 回归**：
  - Windows / Linux 优先 WebDriver
  - macOS 视能力选嵌入式 WebDriver 或应用内脚本入口
- **性能问题和数据采集**：
  - 三个平台都建议优先补**应用内 harness**
  - 因为结构化指标、阶段事件、业务侧自定义采样更稳定，也更容易横向对比

### 第 4 步：先看 4 类信号

#### 1. progress phase

用于判断卡在哪个阶段：

- `start`
- `conversations-loaded`
- `conversation-selected`
- `messages-rendered`
- `probe-start`
- `probe-complete`

#### 2. frame time

重点看：

- `averageFrameMs`
- `p95FrameMs`
- `worstFrameMs`
- `estimatedDroppedFrameCount`

#### 3. scroll range drift

重点看：

- `maxScrollTop`
- `finalMaxScrollTop`
- `minObservedMaxScrollTop`
- `maxObservedMaxScrollTop`

#### 4. row height drift

重点看：

- `rowHeightDrift`

如果某一行在 mount 后还持续变高 / 变矮，虚拟列表通常就会出现滚动条跳动。

---

## 怎么根据结果分流

### A. 超时停在 `messages-rendered`

说明问题多半出在**初始渲染 / 初始化**，优先查：

- 重组件 mount
- 脚本执行
- 代码高亮
- Shadow DOM / iframe 初始化
- 大块内容首屏完整展开

### B. `rowHeightDrift` 很大

说明问题多半出在**异步内容改高度**，优先查：

- 图片 / 预览 / 代码块后续长高
- `preview_code` / MCP 卡片 / 富内容预览
- `ResizeObserver` 连锁更新
- 估高策略和真实高度偏差过大

### C. `maxScrollTop` 波动很大

说明问题多半出在**总高度估算不稳定**，优先查：

- 虚拟列表 estimated size
- 最后一条消息或超高消息的估高
- 列表尾部特殊容器

### D. `p95` 高，但 drift 不明显

说明问题更像是**每帧工作量太重**，优先查：

- 滚动事件回调过多
- 可见区域渲染过多
- 重绘 / 合成成本高
- 进入视口时才触发的重逻辑

### E. 只有 `worstFrameMs` 特别高

说明可能不是持续卡，而是**少量孤立 spike**。  
优先找：

- 某一个超重组件
- 某一次性初始化
- 某个进入视口时才执行的逻辑

---

## 改动顺序建议

推荐按下面顺序处理：

1. 先解决“能不能稳定复现和采样”
2. 再解决“是不是某一行高度在漂”
3. 再解决“初始化是不是太重”
4. 最后再调滚动参数、overscan、节流、缓存

原因很简单：

- 如果高度还在漂，滚动条就会跳
- 如果初始化已经卡住，调滚动参数通常没用

---

## 每次修改后的固定回归

每次改完都做同一套动作：

1. `npm run build`
2. `npm run test`
3. `cargo check --manifest-path src-tauri/Cargo.toml`
4. 重跑普通场景
5. 重跑重场景
6. 对比 JSON

重点只看这几件事：

- 问题是否真的改善
- 是否引入新的 timeout
- 是否把普通场景搞差
- 是否把展示逻辑改坏

---

## 当前这套方法对应的代码入口

- `src/utils/chatScrollPerf.ts`
  - 滚动采样逻辑
- `src/windows/ChatUIWindow.tsx`
  - 前端测试入口
  - progress 上报
  - 行高漂移采集
- `src-tauri/src/lib.rs`
  - autorun 触发
  - 结果写盘

如果以后排查别的窗口、别的性能问题，优先仿照这三个入口补新的自跑链路。

---

## 一句话原则

**先把问题做成“能自动得到结果”，再开始优化。**  
这比一开始就猜原因更重要。
