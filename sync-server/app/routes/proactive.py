from fastapi import APIRouter, Depends, HTTPException
from sqlalchemy import delete
from sqlalchemy.exc import IntegrityError
from sqlalchemy.orm import Session

from app.auth import AuthContext, ensure_device, require_auth, require_device_id
from app.db import get_db
from app.models import ProactiveOwner

router = APIRouter(prefix="/v1/sync/proactive", tags=["sync"])


@router.put("/owner")
def claim_owner(auth: AuthContext = Depends(require_auth), device_id: str = Depends(require_device_id),
                db: Session = Depends(get_db)) -> dict[str, str]:
    ensure_device(db, auth, device_id)
    owner = db.get(ProactiveOwner, auth.account_id)
    if owner is None:
        db.add(ProactiveOwner(account_id=auth.account_id, device_id=device_id))
        try:
            db.commit()
            return {"device_id": device_id}
        except IntegrityError:
            db.rollback()
            owner = db.get(ProactiveOwner, auth.account_id)
    if owner is None or owner.device_id != device_id:
        raise HTTPException(409, "另一台设备负责主动整理，请先在原设备关闭主动整理")
    db.commit()
    return {"device_id": device_id}


@router.delete("/owner")
def release_owner(auth: AuthContext = Depends(require_auth), device_id: str = Depends(require_device_id),
                  db: Session = Depends(get_db)) -> dict[str, bool]:
    ensure_device(db, auth, device_id)
    db.execute(delete(ProactiveOwner).where(ProactiveOwner.account_id == auth.account_id,
                                           ProactiveOwner.device_id == device_id))
    db.commit()
    return {"released": True}
