from __future__ import annotations

from fastapi import APIRouter, Depends, HTTPException, Query, Request
from sqlalchemy import select
from sqlalchemy.orm import Session

from app.auth import AuthContext, ensure_device, require_auth, require_device_id
from app.db import get_db
from app.models import SyncChange
from app.schemas import PullChange, PullResponse
from app.services.provider_settings import CAPABILITY_HEADER, UPGRADE_REQUIRED, requires_ref_reader

router = APIRouter(prefix="/v1/sync", tags=["sync"])


@router.get("/pull", response_model=PullResponse)
def pull_changes(
    request: Request,
    cursor: int = Query(default=0, ge=0),
    limit: int = Query(default=500, ge=1, le=1000),
    butler_history: bool = False,
    butler_history_v2: bool = False,
    butler_history_v3: bool = False,
    butler_history_v4: bool = False,
    through: int | None = Query(default=None, ge=0),
    auth: AuthContext = Depends(require_auth),
    device_id: str = Depends(require_device_id),
    db: Session = Depends(get_db),
) -> PullResponse:
    ensure_device(db, auth, device_id)

    query = select(SyncChange).where(SyncChange.account_id == auth.account_id, SyncChange.seq > cursor)
    if butler_history or butler_history_v2 or butler_history_v3 or butler_history_v4:
        kinds = (
            "assistant.appearance", "conversation.group_member", "conversation.group_message",
            "conversation.group_turn", "conversation.read_reply", "conversation.read_group_reply",
        )
        if butler_history_v2 or butler_history_v3 or butler_history_v4:
            kinds += ("assistant.skill", "assistant.prompt_param", "conversation.butler_memory",
                      "conversation.butler_main", "conversation.butler_goal")
        if butler_history_v3 or butler_history_v4:
            kinds += ("conversation.proactive_item", "conversation.proactive_occurrence")
        if butler_history_v4:
            kinds += ("conversation.proactive_source",)
        query = query.where(SyncChange.object_type.in_(kinds))
    if through is not None:
        query = query.where(SyncChange.seq <= through)
    rows = list(db.scalars(query.order_by(SyncChange.seq.asc()).limit(limit + 1)))
    has_more = len(rows) > limit
    rows = rows[:limit]
    if request.headers.get(CAPABILITY_HEADER) != "1" and any(
        requires_ref_reader(row.object_type, row.payload_json) for row in rows
    ):
        raise HTTPException(status_code=409, detail=UPGRADE_REQUIRED)
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

