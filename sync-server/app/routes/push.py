from __future__ import annotations

import logging

from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy import select
from sqlalchemy.exc import IntegrityError
from sqlalchemy.orm import Session

from app.auth import AuthContext, ensure_device, require_auth
from app.config import Settings, get_settings
from app.db import get_db
from app.models import SyncChange, SyncObject
from app.schemas import AcceptedEvent, ConflictEvent, PushRequest, PushResponse, RejectedEvent
from app.services.merge import process_push_event

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/v1/sync", tags=["sync"])


def recover_from_integrity_error(
    db: Session, account_id: str, event
) -> AcceptedEvent | ConflictEvent | RejectedEvent:
    """Map a unique-constraint violation to the correct sync semantics.

    - Duplicate event_id (concurrent retry): replay the stored accepted result.
    - Duplicate (object_type, object_id, version): a concurrent push won the
      version race; report a conflict so the client re-bases.
    """
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
    if current is not None:
        return ConflictEvent(
            event_id=event.event_id,
            object_type=event.object_type,
            object_id=event.object_id,
            server_version=current.version,
            server_payload=current.payload_json,
            server_operation="delete" if current.deleted_at is not None else "upsert",
        )
    return RejectedEvent(event_id=event.event_id, reason="integrity_error")


@router.post("/push", response_model=PushResponse)
def push_events(
    request: PushRequest,
    auth: AuthContext = Depends(require_auth),
    db: Session = Depends(get_db),
    settings: Settings = Depends(get_settings),
) -> PushResponse:
    if len(request.events) > settings.max_events_per_push:
        raise HTTPException(status_code=status.HTTP_413_REQUEST_ENTITY_TOO_LARGE, detail="too many events")

    ensure_device(db, auth, request.device_id, request.device_name)

    accepted: list[AcceptedEvent] = []
    conflicts: list[ConflictEvent] = []
    rejected: list[RejectedEvent] = []

    for event in request.events:
        # Each event runs in a savepoint: a single bad event (or a concurrent
        # duplicate push) must not roll back the whole batch with a 500.
        try:
            with db.begin_nested():
                result = process_push_event(db, settings, auth.account_id, request.device_id, event)
        except IntegrityError:
            result = recover_from_integrity_error(db, auth.account_id, event)
        except Exception:
            logger.exception("failed to process push event %s", event.event_id)
            result = RejectedEvent(event_id=event.event_id, reason="internal_error")
        if isinstance(result, AcceptedEvent):
            accepted.append(result)
        elif isinstance(result, ConflictEvent):
            conflicts.append(result)
        else:
            rejected.append(result)

    db.commit()
    return PushResponse(accepted=accepted, conflicts=conflicts, rejected=rejected)

