from __future__ import annotations

from datetime import datetime

from sqlalchemy import DateTime, ForeignKey, Index, Integer, JSON, String, UniqueConstraint
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column


class Base(DeclarativeBase):
    pass


class SyncAccount(Base):
    __tablename__ = "sync_account"

    id: Mapped[str] = mapped_column(String(64), primary_key=True)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), nullable=False)


class SyncToken(Base):
    __tablename__ = "sync_token"

    id: Mapped[str] = mapped_column(String(64), primary_key=True)
    account_id: Mapped[str] = mapped_column(
        String(64), ForeignKey("sync_account.id", ondelete="CASCADE"), nullable=False
    )
    name: Mapped[str] = mapped_column(String(128), nullable=False)
    token_hash: Mapped[str] = mapped_column(String(64), unique=True, nullable=False)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), nullable=False)
    revoked_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)


class SyncDevice(Base):
    __tablename__ = "sync_device"
    __table_args__ = (UniqueConstraint("account_id", "id", name="uq_sync_device_account_id"),)

    id: Mapped[str] = mapped_column(String(128), primary_key=True)
    account_id: Mapped[str] = mapped_column(
        String(64), ForeignKey("sync_account.id", ondelete="CASCADE"), nullable=False
    )
    name: Mapped[str] = mapped_column(String(256), nullable=False)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), nullable=False)
    last_seen_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)
    revoked_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)


class SyncObject(Base):
    __tablename__ = "sync_object"

    account_id: Mapped[str] = mapped_column(
        String(64), ForeignKey("sync_account.id", ondelete="CASCADE"), primary_key=True
    )
    object_type: Mapped[str] = mapped_column(String(128), primary_key=True)
    object_id: Mapped[str] = mapped_column(String(128), primary_key=True)
    version: Mapped[int] = mapped_column(Integer, nullable=False)
    payload_json: Mapped[dict | None] = mapped_column(JSON, nullable=True)
    payload_hash: Mapped[str] = mapped_column(String(64), nullable=False)
    deleted_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)
    updated_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), nullable=False)
    updated_by_device_id: Mapped[str] = mapped_column(String(128), nullable=False)


class SyncChange(Base):
    __tablename__ = "sync_change"
    __table_args__ = (
        UniqueConstraint("account_id", "event_id", name="uq_sync_change_account_event"),
        UniqueConstraint(
            "account_id",
            "object_type",
            "object_id",
            "version",
            name="uq_sync_change_account_object_version",
        ),
        Index("idx_sync_change_account_seq", "account_id", "seq"),
    )

    seq: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    account_id: Mapped[str] = mapped_column(
        String(64), ForeignKey("sync_account.id", ondelete="CASCADE"), nullable=False
    )
    event_id: Mapped[str] = mapped_column(String(128), nullable=False)
    object_type: Mapped[str] = mapped_column(String(128), nullable=False)
    object_id: Mapped[str] = mapped_column(String(128), nullable=False)
    operation: Mapped[str] = mapped_column(String(16), nullable=False)
    version: Mapped[int] = mapped_column(Integer, nullable=False)
    payload_json: Mapped[dict | None] = mapped_column(JSON, nullable=True)
    deleted_at: Mapped[datetime | None] = mapped_column(DateTime(timezone=True), nullable=True)
    device_id: Mapped[str] = mapped_column(String(128), nullable=False)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), nullable=False)

