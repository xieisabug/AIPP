# AIPP Plugin 商店改造计划

本文档是 AIPP Plugin 商店的端到端改造计划，包含官方推荐获取与安装、从 GitHub/Zip 链接安装、插件详情界面、接口交互以及官方插件打包 GitHub Actions。

## 1. 总体方案

采用和 Skills 类似的三段式流程：

```text
fetch_official_plugins
  -> inspect_plugin_archive_source
  -> install_plugin_archive_source
  -> plugin_registry_changed
  -> PluginRuntime.reloadPlugins()
```

插件开发者或官方 CI 负责提前编译插件。AIPP 客户端只负责下载、校验、解压、安装，不在用户机器上构建插件。

## 2. 目标用户流程

### 2.1 官方推荐安装

1. 用户打开插件中心。
2. 切换到「推荐插件」Tab。
3. 前端调用 `fetch_official_plugins({ useProxy: false })`。
4. 卡片展示名称、描述、版本、类型、权限、安装状态。
5. 用户点击「详情」或「安装」。
6. 前端调用 `inspect_plugin_archive_source` 预览包内容。
7. 展示将安装的插件、版本、权限、贡献点、是否替换已有版本。
8. 用户确认安装。
9. 前端调用 `install_plugin_archive_source`。
10. 后端安装完成后触发 `plugin_registry_changed`。
11. 前端刷新 `list_plugins` 并调用 `pluginRuntime.reloadPlugins()`。

### 2.2 从 GitHub 链接安装

1. 用户打开「从来源安装」。
2. 输入：
   - `https://github.com/owner/repo`
   - `https://github.com/owner/repo/tree/ref/path`
   - 或 `owner/repo#ref`
3. 前端解析成 `PluginInstallRecipeSource`：
   - `type=github`
   - `repo=owner/repo`
   - `ref=main` 或 URL 中解析出来的 ref
4. 如果 URL 中包含路径，可生成 `dirs: [{ from: path, to: basename(path) }]`；否则让后端自动发现。
5. 调用 `inspect_plugin_archive_source`。
6. 用户选择一个或多个可安装插件。
7. 调用 `install_plugin_archive_source`。

### 2.3 从 Zip 链接安装

1. 用户输入 Zip URL。
2. 前端解析成 `source: { type: "zip", url }`。
3. 调用 inspect。
4. 用户确认后安装。

### 2.4 插件详情查看

插件中心本地插件列表中点击任意插件，展示：

- 名称、版本、作者、描述。
- 插件 code 和安装目录。
- 是否已安装、是否启用。
- runtime type、entry、entry sha256。
- pluginTypes。
- permissions。
- contributions：
  - bangs
  - hooks
  - views
  - actions
  - assistantFormFields
- 配置项。
- Hook 注册和最近审计日志。
- sourceUrl 如果可用则显示「打开来源」。

## 3. 后端改造

### 3.1 新增模块

建议新增：

```text
src-tauri/src/plugins/
  installer.rs
```

或者如果当前后端仍以 `api/plugin_api.rs` 为主，也可以先放在：

```text
src-tauri/src/plugin/installer.rs
```

职责：

- source validate。
- 下载 archive。
- 解压 archive。
- 自动发现插件目录。
- 构建 install inspection。
- 安装 selected plugins。
- sha256 校验。
- 安全路径校验。

### 3.2 新增数据类型

参考 `plugin-store-api-requirements.md`：

- `OfficialPlugin`
- `PluginInstallRecipeSource`
- `PluginInstallRecipeSourceType`
- `PluginInstallRecipeDir`
- `PluginInstallPlanPlugin`
- `PluginInstallValidation`
- `PluginArchiveInspection`
- `PluginArchiveInstallResult`
- `PluginDetailItem`

### 3.3 新增 Tauri commands

必做：

```rust
fetch_official_plugins(app_handle, use_proxy)
inspect_plugin_archive_source(app_handle, source, dirs, expected_sha256, use_proxy)
install_plugin_archive_source(app_handle, source, selections, expected_sha256, use_proxy, enable_after_install)
get_plugin_detail(app_handle, code)
verify_plugin_entry_checksum(app_handle, code)
```

可选：

```rust
install_official_plugin(app_handle, official_plugin, use_proxy, enable_after_install)
open_plugin_source_url(url)
```

### 3.4 复用现有逻辑

需要复用或提取现有 helper：

- `get_plugin_root_path`
- `read_plugin_manifest`
- `resolve_runtime_manifest`
- `resolve_plugin_manifest_from_dir`
- `sync_registry`
- `plugin_entry_exists`
- `normalize_plugin_types`
- `normalize_permissions`
- `hook_contributions_to_registrations`
- `emit_plugin_registry_changed`

建议把纯逻辑从 `plugin_api.rs` 拆出去，减少接口文件继续膨胀。

### 3.5 安装事务与回滚

安装过程必须避免半安装状态：

```text
download zip
  -> extract temp
  -> validate selected plugin dirs
  -> copy selected plugin to temp install dir
  -> backup old plugin dir
  -> rename temp install dir to final plugin dir
  -> sync registry
  -> emit event
  -> cleanup backup
```

失败时：

- 如果最终目录尚未替换，直接删除临时目录。
- 如果已替换但 registry 同步失败，恢复 backup。
- 不删除 `PluginData`、`PluginConfigurations`、插件私有 `plugin_data/<code>.db`。

### 3.6 JS 插件 checksum

当前 wasm/process runtime 有后端 checksum 校验，但 JS runtime 是前端直接注入脚本。建议：

1. 安装时计算 `dist/main.js` sha256。
2. 如果 manifest `runtime.checksum` 缺失，官方打包 Action 可以自动写入。
3. 前端加载插件前调用 `verify_plugin_entry_checksum(code)`。
4. 校验失败时不注入 script，并在插件中心显示错误。

## 4. 前端改造

### 4.1 PluginCenterConfig 结构调整

当前 `PluginCenterConfig.tsx` 已有本地插件管理、启用/禁用、卸载、配置、Hook 调试。建议扩展为：

```text
PluginCenterConfig
  Tabs
    Local plugins
    Recommended plugins
    Install from source
```

或者拆组件：

```text
src/components/config/plugin/
  PluginCenterConfig.tsx
  LocalPluginList.tsx
  RecommendedPluginList.tsx
  PluginInstallSourcePanel.tsx
  PluginArchiveInspectDialog.tsx
  PluginDetailPanel.tsx
  PluginPermissionSummary.tsx
```

### 4.2 推荐插件列表

前端状态：

```ts
type OfficialPluginFetchStatus = "idle" | "loading" | "success" | "timeout" | "error";
```

交互：

- 打开 Tab 自动拉取。
- 提供刷新按钮。
- 出错时支持「使用代理重试」。
- 搜索字段支持 name/code/description/tags。
- 卡片 badge：
  - 已安装
  - 有更新
  - 实验性
  - 高风险权限

调用：

```ts
invoke<OfficialPlugin[]>("fetch_official_plugins", { useProxy })
```

### 4.3 安装预览 Dialog

复用 Skills `SkillInstallGuideDialog` 的交互模型，但内容替换为插件：

- 左侧：来源信息、下载 URL、sha256。
- 中间：可安装插件列表，可多选。
- 右侧：选中插件详情。
- 底部：权限风险提示、替换提示。

调用：

```ts
invoke<PluginArchiveInspection>("inspect_plugin_archive_source", {
  source,
  dirs,
  expectedSha256,
  useProxy,
})
```

安装：

```ts
invoke<PluginArchiveInstallResult>("install_plugin_archive_source", {
  source,
  selections,
  expectedSha256,
  useProxy,
  enableAfterInstall: true,
})
```

安装成功：

```ts
toast.success(`已安装 ${result.installedPlugins.length} 个插件`);
await loadPlugins();
await pluginRuntime.reloadPlugins();
```

### 4.4 从 GitHub/Zip 安装

输入解析建议支持：

| 输入 | 解析 |
| --- | --- |
| `owner/repo` | GitHub source，ref=`main` |
| `owner/repo#v1.0.0` | GitHub source，ref=`v1.0.0` |
| `https://github.com/owner/repo` | GitHub source |
| `https://github.com/owner/repo/tree/main/plugin/foo` | GitHub source + dirs from path |
| `https://example.com/foo.zip` | Zip source |

如果无法解析，前端直接提示，不调用后端。

### 4.5 本地详情页增强

本地插件详情页调用：

```ts
invoke<PluginDetailItem>("get_plugin_detail", { code })
```

展示建议：

- 基础信息卡。
- 权限卡。
- Runtime 卡。
- Contributions 卡。
- 配置卡。
- Hook 调试卡。
- 本地路径卡。

### 4.6 权限风险提示

第一版可以用静态规则：

| 权限/类型 | 风险提示 |
| --- | --- |
| `data.read.*` | 可读取本地数据 |
| `assistant.prompt.write` | 可修改助手提示词 |
| `hook.chat.beforeModelRequest` | 可影响模型请求内容 |
| `bang.register` + 执行命令类 bang | 可能触发命令执行 |
| `plugin.storage` | 可写入插件私有存储 |
| `themeType` | 会修改界面主题 |

高风险权限安装按钮前增加确认文案。

## 5. 官方插件打包 GitHub Actions

### 5.1 推荐目录结构

继续使用主仓 monorepo：

```text
plugin/
  directory-bang-plugin/
    package.json
    plugin.json
    src/
    dist/
  prompt-optimizer-plugin/
    package.json
    plugin.json
    src/
    dist/
```

先不拆一插件一仓。这样官方插件可以复用主仓 SDK 类型和 CI。

### 5.2 打包脚本

建议新增 Node 脚本：

```text
scripts/package-official-plugins.js
```

职责：

1. 扫描 `plugin/*/plugin.json`。
2. 校验目录名等于 `plugin.json.code`。
3. 执行每个插件的 `npm run build`。
4. 校验 `dist/main.js` 存在。
5. 计算 `dist/main.js` sha256。
6. 可选：回写临时 manifest 的 `runtime.checksum`。
7. 创建 zip：
   - `plugin.json`
   - `dist/**`
   - `README.md` 如果存在
   - `LICENSE` 如果存在
   - `assets/**` 如果存在
8. 计算 zip sha256。
9. 生成 `official-plugins.json`。

不要把以下内容打进包：

- `src`
- `node_modules`
- `package-lock.json`
- `pnpm-lock.yaml`
- `tsconfig.json`
- 测试文件

### 5.3 GitHub Actions Workflow

建议新增：

```text
.github/workflows/package-official-plugins.yml
```

触发：

```yaml
on:
  workflow_dispatch:
  push:
    tags:
      - "plugins-v*"
```

主要步骤：

```yaml
jobs:
  package-plugins:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm
      - run: npm ci
      - run: node scripts/package-official-plugins.js
      - uses: actions/upload-artifact@v4
        with:
          name: official-plugins
          path: dist-official-plugins/**
      - uses: softprops/action-gh-release@v2
        if: startsWith(github.ref, 'refs/tags/plugins-v')
        with:
          files: dist-official-plugins/**
```

产物：

```text
dist-official-plugins/
  official-plugins.json
  directory-bang-plugin-0.1.0.aipp-plugin.zip
  prompt-optimizer-plugin-0.1.0.aipp-plugin.zip
  ...
```

### 5.4 official-plugins.json 示例

```json
[
  {
    "id": "directory-bang-plugin",
    "code": "directory-bang-plugin",
    "name": "Directory Bang Plugin",
    "description": "Adds !directory and !dir bangs backed by the built-in list_directory tool.",
    "version": "0.1.0",
    "author": "AIPP",
    "tags": ["official", "bang", "tool"],
    "pluginTypes": ["toolType", "applicationType"],
    "permissions": ["bang.register"],
    "source": {
      "type": "zip",
      "url": "https://github.com/xieisabug/AIPP/releases/download/plugins-v0.1.0/directory-bang-plugin-0.1.0.aipp-plugin.zip"
    },
    "dirs": [
      {
        "from": "directory-bang-plugin",
        "to": "directory-bang-plugin"
      }
    ],
    "sourceUrl": "https://github.com/xieisabug/AIPP/tree/main/plugin/directory-bang-plugin",
    "sha256": "sha256:..."
  }
]
```

### 5.5 发布策略

短期：

- 使用 AIPP 主仓 GitHub Release 承载 zip。
- `https://aipp-helper.xiejingyang.com/api/plugins` 返回 release 中的 `official-plugins.json` 内容。

中期：

- `official-plugins.json` 可发布到 GitHub Pages 或对象存储。
- API 只做代理和缓存。

长期：

- 支持第三方提交 catalog。
- 每个插件可独立仓库发布，但 catalog 格式保持不变。

## 6. 第一批官方插件改造清单

| 插件 | 是否推荐首批 | 需要补充 |
| --- | --- | --- |
| `directory-bang-plugin` | 是 | README、风险说明 |
| `run-script-bang-plugin` | 是，但标高风险 | README、高风险 badge、安装二次确认 |
| `think-markdown-plugin` | 是 | README |
| `hidden-first-turn-context-plugin` | 是 | README、hook 权限说明 |
| `prompt-optimizer-plugin` | 是 | README、assistant 写权限说明 |
| `usage-dashboard-plugin` | 是 | README、本地数据读取说明 |
| `benchmark-plugin` | 是，标 experimental | README、experimental badge |
| `guofeng-zhusha-theme-plugin` | 是 | README、主题截图可选 |

建议每个插件补一个简短 `README.md`，内容包括：

- 插件用途。
- 权限说明。
- 使用方式。
- 风险或限制。

## 7. 分阶段实施

### 阶段一：后端安装基础能力

交付：

- source 类型和校验。
- archive 下载/解压。
- 自动发现插件。
- inspect/install commands。
- 安装后 registry 同步和 event。
- 后端测试。

### 阶段二：插件中心 UI

交付：

- 推荐插件 Tab。
- 从来源安装 Tab。
- 安装预览 Dialog。
- 权限风险提示。
- 安装成功后 runtime reload。

### 阶段三：详情与安全增强

交付：

- `get_plugin_detail`。
- JS entry checksum 校验。
- 详情页展示 runtime、permissions、contributions、hooks。
- 高风险权限二次确认。

### 阶段四：官方插件打包自动化

交付：

- `scripts/package-official-plugins.js`。
- `.github/workflows/package-official-plugins.yml`。
- release 产物。
- `official-plugins.json`。
- 官方 API 接入。

### 阶段五：官方插件内容完善

交付：

- 每个官方插件 README。
- 插件截图或图标字段可选。
- 官方推荐列表补 tags、experimental、sourceUrl、sha256。

## 8. 验收标准

后端：

- 可以从 GitHub source 自动发现并预览多个插件。
- 可以从官方 release zip 安装单个插件。
- sha256 不匹配会拒绝安装。
- 缺 `plugin.json` 或 `dist/main.js` 会拒绝安装。
- 安装后 `list_plugins` 能看到插件。
- 安装后 `get_enabled_plugins` 能返回启用插件。
- 更新插件不会删除插件配置和数据。

前端：

- 推荐插件可加载、搜索、刷新、代理重试。
- 安装前可查看权限和贡献点。
- 安装成功后无需重启即可刷新运行时。
- GitHub/Zip 自定义来源可预览和安装。
- 高风险插件有明确提示。

打包：

- GitHub Actions 可自动构建所有官方插件。
- 每个 zip 包只包含运行所需文件。
- 生成 `official-plugins.json`。
- 每个 item 有可校验 sha256。
