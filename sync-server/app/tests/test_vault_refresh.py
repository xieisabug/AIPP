from app.tests.test_sync_api import auth_headers, make_client, sample_event


def test_refresh_claim_excludes_other_device_and_requires_current_version():
    client = make_client()
    event = sample_event("vault-event", "vault-object")
    event["object_type"] = "mcp.vault_secret"
    event["payload"] = {"fields": {"value": {"alg": "aipp-e2e-aes256gcm-v1", "kid": "key", "nonce": "nonce", "ct": "ciphertext"}}, "refs": {}}
    assert client.post("/v1/sync/push", headers=auth_headers(), json={"device_id": "a", "events": [event]}).json()["accepted"]
    claim = {"device_id": "a", "object_id": "vault-object", "version": 1, "claim_id": "claim-a"}
    url = "/v1/sync/vault/claim-refresh"
    assert client.post(url, headers=auth_headers(), json=claim).status_code == 200
    assert client.post(url, headers=auth_headers(), json=claim).status_code == 200
    assert client.post(url, headers=auth_headers(), json={**claim, "device_id": "b", "claim_id": "claim-b"}).status_code == 409
    event.update(event_id="vault-new", base_version=1)
    event["payload"]["fields"]["value"] = {"alg": "aipp-e2e-aes256gcm-v1", "kid": "key", "nonce": "new-nonce", "ct": "new-ciphertext"}
    assert client.post("/v1/sync/push", headers=auth_headers(), json={"device_id": "a", "events": [event]}).json()["accepted"]
    assert client.post(url, headers=auth_headers(), json=claim).status_code == 409
    assert client.post(url, headers=auth_headers(), json={**claim, "version": 2, "device_id": "b", "claim_id": "claim-b"}).status_code == 200


def test_refresh_claim_requires_auth_and_never_claims_missing_object():
    client = make_client()
    claim = {"device_id": "a", "object_id": "unknown", "version": 1, "claim_id": "claim-a"}
    assert client.post("/v1/sync/vault/claim-refresh", json=claim).status_code == 401
    assert client.post("/v1/sync/vault/claim-refresh", headers=auth_headers(), json=claim).status_code == 409


def test_vault_push_rejects_plaintext():
    client = make_client()
    event = sample_event("plaintext-event", "vault-object")
    event["object_type"] = "mcp.vault_secret"
    event["payload"] = {"fields": {"value": "secret-that-must-not-be-stored"}, "refs": {}}
    result = client.post("/v1/sync/push", headers=auth_headers(), json={"device_id": "a", "events": [event]}).json()
    assert result["accepted"] == []
    assert result["rejected"][0]["reason"] == "vault_ciphertext_required"


def test_vault_rejects_old_key_epoch_after_key_rotation():
    import json
    client = make_client()
    key_event = sample_event("key-event", "key-check")
    key_event["object_type"] = "system.feature_config"
    key_event["payload"] = {"fields": {"feature_code": "data_sync_secret", "key": "key_check", "value": json.dumps({"check": "new-key-check"})}, "refs": {}}
    assert client.post("/v1/sync/push", headers=auth_headers(), json={"device_id": "a", "events": [key_event]}).json()["accepted"]
    event = sample_event("old-secret", "vault-object")
    event["object_type"] = "mcp.vault_secret"
    event["payload"] = {"fields": {"value": {"alg": "aipp-e2e-aes256gcm-v1", "kid": "old-key", "nonce": "nonce", "ct": "ciphertext"}}, "refs": {}}
    result = client.post("/v1/sync/push", headers=auth_headers(), json={"device_id": "offline-device", "events": [event]}).json()
    assert result["accepted"] == []
    assert result["rejected"][0]["reason"] == "secret_key_epoch_mismatch"
    event["event_id"] = "new-secret"
    event["payload"]["fields"]["value"]["kid"] = "new-key-check"[:12]
    assert client.post("/v1/sync/push", headers=auth_headers(), json={"device_id": "a", "events": [event]}).json()["accepted"]


def test_key_rotation_waits_for_an_active_refresh():
    import json
    client = make_client()
    event = sample_event("vault-event", "vault-object")
    event["object_type"] = "mcp.vault_secret"
    event["payload"] = {"fields": {"value": {"alg": "aipp-e2e-aes256gcm-v1", "kid": "key", "nonce": "nonce", "ct": "ciphertext"}}, "refs": {}}
    assert client.post("/v1/sync/push", headers=auth_headers(), json={"device_id": "a", "events": [event]}).json()["accepted"]
    claim = {"device_id": "a", "object_id": "vault-object", "version": 1, "claim_id": "claim-a"}
    assert client.post("/v1/sync/vault/claim-refresh", headers=auth_headers(), json=claim).status_code == 200
    key_event = sample_event("key-event", "key-check")
    key_event["object_type"] = "system.feature_config"
    key_event["payload"] = {"fields": {"feature_code": "data_sync_secret", "key": "key_check", "value": json.dumps({"check": "new-key"})}, "refs": {}}
    response = client.post("/v1/sync/push", headers=auth_headers(), json={"device_id": "b", "events": [key_event]}).json()
    assert response["accepted"] == []
    assert response["rejected"][0]["reason"].startswith("vault_refresh_in_progress")
