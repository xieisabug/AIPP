"""Large graph/retry/pagination scenarios against the actual server API, in memory."""
from copy import deepcopy

from fastapi.testclient import TestClient

from app.config import Settings
from app.factory import create_app


def memory_client() -> TestClient:
    return TestClient(create_app(Settings(
        _env_file=None, database_url="sqlite:///:memory:", bootstrap_token="test-token",
        bootstrap_account_id="test", base_url="http://localhost", token_ttl_days=0,
    )))


def event(kind: str, identity: str, fields: dict, refs: dict | None = None) -> dict:
    return {"event_id": f"create-{identity}", "object_type": kind, "object_id": identity,
            "operation": "upsert", "base_version": 0, "client_schema_version": 2,
            "payload": {"fields": fields, "refs": refs or {}}}


def push(client, events, device="device-a"):
    response = client.post("/v1/sync/push", headers={"Authorization": "Bearer test-token"},
                           json={"device_id": device, "events": events})
    assert response.status_code == 200, response.text
    return response.json()


def pull_all(client, device, limit):
    cursor = 0
    changes = []
    while True:
        response = client.get(f"/v1/sync/pull?cursor={cursor}&limit={limit}", headers={
            "Authorization": "Bearer test-token", "X-AIPP-Device-ID": device})
        assert response.status_code == 200, response.text
        page = response.json()
        assert all(c["seq"] > cursor for c in page["changes"])
        changes.extend(page["changes"])
        assert page["cursor"] >= cursor
        cursor = page["cursor"]
        if not page["has_more"]:
            return changes


def test_sync_large_graph_out_of_order_retries_and_three_device_pagination():
    # Children intentionally arrive before their parents, across 20+ pages.
    # The server is a sequenced log; dependency resolution belongs to the client.
    events = [event("conversation.message", f"m-{i}",
                    {"conversation_id": 99, "llm_model_id": 88, "content": f"message-{i}",
                     "generation_group_id": f"g-{i}", "parent_group_id": "root"},
                    {"conversation_id": "conversation-1", "llm_model_id": "model-1"})
              for i in range(10_000)]
    events += [event("conversation", "conversation-1", {"name": "one", "assistant_id": 7},
                     {"assistant_id": "assistant-1"}),
               event("assistant", "assistant-1", {"name": "assistant"}),
               event("llm.model", "model-1", {"llm_provider_id": 8, "code": "test"},
                     {"llm_provider_id": "provider-1"}),
               event("llm.provider", "provider-1", {"name": "provider"})]
    with memory_client() as client:
        for start in range(0, len(events), 500):
            batch = events[start:start + 500]
            first = push(client, batch)
            assert len(first["accepted"]) == len(batch)
            assert not first["conflicts"] and not first["rejected"]
            assert push(client, batch) == first  # Lost ACK retry adds no changes.
        b = pull_all(client, "device-b", 499)
        c = pull_all(client, "device-c", 137)
        assert b == c
        assert len(b) == len(events)
        assert len({r["seq"] for r in b}) == len(events)
        assert sum(r["object_type"] == "conversation" for r in b) == 1
        available = {r["object_id"] for r in b}
        assert all(ref in available for r in b for ref in r["payload"]["refs"].values())


def test_sync_stale_update_delete_and_reused_event_do_not_corrupt_log():
    with memory_client() as client:
        original = event("conversation", "c", {"name": "original"})
        assert len(push(client, [original])["accepted"]) == 1
        update = deepcopy(original)
        update.update(event_id="update-c", base_version=1)
        update["payload"]["fields"]["name"] = "latest"
        assert len(push(client, [update], "device-b")["accepted"]) == 1
        stale = deepcopy(original)
        stale["event_id"] = "stale-c"
        assert len(push(client, [stale], "device-c")["conflicts"]) == 1
        mutated_retry = deepcopy(original)
        mutated_retry["payload"]["fields"]["name"] = "mutated"
        assert push(client, [mutated_retry])["rejected"][0]["reason"] == "event_id_conflict"
        delete = dict(original, event_id="delete-c", operation="delete", base_version=2, payload=None)
        assert push(client, [delete])["accepted"][0]["server_version"] == 3
        assert push(client, [original])["accepted"][0]["server_version"] == 1
        rows = pull_all(client, "device-b", 1)
        assert [r["version"] for r in rows] == [1, 2, 3]
        assert rows[-1]["operation"] == "delete" and rows[-1]["payload"] is None


def test_sync_uniqueness_race_rejects_mutated_retry_without_losing_other_events(monkeypatch):
    import app.routes.push as route
    from sqlalchemy.exc import IntegrityError

    with memory_client() as client:
        original = event("conversation", "c", {"name": "original"})
        accepted = push(client, [original])["accepted"]
        process = route.process_push_event

        def simulate_race(db, settings, account_id, device_id, incoming):
            if incoming.event_id == original["event_id"]:
                raise IntegrityError("simulated duplicate event race", {}, Exception("UNIQUE"))
            return process(db, settings, account_id, device_id, incoming)

        monkeypatch.setattr(route, "process_push_event", simulate_race)
        assert push(client, [original])["accepted"] == accepted
        mutated = deepcopy(original)
        mutated["payload"]["fields"]["name"] = "different write"
        result = push(client, [mutated, event("conversation", "good", {"name": "good"})])
        assert result["rejected"] == [{"event_id": original["event_id"], "reason": "event_id_conflict"}]
        assert len(result["accepted"]) == 1 and not result["conflicts"]
        rows = pull_all(client, "device-b", 1)
        assert len(rows) == 2 and rows[0]["payload"]["fields"]["name"] == "original"
