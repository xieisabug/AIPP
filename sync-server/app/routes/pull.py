from __future__ import annotations

from fastapi import APIRouter, Depends, Query
from sqlalchemy import select
from sqlalchemy.orm import Session

from app.auth import AuthContext, ensure_device, require_auth, require_device_id
from app.db import get_db
from app.models import SyncChange
from app.schemas import PullChange, PullResponse

router = APIRouter(prefix="/v1/sync", tags=["sync"])


@router.get("/pull", response_model=PullResponse)
def pull_changes(
    cursor: int = Query(default=0, ge=0),
    limit: int = Query(default=500, ge=1, le=1000),
    auth: AuthContext = Depends(require_auth),
    device_id: str = Depends(require_device_id),
    db: Session = Depends(get_db),
) -> PullResponse:
    ensure_device(db, auth, device_id)

    rows = list(
        db.scalars(
            select(SyncChange)
            .where(SyncChange.account_id == auth.account_id, SyncChange.seq > cursor)
            .order_by(SyncChange.seq.asc())
            .limit(limit + 1)
        )
    )
    has_more = len(rows) > limit
    rows = rows[:limit]
    next_cursor = rows[-1].seq if rows else cursor
    db.commit()

    return PullResponse(
        cursor=next_cursor,
        has_more=has_more,
        changes=[
            PullChange(
                seq=row.seq,
                event_id=row.event_id,
                device_id=row.device_id,
                object_type=row.object_type,
                object_id=row.object_id,
                operation=row.operation,  # type: ignore[arg-type]
                version=row.version,
                payload=row.payload_json,
                deleted_at=row.deleted_at,
                created_at=row.created_at,
            )
            for row in rows
        ],
    )

