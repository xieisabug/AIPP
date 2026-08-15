from __future__ import annotations

from dataclasses import dataclass
from datetime import UTC, datetime

from fastapi import Depends, Header, HTTPException, Request, status
from sqlalchemy import select
from sqlalchemy.orm import Session

from app.db import get_db, token_hash
from app.models import SyncDevice, SyncToken


@dataclass(frozen=True)
class AuthContext:
    account_id: str
    token_id: str


def require_auth(request: Request, db: Session = Depends(get_db)) -> AuthContext:
    authorization = request.headers.get("Authorization", "")
    scheme, _, token = authorization.partition(" ")
    if scheme.lower() != "bearer" or not token:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="missing bearer token")

    row = db.scalar(
        select(SyncToken).where(SyncToken.token_hash == token_hash(token), SyncToken.revoked_at.is_(None))
    )
    if row is None:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="invalid bearer token")
    if row.expires_at is not None:
        now = datetime.now(UTC)
        expires_at = row.expires_at if row.expires_at.tzinfo else row.expires_at.replace(tzinfo=UTC)
        if expires_at <= now:
            raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="token expired")
    return AuthContext(account_id=row.account_id, token_id=row.id)


def ensure_device(
    db: Session,
    auth: AuthContext,
    device_id: str,
    device_name: str | None = None,
) -> SyncDevice:
    now = datetime.now(UTC)
    device = db.scalar(
        select(SyncDevice).where(
            SyncDevice.account_id == auth.account_id,
            SyncDevice.id == device_id,
        )
    )
    if device is None:
        device = SyncDevice(
            id=device_id,
            account_id=auth.account_id,
            name=device_name or device_id,
            created_at=now,
            last_seen_at=now,
            revoked_at=None,
        )
        db.add(device)
        db.flush()
        return device

    if device.revoked_at is not None:
        raise HTTPException(status_code=status.HTTP_403_FORBIDDEN, detail="device revoked")

    if device_name and device.name != device_name:
        device.name = device_name
    device.last_seen_at = now
    db.flush()
    return device


def require_device_id(x_aipp_device_id: str | None = Header(default=None)) -> str:
    if not x_aipp_device_id:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="missing X-AIPP-Device-ID")
    return x_aipp_device_id

