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


def make_client(tmp_path, token: str = "test-token", **setting_overrides) -> TestClient:
    settings = Settings(
        database_url=f"sqlite:///{tmp_path / (str(uuid4()) + '.db')}",
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


def test_status_requires_bearer_token(tmp_path) -> None:
    client = make_client(tmp_path)

    response = client.get("/v1/sync/status")

    assert response.status_code == 401


def test_push_is_idempotent_by_event_id(tmp_path) -> None:
    client = make_client(tmp_path)
    body = {"device_id": "device-a", "events": [sample_event("event-1", "conv-1")]}

    first = client.post("/v1/sync/push", headers=auth_headers(), json=body)
    second = client.post("/v1/sync/push", headers=auth_headers(), json=body)

    assert first.status_code == 200
    assert second.status_code == 200
    assert first.json()["accepted"] == second.json()["accepted"]
    assert first.json()["conflicts"] == []
    assert second.json()["conflicts"] == []


def test_pull_returns_monotonic_cursor_and_changes(tmp_path) -> None:
    client = make_client(tmp_path)
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


def test_stale_message_update_returns_conflict(tmp_path) -> None:
    client = make_client(tmp_path)
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


def test_push_accepts_large_payload_when_payload_limit_is_disabled(tmp_path) -> None:
    client = make_client(tmp_path)
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


def test_push_rejects_large_payload_when_payload_limit_is_configured(tmp_path) -> None:
    client = make_client(tmp_path, max_payload_bytes=1024)
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


def test_accounts_are_isolated_by_token(tmp_path) -> None:
    client = make_client(tmp_path)
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


def test_revoked_device_cannot_push_or_pull(tmp_path) -> None:
    client = make_client(tmp_path)
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


def test_non_localhost_without_bootstrap_token_refuses_to_start(tmp_path) -> None:
    import pytest

    settings = Settings(
        database_url=f"sqlite:///{tmp_path / 'no-token.db'}",
        base_url="https://sync.example.com",
        bootstrap_token=None,
    )

    with pytest.raises(RuntimeError, match="AIPP_SYNC_BOOTSTRAP_TOKEN"):
        create_app(settings)


def test_non_localhost_with_public_default_token_refuses_to_start(tmp_path) -> None:
    import pytest

    settings = Settings(
        database_url=f"sqlite:///{tmp_path / 'dev-token.db'}",
        base_url="https://sync.example.com",
        bootstrap_token="dev-token",
    )

    with pytest.raises(RuntimeError, match="dev-token"):
        create_app(settings)


def test_non_localhost_with_private_token_starts(tmp_path) -> None:
    client = make_client(
        tmp_path,
        base_url="https://sync.example.com",
    )

    response = client.get("/health")

    assert response.status_code == 200


def test_localhost_without_bootstrap_token_starts_but_creates_no_token(tmp_path) -> None:
    client = make_client(tmp_path, token=None)

    response = client.get("/health")
    assert response.status_code == 200

    with get_sessionmaker()() as db:
        assert isinstance(db, Session)
        tokens = db.query(SyncToken).count()
    assert tokens == 0


def test_busy_timeout_pragma_is_set(tmp_path) -> None:
    from sqlalchemy import text

    make_client(tmp_path)
    with get_sessionmaker()() as db:
        assert isinstance(db, Session)
        value = db.execute(text("PRAGMA busy_timeout")).scalar()
    assert value == 5000


def test_push_batch_with_integrity_conflict_does_not_fail_whole_batch(tmp_path) -> None:
    """预置一个 version=2 的 change（不同 event_id），再推一条会算出 version=2 的事件。

    唯一约束 uq_sync_change_account_object_version 触发 IntegrityError，
    恢复逻辑应把它转成 conflict，且同批其他事件正常 accepted、HTTP 200。
    """
    from app.models import SyncChange, SyncObject

    client = make_client(tmp_path)
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


def test_concurrent_duplicate_event_id_returns_idempotent_accept(tmp_path) -> None:
    """另一条连接抢先写入相同 event_id（模拟并发重试），API 仍返回幂等 accepted。"""
    from app.models import SyncChange, SyncObject
    from app.routes.push import recover_from_integrity_error

    make_client(tmp_path)
    now = datetime.now(UTC)
    with get_sessionmaker()() as db:
        assert isinstance(db, Session)
        db.add(
            SyncObject(
                account_id="acct-a",
                object_type="conversation",
                object_id="conv-dup",
                version=3,
                payload_json={"fields": {"name": "已有"}},
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
                payload_json={"fields": {"name": "已有"}},
                deleted_at=None,
                device_id="device-b",
                created_at=now,
            )
        )
        db.commit()

        from app.schemas import PushEvent

        event = PushEvent(**sample_event("dup-event", "conv-dup"))
        result = recover_from_integrity_error(db, "acct-a", event)
        assert result.event_id == "dup-event"
        assert result.server_version == 3


def test_stale_write_conflicts_by_default_for_non_whitelisted_type(tmp_path) -> None:
    """conversation（不在 LWW 白名单）的 stale 写入应返回 conflict，而不是静默覆盖。"""
    client = make_client(tmp_path)
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


def test_stale_write_accepted_for_whitelisted_lww_type(tmp_path) -> None:
    client = make_client(tmp_path, stale_lww_types=["conversation"])
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


def test_event_id_replay_with_different_content_is_rejected(tmp_path) -> None:
    """同一 event_id 携带不同内容重推（客户端 bug）不得被幂等确认。"""
    client = make_client(tmp_path)
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


def test_conflict_with_tombstone_reports_delete_operation(tmp_path) -> None:
    """服务器端对象是墓碑时，冲突响应必须带 server_operation=delete。"""
    client = make_client(tmp_path)
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
