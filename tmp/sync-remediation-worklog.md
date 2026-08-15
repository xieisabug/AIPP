# 同步系统修复 工作日志

> 依据 `docs/sync-remediation-plan.md` 实施。按时间倒序记录，最新在最上。

## 2026-08-15

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
