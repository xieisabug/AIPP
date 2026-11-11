from __future__ import annotations

import sqlalchemy as sa
from alembic import op

revision = "0004_vault_refresh_claim"
down_revision = "0003_sync_token_expires_at"
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.create_table("vault_refresh_claim",
        sa.Column("account_id", sa.String(64), sa.ForeignKey("sync_account.id", ondelete="CASCADE"), primary_key=True),
        sa.Column("object_id", sa.String(128), primary_key=True),
        sa.Column("version", sa.Integer(), primary_key=True),
        sa.Column("claim_id", sa.String(128), nullable=False),
        sa.Column("device_id", sa.String(128), nullable=False),
        sa.Column("created_at", sa.DateTime(timezone=True), nullable=False))


def downgrade() -> None:
    op.drop_table("vault_refresh_claim")
