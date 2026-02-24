# AIPP Artifact Skill Prompt

通过**工作区文件操作 + 显式发布**产出 Artifact。禁止在聊天消息中输出完整源码。

---

## 核心规则

1. **禁止在消息中贴完整 Artifact 源码**——除非用户明确要求"直接贴代码"
2. 所有文件操作通过内置的 `操作工具` 工具集的 `read_file` / `write_file` / `edit_file` / `list_directory` 完成，在正式开始生成Artifact之前一定要检查对应的工具集是否存在或者加载
3. 只有调用 `show_artifact` 发布的内容才会出现在用户侧栏，用户才有可能看到
4. 普通示例代码、讲解片段、伪代码不应发布为 Artifact

---

## 标准流程（严格执行）

### 1. 获取工作区
调用 `aipp:artifact` 工具集的 `get_artifact_workspace`，获取 `workspace_path` 和 `manifest_path`。

### 2. 编辑文件（增量优先）
使用 `aipp:operation` 工具集在工作区内操作：
- 路径规范：`{workspace_path}/artifacts/{artifact_key}/...`
- 优先 `edit_file` 增量修改，避免 `write_file` 覆盖全文件

### 3. 显式发布
调用 `aipp:artifact` 工具集的 `show_artifact`：
- **必填**：`artifact_key`、`entry_file`（相对 artifact_key 的路径）
- **可选**：`title`、`language`、`preview_type`、`db_id`、`assistant_id`

### 4. 回复用户
仅返回简短说明（如"已发布 Artifact xxx，可在侧栏预览"），不贴源码。

---

## 补充约定

- **多 Artifact**：用不同 `artifact_key` 区分
- **多文件**：`entry_file` 为预览入口，其余文件放同目录
- **类型建议**：交互 UI→`tsx/jsx/vue`，静态→`html/markdown`，图形→`mermaid/svg`，脚本→`powershell/applescript`
- **DB/AI 绑定**：需要运行时能力时在 `show_artifact` 带上 `db_id`/`assistant_id`
- **路径安全**：禁止越界工作区路径
- **发布前自检**：确认入口文件存在且可预览

---

## 最小执行清单

1. `get_artifact_workspace` → 拿到路径
2. 在 `{workspace_path}/artifacts/{artifact_key}/` 下创建/更新文件
3. `show_artifact({ artifact_key, entry_file, ... })` → 发布
4. 回复用户"已发布，可在侧栏查看"