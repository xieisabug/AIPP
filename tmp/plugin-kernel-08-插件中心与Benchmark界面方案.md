# 插件中心 + Benchmark 独立界面方案（重设计草案）

> 本文档用于替换“Benchmark 作为 assistantType 插件”的路线。  
> 新目标：**Benchmark 必须是独立界面插件（UI + Worker），并在设置页通过“插件”菜单统一管理。**

---

## 1. 目标与边界

### 1.1 目标

1. 在设置页新增一级菜单：`插件`（与“程序功能”同级）。
2. 插件配置统一收敛到“插件中心”，不再散落到助手类型配置里。
3. Benchmark 改为独立插件界面，支持：
   - QA 集创建/编辑/删除
   - QA 条目手动录入并持久化
   - CSV 导入 QA 条目
   - 每个 QA 集独立 Judge Prompt / 运行配置
   - 批量运行、运行记录、每题排行榜

### 1.2 非目标（本轮不做）

1. 插件市场/远程下载/签名体系。
2. 多租户隔离与复杂沙箱。
3. 与 assistantType 插件继续混用同一 UI 配置入口。

---

## 2. 现状与需求不匹配点

| 需求 | 当前状态 | 缺口 |
|---|---|---|
| Benchmark 是独立界面 | 当前实现为 assistantType 插件 | 插件类型与交互模型错误 |
| 设置页有“插件”菜单 | `ConfigWindow` 仅固定 5 个菜单 | 缺少插件中心入口 |
| 插件配置统一管理 | 当前仅有基础启停与 KV 数据能力 | 缺少配置表单与页面挂载机制 |
| QA 集独立配置 | 当前无 QA Set 数据结构 | 缺少结构化存储模型 |
| CSV 导入 | 当前无导入映射与校验流程 | 缺少文件导入能力与解析 API |
| 运行记录与排行榜 | 当前无 benchmark 任务/记录模型 | 缺少任务编排、状态与聚合查询 |

---

## 3. 需要补齐的底层能力（能力缺口）

## 3.1 ConfigWindow 扩展点：插件中心

新增设置页扩展位：

- 菜单项：`plugins-config`
- 页面容器：`config.plugins.main`
- 子面板扩展位：`config.plugins.detail`

要求：

1. 插件中心是 Host 页面（宿主渲染主框架）。
2. 插件只提供声明/组件入口，不直接改宿主菜单数据结构。
3. 继续保持移动端与桌面端一致的导航行为。

## 3.2 UI 插件在 config 作用域的生命周期

现有 runtime 主要服务 assistantType，需补：

- `onActivate(scope = "config")`
- `onDeactivate(scope = "config")`
- 设置页面挂载/卸载回收（dispose）

## 3.3 插件配置契约（Schema + Value）

除 `get_plugin_config/set_plugin_config` 的 KV 外，需要：

1. `settingsSchema`（字段定义、校验、分组、默认值）
2. `settingsValue`（已保存值）
3. Host 表单渲染与保存回调

建议新增：

- `get_plugin_settings_schema(plugin_id)`
- `get_plugin_settings(plugin_id)`
- `set_plugin_settings(plugin_id, payload)`

## 3.4 结构化持久化能力（用于 QA / Run / 排行榜）

`PluginData(session_id + key + value)` 适合轻量配置，不适合 benchmark 结构化查询。  
需新增“插件私有结构化存储”能力（二选一）：

### 方案 A（推荐）

在 `plugin.db` 增加 benchmark 领域表（按 plugin_id 隔离）：

- `benchmark_sets`
- `benchmark_items`
- `benchmark_runs`
- `benchmark_run_items`

### 方案 B（备用）

提供通用 `plugin_records`（collection + json + index 字段），再由插件侧组装查询。  
此方案实现快，但复杂排行查询可维护性差。

## 3.5 CSV 导入能力

需要统一导入链路：

1. 文件选择（本地 CSV）
2. 解析（含编码/分隔符探测）
3. 列映射（question/reference 必填）
4. 校验与错误报告
5. 批量写入（事务）

建议新增命令：

- `benchmark_preview_csv(file_path, options)`
- `benchmark_import_csv(set_id, mapping, rows)`

## 3.6 运行任务编排能力（Worker）

Benchmark 运行是长任务，需后台编排：

1. 创建 run（queued/running/success/failed/cancelled）
2. 分题执行 + 进度上报
3. 可取消
4. 失败可定位到题目级别

建议事件：

- `benchmark_run_progress`
- `benchmark_run_completed`
- `benchmark_run_failed`

## 3.7 复现实验能力

每次运行需记录快照：

- runner 类型（assistant/model）
- runner 标识（assistant_id/model_id）
- judge 标识与 judge_prompt
- set 版本（items 哈希）

否则排行榜不可解释、不可复现。

---

## 4. 插件中心（设置页“插件”菜单）界面设计

## 4.1 信息架构

在设置页新增一级菜单：

- 模型提供商
- 个人助手
- MCP
- Skills
- 程序功能
- **插件（新增）**

“插件”页面结构：

1. 左侧：已安装插件列表（启用状态、版本、类型）
2. 右侧：插件详情区域（Tab）
   - 概览
   - 配置
   - 数据
   - 日志（可后置）

## 4.2 基础交互

1. 选择插件 -> 加载 manifest + 配置 schema + 运行状态
2. 启停插件 -> 触发 registry changed 并局部刷新
3. 删除插件 -> 二次确认 + 说明数据清理范围
4. 打开插件界面 -> 在详情区渲染该插件的 `config` 入口 UI

## 4.3 Host 与插件分工

Host 负责：

- 列表、启停、错误边界、权限可视化
- 通用表单渲染（Schema 驱动）
- 统一样式与导航

插件负责：

- 业务界面（如 Benchmark QA/Run 页面）
- 业务数据读写（走 capability API）

---

## 5. Benchmark 新插件设计（UI + Worker）

## 5.1 插件类型

- `kinds: ["ui", "worker"]`
- **不再注册 assistantType**

## 5.2 功能模块

1. **QA 集管理**
   - 新建/重命名/删除
   - 每个 Set 独立配置（judge_prompt、默认 runner/judge、并发、超时）
2. **QA 条目编辑**
   - 手动新增/编辑/删除
   - 支持批量粘贴
3. **CSV 导入**
   - 预览
   - 列映射
   - 校验错误展示
   - 导入结果回执
4. **运行中心**
   - 选择 Set
   - 选择 Runner（助手/模型）
   - 选择 Judge（助手/模型）
   - 启动 / 取消 / 重试
5. **结果中心**
   - 运行记录列表
   - 单次运行明细（每题得分、原因、答案）
   - 每题排行榜（TopN）

## 5.3 运行流程（高层）

1. 读取 Set 与配置
2. 创建 Run（状态 `queued` -> `running`）
3. 按题执行 runner
4. 用 judge_prompt 评分并产出结构化结果
5. 写入 run_items
6. 聚合总分/均分并更新排行榜
7. 发事件刷新 UI

---

## 6. Benchmark 持久化模型（建议）

## 6.1 `benchmark_sets`

- `id` (pk)
- `plugin_id`
- `name` (unique within plugin)
- `description`
- `judge_prompt`
- `default_runner_json`
- `default_judge_json`
- `created_at` / `updated_at`

## 6.2 `benchmark_items`

- `id` (pk)
- `set_id` (fk)
- `case_id` (业务可读 id)
- `question`
- `reference_answer`
- `tags_json`
- `weight` (default 1.0)
- `created_at` / `updated_at`

## 6.3 `benchmark_runs`

- `id` (pk)
- `set_id` (fk)
- `status` (queued/running/success/failed/cancelled)
- `runner_json`
- `judge_json`
- `judge_prompt_snapshot`
- `set_snapshot_hash`
- `total_score`
- `avg_score`
- `started_at` / `finished_at`

## 6.4 `benchmark_run_items`

- `id` (pk)
- `run_id` (fk)
- `item_id` (fk)
- `runner_answer`
- `judge_score`
- `judge_reason`
- `judge_raw`
- `latency_ms`
- `token_usage_json`
- `created_at`

---

## 7. API 设计（新增/调整）

## 7.1 插件中心通用 API

- `list_plugins`（已有）
- `enable_plugin` / `disable_plugin`（已有）
- `get_plugin_settings_schema`（新增）
- `get_plugin_settings`（新增）
- `set_plugin_settings`（新增）
- `get_plugin_permissions`（新增，可后置）

## 7.2 Benchmark 领域 API

- `benchmark_list_sets(plugin_id)`
- `benchmark_create_set(plugin_id, payload)`
- `benchmark_update_set(set_id, payload)`
- `benchmark_delete_set(set_id)`
- `benchmark_list_items(set_id, page, page_size)`
- `benchmark_upsert_items(set_id, items)`
- `benchmark_preview_csv(file_path, options)`
- `benchmark_import_csv(set_id, mapping, rows)`
- `benchmark_start_run(payload)`
- `benchmark_cancel_run(run_id)`
- `benchmark_get_run(run_id)`
- `benchmark_list_runs(set_id, page, page_size)`
- `benchmark_get_leaderboard(set_id, item_id?, top_n?)`

---

## 8. CSV 导入规范（首版）

必填列（支持映射）：

- `question`
- `reference`（或 `answer`）

可选列：

- `case_id`
- `tags`
- `weight`

导入策略：

1. 逐行校验，返回 `row_number + error`
2. 用户确认后再写入
3. 批量事务写入，失败整体回滚

---

## 9. 迁移与兼容策略

1. 当前 assistantType 的 benchmark 插件标记为“过渡实现”。
2. 新版 UI 插件落地后，默认入口切到“插件 > Benchmark”。
3. 如存在旧数据（KV JSON），提供一次性迁移脚本到结构化表。

---

## 10. 分阶段落地建议

### Phase A：插件中心壳子

- 设置页新增“插件”菜单
- 插件列表 + 启停 + 基础详情

### Phase B：配置契约

- settings schema/value API
- Host 统一配置渲染

### Phase C：Benchmark 数据层

- Set/Item/Run 表与 CRUD API
- CSV 导入链路

### Phase D：Benchmark 运行层

- Worker 执行编排
- 进度事件 + 取消
- 排行榜聚合

---

## 11. 验收标准（DoD）

1. 设置页出现“插件”菜单，且可查看已安装插件。
2. Benchmark 以独立插件页面使用，不依赖助手类型。
3. QA 集可手工创建与 CSV 导入，数据重启后仍在。
4. 每个 QA 集可独立配置 judge_prompt。
5. 可执行 benchmark，生成运行记录与每题排行榜。
6. 运行失败可定位到题目级错误，不出现静默失败。

---

## 12. 一句话结论

Benchmark 应从“对话助手插件”升级为“插件中心内的独立业务界面插件”，  
先补齐插件中心与结构化能力，再落地 QA 管理、评测运行与排行榜。
