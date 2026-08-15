# 同步系统修复 工作日志

> 依据 `docs/sync-remediation-plan.md` 实施。按时间倒序记录，最新在最上。

## 2026-08-15

### P4-12（C10 部分）：sync.db 连接复用重构 —— 完成

- `src-tauri/src/sync.rs`：把 sync.db 辅助函数全部改为 conn 级签名，消除热循环里"每行/每对象重开连接"：
  - `ensure_device_id` / `ensure_object_id` / `find_object_id` / `find_local_id_by_object_id` / `save_object_map` / `upsert_shadow_from_remote` / `enqueue_snapshot_if_changed` / `record_dead_letter` / `update_dead_letter_failure` 改为接收 `&Connection`；删除 `ensure_official_object_id` 包装（调用方直接用 `upsert_official_object_map`）。
  - `scan_local_changes`：整轮扫描复用单个 sync 连接，`device_id` 只在入口取一次；`read_local_snapshots` / `scan_local_deletes` / `local_row_exists` 均复用该连接。
  - `pull_remote`：每页开一个 sync 连接复用于整页 `apply_change` / `record_dead_letter`；`replay_dead_letters` 单连接复用；`apply_change` / `apply_delete` 改为接收 sync 连接（map/shadow 操作不再各自重开）。
  - 顺带修复：上一小项新增的 `open_data_db` 递归调用自身（运行即栈溢出），改回 `Connection::open`；`enqueue_snapshot_if_changed` 损坏 payload 改报错、run_sync_once_inner 死分支删除此前已完成，本次一并验证。
- 验证：`cargo test sync::tests` 23 passed。

### P3-11（S7）：Docker 部署修正 —— 完成

- `sync-server/Dockerfile`（此前已改，本次补 README）：CMD 先 `alembic upgrade head` 再起 uvicorn；`VOLUME /app/data`；非 root 用户 `aipp` 运行。
- `README.md` 新增 Docker Deployment 章节：卷挂载、环境变量、自动迁移与非 localhost token 要求说明。
- 验证：Dockerfile 静态检查通过；本机 Docker Desktop 守护进程未运行，镜像构建未能实测（`docker build` 报 pipe 连接失败）。

### P3-9（C7+C8）：failed 事件生命周期与 outbox 清理 —— 完成

- `src-tauri/src/sync.rs`：
  - `sync_outbox` 新增 `next_retry_at INTEGER`（epoch 秒，避开 RFC3339 字典序比较陷阱）；`ensure_sync_db` 增加幂等列迁移 `ensure_column`（CREATE IF NOT EXISTS 不会给老表补列）。
  - 网络/5xx 批量失败（`mark_events_failed` → conn 级 `record_event_failure`）：保持 pending，`retry_count+1`，指数退避 `2^n` 秒封顶 300s，连续 10 次（`MAX_EVENT_RETRY_COUNT`）才转 failed。
  - `load_pending_outbox` 按 `next_retry_at IS NULL OR next_retry_at <= now` 过滤，退避中的事件不再每轮重复打服务器。
  - `enqueue_snapshot_if_changed` 入队新 upsert 前 `drop_failed_upserts_for_object` 清理同对象 failed 旧事件，避免旧 payload 被重试覆盖新数据。
  - push ack 后直接 DELETE outbox 行（不再残留 'acked'）；`reset_failed_events` 同时清 `next_retry_at`。
- 新增 2 个单测：退避到上限转 failed 的状态流转、failed 清理保留 pending。
- 验证：`cargo test sync::tests` 23 passed。

### P3-10（S4+S5+S6）：服务端输入校验与认证加固 —— 完成

- `sync-server/app/schemas.py`：新增 `SyncId` 约束类型（1-128 字符，pattern `^[A-Za-z0-9._:+/=\-]+$`）应用于 event_id/object_type/object_id/device_id；`base_version`/`local_version`/schema 版本加 `ge=0`；`device_name` 限长 256。
  - 与计划的偏差：pattern 必须包含 `+/=`，因为 natural object_id 内嵌标准 base64（`natural:<base64>`），按计划原样字符集会直接 422 掉所有 feature_config 同步。
- `sync-server/app/factory.py`：请求体大小限制中间件（按 Content-Length 提前 413，`AIPP_SYNC_MAX_REQUEST_BODY_BYTES` 默认 16MB，0 不限制；chunked 无 Content-Length 时由事件级校验兜底）。
- S5/S6：
  - `models.py`：`SyncToken` 新增 `expires_at`（NULL 永不过期，向后兼容存量 token）；alembic `0003_sync_token_expires_at`（已实测 upgrade 通过）。
  - `config.py`：`token_ttl_days` 默认 365（0 永不过期）；bootstrap token 创建时写入 expires_at。
  - `auth.py`：过期 token 返回 401（兼容 SQLite 存取的 naive datetime）。
  - 新增 `routes/admin.py` 并注册：token 列表/创建（明文只返回一次）/撤销/轮换，设备列表/撤销，全部按 account 隔离。
- `README.md`：补充管理 API、输入校验与限制说明、device_id 自报为 MVP 限制、MVP scope 更新（明确仅支持 SQLite）。
- 新增 7 个测试：非法字符 ID 422 / 负 base_version 422 / natural base64 ID 放行 / 超限请求体 413 / 过期 token 401 / token 创建-撤销-轮换全流程 / 设备撤销后 push 403。
- 验证：pytest 26 passed。

### P3-8（C6）：官方/内置种子数据确定性 object_id —— 完成

- `src-tauri/src/sync.rs`：
  - `SyncTableSpec` 新增 `official_column`：`llm.provider` → `is_official`，`mcp.server` → `is_builtin`。
  - `read_local_snapshots`：官方标记为真的行用确定性 object_id `official:{object_type}:{local_id}`（`official_object_id`/`parse_official_object_id`），经 `ensure_official_object_id`（核心为 conn 级 `upsert_official_object_map`）写入 map；老设备已有的随机 UUID 映射被就地替换，服务器侧旧随机对象经 C1 的删除检测自然收敛清理（一次性重复按计划文档认可的方式收敛）。
  - `apply_change`：远端官方对象按确定性 id 对齐——本机同 id 行存在则更新，不存在则 `insert_row_with_id` 按固定 id 插入（`insert_row` 重构出 `insert_row_inner`），避免 fresh 设备 pull 时重复插入。
  - `is_local_sync_scope_empty`：判空排除官方/内置行（`COALESCE(col,0)=0`），恢复纯拉取 bootstrap 分支可达。
- 新增 3 个单测：official id 生成/解析（含类型不匹配与非法格式）、随机 UUID 映射迁移为确定性 id、insert_row_with_id 定值插入。
- 验证：`cargo test sync::tests` 21 passed。
- 注：官方 provider 级联的 config/model 行不单独做确定性 id——它们不是固定种子（由用户操作产生），强行按 rowid 确定性化会把不同设备的无关行错误合并；它们通过 FK refs 指向确定性父 id 已可正常收敛。

### P2-6（C4+S 联合）：冲突语义重做（协议 v2）—— 完成

- 客户端 `src-tauri/src/sync.rs`：
  - `CLIENT_SCHEMA_VERSION` 升到 2；`ConflictEvent` 新增 `server_operation`（serde 默认 "upsert" 兼容旧服务端）。
  - conflict 分支改为**远端赢落地**：构造等价 PullChange 走 `apply_change` 写路径（server_operation=delete → apply_delete 语义 + 墓碑；upsert → server_payload 覆盖本地行 + 刷新 shadow），成功后**删除** outbox 事件（不再标 failed 死等）；落地失败才标 failed（该变更后续仍会经 pull 下发，不丢）。
  - `push_pending` 拆出 `push_pending_loop`，累计本轮远端赢解决数，更新 `SyncRuntimeStatus.conflict_resolved_count` 并 emit 状态；`SyncStatusDto` 新增该字段。
  - `reset_failed_events`（抽出 `reset_failed_events_inner`）：重试前用 shadow 当前 server_version 刷新 failed 事件的 base_version（COALESCE 子查询，无 shadow 保持原值），消除冲突重试死循环。
  - 批次错误只统计 rejected；冲突不再计入失败。
- 前端 `DataFolderConfigForm.tsx`：监听 `sync_status_changed` 时比较 `conflict_resolved_count` 增量，toast 提示"有 N 处修改与服务器冲突，已采用服务器版本"。
- 服务端 `config.py`：`max_client_schema_version = 2`（min 保持 1，旧客户端可继续按旧语义工作）。
- 新增 1 个单测（failed 重试刷新 base_version）。
- 验证：`cargo test sync::tests` 18 passed；服务端 pytest 19 passed；前端 `npm run build` 通过。
- 服务端部分（默认 conflict、server_operation、event_id 内容校验）此前已完成并提交（e6d4425）。

### P1-5（C5）：切换服务器/账号时重置同步状态 —— 完成

- `src-tauri/src/sync.rs`：
  - `SYNC_SCHEMA_SQL` 新增 `sync_meta(key, value)` 表；新增 conn 级辅助 `get_sync_meta`/`set_sync_meta`/`needs_reset`/`has_sync_state`/`clear_sync_state`（纯 SQL，可内存库测试）。
  - `save_sync_settings`：保存前读取旧配置，`server_url` 或 token 实际变更且 sync.db 已有状态（cursor/shadow/map 任一非空）时，保存配置但置 `needs_reset=1` 并跳过自动同步（warn 日志），不再静默沿用旧 cursor/shadow。
  - `run_sync_once_inner`：`needs_reset` 状态下拒绝执行 push/pull，返回明确错误提示用户去设置里确认重置。
  - 新 command `reset_sync_state`：清空 cursor/shadow/map/outbox/dead_letter 五张表 + 解除 needs_reset，随后立即触发全量同步（bootstrap）。
  - `SyncStatusDto` 新增 `needs_reset` 字段。
  - `lib.rs` 注册 `reset_sync_state`。
- 前端 `DataFolderConfigForm.tsx`：`needs_reset` 时展示警告说明行和"重置并重新全量同步"按钮，点击弹出 `ConfirmDialog` 确认后调用 `reset_sync_state`。
- 新增 3 个单测：needs_reset 读写、has_sync_state 检测、clear_sync_state 全清。
- 验证：`cargo test sync::tests` 17 passed；`npm run build` 通过。

### P1-4（C1）：本地删除同步上传 —— 完成

- `src-tauri/src/sync.rs`：
  - `read_local_snapshots` 返回值改为 `(snapshots, present_ids)`：外键未决被跳过的行也计入 present 集合，避免删除检测误判"暂时跳过"为"已删除"。
  - `scan_local_changes` 每张表 upsert 扫描后执行 `scan_local_deletes`：shadow 中非墓碑且不在 present 集合的对象（`find_deleted_candidates`），经 `local_row_exists` 确认本地行确实不存在（忽略 where_clause 范围过滤）后，由 `enqueue_delete_event` 入队 delete 事件（去重、同时移除同对象的 pending/failed upsert）。
  - 新增后未推送即删除的场景：`find_stale_pending_upserts` 找出 outbox 中残留 upsert，确认行已删除后直接丢弃，防止幽灵对象推上服务器。
  - push ack 路径抽出 `apply_ack_to_shadow`：delete 事件置 shadow 墓碑（保留行供去重）并删除 `sync_object_map` 映射；upsert 行为不变。
  - `apply_delete`（远端删除落地）同步删除 `sync_object_map` 映射，同 object_id 后续 upsert 走重新 insert（对象复活）。
  - 建表 DDL 抽为 `SYNC_SCHEMA_SQL` 常量，单测可在内存库复用同一 schema。
- 新增 4 个单测：删除候选过滤（墓碑/present 跳过）、delete 入队去重+清理 stale upsert、delete ack 墓碑+map 清理/upsert ack 复活、stale upsert 检测。
- 验证：`cargo test sync::tests` 14 passed。
- 已提交：服务端修复 e6d4425。

### P0-2（C3）：Pull dead-letter 防卡死 —— 完成

- `src-tauri/src/sync.rs`：
  - `ensure_sync_db` 新增 `sync_dead_letter` 表（id/object_type/object_id/operation/change_json/error/failed_at/retry_count + 对象索引）。
  - `pull_remote` 重写单条失败处理：`apply_change` 失败时，依赖未决类错误（"缺少同步依赖"/"远端变更缺少外键引用"前缀，新函数 `is_dependency_pending_error`）进本页 deferred 列表，页末重放一次；仍失败或其他错误 → `record_dead_letter` 落库（同对象只留最新一条），继续后续 change，cursor 照常推进。死信写入失败只记 warn 不阻断 pull。
  - pull 全部完成后自动 `replay_dead_letters` 一次（自愈：跨页依赖此时已就位）；新增 `retry_sync_dead_letters` command 手动重放（成功删行、失败更新 error/retry_count）。
  - `PullChange` 加 `Serialize`（死信落库用）；`SyncStatusDto` 加 `dead_letter_count`，`sync_status` 查表填充。
  - `src-tauri/src/lib.rs` 注册 `retry_sync_dead_letters`。
- 前端部分此前已完成（`DataFolderConfigForm.tsx` 展示 dead_letter_count + 重试按钮），现在前后端对齐。
- 新增 1 个单测（依赖错误分类）。
- 验证：`cargo test sync::tests` 10 passed。
- 注：计划中的"毒消息 + 正常消息同页"集成测试需要真实 AppHandle 数据目录，暂缓（同 C2 的取舍）；核心分支逻辑由单测 + 代码审查覆盖。

### 构建环境问题：cargo test 报 0xc0000139 —— 已修复并验证

- 现象：测试二进制编译通过但启动即 `STATUS_ENTRYPOINT_NOT_FOUND`。
- 定位：用 pefile 分析导入表，测试 exe 静态导入 `TaskDialogIndirect`（tauri 对话框依赖链），但 exe 未嵌入 RT_MANIFEST，loader 绑定到 comctl32 v5（无该导出）。tauri-build 经 winres/embed-resource 只把 manifest 链进 bin 目标（`rustc-link-arg-bins`），lib 测试二进制没有。
- 最终方案：`src-tauri/build.rs` 在 Windows 下改用 `tauri_build::try_build(Attributes::new().windows_attributes(WindowsAttributes::new_without_app_manifest()))` 关闭 tauri 默认 manifest 注入，再由 `embed_resource::compile_for_everything` 把一份与 tauri 默认一致的 manifest（comctl32 v6 dependency）链进所有目标；`Cargo.toml` build-dependencies 加 `embed-resource = "3"`。非 Windows 仍走 `tauri_build::build()`。
- 走过的弯路（已回退）：`cargo:rustc-link-arg-tests` 被 cargo 1.94 拒绝（包无 `[[test]]` 目标），曾加锚点测试文件，确认该指令不作用于 lib unittest 后已删除。
- 验证：
  - `cargo test ... sync::tests` 全绿（9 passed，含 C2 的 6 个新测试），测试 exe 不再报 0xc0000139。
  - pefile 检查 `src-tauri/target/debug/Aipp.exe`：RT_MANIFEST（/24/1/1033）存在且含 comctl32 v6（`6595b64144ccf1df`），主程序 manifest 未受影响。
- 备注：用户运行中的 Aipp.exe 锁定 target 下部分文件属正常现象。

### P2-6（C4 服务端部分）：冲突语义默认拒绝 stale —— 完成

- `sync-server/app/services/conflict.py`：stale 写入默认全部返回 conflict；LWW 白名单改为 `Settings.stale_lww_types`（默认空），原 message/artifact 硬编码白名单移除。
- `sync-server/app/schemas.py`：`ConflictEvent` 新增 `server_operation`（`upsert`/`delete`），区分墓碑与空 payload。
- `sync-server/app/services/merge.py` / `routes/push.py`：冲突响应填充 `server_operation`；event_id 幂等重放增加内容校验（object_type/object_id/operation/payload 不一致 → rejected `event_id_conflict`）。
- 新增 4 个测试：非白名单 stale 冲突 / 白名单 LWW 放行 / event_id 篡改拒绝 / 墓碑冲突 server_operation=delete。
- 验证：`pytest app/tests/test_sync_api.py` 19 passed。
- 客户端部分（远端赢落地、failed base_version 刷新、协议 v2 协调）待做。

### P2-7（S1+S2 服务端部分）：并发写防护 —— 完成

- `sync-server/app/db.py`：SQLite 连接加 `PRAGMA busy_timeout=5000 / journal_mode=WAL / foreign_keys=ON`（connect event listener），并发写不再立刻 `database is locked`，外键 cascade 生效。
- `sync-server/app/models.py`：`sync_change` 新增唯一约束 `uq_sync_change_account_object_version (account_id, object_type, object_id, version)`，作为并发版本竞争的兜底。
- `sync-server/app/services/merge.py`：`SyncObject` 读取加 `with_for_update=True` 行锁（PG 下生效，SQLite 下无操作）。
- `sync-server/app/routes/push.py`：每个事件在 savepoint 中处理；`IntegrityError` 走 `recover_from_integrity_error`（重复 event_id → 幂等返回已存结果；版本撞车 → conflict）；其他单事件异常记 rejected(`internal_error`)，不再整批 500 回滚。
- 新增 alembic 迁移 `0002_sync_change_version_unique`（batch 模式加约束，已用临时库验证 `alembic upgrade head` 通过）。
- 新增 3 个测试：busy_timeout 生效、混合批次中单事件撞约束不影响其他事件（200 + accepted/conflict 分流）、`recover_from_integrity_error` 幂等路径。
- 验证：`pytest app/tests/test_sync_api.py` 15 passed。
- S2 的 seq 空洞问题按计划记录为已知限制：服务端明确仅支持 SQLite（README 更新留待 P3-11 一并处理）。

### P0-1（C2）：FK 未解析不再发送裸 rowid —— 完成

- `src-tauri/src/sync.rs`：
  - 新增 `build_fk_refs`：任一非空外键无法解析为同步 object_id 时返回 `None`；`read_local_snapshots` 对该行跳过本轮入队（debug 日志），下轮扫描自然重试，payload 中不再出现本地整数 rowid。
  - 新增 `resolve_remote_foreign_keys`：接收端在 FK 有值但 refs 缺失时返回明确错误，拒绝把来源设备的本地 rowid 写进本机 DB。
  - `apply_change` 改用该函数。
- 新增 6 个单元测试（refs 完整解析 / 依赖未决返回 None / null FK 跳过 / 接收端正常改写 / refs 缺失拒绝 / null 放行），纯函数测试不依赖 AppHandle。
- C3 前端部分已同步完成：`DataFolderConfigForm.tsx` 增加 `dead_letter_count` 展示与"重试无法应用的变更"按钮（调用待实现的 `retry_sync_dead_letters`）。
- 验证：`cargo test sync::tests` 9 passed（含上述 6 个新测试），编译与 manifest 问题一并解决（见上条）。

### P0-3（S3）：移除服务端公开默认 token —— 完成

- `sync-server/app/config.py`：`bootstrap_token` 默认值改为 `None`；新增 `is_local_base_url()` 与 `validate_bootstrap_security()`——非 localhost 且 token 为空或为公开默认值 `dev-token` 时拒绝启动；localhost 且未配置时打印 warning。
- `sync-server/app/factory.py`：`create_app` 启动时先执行安全校验。
- `sync-server/README.md`：更新 bootstrap token 说明与部署示例（`openssl rand -hex 32`）。
- 新增 4 个测试（非 localhost 无 token 拒启 / dev-token 拒启 / 私有 token 正常启动 / localhost 无 token 启动但不建 token）。
- 验证：`pytest app/tests/test_sync_api.py` 12 passed。

### 开始实施

- 建立本日志与任务清单，按 P0 → P4 顺序推进。
