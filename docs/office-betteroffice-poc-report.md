# BetterOffice（0.5.1）技术验证报告（LeaderAI #328）

> 验证日期：2026-11-20  
> 关联需求：LeaderAI **#328**（为 **#329** 统一预览、**#330** 选区 AI 修改闭环、**#331** 默认另存副本明确选型与能力边界）  
> 选型结论：**限定范围采用（推荐按「官方插件按需安装」模式落地，不直接打入 AIPP 主安装包）**

---

## 一、核心结论与能力边界

### 1.1 选型结论：**限定范围采用（Conditionally Adopt）**

BetterOffice `0.5.1` 的 Rust/WASM OOXML 内核在 **DOCX / XLSX / PPTX** 三格式的**结构保真、公式重算、版本校验原子编辑（`expectVersion` + `expect` 守卫）与跨端 Worker 异步执行**方面表现超出预期：
- 三格式经过「打开 → 简单编辑 → 另存副本」后，OOXML 部件级与 XML 结构比对 **37/37 项全部通过**，未丢失页眉页脚、跨页重复表头、合并单元格、跨表公式、柱状图、幻灯片母版、切换与进入动画；
- 另存副本在 **Microsoft Office 2010（Word / Excel / PowerPoint）** 与 **WPS Office 12（Writer / Spreadsheets / Presentation）** 中 **6/6 全部无报错、无修复提示打开**，DOCX 分页页数（5 页）、XLSX 重算结果与图表数量、PPTX 动画/切换数量完全一致；
- 在严格镜像 AIPP 桌面 CSP（`default-src 'self'; script-src 'self' 'unsafe-eval'; font-src 'self' data:`）且**未启用** `SharedArrayBuffer` / COOP-COEP 的环境下，桌面 Edge（WebView2 同核）与 Android API 36（Chrome 134）均实现了 **0 CSP 违规、0 外网请求**，三格式均通过同源 ES Module Worker 异步执行。

**为何是「限定范围采用」而非「无条件全量采用」**：
1. **体积极重，禁止直接全量打入主安装包**：全量内置（3 套 WASM + 拉丁字体 + 5 语种 CJK 字体）使前端 `dist/` 增加 **98.80 MB raw**（gzip **53.79 MB** / brotli **48.62 MB**；裁剪日/韩/繁中仅留简中仍为 **84.63 MB raw / 41.82 MB gzip / 36.92 MB br**），而 AIPP 当前整个前端 `dist/` 仅 **27.66 MB**。**必须采用「官方插件按需下载安装」分发策略**（见第六节）。
2. **PPTX 0.5.1 存在必须规避的 CJK 字形重叠缺陷**：当向 `<PptxEditor fonts={...} />` 同时传入拉丁字体（如 `Carlito`）与中文字体时，引擎优先按 `<a:latin>`（Calibri→Carlito）排布整段文本且不对 `<a:ea>` 自动回退，导致中文字符全部命中 `.notdef`（`glyphId=0`），宽度压缩约一半并严重重叠。**已验证有效规避方案**：通过 `api.registerFallbackFont({ family: 'Noto Sans SC', bytes })`（推荐，仅一份 8 MB 内存）或仅用含拉丁字形的 CJK 字体映射默认族，可实现 `notdefCount=0`、中文清晰无重叠。
3. **非全功能 Office 替代**：PPTX 动画/切换在编辑器画布中**静态展示不播放**（但编辑保存时 XML 完整无损保留）；DOCX 编辑态 Worker WASM 内存占用约 **97 MB**（Android JS 堆上限实测仅 527 MB），必须严格限制大文件并在关闭/切换预览时立刻卸载释放。

> **勘误（2026-11-20，#329 实现时实测）**：`registerFallbackFont` 通过编辑器命令下发，**只读模式（`readOnly`）下会被拒绝**（`The editor is read-only`），不能用于只读预览。只读预览的有效做法是在打开文件前，经 `<PptxEditor fonts={...}>` 传入 Noto Sans SC / Noto Serif SC 各一份（不做多别名复制），实测中文无重叠、无内存翻倍。

### 1.2 分格式能力边界（供 #329 / #330 / #331 直接引用）

| 格式 | 只读预览（#329） | 简单编辑与选区 AI 修改（#330 / #331） | 明确不支持 / 已知限制 |
| --- | --- | --- | --- |
| **DOCX** | **支持**：正文排版、多级标题、列表、页眉页脚与页码域、跨页表格、横/纵向合并单元格、底纹边框、简中字体（Noto Sans/Serif SC 映射微软雅黑/黑体/宋体），与 Word/WPS 分页一致（本样本均 5 页） | **支持**：`readOnly` 运行期强制拦截写操作；`findText` / `readParagraphs` / `getParagraphIdentities` / `applyEdits`（支持 `expectVersion` 与 `expect` 内容守卫，版本过期返回 `stale-version`）；支持 `proposeChanges`（修订建议） | 字体为度量兼容开源替代（非 Windows 私有宋体/微软雅黑原字库），复杂公文精准对齐仍可能有细微行距差异；移动端默认 `100%` 缩放超出屏幕宽度，需按视口自适应 `initialZoom`；复杂宏/ActiveX/旧版 `.doc` 不支持 |
| **XLSX** | **支持**：多工作表切换、合并单元格、边框填充、货币/百分比数字格式、中文文本、常见柱状图实时渲染 | **支持**：`readCells` / `applyEdits`（支持 `setCellInputs` / `setCellValues` / `setFormulas` / `patchStyle`）+ `expectVersion` 校验；编辑后引擎自动重算 `SUM` / `AVERAGE` / `IF` / `COUNTIF` / `SUMIF` 与跨表公式，并刷新图表与缓存值 | 复杂数据透视表、VBA 宏、外部链接、`.xls` 不支持；图表样式在复杂组合图下可能与 Excel 有视觉差异（但保存时 `xl/charts/*.xml` 原样保留） |
| **PPTX** | **支持（须启用 CJK Fallback 字体修复）**：多页缩略图切换、SlideMaster 母版背景/装饰条/页脚、中文文本框、基础矢量图形（圆角矩形/矩形/箭头等） | **支持**：`findText` / `applyEdits`（文本替换、格式、图形矩形/填充/描边、备注等）+ `expectVersion` 校验；保存时保留未触及的母版、主题、切换与动画 XML | **必须**在打开后调用 `registerFallbackFont` 加载中文字体，否则中文严重重叠；动画与页面切换仅**无损保留不播放**；SmartArt、复杂三维特效、音视频嵌入与 `.ppt` 不支持 |

---

## 二、测试样本与环境说明

### 2.1 原始样本清单（全程只读，SHA-256 前后一致未修改）

所有原始样本存放于 `tmp/office-poc-328/fixtures/originals/`，PoC 仅读取副本，测试前后核对 SHA-256 完全一致：

| 样本文件 | 大小 | SHA-256（测试前 = 测试后） | 覆盖的典型排版特性 |
| --- | ---: | --- | --- |
| `sample_cn.docx` | 5,249 B | `7035a5fa72d68dbeb46bbe325d01e24e76e5b53140d38a27160ca8ada0a4db6b` | 黑体/微软雅黑/宋体、Heading1/2、首行缩进、1.5 倍行距、加粗红字、斜体下划线、项目符号列表、页眉、含 `PAGE`/`NUMPAGES` 域的页脚、38 行跨页表格（含 `w:tblHeader` 重复表头、横向 `gridSpan=2`、纵向 `vMerge`、多色底纹） |
| `sample_cn.xlsx` | 6,132 B | `fb08acebee8f42f77a7a9a6ae3c1d895a9873bf325a128a6056af2eaaba18fa0` | 双工作表（`销售汇总` / `明细数据`）、合并标题 `A1:F1`、微软雅黑与边框底纹、货币 `¥#,##0.00` 与百分比 `0.0%`、公式 `COUNTIF` / `SUMIF`（跨表）/ `IF` / `SUM` / `AVERAGE` / 行内减法、簇状柱状图 `xl/charts/chart1.xml` |
| `sample_cn.pptx` | 8,302 B | `1eee3f88af7def717386fd16582e06a671a3ffd27bc3a20fc71d2a424b8d9228` | 自定义 `slideMaster1.xml`（顶部蓝色色条 + 微软雅黑母版页脚）、主题 `theme1.xml`、3 页幻灯片（标题页、含 `roundRect`/`rightArrow`/`rect` 图形页、含 `<p:transition><p:fade/>` 切换与 `<p:timing>` 淡入动画的要点页） |

### 2.2 实测环境
- **前端工程栈**：React `19.2.8` + TypeScript `7.0.2` + Vite `8.3.8`（对齐 AIPP `package.json` 的 React 19.2.8 + Vite 8.x），注入与 `src-tauri/tauri.conf.json` 一致的严格 CSP（`default-src 'self'; script-src 'self' 'unsafe-eval'; connect-src 'self'; font-src 'self' data:; style-src 'self' 'unsafe-inline'`）。
- **桌面端运行环境**：Windows 11 + Microsoft Edge `131.0.2903.99`（与 Tauri 2 Windows WebView2 同核）。
- **Android 运行环境**：Android Emulator `Medium_Phone_API_36.1`（Android API 36，Viewport `522×1029` / `dpr=2.625`，Chrome `134.0.6998.135`，同机已安装 WebView `156.0.8078.47`）。
- **外部办公软件重开环境**：
  - **Microsoft Office 2010 (Office14)**：Word / Excel / PowerPoint COM 自动化实测；
  - **WPS Office 12.1.0.29670**：KWPS / KET / KWPP COM 自动化实测。

---

## 三、样例测试矩阵与保真度核对

### 3.1 端到端链路测试矩阵

| 格式 | 打开（只读/编辑） | 简单编辑（含版本守卫） | 另存副本 | OOXML XML 结构对比 | MS Office 2010 重开 | WPS Office 12 重开 |
| --- | --- | --- | --- | --- | --- | --- |
| **DOCX** | **通过**（5 页，中文与表格排版清晰，`readOnly` 正确拒绝写操作） | **通过**（替换 `【待修改原文A】` → `【BetterOffice已修改A】`，过期版本返回 `stale-version`） | **通过**（5,218 B，原文件未改） | **18/18 通过**（0 部件增删） | **通过**（无警告，5 页，1 表 38 行，正文/页眉/页脚与修改内容一致） | **通过**（无警告，5 页，1 表 38 行，正文/页眉/页脚与修改内容一致） |
| **XLSX** | **通过**（双表、样式、公式值与柱状图完整渲染，`readOnly` 正确拒绝写操作） | **通过**（改 `明细数据!C2: 12000→15000`、`销售汇总!E6` 文本，自动重算 8 个依赖单元格并更新柱状图） | **通过**（6,223 B，原文件未改） | **10/10 通过**（0 部件增删） | **通过**（无警告，2 表 1 图表，合并单元格、公式与重算值 `C3=23400` / `C6=73500` / `D6=12250` 完全一致） | **通过**（无警告，2 表 1 图表，重算值 `C2=15000` / `E2=7500` / `C3=23400` / `C6=73500` 完全一致） |
| **PPTX** | **有条件通过**（默认配置中文严重重叠；采用 CJK Fallback 字体修复后**清晰无重叠**，`readOnly` 正确拒绝写操作） | **通过**（替换第 2 页图形内 `【原文标记B】` → `【BetterOffice已修改B】`） | **通过**（8,312 B，原文件未改） | **9/9 通过**（0 部件增删） | **通过**（无警告，3 页，母版 2 图形，第 2 页 4 图形文本已更新，第 3 页 1 个动画 + Fade 切换 `3849` 保留） | **通过**（无警告，3 页，母版 2 图形，第 2 页文本已更新，第 3 页 1 个动画 + Fade 切换 `3849` 保留） |
| **损坏文件** | 传入非 ZIP 垃圾字节 | — | — | — | — | — |
| └ 错误反馈 | **通过**：DOCX/XLSX 明确报错 `bad zip: invalid Zip archive: Could not find EOCD`；PPTX 报错 `Presentation command connection closed` | — | — | — | — | — |

### 3.2 保存前后逐项保真与损耗详情

1. **Word（.docx）**：
   - **保留完好**：`w:rFonts` 中的 `宋体` / `微软雅黑` / `黑体`、`Heading1` / `Heading2` 样式、被编辑文字所在的加粗 + 红字（`CC0000`）Run 属性、斜体下划线、首行缩进 `w:firstLine`、1.5 倍行距 `w:line="360"`、编号列表 `w:numPr` + `numbering.xml`、表格 38 行 + 重复表头 `w:tblHeader` + 横向合并 `w:gridSpan="2"` + 纵向合并 `w:vMerge` + 单元格底纹（`D9E2F3`/`FFF2CC`/`E2EFDA`）、页眉 `header1.xml`、页脚 `footer1.xml` 及 `PAGE`/`NUMPAGES` 域。
   - **已知差异/损耗**：端内预览使用开源字体 `Noto Sans SC` / `Noto Serif SC` 替代 Windows 私有中文字体，视觉字形与宋体/微软雅黑略有差异，但保存回 DOCX 时**保留原字体名**，在 Word/WPS 重开后仍按原字体渲染。

2. **Excel（.xlsx）**：
   - **保留完好**：全部工作表与关系、合并单元格 `A1:F1`、`styles.xml`（微软雅黑、边框、底纹、`¥#,##0.00`、`0.0%`）、所有公式文本（`C2-D2`、`COUNTIF`、`SUMIF`、`IF`、`SUM`、`AVERAGE`、`C3/$C$6`）、重算后的缓存值 `<v>`（`C3=23400`、`D3=11700`、`C6=73500`、`D6=12250`、`F3=0.318367...`）、绘图关系 `sheet1.xml.rels` + `drawing1.xml` + 柱状图 `chart1.xml`。
   - **细节记录**：编辑写入的文本单元格 `E6` 序列化为 `t="inlineStr"` 而非 `sharedStrings.xml` 索引，符合 OOXML 标准，MS Excel 2010 与 WPS 12 均正常识别。

3. **PowerPoint（.pptx）**：
   - **保留完好**：`slideMaster1.xml`（蓝色顶部色条 + 中文母版页脚）、`theme1.xml`、第 2 页三种矢量图形（`roundRect`/`rightArrow`/`rect`）与填充描边及 `微软雅黑` 字体声明、第 3 页 `<p:transition spd="med"><p:fade/></p:transition>` 与 `<p:timing>` 淡入动画（`spid="31"`）。
   - **已知缺陷与限制**：
     - **CJK 字形重叠缺陷（须规避）**：`@betteroffice/pptx@0.5.1` 传入拉丁字体 `Carlito` 时会忽略 `<a:ea>`，导致中文字形全部回退为 `.notdef` 窄宽度并严重重叠；已在 WASM 与浏览器实测通过 `registerFallbackFont({ family: 'Noto Sans SC', bytes })`（或以含拉丁字形的 Noto Sans/Serif SC 替代默认拉丁族）完全消除重叠（`notdefCount: 0`）。切勿将 8 MB 字体字节克隆多份传入多个别名（PoC 克隆 14 份时 JS 堆一度飙升至 339 MB），应优先使用单份 `registerFallbackFont`。
     - **动画/切换不播放**：画布仅静态渲染页面内容，但编辑后不破坏原有动画与切换 XML。

### 3.3 明确标为「未验证」的项目
- **密码加密 Office 文件**（OOXML Agile/Standard Encryption）：本次未构造加密样本，未实测报错文案与降级流程（#329 开发阶段需补测）。
- **大文件（>10 MB / 数百页 DOCX / 万行 XLSX / 含高清图 PPTX）极限压测**：本次样本为典型结构小样本（5–8 KB），大文件打开耗时、内存峰值与 Worker 取消回收需在 #329 压测。
- **Tauri 2 真实打包后的原生自定义协议（`aipp-office://` / `http://asset.localhost`）加载插件内 `.wasm` + Module Worker**：本次在 Edge 131 + Android Chrome 134 下用严格镜像 AIPP 的 CSP 实测通过，但尚未实际打包进 AIPP Tauri 桌面/Android 壳验证自定义协议响应头（`Content-Type: application/wasm` 与 `application/javascript`）。
- **macOS（WKWebView）与 Linux（WebKitGTK）实测**：本机为 Windows 11，WebKit 引擎下的 `OffscreenCanvas` / Module Worker 表现尚未实测。
- **Android 真实物理低端机内存压测**：Android 实测在 API 36 模拟器完成，低内存真机（3–4 GB RAM）连续切换大文档的 OOM 边界未实测。

---

## 四、AIPP 工程与跨端适配实测数据

### 4.1 打包体积实测（Vite 8.3.8 生产构建）

对比基线：AIPP 当前整个前端 `dist/` 为 **27.66 MB**（345 个文件）。

| 资源类别 | 文件数 | 未压缩（raw） | gzip -9（≈ APK/zip） | brotli -6（≈ 桌面压缩） | 说明 |
| --- | ---: | ---: | ---: | ---: | --- |
| JS + 3 个 Worker + CSS | 50 | 4.02 MB | 1.19 MB | 1.06 MB | `docxWorker` 667 KB / `xlsxSessionWorker` 164 KB / `pptxSessionWorker` 163 KB |
| 6 个 Rust/WASM 引擎 | 6 | 47.18 MB | 17.12 MB | 13.60 MB | `docx_edit_bg.wasm` 24.42 MB、`xlsx` 7.58 MB、`docx_parse` 6.42 MB、`pptx` 6.30 MB、`docx_layout` 4.37 MB、`ooxml_opc` 0.38 MB |
| 拉丁/常用字体（`@betteroffice/fonts`） | 70 | 14.40 MB | 7.18 MB | 6.42 MB | Carlito / Caladea / Liberation / Inter / Roboto / Arabic / Hebrew 等 |
| 简中字体（NotoSansSC + NotoSerifSC） | 2 | 19.03 MB | 16.33 MB | 15.84 MB | OTF 压缩率较低，19 MB 压完仍占 ~16 MB |
| 日/韩/繁中字体（JP / KR / TC） | 3 | 14.17 MB | 11.98 MB | 11.70 MB | 中文场景可裁剪 |
| **全量内置合计** | **131** | **98.80 MB** | **53.79 MB** | **48.62 MB** | **不建议直接打入主包** |
| **裁剪日/韩/繁中（保留简中）** | **128** | **84.63 MB** | **41.82 MB** | **36.92 MB** | **适合打成官方插件独立 zip** |

> 补充验证：仅开启 `readOnly={true}` **无法减重**。实查 `docxWorker.mjs` 仍直接引用 `docx_edit_bg.wasm`（24.42 MB），且 `xlsx`/`pptx` 为单 WASM 包含解析+渲染+编辑；字体同样为预览排版测量所需。

### 4.2 桌面端 vs Android 端实测性能与运行对比

| 指标 | 桌面端（Windows Edge 131 / WebView2 同核） | Android 端（API 36 模拟器 / Chrome 134） | 差异与适配要求 |
| --- | --- | --- | --- |
| CSP 违规 / 外网请求 | **0** / **0** | **0** / **0** | 必须调用 `configureDefaultFonts({ fonts })`，否则 DOCX 会尝试请求 `fonts.googleapis.com` 被 CSP 拦截 |
| Worker / WASM / OffscreenCanvas | 全部启用（`SharedArrayBuffer=false` 下正常） | 全部启用（`SharedArrayBuffer=false` 下正常） | 无需 COOP/COEP 响应头；老旧 Android WebView（< Chrome 80）可能缺少 Module Worker / OffscreenCanvas，需做能力检测与外部打开回退 |
| DOCX 冷打开 / 热打开 / 保存 | **894 ms** / **377 ms** / **234 ms** | **2,906 ms** / **1,898 ms** / **762 ms** | Android 冷启约 3 秒，需展示明确加载态；Android 窄屏下 100% 缩放横向溢出，需自动计算 `initialZoom` 适配屏宽 |
| XLSX 冷打开 / 热打开 / 保存 | **134 ms** / **43 ms** / **18 ms** | **344 ms** / **115 ms** / **43 ms** | 移动端表格与图表渲染流畅 |
| PPTX 冷打开 / 热打开 / 保存 | **771 ms** / **688 ms** / **8 ms**（未传多份别名时） | **1,237 ms** / **1,041 ms** / **17 ms** | 需改用 `registerFallbackFont` 单份加载 CJK 字体，避免内存翻倍 |
| 内存占用（DOCX 峰值） | JS 堆 ~162 MB（上限 4,096 MB）；Worker WASM 缓冲 **97.2 MB**（live 40.1 MB / peak 70.7 MB） | JS 堆 ~77.6 MB（**上限 527.4 MB**）；Worker WASM 缓冲 **97.2 MB**（live 40.1 MB / peak 70.7 MB） | Android 堆上限约为桌面 1/8；关闭预览/切文档时必须彻底 unmount 并释放 Worker |

---

## 五、依赖版本、开源许可证与字体分发合规核查

| npm 包名 | 实测锁定版本 | 许可证 | 关键合规与分发义务 |
| --- | --- | --- | --- |
| `@betteroffice/docx` / `docx-react` / `docx-i18n` | `0.5.1` | **Apache-2.0**（内嵌 5 个 Lucide 图标：ISC / MIT） | 商业/闭源/开源分发均允许；需在发行产物或关于页保留 `LICENSE` 及 `THIRD-PARTY-NOTICES.md` |
| `@betteroffice/xlsx` / `xlsx-react` / `xlsx-i18n` | `0.5.1` | **Apache-2.0** | 需保留 Apache-2.0 声明 |
| `@betteroffice/pptx` / `pptx-react` / `pptx-i18n` | `0.5.1` | **Apache-2.0** | 需保留 Apache-2.0 声明 |
| `@betteroffice/fonts` | `0.4.2` | **`(Apache-2.0 AND OFL-1.1)`** | 加载代码为 Apache-2.0；65 个 TTF 字体为 SIL OFL 1.1（允许随软件免费捆绑分发，禁止单独出售字体文件，且**必须随包保留 `LICENSES/OFL-*.txt` 完整许可证与版权声明**） |
| `@betteroffice/fonts-cjk` | `0.4.2` | **`(Apache-2.0 AND OFL-1.1)`** | 5 个 Noto CJK SubsetOTF（Adobe 版权，保留字体名 `"Source"`）；须随插件包保留 `LICENSES/OFL-NotoCJK.txt`，不得改名冒用保留字体名；裁剪未使用的 JP/KR/TC 字体文件合规 |

> **离线与隐私合规**：严禁使用 `@betteroffice/fonts/cdn`（会向 `cdn.jsdelivr.net` 泄露文档所用字体列表）或默认 Google Fonts 回退（`fonts.googleapis.com`），全部字体均随插件包本地同源分发。

---

## 六、推荐落地架构：官方插件按需安装（供 #329 / #330 / #331）

结合 AIPP 现有官方插件体系（`plugin/`、`scripts/package-official-plugins.js`）与 `AIPP-helper`（`src/worker/data/plugins.ts` `/api/plugins`）：

1. **主安装包增量 ≈ 0 MB**：
   - 将 BetterOffice 运行时（React 封装 + 3 个 Worker + 6 个 WASM + `@betteroffice/fonts` + 简中 `NotoSansSC`/`NotoSerifSC` + 完整 `LICENSE`/`LICENSES`）作为独立官方插件放入 `plugin/office-preview-plugin/`（裁剪日/韩/繁中后 zip 约 **~42 MB**）。
   - 通过 `scripts/package-official-plugins.js` 打包为 `office-preview-plugin-<ver>.aipp-plugin.zip` 发布至 **GitHub Releases**（避免触碰 Cloudflare Workers 静态资源单文件 25 MiB 上限），并在 `AIPP-helper` 的 `/api/plugins` 注册条目及 `sha256`。
2. **双触发安装与进度提示**：
   - 用户既可在「插件商店」提前安装，也可在**首次点击预览 `.docx/.xlsx/.pptx`** 时检测到插件未安装，直接弹出内联安装卡片显示下载进度条、校验 SHA-256 并解压至 `<AppDataDir>/plugin/office-preview-plugin/`，完成后无缝进入预览（且同时提供「用外部应用打开」兜底按钮）。
3. **本地协议与安全边界**：
   - 桌面与 Android（`tauri.android.conf.json` 已开放 `$APPDATA/plugin/**`）统一通过 Tauri 本地协议提供插件内 `.js`/`.wasm`/`.ttf`/`.otf` 静态资源，确保正确返回 `application/wasm` 与 `application/javascript` MIME 且满足同源 Worker + 严格 CSP。
   - 预览组件只接收当前已授权文件的字节，不获得额外目录遍历或网络权限；编辑保存全部交回 AIPP 宿主走 #331 默认另存副本原子写盘逻辑。
4. **若未来需极轻量纯预览备选方案（不采用 BetterOffice 时）**：
   - 可退回纯前端/Rust 轻量方案：DOCX 用 `docx-preview`（Apache-2.0，~1 MB）、XLSX 用 Rust `calamine` + 前端虚拟表、PPTX 用 `pptx-preview`（ISC，~1.7 MB）或参考 T3 Code Worker 解析，总前端增量约 1–3 MB，但会丧失 Word 级分页保真、公式实时重算与端内所见即所得编辑/修订能力。
