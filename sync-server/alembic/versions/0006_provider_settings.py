"""Publish portable versions of provider settings with verifiable identities."""
from alembic import op
from sqlalchemy import select
from sqlalchemy.orm import Session

from app.config import get_settings
from app.models import SyncObject
from app.services.merge import repair_provider_setting

revision = "0006_provider_settings"
down_revision = "0005_merge_device_vault"
branch_labels = None
depends_on = None


def upgrade():
    with Session(bind=op.get_bind()) as db:
        settings = get_settings()
        objects = db.scalars(select(SyncObject).where(
            SyncObject.object_type.in_(("system.feature_config", "assistant.model_config")),
            SyncObject.deleted_at.is_(None),
        ).with_for_update()).all()
        for current in objects:
            repair_provider_setting(db, settings, current)
        db.commit()


def downgrade():
    # Accepted changes are append-only. Keep their versions and cursors on downgrade.
    pass
