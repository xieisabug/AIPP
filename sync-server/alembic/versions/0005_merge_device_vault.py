from __future__ import annotations

import sqlalchemy as sa
from alembic import op

revision = "0005_merge_device_vault"
down_revision = ("0004_sync_device_account_key", "0004_vault_refresh_claim")
branch_labels = None
depends_on = None


def upgrade() -> None:
    # Keep both published revisions valid for databases upgraded from either
    # branch. Alembic applies the missing migration before reaching this head.
    pass


def downgrade() -> None:
    # Fail before splitting the head: otherwise a downgrade of both branches
    # could remove Vault claims before the device branch rejects duplicate IDs.
    devices = sa.table("sync_device", sa.column("id"))
    duplicate = op.get_bind().execute(
        sa.select(devices.c.id).group_by(devices.c.id).having(sa.func.count() > 1).limit(1)
    ).first()
    if duplicate is not None:
        raise RuntimeError(
            "Cannot downgrade the combined sync schema: device IDs are registered in multiple accounts; "
            "keep the upgraded schema or restore a pre-upgrade backup."
        )
