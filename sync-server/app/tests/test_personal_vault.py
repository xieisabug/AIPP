"""Personal-vault withdrawal contract against the actual HTTP API, using only memory DBs."""
from copy import deepcopy

from app.db import get_sessionmaker
from app.models import SyncObject
from app.tests.test_sync_api import auth_headers, make_client, sample_event


def test_personal_vault_withdrawal_clears_active_payload_and_reaches_other_devices():
    with make_client() as client:
        original = sample_event("vault-create", "vault-object")
        original["object_type"] = "vault.entry"
        original["payload"] = {"fields": {
            "uuid": "11111111-1111-4111-8111-111111111111",
            "handle": "test_token", "sync_enabled": 1,
            "value": {"alg": "aipp-e2e-aes256gcm-v1", "kid": "test-key",
                      "nonce": "test-nonce", "ct": "test-ciphertext"},
        }, "refs": {}}

        def push(event, device="device-a"):
            response = client.post("/v1/sync/push", headers=auth_headers(),
                                   json={"device_id": device, "events": [event]})
            assert response.status_code == 200, response.text
            return response.json()

        created = push(original)
        assert created["accepted"][0]["server_version"] == 1
        withdrawal = dict(original, event_id="vault-withdraw", operation="delete",
                          base_version=1, payload=None)
        deleted = push(withdrawal)
        assert deleted["accepted"][0]["server_version"] == 2
        assert not deleted["conflicts"] and not deleted["rejected"]
        assert push(withdrawal) == deleted  # Lost-ACK retry is idempotent.

        with get_sessionmaker()() as db:
            current = db.get(SyncObject, ("acct-a", "vault.entry", "vault-object"))
            assert current.deleted_at is not None
            assert current.payload_json is None

        for device in ["device-b", "device-c"]:
            response = client.get("/v1/sync/pull?cursor=1&limit=10",
                                  headers=auth_headers(device_id=device))
            assert response.status_code == 200
            changes = response.json()["changes"]
            assert len(changes) == 1
            assert changes[0]["object_type"] == "vault.entry"
            assert changes[0]["operation"] == "delete"
            assert changes[0]["payload"] is None

        stale = deepcopy(original)
        stale.update(event_id="offline-edit", base_version=1)
        conflict = push(stale, "device-b")
        assert not conflict["accepted"]
        assert conflict["conflicts"][0]["server_operation"] == "delete"
        assert conflict["conflicts"][0]["server_payload"] is None
        # Replaying an already-accepted old upload cannot restore the active object.
        assert push(original) == created
        with get_sessionmaker()() as db:
            current = db.get(SyncObject, ("acct-a", "vault.entry", "vault-object"))
            assert current.deleted_at is not None and current.payload_json is None
