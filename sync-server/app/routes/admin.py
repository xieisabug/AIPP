from __future__ import annotations

from datetime import UTC, datetime
from uuid import uuid4

from fastapi import APIRouter, Depends, HTTPException, status
from pydantic import BaseModel, Field
from secrets import token_urlsafe
from sqlalchemy import select
from sqlalchemy.orm import Session

from app.auth import AuthContext, require_auth
from app.config import Settings, get_settings
from app.db import get_db, token_expires_at, token_hash
from app.models import SyncDevice, SyncToken

router = APIRouter(prefix="/v1/admin", tags=["admin"])


class CreateTokenRequest(BaseModel):
    name: str = Field(min_length=1, max_length=128)


class TokenInfo(BaseModel):
    id: str
    name: str
    created_at: datetime
    revoked_at: datetime | None
    expires_at: datetime | None


class CreatedToken(BaseModel):
    token: TokenInfo
    # 明文 token 仅在创建/轮换时返回一次，服务端只存哈希
    plaintext: str


class DeviceInfo(BaseModel):
    id: str
    name: str
    created_at: datetime
    last_seen_at: datetime | None
    revoked_at: datetime | None


def _to_token_info(row: SyncToken) -> TokenInfo:
    return TokenInfo(
        id=row.id,
        name=row.name,
        created_at=row.created_at,
        revoked_at=row.revoked_at,
        expires_at=row.expires_at,
    )


def _create_token(db: Session, settings: Settings, account_id: str, name: str) -> CreatedToken:
    plaintext = token_urlsafe(32)
    row = SyncToken(
        id=str(uuid4()),
        account_id=account_id,
        name=name,
        token_hash=token_hash(plaintext),
        created_at=datetime.now(UTC),
        revoked_at=None,
        expires_at=token_expires_at(settings),
    )
    db.add(row)
    db.flush()
    return CreatedToken(token=_to_token_info(row), plaintext=plaintext)


def _get_account_token(db: Session, account_id: str, token_id: str) -> SyncToken:
    row = db.scalar(select(SyncToken).where(SyncToken.id == token_id, SyncToken.account_id == account_id))
    if row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="token not found")
    return row


@router.get("/tokens", response_model=list[TokenInfo])
def list_tokens(auth: AuthContext = Depends(require_auth), db: Session = Depends(get_db)) -> list[TokenInfo]:
    rows = db.scalars(
        select(SyncToken).where(SyncToken.account_id == auth.account_id).order_by(SyncToken.created_at)
    ).all()
    return [_to_token_info(row) for row in rows]


@router.post("/tokens", response_model=CreatedToken, status_code=status.HTTP_201_CREATED)
def create_token(
    request: CreateTokenRequest,
    auth: AuthContext = Depends(require_auth),
    db: Session = Depends(get_db),
    settings: Settings = Depends(get_settings),
) -> CreatedToken:
    created = _create_token(db, settings, auth.account_id, request.name)
    db.commit()
    return created


@router.post("/tokens/{token_id}/revoke", response_model=TokenInfo)
def revoke_token(
    token_id: str,
    auth: AuthContext = Depends(require_auth),
    db: Session = Depends(get_db),
) -> TokenInfo:
    row = _get_account_token(db, auth.account_id, token_id)
    if row.revoked_at is None:
        row.revoked_at = datetime.now(UTC)
        db.commit()
    return _to_token_info(row)


@router.post("/tokens/{token_id}/rotate", response_model=CreatedToken, status_code=status.HTTP_201_CREATED)
def rotate_token(
    token_id: str,
    auth: AuthContext = Depends(require_auth),
    db: Session = Depends(get_db),
    settings: Settings = Depends(get_settings),
) -> CreatedToken:
    row = _get_account_token(db, auth.account_id, token_id)
    created = _create_token(db, settings, auth.account_id, f"{row.name} (rotated)")
    if row.revoked_at is None:
        row.revoked_at = datetime.now(UTC)
    db.commit()
    return created


@router.get("/devices", response_model=list[DeviceInfo])
def list_devices(auth: AuthContext = Depends(require_auth), db: Session = Depends(get_db)) -> list[DeviceInfo]:
    rows = db.scalars(
        select(SyncDevice).where(SyncDevice.account_id == auth.account_id).order_by(SyncDevice.created_at)
    ).all()
    return [
        DeviceInfo(
            id=row.id,
            name=row.name,
            created_at=row.created_at,
            last_seen_at=row.last_seen_at,
            revoked_at=row.revoked_at,
        )
        for row in rows
    ]


@router.post("/devices/{device_id}/revoke", response_model=DeviceInfo)
def revoke_device(
    device_id: str,
    auth: AuthContext = Depends(require_auth),
    db: Session = Depends(get_db),
) -> DeviceInfo:
    row = db.scalar(select(SyncDevice).where(SyncDevice.id == device_id, SyncDevice.account_id == auth.account_id))
    if row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="device not found")
    if row.revoked_at is None:
        row.revoked_at = datetime.now(UTC)
        db.commit()
    return DeviceInfo(
        id=row.id,
        name=row.name,
        created_at=row.created_at,
        last_seen_at=row.last_seen_at,
        revoked_at=row.revoked_at,
    )
