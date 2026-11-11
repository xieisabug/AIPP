from __future__ import annotations

from datetime import UTC, datetime

import pytest
from alembic import command
from sqlalchemy import select

from app.db import ensure_bootstrap_token, get_engine, get_sessionmaker, token_hash
from app.config import Settings
from app.models import Base, SyncAccount, SyncDevice, SyncToken
from app.tests.test_device_migration import migration_config
from app.tests.test_sync_api import auth_headers, make_client, sample_event

DEVICE_ID = "same-installation"
TOKEN_X = "test-token"
TOKEN_Y = "test-token-y"


@pytest.fixture(params=["fresh", "migrated"])
def client(request):
    client = make_client()
    if request.param == "migrated":
        # Replace only this test's memory DB with the real old schema. Register
        # X before upgrading, just as an existing deployment would have done.
        engine = get_engine()
        Base.metadata.drop_all(engine)
        with engine.begin() as connection:
            command.upgrade(migration_config(connection), "0003_sync_token_expires_at")
        ensure_bootstrap_token(Settings(
            _env_file=None, bootstrap_account_id="acct-a", bootstrap_token=TOKEN_X,
        ))
        assert client.get("/v1/sync/pull", headers=auth_headers(TOKEN_X, DEVICE_ID)).status_code == 200
        with engine.begin() as connection:
            command.upgrade(migration_config(connection), "head")
    with get_sessionmaker()() as db:
        now = datetime.now(UTC)
        db.add(SyncAccount(id="acct-y", created_at=now))
        db.add(SyncToken(
            id="token-y", account_id="acct-y", name="Y", token_hash=token_hash(TOKEN_Y),
            created_at=now,
        ))
        db.commit()
    return client


def push(client, token, event_id, *, name=None, base_version=0, schema=1):
    event = sample_event(event_id, "shared-object", base_version)
    event["payload"]["fields"]["name"] = f"{token}/{event_id}"
    event["client_schema_version"] = schema
    response = client.post("/v1/sync/push", headers=auth_headers(token), json={
        "device_id": DEVICE_ID, "device_name": name, "events": [event],
    })
    assert response.status_code == 200
    assert response.json()["conflicts"] == []
    assert response.json()["rejected"] == []
    return response.json()["accepted"][0]


def pull(client, token, cursor=0):
    response = client.get("/v1/sync/pull", params={"cursor": cursor},
                          headers=auth_headers(token, DEVICE_ID))
    assert response.status_code == 200
    return response.json()


@pytest.mark.parametrize("schema", [1, 2])
def test_device_accounts_x_y_x_push_pull_and_legacy_protocol(client, schema):
    first = push(client, TOKEN_X, "same-event", name="Device X", schema=schema)
    x = pull(client, TOKEN_X)
    assert [row["event_id"] for row in x["changes"]] == ["same-event"]
    # Y starts from its own cursor baseline; event/object IDs are also scoped.
    y_first = push(client, TOKEN_Y, "same-event", name="Device Y", schema=schema)
    y = pull(client, TOKEN_Y)
    assert y_first["server_version"] == first["server_version"] == 1
    assert [row["payload"]["fields"]["name"] for row in y["changes"]] == [f"{TOKEN_Y}/same-event"]
    push(client, TOKEN_X, "x-second", name="Device X renamed", base_version=1, schema=schema)
    x_next = pull(client, TOKEN_X, x["cursor"])
    assert [row["event_id"] for row in x_next["changes"]] == ["x-second"]
    assert pull(client, TOKEN_Y, y["cursor"])["changes"] == []
    # Rejoining X with a reset cursor replays X only, and does not re-register D.
    assert [row["event_id"] for row in pull(client, TOKEN_X)["changes"]] == ["same-event", "x-second"]
    assert push(client, TOKEN_X, "same-event", schema=schema) == first
    for token, name in ((TOKEN_X, "Device X renamed"), (TOKEN_Y, "Device Y")):
        devices = client.get("/v1/admin/devices", headers=auth_headers(token)).json()
        assert len(devices) == 1
        assert devices[0]["id"] == DEVICE_ID
        assert devices[0]["name"] == name
        status = client.get("/v1/sync/status", headers=auth_headers(token, DEVICE_ID)).json()
        assert status["device_registered"] is True
        assert status["object_count"] == 1
    # ORM identity must include the account, even inside one session.
    with get_sessionmaker()() as db:
        x_device = db.get(SyncDevice, ("acct-a", DEVICE_ID))
        y_device = db.get(SyncDevice, ("acct-y", DEVICE_ID))
        assert x_device is not y_device
        assert x_device.name == "Device X renamed"
        assert y_device.name == "Device Y"


def test_device_accounts_pull_registers_same_id_without_changing_other_space(client):
    push(client, TOKEN_X, "x", name="Device X")
    x_before = client.get("/v1/admin/devices", headers=auth_headers(TOKEN_X)).json()
    assert client.get("/v1/sync/status", headers=auth_headers(TOKEN_Y, DEVICE_ID)).json()["device_registered"] is False
    assert pull(client, TOKEN_Y)["changes"] == []
    assert client.get("/v1/admin/devices", headers=auth_headers(TOKEN_X)).json() == x_before
    assert client.get("/v1/sync/status", headers=auth_headers(TOKEN_Y, DEVICE_ID)).json()["device_registered"] is True


def test_device_accounts_revocation_and_admin_permissions_are_scoped(client):
    push(client, TOKEN_X, "x", name="Device X")
    assert client.post(f"/v1/admin/devices/{DEVICE_ID}/revoke", headers=auth_headers(TOKEN_Y)).status_code == 404
    revoked = client.post(f"/v1/admin/devices/{DEVICE_ID}/revoke", headers=auth_headers(TOKEN_X))
    assert revoked.status_code == 200 and revoked.json()["revoked_at"] is not None
    # Revocation in X does not prevent first registration or normal use in Y.
    push(client, TOKEN_Y, "y", name="Device Y")
    assert len(pull(client, TOKEN_Y)["changes"]) == 1
    for endpoint in ("pull", "notify"):
        response = client.get(f"/v1/sync/{endpoint}", headers=auth_headers(TOKEN_X, DEVICE_ID))
        assert response.status_code == 403
        assert response.json()["detail"] == "device revoked"
    assert client.post("/v1/sync/push", headers=auth_headers(TOKEN_X), json={
        "device_id": DEVICE_ID, "events": [sample_event("blocked", "blocked")],
    }).status_code == 403
    for token, registered in ((TOKEN_X, False), (TOKEN_Y, True)):
        assert client.get("/v1/sync/status", headers=auth_headers(token, DEVICE_ID)).json()["device_registered"] is registered
    with get_sessionmaker()() as db:
        rows = db.scalars(select(SyncDevice).where(SyncDevice.id == DEVICE_ID)).all()
        assert len(rows) == 2
        assert next(row for row in rows if row.account_id == "acct-y").revoked_at is None


def test_device_accounts_notify_and_inspect_do_not_leak_other_space(client):
    x = push(client, TOKEN_X, "x")
    y = push(client, TOKEN_Y, "y")
    # Both endpoints must keep account filtering when a device ID is shared.
    for token, own, other in ((TOKEN_X, x, y), (TOKEN_Y, y, x)):
        response = client.post("/v1/sync/inspect", headers=auth_headers(token), json={
            "sequences": [own["server_seq"], other["server_seq"]],
        })
        assert response.status_code == 200
        assert [row["seq"] for row in response.json()["changes"]] == [own["server_seq"]]
        with client.stream("GET", "/v1/sync/notify?cursor=0", headers=auth_headers(token, DEVICE_ID)) as notification:
            assert notification.status_code == 200
            body = "".join(notification.iter_text())
        assert f'"cursor":{own["server_seq"]}' in body
        assert f'"cursor":{other["server_seq"]}' not in body
