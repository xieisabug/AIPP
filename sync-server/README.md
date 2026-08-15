# AIPP Sync Server

AIPP Sync Server is a business-level sync service for AIPP desktop data. It is an MVP FastAPI service that stores sync objects, change logs, devices, and token-based account isolation.

It does not sync SQLite database files directly. Clients push and pull business events through `/v1/sync/push` and `/v1/sync/pull`.

## Concepts

### Account

An account is the server-side sync space.

It is not currently an AIPP desktop login account. In this MVP, an account means: all devices using tokens bound to the same `sync_account.id` read and write the same remote change log and object snapshots.

Local development example:

```text
account_id: default
token: dev-token
```

The bootstrap account and token are created automatically on service startup only when `AIPP_SYNC_BOOTSTRAP_TOKEN` is set (there is no default value). When `AIPP_SYNC_BASE_URL` points at a non-localhost address, the service refuses to start without a private token — the publicly documented `dev-token` is rejected in that case.

### Token

The client authenticates with:

```http
Authorization: Bearer <token>
```

The server stores only the token hash in `sync_token`, not the plain token.

For local development, set one explicitly, for example:

```bash
export AIPP_SYNC_BOOTSTRAP_TOKEN=dev-token
```

For real deployment, generate a private token and set it with `AIPP_SYNC_BOOTSTRAP_TOKEN`:

```bash
openssl rand -hex 32
```

### Device

A device is one AIPP client installation.

Push requests include `device_id` in JSON:

```json
{
  "device_id": "device-a",
  "events": []
}
```

Pull requests include the device id in a header:

```http
X-AIPP-Device-ID: device-a
```

The server auto-registers a device the first time it sees a valid token plus device id. Revoked devices cannot push or pull.

> MVP limitation: `device_id` is self-reported by the client. A future version will switch to server-issued device credentials.

### Token & Device Management API

Tokens expire after `AIPP_SYNC_TOKEN_TTL_DAYS` days (default 365; `0` means never). Manage tokens and devices of your account with any valid token:

```text
GET  /v1/admin/tokens                      list tokens
POST /v1/admin/tokens        {name}        create token (plaintext returned once)
POST /v1/admin/tokens/{id}/revoke          revoke token
POST /v1/admin/tokens/{id}/rotate          rotate: issue a new token and revoke the old one
GET  /v1/admin/devices                     list devices
POST /v1/admin/devices/{id}/revoke         revoke device
```

### Input Validation & Limits

- ID fields (`event_id`, `object_type`, `object_id`, `device_id`) accept `[A-Za-z0-9._:+/=-]`, 1-128 chars. The set includes `+/=` because natural object ids embed standard base64 (`natural:<base64>`).
- `base_version` must be >= 0.
- Request bodies larger than `AIPP_SYNC_MAX_REQUEST_BODY_BYTES` (default 16MB) are rejected with 413 based on `Content-Length`.

## Local Setup with uv

Run these commands in PowerShell:

```powershell
cd E:\workspace\rust\aipp\sync-server
uv venv --python 3.12
.\.venv\Scripts\Activate.ps1
uv pip install -e ".[dev]"
```

If `uv venv --python 3.12` says Python 3.12 is missing:

```powershell
uv python install 3.12
uv venv --python 3.12
```

## Start the Server

```powershell
uvicorn app.main:app --host 127.0.0.1 --port 8080
```

Default local settings:

```text
service url: http://127.0.0.1:8080
database: ./data/aipp-sync.db
account: default
token: dev-token
```

Environment variables use the `AIPP_SYNC_` prefix:

```powershell
$env:AIPP_SYNC_DATABASE_URL = "sqlite:///./data/aipp-sync.db"
$env:AIPP_SYNC_BOOTSTRAP_ACCOUNT_ID = "default"
$env:AIPP_SYNC_BOOTSTRAP_TOKEN = "replace-this-token"
uvicorn app.main:app --host 127.0.0.1 --port 8080
```

`AIPP_SYNC_MAX_PAYLOAD_BYTES` controls the maximum size of a single object payload. The default is `0`, which means no application-level payload limit. If you put the service behind a reverse proxy, keep the proxy body-size limit aligned with your expected largest synced object.

## Docker Deployment

Build and run:

```bash
docker build -t aipp-sync-server .
docker run -d --name aipp-sync \
  -p 8080:8080 \
  -v aipp-sync-data:/app/data \
  -e AIPP_SYNC_BASE_URL=https://sync.example.com \
  -e AIPP_SYNC_BOOTSTRAP_TOKEN=$(openssl rand -hex 32) \
  aipp-sync-server
```

Notes:

- The container starts as a non-root user (`aipp`) and runs `alembic upgrade head` before launching uvicorn, so schema migrations are applied automatically on every start.
- The SQLite database lives in `/app/data`; always mount a volume there, otherwise all sync state is lost when the container is recreated.
- When `AIPP_SYNC_BASE_URL` is not localhost, `AIPP_SYNC_BOOTSTRAP_TOKEN` must be set to a private token or the container exits at startup.
- Only the SQLite backend is supported in this version.

## Smoke Test

Open another PowerShell window while the server is running.

Health check:

```powershell
curl http://127.0.0.1:8080/health
```

Expected:

```json
{"status":"ok"}
```

Sync status:

```powershell
curl -H "Authorization: Bearer dev-token" http://127.0.0.1:8080/v1/sync/status
```

Push one object:

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

Pull as another device:

```powershell
curl "http://127.0.0.1:8080/v1/sync/pull?cursor=0&limit=10" `
  -H "Authorization: Bearer dev-token" `
  -H "X-AIPP-Device-ID: device-b"
```

If the pull response contains `event-1`, the basic push/pull path is working.

## API Summary

### `GET /health`

Returns service health.

### `GET /v1/sync/status`

Requires bearer token. Returns account sync state, latest cursor, schema version range, and whether the remote is empty.

### `POST /v1/sync/push`

Requires bearer token. Accepts local change events and writes:

- latest object snapshot into `sync_object`
- append-only change record into `sync_change`
- device registration/update into `sync_device`

Push is idempotent by `(account_id, event_id)`.

### `GET /v1/sync/pull?cursor=0&limit=500`

Requires bearer token and `X-AIPP-Device-ID`. Returns changes after the cursor in ascending `seq` order.

The returned `cursor` is the last returned `seq`. Clients should update their local cursor only after applying the changes successfully.

## Run Tests

```powershell
.\.venv\Scripts\python.exe -B -m pytest -p no:cacheprovider app\tests
```

If pytest cannot write to the default Windows temp directory, run it from a normal user shell or set a writable base temp directory.

## Reset Local Data

Stop the server, then remove the SQLite database:

```powershell
Remove-Item .\data\aipp-sync.db
```

On the next startup, the default account and bootstrap token will be recreated.

## Current MVP Scope

Implemented:

- FastAPI service
- SQLite/PostgreSQL-capable SQLAlchemy models (SQLite is the only tested/supported backend in this version)
- Alembic migrations
- token authentication with expiry, rotation and revocation (`/v1/admin/*`)
- account isolation
- device registration and revocation checks
- push idempotency
- cursor-based pull
- conflict reporting for stale writes on all object types (last-write-wins allowlist via `AIPP_SYNC_STALE_LWW_TYPES`)

Not implemented yet:

- AIPP desktop client integration
- user-facing account/token management UI
- secret sync
- blob/file sync
- conflict UI
- production admin console
