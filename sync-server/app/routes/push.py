from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException, status
from sqlalchemy.orm import Session

from app.auth import AuthContext, ensure_device, require_auth
from app.config import Settings, get_settings
from app.db import get_db
from app.schemas import AcceptedEvent, ConflictEvent, PushRequest, PushResponse, RejectedEvent
from app.services.merge import process_push_event

router = APIRouter(prefix="/v1/sync", tags=["sync"])


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
        result = process_push_event(db, settings, auth.account_id, request.device_id, event)
        if isinstance(result, AcceptedEvent):
            accepted.append(result)
        elif isinstance(result, ConflictEvent):
            conflicts.append(result)
        else:
            rejected.append(result)

    db.commit()
    return PushResponse(accepted=accepted, conflicts=conflicts, rejected=rejected)

