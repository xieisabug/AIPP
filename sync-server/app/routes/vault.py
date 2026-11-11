"""Coordinate refresh attempts without receiving any OAuth credential plaintext."""
from datetime import UTC, datetime
import json

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel, Field
from sqlalchemy import select
from sqlalchemy.exc import IntegrityError
from sqlalchemy.orm import Session

from app.auth import AuthContext, ensure_device, require_auth
from app.db import get_db
from app.models import SyncObject, VaultRefreshClaim
from app.schemas import SyncId

router = APIRouter(prefix="/v1/sync/vault", tags=["sync"])


class RefreshRequest(BaseModel):
    device_id: SyncId
    object_id: SyncId
    version: int = Field(ge=1)
    claim_id: SyncId


@router.post("/claim-refresh")
def claim_refresh(request: RefreshRequest, auth: AuthContext = Depends(require_auth),
                  db: Session = Depends(get_db)) -> dict[str, bool]:
    ensure_device(db, auth, request.device_id)
    configs = db.scalars(select(SyncObject).where(
        SyncObject.account_id == auth.account_id, SyncObject.object_type == "system.feature_config",
        SyncObject.deleted_at.is_(None),
    ).with_for_update()).all()
    active_kid = None
    for config in configs:
        fields = (config.payload_json or {}).get("fields", {})
        if fields.get("feature_code") == "data_sync_secret" and fields.get("key") == "key_check":
            try:
                active_kid = json.loads(fields["value"])["check"][:12]
            except (KeyError, TypeError, ValueError):
                raise HTTPException(409, "secret_key_epoch_invalid")
    current = db.scalar(select(SyncObject).where(
        SyncObject.account_id == auth.account_id,
        SyncObject.object_type == "mcp.vault_secret",
        SyncObject.object_id == request.object_id,
    ).with_for_update())
    if current is None or current.deleted_at is not None or current.version != request.version:
        raise HTTPException(409, "credential_version_changed: sync before refreshing")
    if active_kid is not None and (current.payload_json or {}).get("fields", {}).get("value", {}).get("kid") != active_kid:
        raise HTTPException(409, "secret_key_epoch_mismatch: complete key rotation before refreshing")
    existing = db.get(VaultRefreshClaim, (auth.account_id, request.object_id, request.version))
    if existing is not None:
        if existing.claim_id == request.claim_id and existing.device_id == request.device_id:
            return {"granted": True}
        # Never expire a claim and blindly reuse a possibly consumed refresh token.
        raise HTTPException(409, "refresh_already_claimed: wait for synced credentials; if the refresh was interrupted, reconnect the app")
    db.add(VaultRefreshClaim(account_id=auth.account_id, object_id=request.object_id,
                            version=request.version, claim_id=request.claim_id,
                            device_id=request.device_id, created_at=datetime.now(UTC)))
    try:
        db.commit()
    except IntegrityError:
        db.rollback()
        raise HTTPException(409, "refresh_already_claimed: another device won the refresh")
    return {"granted": True}
