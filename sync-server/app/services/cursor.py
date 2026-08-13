from __future__ import annotations

from sqlalchemy import func, select
from sqlalchemy.orm import Session

from app.models import SyncChange


def latest_cursor(db: Session, account_id: str) -> int:
    return db.scalar(select(func.coalesce(func.max(SyncChange.seq), 0)).where(SyncChange.account_id == account_id)) or 0

