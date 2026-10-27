from __future__ import annotations

from datetime import UTC, datetime
from hashlib import sha256
from uuid import uuid4

from fastapi.testclient import TestClient
from sqlalchemy.orm import Session

from app.config import Settings
from app.db import get_sessionmaker
from app.factory import create_app
from app.models import SyncAccount, SyncDevice, SyncToken


def make_client(token: str = "test-token", **setting_overrides) -> TestClient:
    settings = Settings(
        _env_file=None,
        database_url="sqlite:///:memory:",
        bootstrap_account_id="acct-a",
        bootstrap_token=token,
        **setting_overrides,
    )
    return TestClient(create_app(settings))


def auth_headers(token: str = "test-token", device_id: str | None = None) -> dict[str, str]:
    headers = {"Authorization": f"Bearer {token}"}
    if device_id:
        headers["X-AIPP-Device-ID"] = device_id
    return headers


def sample_event(event_id: str, object_id: str, base_version: int = 0) -> dict:
    return {
        "event_id": event_id,
        "object_type": "conversation",
        "object_id": object_id,
        "operation": "upsert",
        "base_version": base_version,
        "local_version": base_version + 1,
        "payload": {
            "fields": {"name": "需求讨论", "updated_time": "2026-05-18T10:00:00.000Z"},
            "refs": {},
        },
        "client_schema_version": 1,
        "object_schema_version": 1,
    }


def test_status_requires_bearer_token() -> None:
    client = make_client()

    response = client.get("/v1/sync/status")

    assert response.status_code == 401


def test_push_is_idempotent_by_event_id() -> None:
    client = make_client()
    body = {"device_id": "device-a", "events": [sample_event("event-1", "conv-1")]}

    first = client.post("/v1/sync/push", headers=auth_headers(), json=body)
    second = client.post("/v1/sync/push", headers=auth_headers(), json=body)

    assert first.status_code == 200
    assert second.status_code == 200
    assert first.json()["accepted"] == second.json()["accepted"]
    assert first.json()["conflicts"] == []
    assert second.json()["conflicts"] == []


def test_pull_returns_monotonic_cursor_and_changes() -> None:
    client = make_client()
    push_body = {
        "device_id": "device-a",
        "events": [
            sample_event("event-1", "conv-1"),
            sample_event("event-2", "conv-2"),
        ],
    }
    push_response = client.post("/v1/sync/push", headers=auth_headers(), json=push_body)
    assert push_response.status_code == 200

    pull_response = client.get(
        "/v1/sync/pull?cursor=0&limit=1",
        headers=auth_headers(device_id="device-b"),
    )
    body = pull_response.json()

    assert pull_response.status_code == 200
    assert body["cursor"] == 1
    assert body["has_more"] is True
    assert [change["event_id"] for change in body["changes"]] == ["event-1"]

    next_response = client.get(
        f"/v1/sync/pull?cursor={body['cursor']}&limit=10",
        headers=auth_headers(device_id="device-b"),
    )
    next_body = next_response.json()

    assert next_response.status_code == 200
    assert next_body["cursor"] == 2
    assert next_body["has_more"] is False
    assert [change["event_id"] for change in next_body["changes"]] == ["event-2"]


def test_stale_message_update_returns_conflict() -> None:
    client = make_client()
    create = {
        "device_id": "device-a",
        "events": [
            {
                **sample_event("event-1", "msg-1"),
                "object_type": "conversation.message",
                "payload": {"fields": {"content": "remote"}, "refs": {"conversation": "conv-1"}},
            }
        ],
    }
    stale = {
        "device_id": "device-b",
        "events": [
            {
                **sample_event("event-2", "msg-1", base_version=0),
                "object_type": "conversation.message",
                "payload": {"fields": {"content": "local stale"}, "refs": {"conversation": "conv-1"}},
            }
        ],
    }

    assert client.post("/v1/sync/push", headers=auth_headers(), json=create).status_code == 200
    response = client.post("/v1/sync/push", headers=auth_headers(), json=stale)
    body = response.json()

    assert response.status_code == 200
    assert body["accepted"] == []
    assert body["conflicts"][0]["event_id"] == "event-2"
    assert body["conflicts"][0]["server_version"] == 1
    assert body["conflicts"][0]["server_payload"]["fields"]["content"] == "remote"


def test_push_accepts_large_payload_when_payload_limit_is_disabled() -> None:
    client = make_client()
    event = {
        **sample_event("event-large", "msg-large"),
        "object_type": "conversation.message",
        "payload": {
            "fields": {"content": "x" * 20_000},
            "refs": {"conversation": "conv-1"},
        },
    }

    response = client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={"device_id": "device-a", "events": [event]},
    )
    body = response.json()

    assert response.status_code == 200
    assert [item["event_id"] for item in body["accepted"]] == ["event-large"]
    assert body["rejected"] == []


def test_push_rejects_large_payload_when_payload_limit_is_configured() -> None:
    client = make_client(max_payload_bytes=1024)
    event = {
        **sample_event("event-large", "msg-large"),
        "object_type": "conversation.message",
        "payload": {
            "fields": {"content": "x" * 20_000},
            "refs": {"conversation": "conv-1"},
        },
    }

    response = client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={"device_id": "device-a", "events": [event]},
    )
    body = response.json()

    assert response.status_code == 200
    assert body["accepted"] == []
    assert body["rejected"][0]["reason"] == "payload_too_large"


def test_accounts_are_isolated_by_token() -> None:
    client = make_client()
    token_b = "other-token"
    with get_sessionmaker()() as db:
        db.add(SyncAccount(id="acct-b", created_at=datetime.now(UTC)))
        db.add(
            SyncToken(
                id=str(uuid4()),
                account_id="acct-b",
                name="other",
                token_hash=sha256(token_b.encode("utf-8")).hexdigest(),
                created_at=datetime.now(UTC),
            )
        )
        db.commit()

    response_a = client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={"device_id": "device-a", "events": [sample_event("event-1", "conv-1")]},
    )
    assert response_a.status_code == 200

    response_b = client.get("/v1/sync/pull?cursor=0", headers=auth_headers(token_b, "device-b"))
    assert response_b.status_code == 200
    assert response_b.json()["changes"] == []


def test_revoked_device_cannot_push_or_pull() -> None:
    client = make_client()
    with get_sessionmaker()() as db:
        assert isinstance(db, Session)
        db.add(
            SyncDevice(
                id="device-a",
                account_id="acct-a",
                name="device-a",
                created_at=datetime.now(UTC),
                last_seen_at=datetime.now(UTC),
                revoked_at=datetime.now(UTC),
            )
        )
        db.commit()

    push_response = client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={"device_id": "device-a", "events": [sample_event("event-1", "conv-1")]},
    )
    pull_response = client.get("/v1/sync/pull?cursor=0", headers=auth_headers(device_id="device-a"))

    assert push_response.status_code == 403
    assert pull_response.status_code == 403


def test_non_localhost_without_bootstrap_token_refuses_to_start() -> None:
    import pytest

    settings = Settings(
        database_url="sqlite:///:memory:",
        base_url="https://sync.example.com",
        bootstrap_token=None,
    )

    with pytest.raises(RuntimeError, match="AIPP_SYNC_BOOTSTRAP_TOKEN"):
        create_app(settings)


def test_non_localhost_with_public_default_token_refuses_to_start() -> None:
    import pytest

    settings = Settings(
        database_url="sqlite:///:memory:",
        base_url="https://sync.example.com",
        bootstrap_token="dev-token",
    )

    with pytest.raises(RuntimeError, match="dev-token"):
        create_app(settings)


def test_non_localhost_with_private_token_starts() -> None:
    client = make_client(
        base_url="https://sync.example.com",
    )

    response = client.get("/health")

    assert response.status_code == 200


def test_localhost_without_bootstrap_token_starts_but_creates_no_token() -> None:
    client = make_client(token=None)

    response = client.get("/health")
    assert response.status_code == 200

    with get_sessionmaker()() as db:
        assert isinstance(db, Session)
        tokens = db.query(SyncToken).count()
    assert tokens == 0


def test_busy_timeout_pragma_is_set() -> None:
    from sqlalchemy import text

    make_client()
    with get_sessionmaker()() as db:
        assert isinstance(db, Session)
        value = db.execute(text("PRAGMA busy_timeout")).scalar()
    assert value == 5000


def test_push_batch_with_integrity_conflict_does_not_fail_whole_batch() -> None:
    """预置一个 version=2 的 change（不同 event_id），再推一条会算出 version=2 的事件。

    唯一约束 uq_sync_change_account_object_version 触发 IntegrityError，
    恢复逻辑应把它转成 conflict，且同批其他事件正常 accepted、HTTP 200。
    """
    from app.models import SyncChange, SyncObject

    client = make_client()
    with get_sessionmaker()() as db:
        assert isinstance(db, Session)
        now = datetime.now(UTC)
        db.add(
            SyncObject(
                account_id="acct-a",
                object_type="conversation",
                object_id="conv-race",
                version=1,
                payload_json={"fields": {"name": "旧版本"}},
                payload_hash="x" * 64,
                deleted_at=None,
                updated_at=now,
                updated_by_device_id="device-z",
            )
        )
        db.add(
            SyncChange(
                account_id="acct-a",
                event_id="other-event",
                object_type="conversation",
                object_id="conv-race",
                operation="upsert",
                version=2,
                payload_json={"fields": {"name": "并发版本"}},
                deleted_at=None,
                device_id="device-z",
                created_at=now,
            )
        )
        db.commit()

    response = client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={
            "device_id": "device-a",
            "events": [
                # conversation 类型 stale（base_version=0 < 1）默认即冲突，不再 LWW 接受
                sample_event("event-race", "conv-race", base_version=0),
                sample_event("event-good", "conv-good", base_version=0),
            ],
        },
    )

    assert response.status_code == 200
    body = response.json()
    assert [item["event_id"] for item in body["accepted"]] == ["event-good"]
    assert [item["event_id"] for item in body["conflicts"]] == ["event-race"]
    assert body["conflicts"][0]["server_operation"] == "upsert"
    assert body["rejected"] == []


def test_concurrent_duplicate_event_id_returns_idempotent_accept() -> None:
    """另一条连接抢先写入相同 event_id（模拟并发重试），API 仍返回幂等 accepted。"""
    from app.models import SyncChange, SyncObject
    from app.routes.push import recover_from_integrity_error

    make_client()
    now = datetime.now(UTC)
    with get_sessionmaker()() as db:
        assert isinstance(db, Session)
        db.add(
            SyncObject(
                account_id="acct-a",
                object_type="conversation",
                object_id="conv-dup",
                version=3,
                payload_json={"fields": {"name": "已有"}, "refs": {}},
                payload_hash="y" * 64,
                deleted_at=None,
                updated_at=now,
                updated_by_device_id="device-b",
            )
        )
        db.add(
            SyncChange(
                account_id="acct-a",
                event_id="dup-event",
                object_type="conversation",
                object_id="conv-dup",
                operation="upsert",
                version=3,
                payload_json={"fields": {"name": "已有"}, "refs": {}},
                deleted_at=None,
                device_id="device-b",
                created_at=now,
            )
        )
        db.commit()

        from app.schemas import PushEvent

        retry = sample_event("dup-event", "conv-dup")
        retry["payload"] = {"fields": {"name": "已有"}, "refs": {}}
        event = PushEvent(**retry)
        result = recover_from_integrity_error(db, "acct-a", event)
        assert result.event_id == "dup-event"
        assert result.server_version == 3


def test_stale_write_conflicts_by_default_for_non_whitelisted_type() -> None:
    """conversation（不在 LWW 白名单）的 stale 写入应返回 conflict，而不是静默覆盖。"""
    client = make_client()
    client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={"device_id": "device-a", "events": [sample_event("event-1", "conv-1", base_version=0)]},
    )

    stale = sample_event("event-2", "conv-1", base_version=0)
    stale["payload"]["fields"]["name"] = "另一台设备的修改"
    response = client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={"device_id": "device-b", "events": [stale]},
    )

    body = response.json()
    assert body["accepted"] == []
    assert [item["event_id"] for item in body["conflicts"]] == ["event-2"]
    conflict = body["conflicts"][0]
    assert conflict["server_version"] == 1
    assert conflict["server_payload"]["fields"]["name"] == "需求讨论"
    assert conflict["server_operation"] == "upsert"


def test_stale_write_accepted_for_whitelisted_lww_type() -> None:
    client = make_client(stale_lww_types=["conversation"])
    client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={"device_id": "device-a", "events": [sample_event("event-1", "conv-1", base_version=0)]},
    )

    response = client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={"device_id": "device-b", "events": [sample_event("event-2", "conv-1", base_version=0)]},
    )

    body = response.json()
    assert [item["event_id"] for item in body["accepted"]] == ["event-2"]
    assert body["conflicts"] == []


def test_event_id_replay_with_different_content_is_rejected() -> None:
    """同一 event_id 携带不同内容重推（客户端 bug）不得被幂等确认。"""
    client = make_client()
    client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={"device_id": "device-a", "events": [sample_event("event-1", "conv-1")]},
    )

    tampered = sample_event("event-1", "conv-1")
    tampered["payload"]["fields"]["name"] = "被篡改的内容"
    response = client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={"device_id": "device-a", "events": [tampered]},
    )

    body = response.json()
    assert body["accepted"] == []
    assert body["rejected"] == [{"event_id": "event-1", "reason": "event_id_conflict"}]


def test_conflict_with_tombstone_reports_delete_operation() -> None:
    """服务器端对象是墓碑时，冲突响应必须带 server_operation=delete。"""
    client = make_client()
    client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={"device_id": "device-a", "events": [sample_event("event-1", "conv-1")]},
    )
    delete_event = sample_event("event-2", "conv-1", base_version=1)
    delete_event["operation"] = "delete"
    delete_event["payload"] = None
    client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={"device_id": "device-a", "events": [delete_event]},
    )

    stale = sample_event("event-3", "conv-1", base_version=0)
    response = client.post(
        "/v1/sync/push",
        headers=auth_headers(),
        json={"device_id": "device-b", "events": [stale]},
    )

    body = response.json()
    assert body["conflicts"][0]["server_operation"] == "delete"
    assert body["conflicts"][0]["server_payload"] is None

def test_push_rejects_invalid_object_id_chars() -> None:
    client = make_client()
    event = sample_event("event-1", "bad id with spaces")
    body = {"device_id": "device-a", "events": [event]}

    response = client.post("/v1/sync/push", headers=auth_headers(), json=body)

    assert response.status_code == 200
    assert response.json()["rejected"][0]["event_id"] == "event-1"


def test_push_rejects_negative_base_version() -> None:
    client = make_client()
    event = sample_event("event-1", "conv-1")
    event["base_version"] = -1
    body = {"device_id": "device-a", "events": [event]}

    response = client.post("/v1/sync/push", headers=auth_headers(), json=body)

    assert response.status_code == 200
    assert response.json()["rejected"][0]["event_id"] == "event-1"


def test_natural_object_id_with_base64_chars_accepted() -> None:
    client = make_client()
    event = sample_event("event-1", "natural:aGVsbG8rLz0=")
    body = {"device_id": "device-a", "events": [event]}

    response = client.post("/v1/sync/push", headers=auth_headers(), json=body)

    assert response.status_code == 200
    assert len(response.json()["accepted"]) == 1


def test_request_body_over_limit_returns_413() -> None:
    client = make_client(max_request_body_bytes=1024)
    event = sample_event("event-1", "conv-1")
    event["payload"]["fields"]["blob"] = "x" * 8192
    body = {"device_id": "device-a", "events": [event]}

    response = client.post("/v1/sync/push", headers=auth_headers(), json=body)

    assert response.status_code == 413


def test_expired_token_rejected() -> None:
    from sqlalchemy import select

    client = make_client()
    with get_sessionmaker()() as db:
        row = db.scalar(select(SyncToken).where(SyncToken.name == "bootstrap"))
        assert row is not None
        row.expires_at = datetime(2020, 1, 1, tzinfo=UTC)
        db.commit()

    response = client.get("/v1/sync/status", headers=auth_headers())

    assert response.status_code == 401


def test_admin_create_revoke_rotate_token() -> None:
    client = make_client()

    created = client.post("/v1/admin/tokens", headers=auth_headers(), json={"name": "laptop"})
    assert created.status_code == 201
    payload = created.json()
    new_token = payload["plaintext"]
    assert client.get("/v1/sync/status", headers=auth_headers(new_token)).status_code == 200

    revoked = client.post(f"/v1/admin/tokens/{payload['token']['id']}/revoke", headers=auth_headers())
    assert revoked.status_code == 200
    assert revoked.json()["revoked_at"] is not None
    assert client.get("/v1/sync/status", headers=auth_headers(new_token)).status_code == 401

    tokens = client.get("/v1/admin/tokens", headers=auth_headers()).json()
    bootstrap = next(t for t in tokens if t["name"] == "bootstrap")
    rotated = client.post(f"/v1/admin/tokens/{bootstrap['id']}/rotate", headers=auth_headers())
    assert rotated.status_code == 201
    rotated_token = rotated.json()["plaintext"]
    assert client.get("/v1/sync/status", headers=auth_headers()).status_code == 401
    assert client.get("/v1/sync/status", headers=auth_headers(rotated_token)).status_code == 200


def test_admin_revoke_device_blocks_push() -> None:
    client = make_client()
    body = {"device_id": "device-a", "device_name": "Device A", "events": [sample_event("event-1", "conv-1")]}
    assert client.post("/v1/sync/push", headers=auth_headers(), json=body).status_code == 200

    revoked = client.post("/v1/admin/devices/device-a/revoke", headers=auth_headers())
    assert revoked.status_code == 200
    assert revoked.json()["revoked_at"] is not None

    body["events"] = [sample_event("event-2", "conv-2")]
    response = client.post("/v1/sync/push", headers=auth_headers(), json=body)
    assert response.status_code == 403


def test_notify_catchup_when_cursor_behind() -> None:
    client = make_client()
    push_body = {"device_id": "device-a", "events": [sample_event("event-1", "conv-1")]}
    assert client.post("/v1/sync/push", headers=auth_headers(), json=push_body).status_code == 200

    with client.stream(
        "GET",
        "/v1/sync/notify?cursor=0",
        headers=auth_headers(device_id="device-b"),
    ) as response:
        assert response.status_code == 200
        assert "text/event-stream" in response.headers["content-type"]
        body = "".join(response.iter_text())

    assert "event: sync" in body
    assert '"cursor":1' in body


def test_notify_hub_publish_delivers_to_subscriber() -> None:
    """In-process hub delivers cursor advances to waiters (push → SSE path)."""
    import asyncio

    from app.services.notify import NotifyHub

    async def exercise() -> None:
        hub = NotifyHub()
        hub.bind_loop(asyncio.get_running_loop())
        queue = await hub.subscribe("acct-a")
        await hub.publish("acct-a", cursor=7, device_id="device-a")
        payload = await asyncio.wait_for(queue.get(), timeout=1)
        assert payload.cursor == 7
        assert payload.device_id == "device-a"
        await hub.unsubscribe("acct-a", queue)

    asyncio.run(exercise())


def test_push_publishes_notify_for_accepted_events(monkeypatch) -> None:
    """Successful push fans out to NotifyHub so SSE subscribers can wake."""
    from app.services.notify import NotifyHub

    calls: list[tuple[str, int, str | None]] = []

    def fake_publish(self, account_id: str, cursor: int, device_id: str | None = None) -> None:
        calls.append((account_id, cursor, device_id))

    monkeypatch.setattr(NotifyHub, "publish_threadsafe", fake_publish)
    client = make_client()
    body = {"device_id": "device-a", "events": [sample_event("event-1", "conv-1")]}
    response = client.post("/v1/sync/push", headers=auth_headers(), json=body)
    assert response.status_code == 200
    assert response.json()["accepted"]
    assert calls == [("acct-a", 1, "device-a")]



def test_notify_requires_auth_and_device() -> None:
    client = make_client()
    assert client.get("/v1/sync/notify").status_code == 401
    assert client.get("/v1/sync/notify", headers=auth_headers()).status_code == 400


def test_push_isolates_invalid_events_from_valid_neighbors() -> None:
    client = make_client()
    invalid = sample_event("invalid", "bad id")
    long_id = sample_event("too-long", "x" * 129)
    negative = sample_event("negative", "negative-object")
    negative["base_version"] = -1
    response = client.post(
        "/v1/sync/push", headers=auth_headers(),
        json={"device_id": "device-a", "events": [
            invalid, sample_event("valid", "valid-object"), long_id, negative, None,
            {"event_id": 42},
        ]},
    )
    assert response.status_code == 200
    body = response.json()
    assert [e["event_id"] for e in body["accepted"]] == ["valid"]
    assert len(body["rejected"]) == 5
    assert all(e["reason"].startswith("invalid_event:") for e in body["rejected"])
    pull = client.get(
        "/v1/sync/pull",
        headers=auth_headers(device_id="device-a"),
        params={"cursor": 0},
    ).json()
    assert [e["event_id"] for e in pull["changes"]] == ["valid"]
