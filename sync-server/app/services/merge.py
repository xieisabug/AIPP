from __future__ import annotations

import json
from datetime import UTC, datetime
from hashlib import sha256

from sqlalchemy import select
from sqlalchemy.orm import Session

from app.config import Settings
from app.models import SyncChange, SyncObject
from app.schemas import AcceptedEvent, ConflictEvent, PushEvent, RejectedEvent
from app.services.conflict import should_accept_stale_event


def payload_to_dict(event: PushEvent) -> dict | None:
    if event.payload is None:
        return None
    return event.payload.model_dump(mode="json")


def payload_hash(payload: dict | None, deleted_at: datetime | None = None) -> str:
    material = {"payload": payload, "deleted_at": deleted_at.isoformat() if deleted_at else None}
    encoded = json.dumps(material, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return sha256(encoded.encode("utf-8")).hexdigest()


def payload_size(payload: dict | None) -> int:
    if payload is None:
        return 0
    return len(json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8"))


def process_push_event(
    db: Session,
    settings: Settings,
    account_id: str,
    device_id: str,
    event: PushEvent,
) -> AcceptedEvent | ConflictEvent | RejectedEvent:
    if not settings.min_client_schema_version <= event.client_schema_version <= settings.max_client_schema_version:
        return RejectedEvent(event_id=event.event_id, reason="schema_version_unsupported")
    if event.object_schema_version != 1:
        return RejectedEvent(event_id=event.event_id, reason="object_schema_version_unsupported")

    payload = payload_to_dict(event)
    if settings.max_payload_bytes > 0 and payload_size(payload) > settings.max_payload_bytes:
        return RejectedEvent(event_id=event.event_id, reason="payload_too_large")

    existing_change = db.scalar(
        select(SyncChange).where(
            SyncChange.account_id == account_id,
            SyncChange.event_id == event.event_id,
        )
    )
    if existing_change is not None:
        return AcceptedEvent(
            event_id=event.event_id,
            object_type=existing_change.object_type,
            object_id=existing_change.object_id,
            server_version=existing_change.version,
            server_seq=existing_change.seq,
        )

    current = db.get(SyncObject, (account_id, event.object_type, event.object_id))
    if not should_accept_stale_event(event, current):
        assert current is not None
        return ConflictEvent(
            event_id=event.event_id,
            object_type=event.object_type,
            object_id=event.object_id,
            server_version=current.version,
            server_payload=current.payload_json,
        )

    now = datetime.now(UTC)
    version = (current.version + 1) if current is not None else 1
    deleted_at = now if event.operation == "delete" else None
    stored_payload = None if event.operation == "delete" else payload
    stored_hash = payload_hash(stored_payload, deleted_at)

    if current is None:
        current = SyncObject(
            account_id=account_id,
            object_type=event.object_type,
            object_id=event.object_id,
            version=version,
            payload_json=stored_payload,
            payload_hash=stored_hash,
            deleted_at=deleted_at,
            updated_at=now,
            updated_by_device_id=device_id,
        )
        db.add(current)
    else:
        current.version = version
        current.payload_json = stored_payload
        current.payload_hash = stored_hash
        current.deleted_at = deleted_at
        current.updated_at = now
        current.updated_by_device_id = device_id

    change = SyncChange(
        account_id=account_id,
        event_id=event.event_id,
        object_type=event.object_type,
        object_id=event.object_id,
        operation=event.operation,
        version=version,
        payload_json=stored_payload,
        deleted_at=deleted_at,
        device_id=device_id,
        created_at=now,
    )
    db.add(change)
    db.flush()

    return AcceptedEvent(
        event_id=event.event_id,
        object_type=event.object_type,
        object_id=event.object_id,
        server_version=version,
        server_seq=change.seq,
    )
