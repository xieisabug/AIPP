from app.tests.test_sync_api import auth_headers, make_client, sample_event


def test_proactive_owner_is_exclusive_until_released_by_owner():
    client = make_client()
    path = "/v1/sync/proactive/owner"
    first, second = auth_headers(device_id="a"), auth_headers(device_id="b")
    assert client.put(path, headers=first).status_code == 200
    assert client.put(path, headers=first).status_code == 200
    assert client.put(path, headers=second).status_code == 409
    assert client.delete(path, headers=second).status_code == 200
    assert client.put(path, headers=second).status_code == 409
    assert client.delete(path, headers=first).status_code == 200
    assert client.put(path, headers=second).status_code == 200


def test_backfill_includes_linked_sources_only_for_v4_readers():
    client = make_client()
    kinds = ["conversation.proactive_item", "conversation.proactive_occurrence", "conversation.proactive_source"]
    events = [dict(sample_event(f"event-{i}", f"object-{i}"), object_type=kind)
              for i, kind in enumerate(kinds)]
    pushed = client.post("/v1/sync/push", headers=auth_headers(), json={"device_id": "a", "events": events})
    assert len(pushed.json()["accepted"]) == len(kinds)
    headers = auth_headers(device_id="b")
    assert client.get("/v1/sync/pull?butler_history_v2=true", headers=headers).json()["changes"] == []
    page = client.get("/v1/sync/pull?butler_history_v3=true", headers=headers).json()
    assert [change["object_type"] for change in page["changes"]] == kinds[:2]
    page = client.get("/v1/sync/pull?butler_history_v4=true", headers=headers).json()
    assert [change["object_type"] for change in page["changes"]] == kinds
