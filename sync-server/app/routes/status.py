from __future__ import annotations

from fastapi import APIRouter, Depends, Header
from sqlalchemy import func, select
from sqlalchemy.orm import Session

from app.auth import AuthContext, require_auth
from app.config import Settings, get_settings
from app.db import get_db
from app.models import SyncDevice, SyncObject
from app.schemas import StatusResponse
from app.services.cursor import latest_cursor

router = APIRouter(prefix="/v1/sync", tags=["sync"])


@router.get("/status", response_model=StatusResponse)
def get_status(
    auth: AuthContext = Depends(require_auth),
    db: Session = Depends(get_db),
    settings: Settings = Depends(get_settings),
    x_aipp_device_id: str | None = Header(default=None),
) -> StatusResponse:
    count = db.scalar(select(func.count()).select_from(SyncObject).where(SyncObject.account_id == auth.account_id)) or 0
    registered = False
    if x_aipp_device_id:
        registered = (
            db.scalar(
                select(SyncDevice).where(
                    SyncDevice.account_id == auth.account_id,
                    SyncDevice.id == x_aipp_device_id,
                    SyncDevice.revoked_at.is_(None),
                )
            )
            is not None
        )

    return StatusResponse(
        account_id=auth.account_id,
        object_count=count,
        latest_cursor=latest_cursor(db, auth.account_id),
        min_client_schema_version=settings.min_client_schema_version,
        max_client_schema_version=settings.max_client_schema_version,
        remote_empty=count == 0,
        device_registered=registered,
    )

