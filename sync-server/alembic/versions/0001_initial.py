from __future__ import annotations

from alembic import op
import sqlalchemy as sa

revision = "0001_initial"
down_revision = None
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.create_table(
        "sync_account",
        sa.Column("id", sa.String(length=64), primary_key=True),
        sa.Column("created_at", sa.DateTime(timezone=True), nullable=False),
    )
    op.create_table(
        "sync_token",
        sa.Column("id", sa.String(length=64), primary_key=True),
        sa.Column("account_id", sa.String(length=64), nullable=False),
        sa.Column("name", sa.String(length=128), nullable=False),
        sa.Column("token_hash", sa.String(length=64), nullable=False, unique=True),
        sa.Column("created_at", sa.DateTime(timezone=True), nullable=False),
        sa.Column("revoked_at", sa.DateTime(timezone=True), nullable=True),
        sa.ForeignKeyConstraint(["account_id"], ["sync_account.id"], ondelete="CASCADE"),
    )
    op.create_table(
        "sync_device",
        sa.Column("id", sa.String(length=128), primary_key=True),
        sa.Column("account_id", sa.String(length=64), nullable=False),
        sa.Column("name", sa.String(length=256), nullable=False),
        sa.Column("created_at", sa.DateTime(timezone=True), nullable=False),
        sa.Column("last_seen_at", sa.DateTime(timezone=True), nullable=True),
        sa.Column("revoked_at", sa.DateTime(timezone=True), nullable=True),
        sa.ForeignKeyConstraint(["account_id"], ["sync_account.id"], ondelete="CASCADE"),
        sa.UniqueConstraint("account_id", "id", name="uq_sync_device_account_id"),
    )
    op.create_table(
        "sync_object",
        sa.Column("account_id", sa.String(length=64), nullable=False),
        sa.Column("object_type", sa.String(length=128), nullable=False),
        sa.Column("object_id", sa.String(length=128), nullable=False),
        sa.Column("version", sa.Integer(), nullable=False),
        sa.Column("payload_json", sa.JSON(), nullable=True),
        sa.Column("payload_hash", sa.String(length=64), nullable=False),
        sa.Column("deleted_at", sa.DateTime(timezone=True), nullable=True),
        sa.Column("updated_at", sa.DateTime(timezone=True), nullable=False),
        sa.Column("updated_by_device_id", sa.String(length=128), nullable=False),
        sa.ForeignKeyConstraint(["account_id"], ["sync_account.id"], ondelete="CASCADE"),
        sa.PrimaryKeyConstraint("account_id", "object_type", "object_id"),
    )
    op.create_table(
        "sync_change",
        sa.Column("seq", sa.Integer(), primary_key=True, autoincrement=True),
        sa.Column("account_id", sa.String(length=64), nullable=False),
        sa.Column("event_id", sa.String(length=128), nullable=False),
        sa.Column("object_type", sa.String(length=128), nullable=False),
        sa.Column("object_id", sa.String(length=128), nullable=False),
        sa.Column("operation", sa.String(length=16), nullable=False),
        sa.Column("version", sa.Integer(), nullable=False),
        sa.Column("payload_json", sa.JSON(), nullable=True),
        sa.Column("deleted_at", sa.DateTime(timezone=True), nullable=True),
        sa.Column("device_id", sa.String(length=128), nullable=False),
        sa.Column("created_at", sa.DateTime(timezone=True), nullable=False),
        sa.ForeignKeyConstraint(["account_id"], ["sync_account.id"], ondelete="CASCADE"),
        sa.UniqueConstraint("account_id", "event_id", name="uq_sync_change_account_event"),
    )
    op.create_index("idx_sync_change_account_seq", "sync_change", ["account_id", "seq"])


def downgrade() -> None:
    op.drop_index("idx_sync_change_account_seq", table_name="sync_change")
    op.drop_table("sync_change")
    op.drop_table("sync_object")
    op.drop_table("sync_device")
    op.drop_table("sync_token")
    op.drop_table("sync_account")

