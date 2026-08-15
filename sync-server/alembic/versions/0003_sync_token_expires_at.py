from __future__ import annotations

import sqlalchemy as sa
from alembic import op

revision = "0003_sync_token_expires_at"
down_revision = "0002_sync_change_version_unique"
branch_labels = None
depends_on = None


def upgrade() -> None:
    # Token 有效期（S5）。NULL = 永不过期，向后兼容存量 token。
    with op.batch_alter_table("sync_token") as batch_op:
        batch_op.add_column(sa.Column("expires_at", sa.DateTime(timezone=True), nullable=True))


def downgrade() -> None:
    with op.batch_alter_table("sync_token") as batch_op:
        batch_op.drop_column("expires_at")
