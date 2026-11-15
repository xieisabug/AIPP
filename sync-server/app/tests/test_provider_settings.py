from copy import deepcopy

import pytest
from alembic import command

from app.db import get_engine, get_sessionmaker
from app.models import SyncChange, SyncObject
from app.services.merge import payload_hash
from app.services.provider_settings import canonical_payload
from app.tests.test_device_migration import migration_config
from app.tests.test_sync_convergence import event, memory_client, pull_all, push


def helper(model, ref=None):
    return event("system.feature_config", "helper", {
        "feature_code": "experimental", "key": "butler_helper_model_id", "value": model,
    }, {"value_provider_id": ref} if ref else {})


def test_global_reference_storage_replay_and_clear():
    client = memory_client()
    record = helper("flash%%41", "global-provider")
    ack = push(client, [record])["accepted"]
    assert push(client, [record])["accepted"] == ack
    assert client.get("/v1/sync/pull", headers={
        "Authorization": "Bearer test-token", "X-AIPP-Device-ID": "old",
    }).status_code == 409
    stored = pull_all(client, "reader", 10)[0]["payload"]
    assert stored == {"fields": {**record["payload"]["fields"], "value": "flash%%@ref"},
                      "refs": {"value_provider_id": "global-provider"}}
    changed = deepcopy(record)
    changed["payload"]["refs"]["value_provider_id"] = "another"
    assert push(client, [changed])["rejected"][0]["reason"] == "event_id_conflict"
    record.update(event_id="legacy", base_version=1)
    record["payload"]["refs"].clear()
    assert push(client, [record])["rejected"][0]["reason"] == "provider_sync_id_required"
    record["payload"]["fields"]["value"] = ""
    assert push(client, [record])["accepted"][0]["server_version"] == 2


@pytest.mark.parametrize("witness", ["unique", "other-device", "ambiguous", "conflicting-setting", "deleted-provider"])
def test_legacy_repair_preserves_remote_choice(witness):
    client = memory_client()
    provider = event("llm.provider", "p", {"name": "remote"})
    model = event("llm.model", "model", {"code": "remote", "llm_provider_id": 52}, {"llm_provider_id": "p"})
    push(client, [provider, model], "another" if witness == "other-device" else "writer")
    if witness == "deleted-provider":
        push(client, [{**provider, "event_id": "delete-p", "base_version": 1,
                       "operation": "delete", "payload": None}], "writer")
    if witness == "ambiguous":
        push(client, [event("assistant.model", "binding", {"provider_id": 52}, {"provider_id": "q"})], "writer")
    if witness == "conflicting-setting":
        old = event("assistant.model_config", "acp", {"name": "acp_provider", "value": "52"}, {"value_provider_id": "q"})
        ack = push(client, [old], "writer")["accepted"][0]
        with get_sessionmaker()() as db:
            db.get(SyncChange, ack["server_seq"]).payload_json = old["payload"]
            db.commit()
    record = helper("remote%%52", "p")
    accepted = push(client, [record], "writer")["accepted"][0]
    # Seed a deployed server's immutable legacy change and current object.
    legacy = deepcopy(record["payload"])
    legacy["refs"].clear()
    with get_sessionmaker()() as db:
        current = db.get(SyncObject, ("test", "system.feature_config", "helper"))
        current.payload_json = legacy
        current.payload_hash = payload_hash(legacy)
        db.get(SyncChange, accepted["server_seq"]).payload_json = legacy
        db.commit()
    with get_engine().begin() as connection:
        config = migration_config(connection)
        command.stamp(config, "0005_merge_device_vault")
        command.upgrade(config, "head")
    stale = helper("different-local-model%%41", "local-provider")
    stale["event_id"] = "stale"
    conflict = push(client, [stale], "reader")["conflicts"][0]
    history = [c for c in pull_all(client, "reader", 1) if c["object_id"] == "helper"]
    assert history[0]["payload"] == legacy
    if witness == "unique":
        assert conflict["server_version"] == 2
        assert conflict["server_payload"]["fields"]["value"] == "remote%%@ref"
        assert conflict["server_payload"]["refs"]["value_provider_id"] == "p"
        assert history[-1]["payload"] == conflict["server_payload"]
    else:
        assert conflict["server_version"] == 1
        assert conflict["server_payload"] == legacy


@pytest.mark.parametrize(("kind", "fields", "value"), [
    ("system.feature_config", {"feature_code": "conversation_summary", "key": "title_provider_id"}, "41"),
    ("assistant.model_config", {"name": "acp_provider"}, "41"),
    ("system.feature_config", {"feature_code": "experimental", "key": "mcp_summarizer_model_id"}, "code%%41"),
])
def test_provider_setting_shapes(kind, fields, value):
    payload = {"fields": {**fields, "value": value}, "refs": {"value_provider_id": "provider"}}
    assert canonical_payload(kind, payload)["fields"]["value"] == value.replace("41", "@ref")


def test_plain_model_code_and_unrelated_config_are_preserved():
    for feature, key, value in [("conversation_summary", "title_model", "model-name"),
                                ("custom", "title_model", "example%%41")]:
        payload = {"fields": {"feature_code": feature, "key": key, "value": value}, "refs": {}}
        assert canonical_payload("system.feature_config", payload) == payload
