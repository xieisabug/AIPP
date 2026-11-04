# 移动导航与交互（LeaderAI #297 #291 #292 #293 #306）

## 2026-10-31
- 已读取五项任务详情、移动规范，核对租户 xie / 2；#297 进入 in_progress。
- 初始工作区仅 tmp/worklog/mobile-optimization.md 有其他人的修改，保留。
- #297：新增 MobileShell 保留已访问页面挂载、白名单导航及返回栈；移除 __setAppWindow；Android 返回先处理 IME，再调用页面返回，根页退后台且不进入 WebView 历史。
- 验证尚待执行；后续补充路由/状态回归测试、按会话草稿与阅读位置检查，然后处理消息操作、配置、制品预览。
- 用户授权验收后按独立功能小提交，只提交已审核的相关文件。
- #291：常驻更多入口、移动全屏消息编辑、44px版本按钮、回到底部、键盘不抢滚动；草稿按会话保存。路由保存滚动位置，隐藏页跳过 smartScroll。
- #292：移动全屏对话框/单列、保留已访问设置分类；HTTP MCP 地址校验和密钥遮罩；复用刷新能力作为连接测试；助手调用 #301 提供的能力接口决定只读兼容性。
- #293/#306：新增移动收藏列表和隔离静态预览；React/Vue browser-only runtime 随应用构建（React已有版本、Vue3.5.22），明确模块白名单和无宿主API，未改变桌面运行通道。
- 首轮 npm run build 通过，产出 previewRuntime 独立约3.25MB bundle；导航/消息9项及草稿3项测试已通过。静态安全/真实浏览器/Android仍待验证。
- pnpm add 曾触发无关传递依赖升级；锁文件已缩减为仅新增 Vue 的 importer/packages/snapshots，保留原有其他锁定版本。

- 验证更新：本轮9个测试文件27项通过；真实 Chrome + 构建产物验证 React/Vue 点击、具体错误、宿主隔离及网络拦截。首个提交 fd76973 已包含消息触控/编辑/版本组件和测试。真机 OnePlus6 用户确认空闲；等待默认 Cargo 构建锁后打包验证。
- 安全审查发现本机 Wry 0.55.1 Android Ipc.kt 使用 addJavascriptInterface，WebView document-start 脚本也注册到子帧。仅网页沙箱不足以保证原生 IPC 不可达。
- MainActivity 在首个导航前移除旧 ipc Java 接口，改用 WebMessageListener 校验 isMainFrame + 来源主机；保留独立 IPC 执行线程避免把同步命令搬到 UI 线程。缺少 WebMessageListener 的 WebView 明确拒绝初始化，需更新系统 WebView。待 Android 编译与真机越界调用验证。
- 已修快速导航/返回 ref 栈竞态，以及已排队的 smartScroll/稳定滚动回调在隐藏页执行的问题。
- 04e60aa 单独提交 Vue 固定依赖（锁文件增加106行，无其他版本升级）。

- cargo check 等待共享锁后执行，遇到其他任务的 ai_api.rs:792 E0308：queued_message_repo 返回 AppError，pause_dispatch 返回 rusqlite::Error，and_then 错误类型不一致。未修改他人该处代码；Android打包需等待其修复。
- 后续最小修复共享编译阻塞：ai_api.rs 的 pause_dispatch 错误用 ? 转换为 AppError（不纳入整块他人功能提交）。Android Kotlin 编译已通过；0407eed 已提交配置兼容性与 MCP 验证。APK 正在默认目录编译，真机确认空闲。
- 补齐验收审查：pin/follow 与 Virtuoso 初始定位均跳过隐藏移动页；同一预览返回重开保留组件状态；扩展提示词移动全屏和44px入口；外部刷新保留 dirty 助手表单并提示。新增回归待当前 Android 构建结束后串行运行。
- APK 打包成功（20:08，arm64 debug），真机覆盖安装成功且 get_platform=android、MobileShell正常。最终 npm build 通过；14文件93项 + InputArea 9项均通过（合计102项）。预览首轮固定等待过短和触摸回调延迟导致脚本误判，实际 React已渲染；正在按设备等待与原生返回重新验收。
- 真机发现实质缺陷并修复：React/Vue生成同一srcdoc导致iframe未重载，现按内容key更新且等值输入保留状态（2文件6项回归通过）。Wry的setWebView/onWebViewCreate发生在旧IPC注册之前，且默认Back回调晚于应用回调：MainActivity现禁用handleBackNavigation并在setContentView边界替换IPC（初始loadUrl仍排队），等待重新打包真机拒绝日志证明。此前子帧超时不能视为拒绝证据。2b56584 提交提示词全屏及dirty表单保护。
- 真机最终原生与预览验收通过：MainActivity可空签名修正后Kotlin100任务通过，增量APK157任务通过；主框架get_platform正常，子帧调用在AippSecurity记录Rejected IPC（20:23）。React/Vue触摸计数、返回保持、依赖/语法错误、静态Markdown/text/SVG/image/Mermaid、恶意HTML均通过。收藏16个临时项验证450.133px位置恢复与确认删除，已全部清理；预览真实触摸横纵滚动x211.91/y322.13。Android CDP截图遗漏iframe图层，原生screencap确认Vue正常可见。cargo check最终通过4.95s。
- 后续真机发现并修复：移动对话框仍受桌面 Tailwind translate(-50%,-50%) 影响，源码已强制清零并经真实 Chrome 测量验证；头部入口补44px。长对话估高原用桌面65%气泡宽度，现按移动92%估计，4文件45项通过，待新APK性能对比。HTTP MCP完整Authorization又被rmcp加Bearer，已统一连接测试/能力加载/工具执行三条路径保留原头，并补真实initialize wire测试。
- HTTP三项真实wire回归首轮通过（Bearer、Basic、非法头），补充严格JSON校验后再次验证；97e0c55 单独提交会话草稿，其他开发者叠加的本机持久化接入保留在工作区。静态预览改为未挂载template解析；真实Chrome恶意img/iframe/link/CSS外部资源监听0请求，避免解析期加载。真机搜索后抽屉残留已修为打开搜索前收起抽屉。
- 最终HTTP严格校验及真实wire三项通过；cargo check通过30.26s；前端5文件25项（含中文IME防误发送）通过。独立Chrome进一步证明原frame-src允许iframe导航localhost并触发重复postMessage；MobileShell现叠加frame-src self，组件每个iframe只发送一次源码，相关2文件7项通过。最终APK开始构建。96a1dcb 提交聊天阅读状态与回到底部。
- 真机补测：普通会话真实回复/重新生成/版本2→1和分叉通过；空助手发送明确报尚未配置模型且不回退。表格触摸横滚221.87px且页面宽度384不溢出；代码主渲染器实为RustCodeBlock，已补移动预览/44px常驻按钮，代码横滚仍在定位。模型提供商自动保存原有跨字段debounce漏存、失败无toast及刷新覆盖待保存的问题已修，并补慢刷新/保存成功竞态保护回归，待当前APK结束后验证。
- 2026-11-01继续：上轮APK构建marker0；新增provider保存竞态+RustCodeBlock测试2文件9项通过。真机代码横滑失败为手势落在复制按钮，改从内容中部实际触摸scrollLeft=136.18px，无需修改滚动CSS。修复后真机MCP正确认证通过，错误认证明确Auth required，恢复后能力1工具/0资源/0提示词。普通p95=16.8ms/空白0；长会话15秒单程p95=16.8ms/空白0/无反跳，默认2.2秒高速压力p95=183.7ms/空白14，压力限制如实保留。估高812px对实测827px（原1076px）。真机百度拼音候选词确认产生草稿“我的”且未发送，已清空测试草稿；Enter组合边界单测已通过。最新APK已成功构建，开始安装最终复验。

## 最终验收（2026-11-01）
- 最终 arm64 debug APK 构建成功并覆盖安装 OnePlus 6；保留原有数据。前端 build、cargo check、HTTP MCP 3项真实请求测试通过；新增提供商/代码块2文件9项、悬浮工具栏7项、导航/IME/静态预览5文件25项、iframe导航2文件7项均通过（有重复覆盖，不合计为唯一测试数）。
- #297：平台导航、快速返回、IME优先、面板优先、根页退后台/恢复、旋转、回复中设置往返、草稿/阅读位置保留。最终 header44px；全屏Prompt实际rect=(0,0,384,810.67)。桌面窄窗口导航单测与真实Chrome对话框CSS对比通过。
- #291：搜索结果后抽屉关闭；真实回复、取消、编辑重生成、版本2→1、分叉通过；最终触控复制读回内容匹配（未打印原剪贴板，原为空，已写回空文本）；百度拼音候选确认未发送，Enter组合边界单测通过。表格横滚221.87px、代码横滚136.18px，无页面横向溢出。极高速压力限制见下，不宣称压力流畅通过。
- #292：新provider32/model2从空表单创建并启用；空助手4无模型发送明确报错且不回退，显式绑定后新会话8收到本地OpenAI SSE完整回复；数据库只有1条对应用户发送。真实远端模型provider31的回复此前也已验证，二者证据分别记录。空密钥真机报HTTP401/认证失败/具体模型/流式阶段。HTTP MCP正确、错误认证与恢复加载工具通过；非法地址字段保留已验证。保存失败/外部刷新竞态以回归测试为证；真机故障注入因Tauri invoke不可写而未生效，未计为通过。
- #293/#306：最终包 DEVICE_PREVIEW_PASS、STATIC_FORMATS_AND_VUE_RESTORE_PASS；主RustCodeBlock按钮44px且触控后显示“代码入口验收”。Markdown/text/image/HTML/SVG/Mermaid、缩放、列表位置/删除/空态已覆盖。React/Vue交互/错误/返回恢复；原生子帧拒绝此前有AippSecurity日志，最终包子帧调用未成功。最终真机localhost导航攻击0服务请求且提示停止预览，父frame-src self生效。Chrome静态解析/加载恶意资源0请求。
- 性能数据：ordinary p95 16.8ms、空白0；long-touch-speed（15秒单程）p95 16.8ms、空白0、无反跳/外部拉动；long-stress（2.2秒单程，约8.6万px）p95 183.7ms、空白14，仍有极高速压力限制。估高812px对实测827px，旧估高1076px。JSON证据保留在tmp/mobile-a-nov1-perf-*.json。
- 清理：只删除本轮conversation4/5/6/7/8、assistant3/4、MCP2、collection17、provider32及其模型；更早16项收藏已清理。原assistant1/2、provider31及原会话保留。真机键盘恢复英文QWERTY，旋转设置保持1/0；收起键盘，重启到首页，停止本轮fixture PID42188，移除9237 forward及18766 reverse。模拟器未操作。
- 本轮代码已按功能小提交；共享工作区其他人的导出/任务等改动不纳入提交。平台只回写本轮五项任务，需求141及父任务296是否流转按剩余任务状态判断。
