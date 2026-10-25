# AIPP Sync Server

## 简介

AIPP Sync Server 是面向 AIPP 桌面端数据的**业务级同步服务**（FastAPI MVP）。

它**不会**直接同步 SQLite 数据库文件。客户端通过 HTTP API 推拉业务事件：

| 能力 | 路径 |
|------|------|
| 推送本地变更 | `POST /v1/sync/push` |
| 拉取远端变更 | `GET /v1/sync/pull` |
| 实时通知（SSE） | `GET /v1/sync/notify` |

服务端保存同步对象快照、变更日志、设备与基于 Token 的账号隔离。

---

## 核心概念

### Account（账号 / 同步空间）

Account 是服务端的同步空间，**不是**当前桌面端的登录账号。

本 MVP 里：所有绑定到同一 `sync_account.id` 的 Token，读写同一份远端变更日志与对象快照。

本地开发常用：

```text
account_id: default
token:      dev-token
```

启动时，仅当设置了 `AIPP_SYNC_BOOTSTRAP_TOKEN` 才会自动创建 bootstrap 账号与 Token（**代码中无默认 token**）。  
若 `AIPP_SYNC_BASE_URL` 指向**非 localhost**，服务会拒绝空 token，以及公开文档里的 `dev-token`。

### Token（令牌）

客户端认证方式：

```http
Authorization: Bearer <token>
```

服务端在表 `sync_token` 中只存 token 的哈希，不存明文。

本地开发请显式设置，例如：

```bash
export AIPP_SYNC_BOOTSTRAP_TOKEN=dev-token
```

正式部署请生成私有 token：

```bash
openssl rand -hex 32
```

Token 默认有效期由 `AIPP_SYNC_TOKEN_TTL_DAYS` 控制（默认 365 天；`0` 表示永不过期）。可用 `/v1/admin/*` 管理。

### Device（设备）

Device 表示一台 AIPP 客户端安装。

- Push 请求在 JSON 里带 `device_id`
- Pull / Notify 请求在请求头带 `X-AIPP-Device-ID`

首次看到「合法 Token + device_id」时，服务端会自动注册设备。已吊销的设备不能再推拉。

> MVP 限制：`device_id` 由客户端自报。后续版本会改为服务端签发的设备凭证。

---

## 最快上手（傻瓜一键）

前提：本机已安装 [Docker](https://docs.docker.com/get-docker/) 与 Docker Compose。

```bash
cd sync-server
docker compose up -d --build
curl http://127.0.0.1:8080/health
```

期望输出：

```json
{"status":"ok"}
```

然后在客户端填写：

- 同步服务地址：`http://127.0.0.1:8080`（或你的局域网 IP，如 `http://192.168.x.x:8080`）
- Bearer Token：`dev-token`

Compose 默认环境变量：

- `AIPP_SYNC_BASE_URL=http://127.0.0.1:8080`（视为本机，允许 `dev-token`）
- `AIPP_SYNC_BOOTSTRAP_TOKEN=dev-token`

> 生产环境请换成私有 token，并把 `AIPP_SYNC_BASE_URL` 改成非 localhost 的地址（非本机时拒绝空 token 与公开的 `dev-token`）。

数据持久化在命名卷 `aipp-sync-data` → 容器内 `/app/data`。停止服务：

```bash
docker compose down
```

---

## 本地开发（uv + Python 3.12）

### Windows（PowerShell）

```powershell
cd sync-server
uv venv --python 3.12
.\.venv\Scripts\Activate.ps1
uv pip install -e ".[dev]"

$env:AIPP_SYNC_BOOTSTRAP_TOKEN = "dev-token"
uvicorn app.main:app --host 127.0.0.1 --port 8080
```

若提示缺少 Python 3.12：

```powershell
uv python install 3.12
uv venv --python 3.12
```

### bash / macOS

```bash
cd sync-server
uv venv --python 3.12
source .venv/bin/activate
uv pip install -e ".[dev]"

export AIPP_SYNC_BOOTSTRAP_TOKEN=dev-token
uvicorn app.main:app --host 127.0.0.1 --port 8080
```

本地默认：

```text
服务地址:  http://127.0.0.1:8080
数据库:    ./data/aipp-sync.db
账号:      default
Token:     需自行设置 AIPP_SYNC_BOOTSTRAP_TOKEN（本地常用 dev-token）
```

---

## Docker 单容器

不使用 Compose 时，可直接 build / run：

```bash
cd sync-server
docker build -t aipp-sync-server .
docker run -d --name aipp-sync \
  -p 8080:8080 \
  -v aipp-sync-data:/app/data \
  -e AIPP_SYNC_BASE_URL=https://sync.example.com \
  -e AIPP_SYNC_BOOTSTRAP_TOKEN=$(openssl rand -hex 32) \
  aipp-sync-server
```

说明：

- 容器以非 root 用户 `aipp` 运行；启动时先执行 `alembic upgrade head`，再启动 uvicorn（见 `Dockerfile` `CMD`）
- SQLite 默认路径相对工作目录 `/app`，即 `/app/data/aipp-sync.db`；**务必挂卷**到 `/app/data`，否则重建容器会丢数据
- `AIPP_SYNC_BASE_URL` 非 localhost 时，必须设置私有的 `AIPP_SYNC_BOOTSTRAP_TOKEN`
- 本版本仅支持并测试过 SQLite 后端

---

## 环境变量

环境变量前缀：`AIPP_SYNC_`（由 `app/config.py` 的 `Settings` 读取，也可写在 `.env`）。

| 环境变量 | 代码字段 | 默认值 | 说明 |
|----------|----------|--------|------|
| `AIPP_SYNC_DATABASE_URL` | `database_url` | `sqlite:///./data/aipp-sync.db` | SQLAlchemy 连接串；Docker 下相对 `/app` |
| `AIPP_SYNC_BASE_URL` | `base_url` | `http://localhost:8080` | 用于判断是否本机部署；本机 host：`localhost` / `127.0.0.1` / `::1` |
| `AIPP_SYNC_BOOTSTRAP_ACCOUNT_ID` | `bootstrap_account_id` | `default` | 启动时创建的 bootstrap 账号 id |
| `AIPP_SYNC_BOOTSTRAP_TOKEN` | `bootstrap_token` | （空 / `None`） | 有值才创建 bootstrap token；非本机禁止空值与 `dev-token` |
| `AIPP_SYNC_TOKEN_TTL_DAYS` | `token_ttl_days` | `365` | Token 有效期（天）；`0` = 永不过期 |
| `AIPP_SYNC_MAX_REQUEST_BODY_BYTES` | `max_request_body_bytes` | `67108864`（64MB） | 按 `Content-Length` 提前拒绝过大请求体；`0` = 不限制 |
| `AIPP_SYNC_MAX_PAYLOAD_BYTES` | `max_payload_bytes` | `0` | 单个对象 payload 上限；`0` = 应用层不限制 |
| `AIPP_SYNC_MAX_EVENTS_PER_PUSH` | `max_events_per_push` | `500` | 单次 push 最多事件数（1–5000） |
| `AIPP_SYNC_MIN_CLIENT_SCHEMA_VERSION` | `min_client_schema_version` | `1` | 接受的最小客户端 schema |
| `AIPP_SYNC_MAX_CLIENT_SCHEMA_VERSION` | `max_client_schema_version` | `2` | 接受的最大客户端 schema |
| `AIPP_SYNC_STALE_LWW_TYPES` | `stale_lww_types` | `[]`（空列表） | 过期写入按「后写覆盖」接受的对象类型列表；默认空 = 其余类型报冲突 |

注意：

- 若前面有反向代理，请把代理的 body 大小限制与预期最大同步对象对齐。
- ID 字段（`event_id` / `object_type` / `object_id` / `device_id`）允许字符 `[A-Za-z0-9._:+/=-]`，长度 1–128（含 `+/=` 以支持 `natural:<base64>` 形式的对象 id）。
- `base_version` 必须 ≥ 0。

---

## 冒烟测试

服务已启动时，另开一个终端。

### 健康检查

```bash
curl http://127.0.0.1:8080/health
# 期望: {"status":"ok"}
```

### 同步状态

```bash
curl -H "Authorization: Bearer dev-token" \
  http://127.0.0.1:8080/v1/sync/status
```

### 简单 Push（bash）

```bash
curl -X POST http://127.0.0.1:8080/v1/sync/push \
  -H "Authorization: Bearer dev-token" \
  -H "Content-Type: application/json" \
  -d '{
    "device_id": "device-a",
    "events": [
      {
        "event_id": "event-1",
        "object_type": "conversation",
        "object_id": "conv-1",
        "operation": "upsert",
        "base_version": 0,
        "local_version": 1,
        "payload": {
          "fields": {
            "name": "测试对话",
            "updated_time": "2026-05-18T10:00:00.000Z"
          },
          "refs": {}
        },
        "client_schema_version": 1,
        "object_schema_version": 1
      }
    ]
  }'
```

### 简单 Pull（另一台设备）

```bash
curl "http://127.0.0.1:8080/v1/sync/pull?cursor=0&limit=10" \
  -H "Authorization: Bearer dev-token" \
  -H "X-AIPP-Device-ID: device-b"
```

若返回里能看到 `event-1`，说明 push/pull 通路正常。

### Windows PowerShell 版 Push

```powershell
$body = @{
  device_id = "device-a"
  events = @(
    @{
      event_id = "event-1"
      object_type = "conversation"
      object_id = "conv-1"
      operation = "upsert"
      base_version = 0
      local_version = 1
      payload = @{
        fields = @{
          name = "测试对话"
          updated_time = "2026-05-18T10:00:00.000Z"
        }
        refs = @{}
      }
      client_schema_version = 1
      object_schema_version = 1
    }
  )
} | ConvertTo-Json -Depth 10

curl -X POST http://127.0.0.1:8080/v1/sync/push `
  -H "Authorization: Bearer dev-token" `
  -H "Content-Type: application/json" `
  -d $body
```

---

## 客户端怎么连

1. 自建服务地址填：`http://127.0.0.1:8080`（本机）或 `http://<局域网IP>:8080`（同网手机/另一台电脑）
2. Token 填：`Authorization: Bearer <token>` 里的明文（本地 Compose 默认可为 `dev-token`）
3. 每台客户端使用自己的 `device_id`（Push JSON / `X-AIPP-Device-ID`）

局域网访问时，确保防火墙放行 8080，且客户端与服务器在同一网段。

---

## API 摘要

### `GET /health`

健康检查，无需认证。返回 `{"status":"ok"}`。

### `GET /v1/sync/status`

需要 Bearer Token。返回账号同步状态、最新 cursor、schema 版本范围、远端是否为空等。

### `POST /v1/sync/push`

需要 Bearer Token。提交本地变更事件，写入：

- `sync_object`：对象最新快照
- `sync_change`：追加变更记录
- `sync_device`：设备注册/更新

按 `(account_id, event_id)` 幂等。

### `GET /v1/sync/pull?cursor=0&limit=500`

需要 Bearer Token 与 `X-AIPP-Device-ID`。按 `seq` 升序返回 cursor 之后的变更。  
返回的 `cursor` 为本次最后一条的 `seq`；客户端应在成功应用后再更新本地 cursor。

### `GET /v1/sync/notify?cursor=<last_pulled_cursor>`

SSE 实时通知（`text/event-stream`）。需要 Bearer Token 与 `X-AIPP-Device-ID`。

账号 cursor 前进时（其他设备 push 成功，或订阅者已落后）发送：

```text
event: sync
data: {"cursor":12,"device_id":"device-a"}
```

客户端应立即 pull，再用新 cursor 重连。约每 20s 发送 keepalive 注释；单连接约 5 分钟后回收。同源 `X-AIPP-Device-ID` 的事件会跳过，避免被自己的 push 唤醒。

### Admin：Token 与 Device

任意有效 Token 可管理本账号：

```text
GET  /v1/admin/tokens                      列出 token
POST /v1/admin/tokens        {name}        创建 token（明文只返回一次）
POST /v1/admin/tokens/{id}/revoke          吊销 token
POST /v1/admin/tokens/{id}/rotate          轮换：发新 token 并吊销旧的
GET  /v1/admin/devices                     列出设备
POST /v1/admin/devices/{id}/revoke         吊销设备
```

---

## 跑测试

```bash
cd sync-server
# 已有 venv 时：
.venv/bin/python -B -m pytest -p no:cacheprovider app/tests
# 或：
pytest -p no:cacheprovider app/tests
```

Windows：

```powershell
.\.venv\Scripts\python.exe -B -m pytest -p no:cacheprovider app\tests
```

若 pytest 无法写入默认临时目录，请在普通用户 shell 中运行，或设置可写的临时目录。

---

## 重置本地数据

停止服务后删除 SQLite 文件：

```bash
# 本地 uv 开发
rm -f ./data/aipp-sync.db

# Docker Compose：删卷会清空数据
docker compose down -v
```

PowerShell：

```powershell
Remove-Item .\data\aipp-sync.db
```

下次启动（且设置了 `AIPP_SYNC_BOOTSTRAP_TOKEN`）会重新创建默认账号与 bootstrap token。

---

## 当前 MVP 已实现 / 未实现

### 已实现

- FastAPI 服务
- SQLAlchemy 模型（本版本仅 SQLite 为已测试/支持后端；模型侧可兼容 PostgreSQL 风格）
- Alembic 迁移（容器启动自动 `upgrade head`）
- Token 认证（过期、轮换、吊销）与 `/v1/admin/*`
- 账号隔离
- 设备注册与吊销检查
- Push 幂等
- 基于 cursor 的 Pull
- SSE 实时 notify
- 过期写入冲突上报；可通过 `AIPP_SYNC_STALE_LWW_TYPES` 对指定类型做 last-write-wins
- Docker / Compose 一键起服

### 未实现

- 面向终端用户的账号 / Token 管理 UI
- 密钥（secret）同步
- 大文件 / blob 同步
- 冲突解决 UI
- 生产级管理控制台
- 服务端签发的设备凭证（当前 `device_id` 为客户端自报）
