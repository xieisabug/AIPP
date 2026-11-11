# Skills 系统

Skills 系统用于给助手追加“可复用的任务说明书”。它不是数据库里的单条配置，而是一套基于文件系统扫描、按需加载、可绑定到助手的能力包。

---

## Skills 的基本结构

### Skill 文件
- 每个 Skill 以 `SKILL.md` 为核心文件
- 支持 YAML frontmatter 元数据和正文内容两部分
- frontmatter 当前支持的字段包括：
  - `name`
  - `description`
  - `version`
  - `author`
  - `tags`
  - `requires_files`
- 如果没有 frontmatter，系统会退化为使用文件名和正文首段生成基础信息

### 按需加载内容
- 列表页只扫描元数据，减少加载开销
- 打开某个 Skill 详情时，才会通过 `get_skill_content` 读取完整正文
- `requires_files` 中声明的附加文件会一起读取并返回，便于 Skill 携带模板、示例或附属说明
- 支持 YAML 多行说明和嵌套 `metadata.version/author`。GitHub 插件技能使用独立 `agent_plugin` 来源，安装时保留整个包；`load_skill` 的 `file` 参数可读取该技能的配套文本，路径相对于技能目录，并受包目录边界和大小限制。详见 [GitHub 插件包](./21-GitHub插件包.md)。

---

## 技能源与扫描

### 文件系统扫描
- Skills 不直接存储在数据库中，数据库只保存“某个助手启用了哪些 Skill”
- `scan_skills` 会重新扫描技能源并刷新补全索引
- 扫描完成后会触发 `skills-registry-changed` 事件，供前端刷新界面

### 当前内置技能源
- `Agents`：`~/.agents/skills/`
- `Copilot`：`~/.copilot/skills/`
- `Codex`：`~/.codex/skills/`
- `Claude Code Skills`：从 `~/.claude/plugins/installed_plugins.json` 中解析
- 这些来源由后端统一扫描，前端再按当前已接入的来源类型分组展示

### Skill 标识与来源
- 每个 Skill 有唯一标识，格式为 `source_type:relative_path`
- UI 中会展示来源名、文件路径、元数据和存在状态
- 如果文件被删除但助手配置仍然存在，系统会保留配置记录并标记 `exists = false`

---

## Skills 管理界面

### 技能列表与内容预览
- `SkillsConfig.tsx` 按来源分组展示所有扫描到的 Skill
- 支持按名称或标识符搜索
- 选中某个 Skill 后，会在右侧加载其 Markdown 正文用于预览

### 目录操作
- `open_skills_folder`：打开统一技能目录
- `open_skill_parent_folder`：打开某个 Skill 所在目录
- `get_skills_directory`：返回技能目录路径
- `delete_skill`：删除本地 Skill 目录，并在删除后自动重新扫描

### Slash / 上下文联动
- 扫描完成后会重建 Skills 补全索引
- Skill 内容也可被其他模块按需读取，例如 Slash 路由和聊天侧边栏上下文预览

---

## 助手与 Skills 的绑定

### 助手级配置
- Skills 与助手的关系存储在数据库中
- 每条配置包含：
  - `skill_identifier`
  - `is_enabled`
  - `priority`
- 支持单个开关、批量更新、移除配置、清理孤儿配置

### Butler 的特殊行为
- 总管家系统助手不是普通用户助手
- 对 Butler 系统助手读取已启用 Skills 时，后端会直接返回所有扫描到的 Skill，方便总管家做全局调度

---

## 安装与导入

### 官方技能库
- `fetch_official_skills` 可从官方接口拉取技能列表
- 支持可选代理，未配置代理时会给出明确错误提示
- 列表项会标准化为可安装源，并补齐来源地址

### 安装方式
- 从官方列表安装
- 从安装 recipe（JSON）安装
- 从 GitHub 仓库或 Zip 包检查并安装
- 安装完成后都会自动触发重新扫描

### 兼容与迁移
- 旧版 `{app_data}/skills` 中的内容会迁移到 `~/.agents/skills`
- 旧的 `aipp` 来源标识在后端会兼容映射到 `agents`

---

## 与 MCP / Agent 的联动

### 当前真实依赖
- 现在启用 Skill 时，后端重点校验的是 **Agent 工具集**（`aipp:agent`）是否可用
- 更准确地说，必须具备 `load_skill` 相关能力；否则会返回 `AGENT_LOAD_SKILL_REQUIRED`
- 前端会在启用 Skill 前先做检查，必要时弹出确认并一键补齐依赖

### 关闭依赖时的保护
- 关闭全局或助手级 Agent MCP 前，会先检查哪些助手仍依赖 Skill
- 可以选择在关闭 Agent MCP 的同时，自动关闭相关 Skills，避免出现半可用状态

---

## 适合记录到文档的使用理解

- Skill 更像“长期维护的提示词资产”，适合沉淀方法论
- 助手配置决定“谁能用这些 Skill”，文件系统决定“Skill 本体是什么”
- 与普通系统提示词相比，Skill 更适合分来源管理、复用、分享和单独安装

---
相关源码:
- `src-tauri/src/api/skill_api.rs` - Skills API
- `src-tauri/src/skills/types.rs` - Skill 类型与技能源定义
- `src-tauri/src/skills/parser.rs` - `SKILL.md` 解析器
- `src/components/config/SkillsConfig.tsx` - Skills 管理界面
- `src/hooks/useSkillsMcpValidation.ts` - Skills 与 Agent/MCP 联动校验


## Android 应用内 Skills

Android 的扫描、安装、查看与加载统一使用应用数据目录下的 `skills`，不会迁移到桌面的 `~/.agents/skills`。移动页面支持系统 ZIP 导入、HTTPS 压缩包安装、内容查看及助手绑定；绑定按钮明确启用 Agent 的 `load_skill` 工具。未覆盖的同名技能会被拒绝，不自动替换旧版本。

压缩包上限 20 MiB、解压内容上限 50 MiB、最多 4096 项；拒绝路径穿越、绝对路径及符号链接。所引用的附加文件不能越出技能目录。

兼容性与安装状态分开：含脚本目录或明确本地执行指令时提示 Android 不可执行这些步骤；作者用 `instruction-only` 标签声明的纯指令技能单独标注；其余显示“兼容性未验证”。加载时保留完整指令和限制提示，不得静默跳过不可执行步骤并声称完成。安装数据随应用重启保留，卸载清除。
