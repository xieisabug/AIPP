from __future__ import annotations

import sqlalchemy as sa
from alembic import op

revision = "0004_sync_device_account_key"
down_revision = "0003_sync_token_expires_at"
branch_labels = None
depends_on = None


def upgrade() -> None:
    # SQLite needs a table rebuild. Give the legacy unnamed primary key a name
    # during reflection so batch mode can replace it without losing device data.
    with op.batch_alter_table(
        "sync_device", naming_convention={"pk": "pk_%(table_name)s"}
    ) as batch_op:
        batch_op.drop_constraint("pk_sync_device", type_="primary")
        batch_op.drop_constraint("uq_sync_device_account_id", type_="unique")
        batch_op.create_primary_key("pk_sync_device", ["account_id", "id"])


def downgrade() -> None:
    # Never silently discard registrations (especially revoked ones) to satisfy
    # the old globally unique ID. A duplicate requires an operator's decision.
    devices = sa.table("sync_device", sa.column("id"))
    duplicate_id = op.get_bind().execute(
        sa.select(devices.c.id).group_by(devices.c.id).having(sa.func.count() > 1).limit(1)
    ).first()
    if duplicate_id is not None:
        raise RuntimeError(
            "Cannot downgrade sync_device: device IDs are registered in multiple accounts; "
            "keep the upgraded schema or restore a pre-upgrade backup."
        )
    with op.batch_alter_table("sync_device") as batch_op:
        batch_op.drop_constraint("pk_sync_device", type_="primary")
        batch_op.create_primary_key("pk_sync_device", ["id"])
        batch_op.create_unique_constraint("uq_sync_device_account_id", ["account_id", "id"])
