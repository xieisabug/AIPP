"""Choose one proactive scheduler per sync account."""
from alembic import op
import sqlalchemy as sa

revision = "0007_proactive_owner"
down_revision = "0006_provider_settings"
branch_labels = None
depends_on = None


def upgrade():
    op.create_table("proactive_owner",
                    sa.Column("account_id", sa.String(64), sa.ForeignKey("sync_account.id", ondelete="CASCADE"), primary_key=True),
                    sa.Column("device_id", sa.String(128), nullable=False))


def downgrade():
    op.drop_table("proactive_owner")
