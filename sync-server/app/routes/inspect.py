"""Optional, read-only connection inspection. Legacy sync routes remain unchanged."""
from __future__ import annotations

import base64

from fastapi import APIRouter, Depends
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.orm import Session

from app.auth import AuthContext, require_auth
from app.db import get_db
from app.models import SyncChange, SyncObject
from app.schemas import PullChange
from app.services.cursor import latest_cursor

router = APIRouter(prefix="/v1/sync", tags=["sync"])


class InspectRequest(BaseModel):
    sequences: list[int] = Field(default_factory=list, max_length=10)


@router.post("/inspect")
def inspect_history(
    body: InspectRequest,
    auth: AuthContext = Depends(require_auth),
    db: Session = Depends(get_db),
) -> dict:
    cursor = latest_cursor(db, auth.account_id)
    has_objects = db.scalar(select(SyncObject.object_id).where(
        SyncObject.account_id == auth.account_id
    ).limit(1)) is not None
    rows = db.scalars(select(SyncChange).where(
        SyncChange.account_id == auth.account_id,
        SyncChange.seq.in_(body.sequences),
    )).all()
    # feature_config has a stable natural key; this lookup never scans payloads.
    key_id = "natural:" + base64.b64encode(b"data_sync_secret\x1fkey_check").decode()
    key = db.get(SyncObject, (auth.account_id, "system.feature_config", key_id))
    return {
        "account_id": auth.account_id,
        "latest_cursor": cursor,
        "remote_empty": not has_objects and cursor == 0,
        "key_check": key.payload_json if key and key.deleted_at is None else None,
        "changes": [PullChange(
            seq=r.seq, event_id=r.event_id, device_id=r.device_id,
            object_type=r.object_type, object_id=r.object_id,
            operation=r.operation, version=r.version, payload=r.payload_json,
            deleted_at=r.deleted_at, created_at=r.created_at,
        ) for r in rows],
    }
