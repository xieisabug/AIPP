# 助手形象与配饰插件开发指南（AIPP）

本文档说明如何制作可安装的 **AIPP 助手形象与配饰插件**（`avatarType` + `declarative` 运行时）。

这类插件使用**纯声明式静态多色矢量轮廓**（JSON），**不需要编写或编译 `dist/main.js`**，自动接入宿主全部 53 种状态动画、视线追踪、17 种内置配饰及预置配饰动作，并在桌面端与 Android 移动端保持一致体验。

示例插件见 `plugin/cute-pets-avatar-plugin/`。

---

## 1. 目录结构与 `plugin.json`

最小目录结构：

```text
<plugin_code>/
├── plugin.json
├── avatars/
│   └── orange-cat.json
├── accessories/
│   └── star-glasses.json
└── README.md                # 可选
```

`plugin.json` 示例：

```json
{
  "id": "my-avatar-pack",
  "code": "my-avatar-pack",
  "name": "我的形象包",
  "version": "1.0.0",
  "description": "自定义多色矢量助手形象与配饰",
  "author": "YourName",
  "minAippVersion": "0.5.0",
  "pluginTypes": ["avatarType"],
  "runtime": {
    "type": "declarative"
  },
  "contributions": {
    "avatars": ["avatars/orange-cat.json"],
    "accessories": ["accessories/star-glasses.json"]
  }
}
```

关键说明：
- `runtime.type` 设为 `"declarative"` 时，后端跳过 `dist/main.js` 校验，默认以 `plugin.json` 为入口，移动端默认允许启用。
- `contributions.avatars` 与 `contributions.accessories` 的每一项既可以是**相对 JSON 文件路径**（支持单对象或对象数组），也可以直接**内联 JSON 对象**。
- 也可以把 `contributions.avatars` / `accessories` 添加到已有 `themeType` 或其他 JS 插件的 `plugin.json` 中，做到“一套主题附带配套形象”。
- 注册后的全局 ID 自动加上插件命名空间：`<plugin_code>:<local_id>`（例如 `my-avatar-pack:orange-cat`），不会与内置形象或其它插件冲突。

---

## 2. 统一白名单矢量原语（`VectorLayer`）

为防止脚本注入并保证多尺寸（22px / 32px / 44px / 200px）60fps 动画性能，形象 `parts` 与配饰 `layers` 均使用统一的 `VectorLayer` 数组：

| 字段 | 说明 |
| :--- | :--- |
| `type` | 必填：`"path"` \| `"rect"` \| `"circle"` \| `"ellipse"` |
| `d` | `type="path"` 必填，仅允许标准 SVG path 指令（`M/L/H/V/C/S/Q/T/A/Z` 与数字） |
| `x`, `y`, `width`, `height`, `rx`, `ry` | `type="rect"` 几何属性 |
| `cx`, `cy`, `r` | `type="circle"` 几何属性 |
| `cx`, `cy`, `rx`, `ry` | `type="ellipse"` 几何属性 |
| `fill` / `stroke` | 颜色值：支持动态颜色槽或固定 `#RRGGBB` |
| `fillOpacity` / `strokeOpacity` / `opacity` | `0 ~ 1` 透明度 |
| `strokeWidth` | 描边宽度（`0 ~ 120`） |
| `strokeLinecap` / `strokeLinejoin` | `butt\|round\|square` / `miter\|round\|bevel` |
| `whiteEdge` | `boolean`，设为 `true` 且当前填充为 `#FFFFFF` 时自动描 `#E4E4E4` 细边 |
| `transform` | 可选结构化变换：`{ x?, y?, rotate?, scale?, scaleX?, scaleY? }` |

**支持的颜色槽位**：
- `"primary"`：用户的第一颜色（通常为亮色/脸部/内圈）
- `"secondary"`：用户的第二颜色（通常为深色/身体/外圈）
- `"white"`：固定 `#FFFFFF`
- `"core"`：跟随形象主色，黑白默认配色时自动跟随前景色 `currentColor`
- `"none"`：无填充/无描边
- `"#RRGGBB"`：固定十六进制色（用于腮红、耳蜗、星徽等不随换色改变的多色细节）

---

## 3. 助手形象（Avatar）Schema 与 510 坐标系

所有助手形象均绘制在 **`510 × 510` 坐标系** 内（画布中心为 `(255, 255)`，有效主体视觉宽度建议接近 460~507 单位）：

```json
{
  "schemaVersion": 1,
  "id": "orange-cat",
  "label": "小猫",
  "description": "可选简介",
  "artworkScale": 1.04,
  "faceColorSlot": "primary",
  "eyeColor": "#111A2D",
  "eyeScale": 1.0,
  "defaultColors": {
    "primary": "#FFF0B5",
    "secondary": "#D87855"
  },
  "parts": [ /* 1~32 个 VectorLayer，按从底到顶的顺序绘制 */ ],
  "anchors": {
    "face":       { "x": 255, "y": 248, "scale": 1.12 },
    "crown":      { "x": 255, "y": 106, "scale": 0.92 },
    "headphones": { "x": 255, "y": 98,  "scale": 0.92, "scaleY": 0.88 },
    "clip":       { "x": 152, "y": 122, "scale": 0.92, "rotation": -22 },
    "propDock":   { "x": 408, "y": 355, "scale": 0.80, "rotation": -24 }
  },
  "colorLabels": {
    "primary": "脸部",
    "secondary": "毛色"
  },
  "capabilities": ["eyes", "headwear"]
}
```

### 锚点校准速查
- **`face`（必填）**：双眼中心。双眼位于局部坐标 `(-44, 0)` 与 `(+44, 0)`，所有脸部配饰也以此为原点。
- **`crown`**：头顶中线位置，承载棒球帽、贝雷帽、皇冠、魔法帽、竹蜻蜓、嫩芽、睡帽。
- **`headphones`**：头戴式耳机弓顶位置，`scaleY` 用于调节耳罩向下包覆的长度。
- **`clip`**：侧边发卡（铃铛蝴蝶结、小花等）挂载点，`rotation` 控制贴合轮廓的倾角。
- **`propDock`**：眼镜摘下悬浮时的停靠位置（通常在角色右下角附近）。
- 未填写 `crown`/`headphones`/`clip`/`propDock` 时，后端会自动补齐默认安全坐标。

---

## 4. 配饰（Accessory）Schema

```json
{
  "schemaVersion": 1,
  "id": "star-glasses",
  "label": "星星眼镜",
  "description": "可选简介",
  "slot": "eyes",
  "anchor": "face",
  "followGaze": false,
  "action": "glasses",
  "previewViewBox": "-146 -70 292 140",
  "layers": [ /* 1~32 个 VectorLayer，以锚点 (0, 0) 为原点绘制 */ ]
}
```

- **`slot`**：`"eyes"`（脸部）或 `"head"`（头部）。
- **`anchor`**：
  - `slot="eyes"` 时固定为 `"face"`；
  - `slot="head"` 时可选 `"crown"`（默认）、`"headphones"` 或 `"clip"`。
- **`followGaze`**：仅 `slot="eyes"` 有效；`true` 时随眼球视线一起轻微移动（如眉毛），`false` 时固定在面部（如眼镜、胡须、腮红）。
- **`previewViewBox`**：编辑器 32px 按钮和插件中心预览的 SVG `viewBox`（`minX minY width height`）。
- **支持的预置交互 `action`**：
  - 脸部推荐：`null`（纯静态）、`"glasses"`（悬浮摘下停靠 `propDock` 再戴回）、`"peer"`（上抬偷看并眯眼）、`"blush"`（害羞轻扭）、`"blink"`（眨眼）、`"twirl"`（微笑互动）
  - 头部推荐：`null`（纯静态）、`"hat"`（抬帽致意）、`"straighten-crown"`（滑歪 → 抬头看 → 扶正）、`"listen"`（听歌摇摆）、`"jingle"`（摇头晃动）、`"sprout-sway"`（轻晃）、`"fly"`（短暂悬浮）、`"doze"`（打盹点点头）

---

## 5. 后端校验红线与降级约定

安装前与加载时，Rust 端（`src-tauri/src/plugin/avatar_assets.rs`）会强制校验：
- `local_id` 仅允许 `^[a-z0-9][a-z0-9_-]{0,47}$`，同插件内不可重名；
- 单个插件最多 32 个形象、64 个配饰；每个资产最多 32 个图层；单文件 ≤ 256 KiB；单条 `path d` ≤ 16 KiB；
- 禁止路径穿越（`..`、绝对路径或越出插件目录的符号链接）；
- 禁用或卸载插件后，助手数据库中的原配置 ID 不会被清除，渲染自动降级为圆团 / 跳过缺失配饰；重新启用或在跨设备同步端安装同一插件后自动恢复。

---

## 6. 打包与本地调试

1. 本地调试：将目录复制到 `<AppDataDir>/plugin/<plugin_code>/`，打开「设置 → 插件中心 → 扫描插件」即可实时预览。
2. 官方发布打包：`scripts/package-official-plugins.js` 已原生支持 `runtime.type === "declarative"`，会自动跳过 `npm run build` 并打包 `plugin.json`、`avatars/`、`accessories/` 为 `.aipp-plugin.zip`。

---

## 7. 附：可直接增补到 `aipp-plugin` Skill 的参考段落

> 若你在全局 Skills 目录维护了 `aipp-plugin` Skill，可将下方内容直接追加到其 `SKILL.md` 中：

```markdown
### 助手形象与配饰插件（`avatarType` + `declarative`）

- 优先使用纯声明式运行时（`"runtime": { "type": "declarative" }`，`"pluginTypes": ["avatarType"]`），**无需 `dist/main.js` 或前端构建步骤**，桌面与 Android 移动端自动兼容。
- 在 `plugin.json` 的 `contributions` 中声明 `avatars`（形象 JSON 路径或内联数组）和/或 `accessories`（配饰 JSON 路径或内联数组）。
- 所有图形仅使用白名单 `VectorLayer`（`path` / `rect` / `circle` / `ellipse`），严禁原始 SVG HTML 字符串；颜色支持槽位 `primary` / `secondary` / `white` / `core` / `none` 及固定 `#RRGGBB`。
- 形象统一使用 **510×510 坐标系**（中心 `255,255`），至少提供 `face` 锚点，推荐同步校准 `crown`、`headphones`、`clip`、`propDock` 及 `defaultColors`、`faceColorSlot`。
- 配饰以 `(0, 0)` 为锚点原点绘制，设置 `slot`（`eyes` | `head`）、`anchor`（`face` | `crown` | `headphones` | `clip`）、`previewViewBox`（4 个数字）及可选预置 `action`（如 `glasses` / `peer` / `hat` / `straighten-crown` / `listen`）。
- 完整 Schema、校验红线与示例见仓库 `docs/avatar-plugin-development.md` 与 `plugin/cute-pets-avatar-plugin/`。
```
