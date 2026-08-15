# 数据目录同步系统修复实施计划

> 来源：2026-08 对客户端 `src-tauri/src/sync.rs` 与服务端 `sync-server/` 的整体 code review。
> 本文档按优先级排列修复项，每项包含：问题位置、改动方案、涉及函数、测试要求、验收标准。
> 问题编号沿用 review 报告（C = 客户端，S = 服务端）。

## 总体原则

- **先保证不丢数据，再保证体验**：P0/P1 全部是数据正确性问题，未修完前同步功能不对真实用户开放。
- **协议变更集中处理**：C4、S1 涉及 push/pull 协议语义变化，客户端与服务端必须同版本发布，通过 `schema_version` 握手保护（复用现有 `min/max_client_schema_version` 机制）。
- **每项改动必须带测试**：客户端用内存 SQLite（`Connection::open_in_memory`），服务端沿用 `app/tests/test_sync_api.py` 的 pytest 风格。

---

## P0 — 阻断级（不修则必然损坏数据）

### 1. C2：外键未解析时禁止发送裸本地 rowid

**问题**：`sync.rs:847-853` 把含本地 FK 整数的列原样放入 `fields`；`sync.rs:864-870` 仅在 `find_object_id` 命中时写 `refs`。接收方 `apply_change`（`sync.rs:968-980`）在 refs 缺失时保留裸整数，导致消息挂到错误的父消息/模型上。

**改动**：

- `sync.rs` `scan_local_changes`（约 806-884 行）：
  - 遍历 `spec.foreign_keys` 时，若 FK 值非空但 `find_object_id` 返回 `None`，则该行的快照标记为"依赖未就绪"：本轮跳过入队（不写入 fields 裸 id），并在日志中记录一次 `warn`。
  - 实现方式：`build_snapshot` 返回值改为 `Result<Option<LocalObjectSnapshot>>`，`None` 表示依赖未决。未决行不更新 shadow，下轮扫描自然重试（依赖对象通常下一轮就已入队）。
- `sync.rs` `apply_change`（约 940-993 行）：接收端防御——`spec.foreign_keys` 中任一 FK 在 `refs` 中缺失且 `fields` 里是非空整数时，返回明确的依赖错误（与现有 `sync.rs:971-977` 行为一致，但错误信息带上 object_type/object_id/缺失的 FK 列名，便于诊断）。

**测试**（`src-tauri/src/` 新增 `sync_tests.rs` 或并入现有测试模块）：

- 构造 message 的 `parent_id` 指向未同步消息：断言不产生 outbox 事件、shadow 不前进。
- 下一轮扫描在父消息入队后：断言该 message 正常入队且 `refs` 含 UUID。
- 接收端收到缺 refs 的 change：断言报错且不写行。

**验收**：任何情况下推送的 payload 中 FK 列只能是 UUID 引用或 null，绝不出现本地整数 rowid。

### 2. C3：Pull 增加 dead-letter，毒消息不再卡死同步

**问题**：`sync.rs:788-790` 任一 change apply 失败则整页失败、cursor 不前进，此后每轮 pull 卡在同一条，同步永久中断。

**改动**：

- `sync.db` 新增表 `sync_dead_letter`（id、object_type、object_id、operation、payload_json、error、failed_at、retry_count），迁移写在 `ensure_sync_db` 的建表区（约 1330-1370 行）。
- `pull_remote`（约 760-800 行）：单条 `apply_change` 失败时：
  1. 先判断是否为"依赖未决"类错误（C1 引入的错误类型）——是则记入本页 deferred 列表，本页其余 change 处理完后重放一次；
  2. 重放仍失败或非依赖错误 → 写入 `sync_dead_letter`，继续处理后续 change；
  3. 整页处理完后 cursor 照常推进（dead-letter 条目不阻塞 cursor）。
- `sync_status` DTO 增加 `dead_letter_count` 字段，前端同步设置页展示"有 N 条变更无法应用"并提供"重试死信"按钮（新 command `retry_sync_dead_letters`：把死信按序重放，成功则删除）。
- 服务端已有变更不会重复下发，因此 dead-letter 重试的数据源是本地落库的 payload，不需要重新 pull。

**测试**：

- 注入一条永远失败的 change + 一条正常 change：断言正常的被应用、cursor 推进、死信表有 1 条。
- 依赖 deferred 场景：本页内子消息排在父消息之前，断言重放后两条都应用成功、无死信。

**验收**：构造毒消息后同步不再中断，状态页可见死信数量，重试入口可用。

### 3. S3：移除公开默认 token

**问题**：`sync-server/app/config.py:16` `bootstrap_token` 默认值 `"dev-token"` 写在公开 README 中，忘配环境变量即全网可读写。

**改动**：

- `config.py`：`bootstrap_token: str | None = None`。
- `factory.py` / `main.py` 启动时校验：若 `base_url` 非 localhost 且 `bootstrap_token` 为空或等于 `"dev-token"`，直接 `raise RuntimeError` 拒绝启动；localhost 下为空时打印醒目 warning。
- `README.md` 部署章节同步更新：明确必须设置 `AIPP_SYNC_BOOTSTRAP_TOKEN`，给出 `openssl rand -hex 32` 示例。

**测试**：`test_sync_api.py` 新增：默认配置（无 env）在非 localhost base_url 下启动报错。

**验收**：全新部署不配置 token 时服务无法启动或仅监听 localhost。

---

## P1 — 数据一致性（确定性发散来源）

### 4. C1：本地删除同步上传

**问题**：`sync.rs:921-935` 入队硬编码 `'upsert'`，删除的行不再出现在扫描结果中，永远不会产生 delete 事件，多端发散。

**改动**：

- `scan_local_changes`（约 806-818 行）：扫描完每张表后，对比 `sync_shadow` 中该 object_type 的现存条目与本轮快照集合——shadow 有而快照无且 `deleted_at IS NULL` 的 object_id，通过 `sync_object_map` 反查确认本地行确实不存在后，入队 `operation='delete'`、payload 为 null 的事件。
- 推送成功 ack 时（约 1200-1210 行附近）：delete 事件把 shadow 的 `deleted_at` 置为当前时间（保留墓碑供下轮对比，避免重复入队），并删除 `sync_object_map` 对应映射。
- 注意顺序：delete 检测必须在 upsert 扫描之后，且只对本设备已 ack 过的对象生效（shadow 有 server_version），避免把"从未推送过的新增后删除"误判为 delete 事件——这种情况直接清理 shadow 即可，无需上传。
- `apply_delete`（约 995-1004 行）顺手修复：删除本地行后同步删除 `sync_object_map` 映射，保证同 object_id 后续 upsert 能重新 insert（对象复活）。

**测试**：

- 删除已同步的会话 → 下一轮 outbox 出现 delete 事件 → 推送后 shadow 有墓碑、map 清理。
- 新增未推送即删除 → 不产生 delete 事件。
- 远端 delete 拉到本地后再收到同 object_id 的 upsert → 本地重新插入行（复活）。

**验收**：A 设备删除会话，B 设备下一轮 pull 后该会话消失。

### 5. C5：切换服务器/账号时重置同步状态

**问题**：`sync.rs:504-545` `save_sync_settings` 覆盖 server_url/token 但保留 cursor/shadow/map/outbox，换服务器后 cursor 错位、shadow 错配。

**改动**：

- `save_sync_settings`：检测 `server_url` 或 token 与已存值不同且 sync.db 中存在任何 cursor/shadow/map 记录时，**不静默清空**——先保存配置但将同步置为 `needs_reset` 状态（新列或新表 `sync_meta`），worker 与手动同步在该状态下拒绝执行并在 `sync_status` 中返回 `needs_reset: true`。
- 新 command `reset_sync_state`：用户在前端确认"重新全量同步"后，清空 `sync_cursor`/`sync_shadow`/`sync_object_map`/`sync_outbox`/`sync_dead_letter` 四表+游标，解除 `needs_reset`。
- 前端 `DataFolderConfigForm.tsx`：检测到 `needs_reset` 时弹确认框（说明本机数据将与新服务器重新对齐），确认后调用 `reset_sync_state` 再触发同步。

**测试**：

- 保存新 server_url 后 `sync_status` 返回 `needs_reset=true`，`run_sync_once` 不执行 push/pull。
- 调用 `reset_sync_state` 后四表清空、状态解除、全量 bootstrap 正常。

**验收**：换服务器后不会出现 cursor 错位导致的漏拉/错拉，用户有明确的重置确认动作。

---

## P2 — 冲突与并发正确性（协议变更，客户端+服务端同发）

### 6. C4+S 联合：冲突语义重做

**问题**：
- 服务端 `conflict.py:13-20` 仅对 message/artifact 返回 conflict，其余类型 stale 写入按 LWW 静默覆盖（无测试覆盖）。
- 客户端 `sync.rs:1251-1289` 收到 conflict 只更新 shadow 不落地，随后 pull 静默覆盖本地未推送修改；failed 事件 `base_version` 冻结，重试死循环。

**改动**（协议版本从 1 升到 2，两端同时发布）：

服务端：

- `conflict.py`：默认对所有 object_type 的 stale 事件返回 conflict；白名单改为显式配置（`Settings.stale_lww_types: list[str] = []`，默认空）。后续若某类型确需 LWW，逐项评估后加入配置。
- `schemas.py` `ConflictEvent` 增加 `server_operation: str`（`"upsert" | "delete"`）字段，解决 `server_payload=None` 无法区分"对方删除"与"空 payload"的问题。
- `merge.py`：幂等重放校验加强——`event_id` 命中已有 change 时比对 `object_id` 与 `payload_hash`，不一致返回 `RejectedEvent(reason="event_id_conflict")`，防止客户端 bug 复用 event_id 导致静默丢数据。

客户端：

- `push_pending` 的 conflict 分支（约 1251-1289 行）改为**远端赢落地**：
  1. `server_operation == "delete"` → 对本地执行 `apply_delete` 语义；
  2. `server_operation == "upsert"` → 用 `server_payload` 走与 `apply_change` 相同的写路径更新本地行；
  3. 更新 shadow（现有逻辑保留）；
  4. **删除该 outbox 事件**而不是标 failed（冲突已解决，无需重试）。
- 远端赢意味着本地未推送修改被覆盖，因此必须在 UI 可感知：`sync_status_changed` 事件 payload 增加 `conflict_resolved_count`，前端 toast 提示"有 N 处修改与服务器冲突，已采用服务器版本"。
- `reset_failed_events`（约 560-576 行调用的重置函数）：重试前用当前 shadow 的 `server_version` 刷新 failed 事件的 `base_version`，消除死循环。

**测试**：

- 服务端：两设备基于同一 base_version 编辑同一 conversation，后到者收到 conflict（原 LWW 路径反转）。
- 客户端：构造 conflict 响应 → 断言本地行被 server_payload 覆盖、outbox 事件删除、shadow 更新。
- 客户端：failed 事件重试时 base_version 已刷新，二次推送不再冲突。

**验收**：双端离线编辑同一对象，恢复在线后：后到修改方收到冲突提示，最终两端数据收敛一致，无永久 failed 事件。

### 7. S1+S2：服务端并发写防护与 cursor 提交序

**问题**：
- `merge.py:65-101` 读-改-写无锁，并发 push 同对象版本错乱（PostgreSQL 下静默丢数据，SQLite 下 500）。
- event_id 幂等"先查后插"（`merge.py:50-63`），并发重复 push 撞唯一索引整批 500。
- `pull.py:25-35` 的 seq 空洞问题在 PostgreSQL 下丢数据。

**改动**：

- `merge.py` `process_push_event`：
  - `db.get(..., with_for_update=True)` 锁定 SyncObject 行（SQLite 下退化为无操作，无害）；
  - version 递增改为条件更新兜底：`UPDATE sync_object SET version = :v + 1 ... WHERE version = :expected`，rowcount 为 0 时转为 conflict 返回；
  - event_id 幂等改为先 `INSERT ... ON CONFLICT DO NOTHING`（或捕获 `IntegrityError` 后按 event_id 重查走幂等返回），单事件失败不再 500 整批。
- `models.py`：`sync_change` 增加 `(account_id, object_type, object_id, version)` 唯一约束（alembic 新增迁移 `0002`）。
- `db.py`：连接 event listener 执行 `PRAGMA busy_timeout=5000; PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON`（同时修复外键 cascade 不生效问题）。
- `push.py`：整批事务保留，但单事件异常按事件粒度捕获记入 rejected，不再整批回滚。
- 文档明确：当前版本**仅支持 SQLite**（README 删除 PostgreSQL 暗示）；待将来换 PG 时用单调水位线方案解决 seq 空洞（记录为已知限制，不在本期实现）。

**测试**（新增并发用例，用两个 Session 模拟交错提交）：

- 并发 push 同一 object_id：一个 accepted 一个 conflict，无 500。
- 并发重复 event_id：两个请求都返回 accepted 且版本一致，change 表只有一条。
- 一批中混一条坏事件：其余事件 accepted，坏事件 rejected，无整批回滚。

**验收**：并发压测（10 线程 × 100 事件）无 500、无版本空洞、无重复 change。

---

## P3 — 健壮性与服务端加固

### 8. C6：内置/种子数据确定性 object_id

**问题**：`sync.rs:223-330` 各表 `natural_key_columns` 全为空，官方 provider（固定 id 1/10/20/30 种子）在每设备映射为不同随机 UUID，多端接入后全套重复；`is_local_sync_scope_empty`（约 1572-1589 行）因此永远为 false，纯拉取 bootstrap 分支不可达。

**改动**：

- `ensure_object_id`（约 1461-1487 行）：对 `is_official = 1` 的 `llm_provider`、其级联 config/model、`is_builtin = 1` 的 `mcp_server`，使用确定性 object_id（格式 `official:{object_type}:{local_id}`），保证各设备对同一内置行生成相同 object_id。
- 已存在随机 UUID 映射的老设备：升级时在 `ensure_sync_db` 里做一次映射迁移——把官方行的 map 条目从随机 UUID 改为确定性 id（需先在服务器侧确认该 UUID 无已推送数据，或接受一次性重复后自然收敛；方案取舍在实施时以数据量评估为准）。
- `is_local_sync_scope_empty`：排除 `is_official`/`is_builtin` 行后再判空，恢复纯拉取 bootstrap 分支可达。

**测试**：两台"全新设备"各有官方种子，A push 后 B pull，断言 B 不出现重复 provider/model。

### 9. C7+C8：failed 事件生命周期与 outbox 清理

**改动**：

- `push_pending` 网络/5xx 失败（约 744-751 行）：整批标 failed 改为**保持 pending 并记录 `retry_count`+指数退避**（新增 `next_retry_at` 列，`load_pending_outbox` 按 `next_retry_at <= now` 过滤）；连续失败超过阈值（如 10 次）才转 failed。
- 入队新 upsert 时（`enqueue_snapshot_if_changed`，约 890 行）：删除同 `(object_type, object_id)` 的所有 failed 旧事件，避免旧 payload 覆盖新数据。
- ack 后删除 outbox 事件（替代标 acked 永久残留）；如担心排查困难，可保留最近 7 天再物理删除。

**测试**：网络失败 3 次内自动重试成功；同对象新旧事件交替时旧 failed 被清理；ack 后 outbox 行数不增长。

### 10. S4+S5+S6：服务端输入校验与认证加固

**改动**：

- `schemas.py`：所有 ID 字段 `Field(min_length=1, max_length=128, pattern=r"^[A-Za-z0-9._:-]+$")`；`base_version: int = Field(ge=0)`，移除 `None` 语义；`events` 列表长度与 `max_events_per_push` 对齐校验。
- ASGI 层加请求体大小上限中间件（与 `max_payload_bytes` 同量级，默认 16MB，可配）。
- `auth.py`：token 增加 `expires_at`（默认 1 年，可配）+ `rotate_sync_token` 管理端点；`revoke` 补齐 token/设备的创建与撤销 API（或文档明确 MVP 不提供并删除悬空字段的误导）。
- 设备注册：README 与代码注释明确 device_id 自报是 MVP 限制，后续改为服务端签发。

**测试**：超长/非法字符 ID 422；负 base_version 422；超限请求体 413；过期 token 401；撤销 API 生效。

### 11. S7：Docker 部署修正

**改动**：

- `Dockerfile`：CMD 改为先 `alembic upgrade head` 再起 uvicorn（或 entrypoint 脚本）；声明 `VOLUME /app/data`；新增非 root 用户运行。
- `README.md` 部署章节补充卷挂载示例与迁移说明。

---

## P4 — 性能与体验（正确性问题清零后再做）

### 12. C10：扫描性能

- `scan_local_changes`/`enqueue_snapshot_if_changed`/`ensure_object_id`/`find_object_id` 改为单连接复用 + 批量事务（当前每行 3-4 次新连接）。
- 中期：业务 API 写后 hook（30+ 处 `schedule_sync_after_local_change` 调用点）直接携带变更对象信息入队，扫描退化为兜底对账（低频，如每 10 分钟）。
- 长期：各表增加 `updated_time` 索引做增量扫描（需 db migration，单独评估）。

### 13. C9/C14/轻微项打包

- `apply_change` 写行 + map + shadow 的顺序调整为先 shadow 后写行，并对 echo 回推做幂等去重（payload_hash 相同则跳过入队）。
- pull 落库后按 object_type emit 领域事件（`conversation_changed`、`assistant_changed` 等），前端对应列表刷新。
- 清理 `sync.rs:689-693` 死代码分支；`sync.rs:1123-1125` 损坏 payload 报错而非静默 None；数据 DB 打开加 `busy_timeout`。
- C11：使用 http 服务器地址时同步设置页显示明文传输告警；C12：master key 迁移到 OS keychain（keyring crate）；C13：敏感列过滤从黑名单改为白名单（仅同步已知安全列）。

---

## 测试总览

| 层 | 新增测试 | 覆盖问题 |
| --- | --- | --- |
| 客户端 `sync_tests.rs` | FK 未决跳过/补发、dead-letter 流程、delete 上传/复活、needs_reset 流程、conflict 远端赢落地、failed 退避 | C1/C2/C3/C4/C5/C7 |
| 服务端 `test_sync_api.py` | 并发 push 同对象、并发重复 event_id、默认 conflict 反转、输入边界、token 过期/撤销、启动 token 校验 | S1/S2/S3/C4/S5/S6 |
| 端到端手测清单 | 双设备离线编辑收敛、换服务器重置、全新设备 bootstrap、种子数据不重复 | 全量 |

## 发布与迁移注意事项

- P2（冲突语义）为协议变更：服务端先发布并开启 `min_client_schema_version = 2` 的时间窗需与客户端发版协调；旧客户端推送 stale 事件时按旧语义处理还是拒绝，需在实施时明确（建议直接拒绝并提示升级）。
- C6 的 object_id 迁移会影响已开启同步的用户，需要一次性数据收敛方案与公告。
- 每期完成后更新 `docs/product/` 对应文档与 `AGENTS.md` 的 Critical Features 描述。
