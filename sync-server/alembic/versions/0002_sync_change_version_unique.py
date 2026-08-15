from __future__ import annotations

from alembic import op

revision = "0002_sync_change_version_unique"
down_revision = "0001_initial"
branch_labels = None
depends_on = None


def upgrade() -> None:
    # Last line of defense against concurrent version races on the same object.
    # SQLite requires a table rebuild for new constraints, so use batch mode.
    with op.batch_alter_table("sync_change") as batch_op:
        batch_op.create_unique_constraint(
            "uq_sync_change_account_object_version",
            ["account_id", "object_type", "object_id", "version"],
        )


def downgrade() -> None:
    with op.batch_alter_table("sync_change") as batch_op:
        batch_op.drop_constraint("uq_sync_change_account_object_version", type_="unique")
