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
